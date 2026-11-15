use super::PhotocraftApp;

impl PhotocraftApp {
    /// Zoom of the active document's main view: screen (device) pixels per document pixel, the
    /// user-facing factor (`100%` is `1.0`).
    pub fn current_zoom(&self) -> f32 {
        self.session.active_index().and_then(|i| self.ui.views.get(i)).map_or(1.0, |v| v.zoom)
    }

    /// Screen (device) pixels per egui point of the canvas (`ctx.pixels_per_point`), as of the
    /// last canvas frame. `1.0` before the first frame.
    pub fn canvas_ppp(&self) -> f32 {
        if self.ppp.is_finite() && self.ppp > 0.0 { self.ppp } else { 1.0 }
    }

    /// The active view's zoom in egui points per document pixel: the unit every screen-space
    /// distance, tolerance and texture-level choice on the canvas is measured in.
    pub fn point_zoom(&self) -> f32 {
        self.current_zoom() / self.canvas_ppp()
    }
}

impl PhotocraftApp {
    /// Reflection used by canvas-local helpers. Extra windows temporarily supply their camera
    /// to the same helpers; their presentation state is copied back only to that window.
    pub fn canvas_flip_horizontal(&self) -> bool {
        self.session.active_index().and_then(|i| self.ui.views.get(i)).is_some_and(|v| v.flip_horizontal)
    }

    pub fn canvas_flip_vertical(&self) -> bool {
        self.session.active_index().and_then(|i| self.ui.views.get(i)).is_some_and(|v| v.flip_vertical)
    }

    pub(crate) fn focused_view(&self) -> Option<&crate::state::View> {
        let document = self.session.active_index()?;
        self.ui
            .focused_window
            .and_then(|id| self.ui.windows.iter().find(|w| w.id == id && w.open && w.document == document))
            .map(|w| &w.view)
            .or_else(|| self.ui.views.get(document))
    }

    pub(crate) fn secondary_canvas(&self) -> bool {
        self.view_composites.rendering_window.is_some()
    }

    pub(crate) fn secondary_view_command(id: &str) -> bool {
        matches!(id, "view.zoomIn" | "view.zoomOut" | "view.flipHorizontal" | "view.flipVertical")
    }

    pub(crate) fn require_main_view(&self) -> Result<(), String> {
        if self.secondary_canvas() {
            return Err(tl!("Edit the document in Main View.").into());
        }
        Ok(())
    }

    fn flip_window(&self) -> Option<&crate::state::DocWindow> {
        let target = self.view_composites.rendering_window.or_else(|| crate::view_visibility::focused_window(self))?;
        self.ui.windows.iter().find(|w| w.id == target && w.open)
    }

    pub(crate) fn flip_enabled(&self) -> bool {
        self.session.active().is_some() && self.flip_window().is_none_or(|w| !w.follow_main_flip)
    }

    pub(crate) fn effective_flip(&self) -> bool {
        self.effective_flip_axis(false)
    }

    pub(crate) fn effective_flip_axis(&self, vertical: bool) -> bool {
        let axis = |v: &crate::state::View| if vertical { v.flip_vertical } else { v.flip_horizontal };
        match self.flip_window() {
            Some(w) if self.view_composites.rendering_window.is_none() && !w.follow_main_flip => axis(&w.view),
            Some(w) => self.ui.views.get(w.document).is_some_and(axis),
            None => self.session.active_index().and_then(|i| self.ui.views.get(i)).is_some_and(axis),
        }
    }

    pub(crate) fn set_flip(&mut self, ctx: &egui::Context, vertical: bool, flipped: bool) {
        let set = |v: &mut crate::state::View| {
            if vertical {
                v.flip_vertical = flipped;
            } else {
                v.flip_horizontal = flipped;
            }
        };
        if !self.flip_enabled() {
            return;
        }
        if let Some(w) = self.flip_window() {
            let (id, document) = (w.id, w.document);
            if self.view_composites.rendering_window.is_some() {
                set(&mut self.ui.views[document]);
            } else if let Some(w) = self.ui.windows.iter_mut().find(|w| w.id == id) {
                set(&mut w.view);
            }
            ctx.request_repaint_of(egui::ViewportId::from_hash_of(("docwin", id)));
        } else if let Some(document) = self.session.active_index()
            && let Some(view) = self.ui.views.get_mut(document)
        {
            set(view);
            for w in self.ui.windows.iter().filter(|w| w.open && w.document == document && w.follow_main_flip) {
                ctx.request_repaint_of(egui::ViewportId::from_hash_of(("docwin", w.id)));
            }
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    /// Capture native focus before shortcuts/control/menu dispatch. Opening the root menu
    /// should retain the last document target; clicking its canvas selects the main view.
    pub(crate) fn update_view_focus(&mut self, ctx: &egui::Context) {
        if self.ui.focused_window.is_some_and(|id| !self.ui.windows.iter().any(|w| w.id == id && w.open)) {
            self.ui.focused_window = None;
        }
        let root_focused = ctx.input(|i| i.viewport().focused == Some(true));
        if crate::panel_docking::modal_open(self, ctx) || egui::Popup::is_any_open(ctx) {
            return;
        }
        if root_focused && ctx.input(|i| i.pointer.any_pressed()) {
            if let Some(id) = crate::canvas::docked_view::pressed(self, ctx) {
                if let Some(st) = self.session.active()
                    && !crate::canvas::secondary_sampling_click(self, ctx, st.doc.id, id)
                {
                    if self.ui.focused_window != Some(id) {
                        crate::canvas::mark_secondary_activation(ctx, id);
                    }
                    self.ui.focused_window = Some(id);
                }
            } else if let Some((index, _)) = self
                .session
                .documents()
                .iter()
                .enumerate()
                .find(|(index, st)| !crate::view_cmds::document_is_floating(self, *index) && crate::canvas::canvas_pressed(ctx, st.doc.id, None))
            {
                self.ui.focused_window = None;
                self.session.set_active(index);
            }
        }
        if !root_focused {
            let window = self
                .ui
                .windows
                .iter()
                .find(|w| {
                    let viewport = egui::ViewportId::from_hash_of(("docwin", w.id));
                    // Alt activation is resolved inside the child viewport, where the canvas
                    // response and pointer event are available.
                    w.open
                        && !w.focused
                        && !crate::canvas::docked_view::contains(self, w.id)
                        && !ctx.input(|i| crate::canvas::secondary_sample_modifier(crate::workspace_ui::sticky_mods(self, i.modifiers)))
                        && !ctx.input_for(viewport, |i| crate::canvas::secondary_sample_modifier(i.modifiers))
                        && ctx.input(|i| i.raw.viewports.get(&viewport).is_some_and(|v| v.focused == Some(true)))
                })
                .map(|w| (w.id, w.document));
            if let Some((id, document)) = window {
                self.ui.focused_window = Some(id);
                self.session.set_active(document);
            }
        }
    }

    /// Run canvas helpers against this window's camera, preserving the main camera, rectangle
    /// and display scale. This prevents focused child input from leaking into the main view.
    pub(crate) fn with_document_window<R>(&mut self, id: u64, draw: impl FnOnce(&mut Self) -> R) -> Option<R> {
        let window = self.ui.windows.iter().find(|w| w.id == id && w.open)?;
        let (document, mut camera) = (window.document, window.view.clone());
        let saved_flip = (camera.flip_horizontal, camera.flip_vertical);
        let following = window.follow_main_flip;
        if following {
            let main = self.ui.views.get(document)?;
            (camera.flip_horizontal, camera.flip_vertical) = (main.flip_horizontal, main.flip_vertical);
        }
        let document_id = self.session.documents().get(document)?.doc.id;
        let main = std::mem::replace(self.ui.views.get_mut(document)?, camera);
        let previous_main = self.view_composites.scoped_main_camera.replace((document_id, main));
        let (rect, ppp) = (self.last_canvas_rect, self.ppp);
        let rendering_window = self.view_composites.rendering_window.replace(id);
        let drawing_window = self.drawing_window.replace(id);
        let result = draw(self);
        self.drawing_window = drawing_window;
        self.view_composites.rendering_window = rendering_window;
        let main = std::mem::replace(&mut self.view_composites.scoped_main_camera, previous_main);
        let document = self.session.documents().iter().position(|st| st.doc.id == document_id);
        if let (Some((_, main)), Some(slot)) = (main, document.and_then(|i| self.ui.views.get_mut(i))) {
            let mut camera = std::mem::replace(slot, main);
            if following {
                (camera.flip_horizontal, camera.flip_vertical) = saved_flip;
            }
            if let Some(window) = self.ui.windows.iter_mut().find(|w| w.id == id) {
                window.view = camera;
            }
        }
        self.last_canvas_rect = rect;
        self.ppp = ppp;
        Some(result)
    }
}
