//! Typed, immutable view settings and one explicitly targeted Properties editing session.
use super::*;
use photocraft_doc::{Adjustment, ColorMode, LayerContent};
use photocraft_engine::adjust_params;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Snapshot {
    pub value: Arc<Adjustment>,
    mode: ColorMode,
    epoch: u64,
}

impl Visibility {
    pub(super) fn sync_adjustments(&mut self, doc: &Document) {
        self.adjustments.retain(|id, _| doc.layer(*id).is_some_and(|l| matches!(l.content, LayerContent::Adjustment(_))));
        for (_, _, layer) in doc.walk() {
            let LayerContent::Adjustment(value) = &layer.content else { continue };
            let reset = self
                .adjustments
                .get(&layer.id)
                .is_none_or(|snapshot| snapshot.mode != doc.mode || std::mem::discriminant(snapshot.value.as_ref()) != std::mem::discriminant(value));
            if reset {
                self.adjustment_serial = self.adjustment_serial.wrapping_add(1).max(1);
                self.adjustments.insert(layer.id, Snapshot { value: Arc::new(value.clone()), mode: doc.mode, epoch: self.adjustment_serial });
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Target {
    pub document: DocId,
    pub layer: LayerId,
    pub window: Option<u64>,
    epoch: u64,
    mode: ColorMode,
    kind: &'static str,
}

pub(crate) fn supported(value: &Adjustment) -> bool {
    !matches!(value, Adjustment::Unsupported { .. })
}

impl Target {
    pub(crate) fn current(app: &mut PhotocraftApp, layer: LayerId) -> Option<Self> {
        Self::new(app, focused_window(app), layer).ok()
    }

    fn new(app: &mut PhotocraftApp, window: Option<u64>, layer: LayerId) -> Result<Self, String> {
        let idx = match window {
            Some(id) => app.ui.windows.iter().find(|w| w.id == id && w.open).map(|w| w.document).ok_or("secondary window is closed")?,
            None => app.session.active_index().ok_or("no document")?,
        };
        let doc = app.session.documents().get(idx).ok_or("document is closed")?.doc.clone();
        let value = match doc.layer(layer).map(|l| &l.content) {
            Some(LayerContent::Adjustment(value)) if supported(value) => value,
            _ => return Err("this adjustment type is not supported".into()),
        };
        let epoch = match window {
            Some(id) => {
                initialize(app, id);
                app.ui
                    .windows
                    .iter()
                    .find(|w| w.id == id)
                    .and_then(|w| w.visibility.adjustments.get(&layer))
                    .map(|s| s.epoch)
                    .ok_or("adjustment is unavailable")?
            }
            None => 0,
        };
        Ok(Self { document: doc.id, layer, window, epoch, mode: doc.mode, kind: photocraft_engine::commands::adjustment_kind(value) })
    }

    pub(crate) fn memory(self) -> egui::Id {
        egui::Id::unique(("view-adjustment-editor", self))
    }

    pub(crate) fn kind(self) -> &'static str {
        self.kind
    }

    pub(crate) fn mode(self) -> ColorMode {
        self.mode
    }

    fn validate(self, app: &PhotocraftApp) -> Result<(), String> {
        let st = app.session.documents().iter().find(|st| st.doc.id == self.document).ok_or("document is closed")?;
        let Some(LayerContent::Adjustment(value)) = st.doc.layer(self.layer).map(|l| &l.content) else { return Err("adjustment no longer exists".into()) };
        if !supported(value) || st.doc.mode != self.mode || photocraft_engine::commands::adjustment_kind(value) != self.kind {
            return Err("adjustment type or document color mode changed".into());
        }
        if let Some(id) = self.window {
            let valid = app.ui.windows.iter().any(|w| {
                w.id == id
                    && w.open
                    && app.session.documents().get(w.document).is_some_and(|d| d.doc.id == self.document)
                    && w.visibility.adjustments.get(&self.layer).is_some_and(|s| s.epoch == self.epoch)
            });
            if !valid {
                return Err("secondary adjustment target is stale".into());
            }
        } else if app.session.active().is_none_or(|st| st.doc.id != self.document) {
            return Err("main document is no longer active".into());
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct EditorSession {
    pub target: Option<Target>,
    draft: Option<Arc<Adjustment>>,
    pub ui_epoch: u64,
    reset_start: Option<Arc<Adjustment>>,
    edited: bool,
}

pub(crate) fn cancel(app: &mut PhotocraftApp) {
    if app.view_composites.editor.target.is_some_and(|t| t.window.is_none()) {
        app.live_adjust = None;
    }
    app.view_composites.editor.draft = None;
}

pub(crate) fn abort(app: &mut PhotocraftApp) {
    cancel(app);
    let widgets: Vec<_> =
        app.ui.dialogs.iter().filter(|d| d.fields.contains_key("__adjustmentOwner")).filter_map(|d| d.fields.get("__widget").and_then(Value::as_u64)).collect();
    app.ui.dialogs.retain(|d| !d.fields.contains_key("__adjustmentOwner"));
    app.widget_colors.retain(|(id, _)| !widgets.contains(id));
    app.view_composites.editor.ui_epoch = app.view_composites.editor.ui_epoch.wrapping_add(1);
}

pub(crate) fn select(app: &mut PhotocraftApp, target: Option<Target>) {
    if app.view_composites.editor.target != target {
        abort(app);
        app.view_composites.editor.target = target;
        app.view_composites.editor.reset_start = target.and_then(|t| committed(app, t).ok());
        app.view_composites.editor.edited = false;
        app.ui.layer_picker = None;
    }
}

/// Async pickers never refocus a document or redirect their result to the current view.
pub(crate) fn accepts_completion(app: &PhotocraftApp, target: Target, epoch: u64) -> bool {
    app.view_composites.editor.target == Some(target)
        && app.view_composites.editor.ui_epoch == epoch
        && target.validate(app).is_ok()
        && focused_window(app) == target.window
        && app.session.active().is_some_and(|st| st.doc.id == target.document && st.active_layer == Some(target.layer))
        && !app.ui.vector_mask_target
        && (!app.ui.mask_target || app.ui.properties_layer == Some(target.layer.0))
}

pub(crate) fn prune(app: &mut PhotocraftApp) {
    if app.view_composites.editor.target.is_some_and(|target| target.validate(app).is_err()) {
        select(app, None);
    }
}

/// Cancel on view/layer/mask target changes, including when Properties is not currently drawn.
pub(crate) fn follow(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let Some(old) = app.view_composites.editor.target else { return };
    let selected = app.session.active().and_then(|s| s.active_layer);
    let mask = app.ui.vector_mask_target || (app.ui.mask_target && app.ui.properties_layer != Some(old.layer.0));
    if old.validate(app).is_err() || selected != Some(old.layer) || focused_window(app) != old.window || mask {
        select(app, None);
        ctx.request_repaint();
    } else if app.view_composites.editor.draft.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        abort(app);
        ctx.request_repaint();
    }
}

pub(crate) fn committed(app: &PhotocraftApp, target: Target) -> Result<Arc<Adjustment>, String> {
    target.validate(app)?;
    if let Some(id) = target.window {
        return app
            .ui
            .windows
            .iter()
            .find(|w| w.id == id)
            .and_then(|w| w.visibility.adjustments.get(&target.layer))
            .map(|s| s.value.clone())
            .ok_or("adjustment unavailable".into());
    }
    let value = app.session.active().and_then(|st| st.doc.layer(target.layer)).map(|l| &l.content);
    match value {
        Some(LayerContent::Adjustment(value)) => Ok(Arc::new(value.clone())),
        _ => Err("adjustment unavailable".into()),
    }
}

pub(crate) fn effective(app: &PhotocraftApp, target: Target) -> Result<Arc<Adjustment>, String> {
    target.validate(app)?;
    if app.view_composites.editor.target == Some(target)
        && let Some(draft) = &app.view_composites.editor.draft
    {
        return Ok(draft.clone());
    }
    committed(app, target)
}

pub(super) fn effective_snapshots(app: &PhotocraftApp, window: u64) -> BTreeMap<LayerId, Arc<Adjustment>> {
    let Some(w) = app.ui.windows.iter().find(|w| w.id == window && w.open) else { return BTreeMap::new() };
    let mut values: BTreeMap<_, _> = w.visibility.adjustments.iter().map(|(id, snapshot)| (*id, snapshot.value.clone())).collect();
    if let Some(target) = app.view_composites.editor.target.filter(|t| t.window == Some(window) && t.validate(app).is_ok())
        && let Some(draft) = &app.view_composites.editor.draft
    {
        values.insert(target.layer, draft.clone());
    }
    values
}

pub(crate) fn preview(app: &mut PhotocraftApp, target: Target, params: &Value) -> Result<(), String> {
    target.validate(app)?;
    let base = effective(app, target)?;
    // A control may finish a gesture without changing its displayed values. Preserve any
    // extra precision in the typed snapshot instead of round-tripping unchanged editor JSON.
    let value = if params == &adjust_params::to_params(&base) {
        (*base).clone()
    } else {
        adjust_params::from_params(target.kind, params, Some(&base), target.mode).map_err(|e| e.to_string())?
    };
    let prior = committed(app, target)?;
    select(app, Some(target));
    if value == *prior {
        cancel(app);
    } else if value != *base {
        app.view_composites.editor.draft = Some(Arc::new(value));
    }
    if target.window.is_none()
        && let Some(draft) = &app.view_composites.editor.draft
    {
        app.live_adjust = Some((target.layer, adjust_params::to_params(draft)));
    }
    Ok(())
}

pub(crate) fn reset_restores_session(app: &PhotocraftApp, target: Target) -> bool {
    target.kind == "colorBalance" && app.view_composites.editor.target == Some(target) && app.view_composites.editor.edited
}

pub(crate) fn commit(app: &mut PhotocraftApp, target: Target, params: &Value, reset: bool) -> Result<(), String> {
    target.validate(app)?;
    let base = effective(app, target)?;
    let value = if !reset && params == &adjust_params::to_params(&base) {
        Ok((*base).clone())
    } else if reset {
        if reset_restores_session(app, target) {
            app.view_composites.editor.reset_start.as_ref().map_or_else(|| adjust_params::default_for(target.kind, target.mode), |v| Ok((**v).clone()))
        } else {
            adjust_params::default_for(target.kind, target.mode)
        }
    } else {
        adjust_params::from_params(target.kind, params, Some(&base), target.mode)
    }
    .map_err(|e| e.to_string())?;
    let prior = committed(app, target)?;
    if value != *prior {
        if let Some(id) = target.window {
            let snapshot =
                app.ui.windows.iter_mut().find(|w| w.id == id).and_then(|w| w.visibility.adjustments.get_mut(&target.layer)).ok_or("adjustment unavailable")?;
            snapshot.value = if value == *base { base.clone() } else { Arc::new(value) };
            if app.view_composites.editor.target == Some(target) {
                app.view_composites.editor.edited = !reset;
            }
        } else {
            let mut params = if !reset && target.kind == "colorLookup" { params.clone() } else { adjust_params::to_params(&value) };
            params["layer"] = json!(target.layer.0);
            app.run("layer.setAdjustment", params)?;
        }
    }
    if app.view_composites.editor.target == Some(target) {
        if reset {
            app.view_composites.editor.edited = false;
        }
        cancel(app);
    }
    Ok(())
}

pub(crate) fn command(app: &mut PhotocraftApp, ctx: &egui::Context, p: &Value) -> Result<Value, String> {
    let window = match p.get("window") {
        Some(value) => value.as_u64().ok_or("window must be a secondary window ID")?,
        None => focused_window(app).ok_or("select a secondary window")?,
    };
    let layer = LayerId(p.get("layer").and_then(Value::as_u64).ok_or("layer must be a layer ID")?);
    let target = Target::new(app, Some(window), layer)?;
    let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
    let action = match p.get("action") {
        None => "commit",
        Some(value) => value.as_str().ok_or("action must be a string")?,
    };
    match action {
        "commit" => commit(app, target, &params, false)?,
        "reset" => commit(app, target, &json!({}), true)?,
        "preview" => preview(app, target, &params)?,
        "cancel" => {
            if app.view_composites.editor.target == Some(target) {
                abort(app);
            }
        }
        _ => return Err("action must be commit, reset, preview or cancel".into()),
    }
    ctx.request_repaint_of(egui::ViewportId::from_hash_of(("docwin", window)));
    ctx.request_repaint();
    let effective = effective(app, target)?;
    Ok(json!({"window": window, "layer": layer.0, "params": adjust_params::to_params(&effective)}))
}
