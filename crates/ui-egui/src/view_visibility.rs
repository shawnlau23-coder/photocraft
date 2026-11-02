//! Display-only layer visibility and opacity. Projections never enter Session, history, save or export.
//! Raster surfaces in the metadata projection retain their existing Arc<Tile> storage.
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Weak};

use photocraft_doc::{DocId, Document, LayerId};
use serde_json::{Value, json};

use crate::{PhotocraftApp, canvas::CanvasCache};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Visibility {
    document: Option<DocId>,
    layers: BTreeMap<LayerId, bool>,
    opacity: BTreeMap<LayerId, f32>,
    solo: Option<(LayerId, BTreeMap<LayerId, bool>)>,
}

impl Visibility {
    pub(crate) fn new(doc: &Document) -> Self {
        let layers = doc.walk();
        Self {
            document: Some(doc.id),
            layers: layers.iter().map(|(_, _, l)| (l.id, l.visible)).collect(),
            opacity: layers.iter().map(|(_, _, l)| (l.id, display_opacity(l.opacity))).collect(),
            solo: None,
        }
    }

    fn sync(&mut self, doc: &Document) {
        if self.document != Some(doc.id) {
            *self = Self::new(doc);
            return;
        }
        let live: BTreeMap<_, _> = doc.walk().into_iter().map(|(_, _, l)| (l.id, l.visible)).collect();
        self.layers.retain(|id, _| live.contains_key(id));
        self.opacity.retain(|id, _| live.contains_key(id));
        for (_, _, layer) in doc.walk() {
            self.opacity.entry(layer.id).or_insert_with(|| display_opacity(layer.opacity));
        }
        if self.solo.as_ref().is_some_and(|(id, _)| !live.contains_key(id)) {
            self.solo = None;
        }
        if let Some((_, saved)) = &mut self.solo {
            saved.retain(|id, _| live.contains_key(id));
        }
        for (id, visible) in live {
            self.layers.entry(id).or_insert(visible);
        }
    }
}

type Caches = HashMap<(DocId, u32), CanvasCache>;

struct Projection {
    source: Weak<Document>,
    source_key: (u64, u64),
    visibility: BTreeMap<LayerId, bool>,
    opacity: BTreeMap<LayerId, f32>,
    shown: Arc<Document>,
    key: u64,
}

/// Separate namespaces: native document identity is never replaced by a made-up document ID.
#[derive(Default)]
pub(crate) struct Composites {
    pub rendering_window: Option<u64>,
    pub caches: HashMap<u64, Caches>,
    pub channels: HashMap<u64, HashMap<u64, crate::channel_view::Cache>>,
    projections: HashMap<u64, Projection>,
    next_key: u64,
}

pub(crate) fn focused_window(app: &PhotocraftApp) -> Option<u64> {
    let doc = app.session.active_index()?;
    app.ui.focused_window.filter(|id| app.ui.windows.iter().any(|w| w.id == *id && w.open && w.document == doc))
}

pub(crate) fn focused_opacity(app: &PhotocraftApp, layer: LayerId) -> Option<f32> {
    let id = focused_window(app)?;
    app.ui.windows.iter().find(|w| w.id == id)?.visibility.opacity.get(&layer).copied()
}

pub(crate) fn initialize(app: &mut PhotocraftApp, id: u64) {
    let Some(w) = app.ui.windows.iter_mut().find(|w| w.id == id && w.open) else { return };
    if let Some(st) = app.session.documents().get(w.document) {
        w.visibility.sync(&st.doc);
    }
}

/// Also runs after native window closure, so textures and metadata cannot outlive a view.
pub(crate) fn retain(app: &mut PhotocraftApp) {
    let live: Vec<u64> = app.ui.windows.iter().filter(|w| w.open && w.document < app.session.documents().len()).map(|w| w.id).collect();
    app.view_composites.caches.retain(|id, _| live.contains(id));
    app.view_composites.projections.retain(|id, _| live.contains(id));
    app.view_composites.channels.retain(|id, _| live.contains(id));
    for id in live {
        initialize(app, id);
        if let Some(doc) = app.ui.windows.iter().find(|w| w.id == id).and_then(|w| w.visibility.document) {
            if let Some(caches) = app.view_composites.caches.get_mut(&id) {
                caches.retain(|(source, _), _| *source == doc);
            }
            if let Some(channels) = app.view_composites.channels.get_mut(&id) {
                channels.retain(|source, _| *source == doc.0);
            }
            if app.view_composites.projections.get(&id).is_some_and(|p| p.shown.id != doc) {
                app.view_composites.projections.remove(&id);
            }
        }
    }
}

pub(crate) fn inspect(app: &PhotocraftApp) -> Value {
    json!(
        app.ui
            .windows
            .iter()
            .filter(|w| w.open)
            .map(|w| { json!({"window": w.id, "document": w.visibility.document, "layers": w.visibility.layers, "opacity": w.visibility.opacity}) })
            .collect::<Vec<_>>()
    )
}

/// Snapshot layer metadata once per source/visibility change. No pixels are copied or written.
pub(crate) fn project(app: &mut PhotocraftApp, window: u64, source: Arc<Document>) -> (Arc<Document>, u64) {
    project_with_key(app, window, source, 0)
}

fn project_with_key(app: &mut PhotocraftApp, window: u64, source: Arc<Document>, source_key: u64) -> (Arc<Document>, u64) {
    initialize(app, window);
    let revision = app.session.documents().iter().find(|st| st.doc.id == source.id).map_or(0, |st| st.revision);
    let source_key = (revision, source_key);
    let Some(w) = app.ui.windows.iter().find(|w| w.id == window && w.open && w.visibility.document == Some(source.id)) else { return (source, 0) };
    let visibility = &w.visibility.layers;
    let opacity = &w.visibility.opacity;
    if let Some(p) = app.view_composites.projections.get(&window)
        && p.source.ptr_eq(&Arc::downgrade(&source))
        && p.source_key == source_key
        && &p.visibility == visibility
        && &p.opacity == opacity
    {
        return (p.shown.clone(), p.key);
    }
    let mut shown = (*source).clone();
    // `walk` provides paths accepted by the existing document model. Mutate only
    // presentation metadata, never Surface::write/Arc::make_mut on any pixel storage.
    for (path, _, layer) in source.walk() {
        if let Some(target) = shown.layer_at_mut(&path) {
            if let Some(visible) = visibility.get(&layer.id) {
                target.visible = *visible;
            }
            if let Some(opacity) = opacity.get(&layer.id) {
                target.opacity = *opacity;
            }
        }
    }
    let shown = Arc::new(shown);
    app.view_composites.next_key = app.view_composites.next_key.wrapping_add(1).max(1);
    let key = app.view_composites.next_key;
    app.view_composites.projections.insert(
        window,
        Projection { source: Arc::downgrade(&source), source_key, visibility: visibility.clone(), opacity: opacity.clone(), shown: shown.clone(), key },
    );
    (shown, key)
}

/// Flattened filter previews have lost the layer identity needed to apply overrides. Secondary
/// views keep the layered source until those operations commit; live paint remains layered.
pub(crate) fn render_document(app: &mut PhotocraftApp, idx: usize, source: Arc<Document>, key: u64) -> (Arc<Document>, u64) {
    let Some(window) = app.view_composites.rendering_window else { return (source, key) };
    let Some(st) = app.session.documents().get(idx) else { return (source, key) };
    let canonical = st.doc.clone();
    let source_ids: Vec<_> = source.walk().into_iter().map(|(_, _, l)| l.id).collect();
    let layered = canonical.walk().iter().all(|(_, _, l)| source_ids.contains(&l.id));
    project_with_key(app, window, if layered { source } else { canonical }, key)
}

pub(crate) fn channel_caches(app: &PhotocraftApp) -> Option<&HashMap<u64, crate::channel_view::Cache>> {
    match app.view_composites.rendering_window {
        Some(id) => app.view_composites.channels.get(&id),
        None => Some(&app.channel_views),
    }
}

pub(crate) fn channel_caches_mut(app: &mut PhotocraftApp) -> &mut HashMap<u64, crate::channel_view::Cache> {
    match app.view_composites.rendering_window {
        Some(id) => app.view_composites.channels.entry(id).or_default(),
        None => &mut app.channel_views,
    }
}

pub(crate) fn cache(app: &PhotocraftApp, doc: DocId, output: Option<u32>) -> Option<&CanvasCache> {
    let key = crate::canvas::cache_key(doc, output);
    match app.view_composites.rendering_window {
        Some(id) => app.view_composites.caches.get(&id)?.get(&key),
        None => app.canvases.get(&key),
    }
}

pub(crate) fn cache_mut(app: &mut PhotocraftApp, doc: DocId, output: Option<u32>) -> Option<&mut CanvasCache> {
    let key = crate::canvas::cache_key(doc, output);
    match app.view_composites.rendering_window {
        Some(id) => app.view_composites.caches.get_mut(&id)?.get_mut(&key),
        None => app.canvases.get_mut(&key),
    }
}

pub(crate) fn handles(id: &str) -> bool {
    matches!(id, "view.layerVisibility" | "view.layerVisibility.showOnly" | "view.layerOpacity")
}

pub(crate) fn command(app: &mut PhotocraftApp, ctx: &egui::Context, id: &str, p: &Value) -> Result<Value, String> {
    let window = match p.get("window") {
        Some(value) => value.as_u64().ok_or("window must be a secondary window ID")?,
        None => focused_window(app).ok_or("select a secondary document window")?,
    };
    let w = app.ui.windows.iter().find(|w| w.id == window && w.open).ok_or("secondary window is closed")?;
    let doc = app.session.documents().get(w.document).ok_or("document is closed")?.doc.clone();
    if id == "view.layerOpacity" {
        return set_opacity(app, ctx, window, &doc, p);
    }
    let layer = LayerId(p.get("layer").and_then(Value::as_u64).ok_or("layer must be a layer ID")?);
    let path = doc.path_of(layer).ok_or("layer does not exist")?;
    let visible = if id == "view.layerVisibility" { Some(p.get("visible").and_then(Value::as_bool).ok_or("visible must be a boolean")?) } else { None };
    initialize(app, window);
    let w = app.ui.windows.iter_mut().find(|w| w.id == window).ok_or("secondary window is closed")?;
    if let Some(visible) = visible {
        w.visibility.layers.insert(layer, visible);
        w.visibility.solo = None;
    } else if w.visibility.solo.as_ref().is_some_and(|(id, _)| *id == layer) {
        if let Some((_, saved)) = w.visibility.solo.take() {
            for (id, visible) in saved {
                if let Some(target) = w.visibility.layers.get_mut(&id) {
                    *target = visible;
                }
            }
        }
    } else {
        let saved = w.visibility.solo.take().map_or_else(|| w.visibility.layers.clone(), |(_, saved)| saved);
        for (candidate, _, l) in doc.walk() {
            // Keep ancestors visible; descendants of a solo group retain their own eyes.
            let visible = path.starts_with(&candidate) || (candidate.starts_with(&path) && saved.get(&l.id).copied().unwrap_or(l.visible));
            w.visibility.layers.insert(l.id, visible);
        }
        w.visibility.solo = Some((layer, saved));
    }
    ctx.request_repaint_of(egui::ViewportId::from_hash_of(("docwin", window)));
    ctx.request_repaint();
    Ok(json!({"window": window, "layer": layer.0, "visible": w.visibility.layers.get(&layer)}))
}

fn display_opacity(value: f32) -> f32 {
    if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 }
}

/// Validate the entire target set before changing display state. Slider coalescing is irrelevant:
/// these updates never enter history, and equal f32 effective values keep the projection cached.
fn set_opacity(app: &mut PhotocraftApp, ctx: &egui::Context, window: u64, doc: &Document, p: &Value) -> Result<Value, String> {
    let value =
        p.get("opacity").and_then(Value::as_f64).filter(|v| v.is_finite() && (0.0..=1.0).contains(v)).ok_or("opacity must be a finite number from 0 to 1")?
            as f32;
    let ids = match p.get("layers").filter(|v| !v.is_null()) {
        Some(value) => value
            .as_array()
            .filter(|a| !a.is_empty())
            .ok_or("layers must be a nonempty array of layer IDs")?
            .iter()
            .map(|id| id.as_u64().map(LayerId).ok_or("layer must be a layer ID"))
            .collect::<Result<Vec<_>, _>>()?,
        None => vec![LayerId(p.get("layer").and_then(Value::as_u64).ok_or("layer must be a layer ID")?)],
    };
    if ids.iter().any(|id| doc.layer(*id).is_none()) {
        return Err("layer does not exist".into());
    }
    initialize(app, window);
    let w = app.ui.windows.iter_mut().find(|w| w.id == window && w.open).ok_or("secondary window is closed")?;
    let changed = ids.iter().any(|id| w.visibility.opacity.get(id) != Some(&value));
    if changed {
        for id in ids {
            w.visibility.opacity.insert(id, value);
        }
        ctx.request_repaint_of(egui::ViewportId::from_hash_of(("docwin", window)));
        ctx.request_repaint();
    }
    Ok(json!({"window": window, "opacity": value, "changed": changed}))
}

/// Only visibility and layer-opacity controls are rerouted. Fill, masks and other edits stay canonical.
pub(crate) fn panel_action(app: &mut PhotocraftApp, ctx: &egui::Context, window: Option<u64>, id: &str, p: &Value) -> bool {
    let Some(window) = window else { return false };
    let command_id = match id {
        "layer.setProps" if p.get("visible").is_some() => "view.layerVisibility",
        "layer.setProps" if p.get("opacity").is_some() => "view.layerOpacity",
        "layer.showOnly" => "view.layerVisibility.showOnly",
        _ => return false,
    };
    let params = json!({"window": window, "layer": p.get("layer"), "layers": p.get("layers"), "visible": p.get("visible"), "opacity": p.get("opacity")});
    if let Err(error) = command(app, ctx, command_id, &params) {
        app.ui.status = error;
        app.ui.status_error = true;
    }
    true
}

#[cfg(test)]
mod tests;
