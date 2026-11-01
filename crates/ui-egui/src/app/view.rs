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

    pub(crate) fn focused_view(&self) -> Option<&crate::state::View> {
        let document = self.session.active_index()?;
        self.ui
            .focused_window
            .and_then(|id| self.ui.windows.iter().find(|w| w.id == id && w.open && w.document == document))
            .map(|w| &w.view)
            .or_else(|| self.ui.views.get(document))
    }

    pub(crate) fn focused_view_mut(&mut self) -> Option<&mut crate::state::View> {
        let document = self.session.active_index()?;
        let window = self.ui.focused_window.and_then(|id| self.ui.windows.iter().position(|w| w.id == id && w.open && w.document == document));
        match window {
            Some(i) => self.ui.windows.get_mut(i).map(|w| &mut w.view),
            None => self.ui.views.get_mut(document),
        }
    }

    pub(crate) fn focused_viewport(&self) -> egui::ViewportId {
        self.ui
            .focused_window
            .filter(|&id| self.ui.windows.iter().any(|w| w.id == id && w.open && Some(w.document) == self.session.active_index()))
            .map_or(egui::ViewportId::ROOT, |id| egui::ViewportId::from_hash_of(("docwin", id)))
    }

    /// Capture native focus before shortcuts/control/menu dispatch. Opening the root menu
    /// should retain the last document target; clicking its canvas selects the main view.
    pub(crate) fn update_view_focus(&mut self, ctx: &egui::Context) {
        if self.ui.focused_window.is_some_and(|id| !self.ui.windows.iter().any(|w| w.id == id && w.open)) {
            self.ui.focused_window = None;
        }
        let root_focused = ctx.input(|i| i.viewport().focused == Some(true));
        let root_focus_event = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true))));
        let pressing_menu = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !self.last_canvas_rect.contains(p)));
        let canvas_press = !egui::Popup::is_any_open(ctx)
            && ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| self.last_canvas_rect.contains(p)));
        if root_focused && (canvas_press || (root_focus_event && !pressing_menu)) {
            self.ui.focused_window = None;
        }
        if !root_focused {
            let window = self
                .ui
                .windows
                .iter()
                .find(|w| {
                    w.open && ctx.input(|i| i.raw.viewports.get(&egui::ViewportId::from_hash_of(("docwin", w.id))).is_some_and(|v| v.focused == Some(true)))
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
        let (document, camera) = (window.document, window.view.clone());
        let document_id = self.session.documents().get(document)?.doc.id;
        let main = std::mem::replace(self.ui.views.get_mut(document)?, camera);
        let (rect, ppp) = (self.last_canvas_rect, self.ppp);
        let result = draw(self);
        let document = self.session.documents().iter().position(|st| st.doc.id == document_id);
        if let Some(slot) = document.and_then(|i| self.ui.views.get_mut(i)) {
            let camera = std::mem::replace(slot, main);
            if let Some(window) = self.ui.windows.iter_mut().find(|w| w.id == id) {
                window.view = camera;
            }
        }
        self.last_canvas_rect = rect;
        self.ppp = ppp;
        Some(result)
    }
}
