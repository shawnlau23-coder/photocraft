//! A transient document-space pointer, projected by each receiving canvas's own camera.
use egui::{Color32, Context, Id, Pos2, ViewportId};
use photocraft_doc::DocId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{PhotocraftApp, canvas::ViewXform};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    #[default]
    Red,
    White,
    Black,
}

impl Color {
    fn paint(self) -> Color32 {
        match self {
            Self::Red => Color32::RED,
            Self::White => Color32::WHITE,
            Self::Black => Color32::BLACK,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// Physical screen pixels, independent of image zoom and viewport display scaling.
    pub diameter: f32,
    pub color: Color,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: false, diameter: 6.0, color: Color::Red }
    }
}

/// Never persisted or installed in an artwork document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    document: DocId,
    viewport: ViewportId,
    canvas: Id,
    position: [f64; 2],
    was_focused: bool,
}

pub(crate) fn handles(id: &str) -> bool {
    id == "view.linkedCursor"
        || id.strip_prefix("view.linkedCursor.size.").is_some_and(|s| s.parse::<u8>().is_ok_and(|n| (2..=20).contains(&n)))
        || matches!(id, "view.linkedCursor.color.red" | "view.linkedCursor.color.white" | "view.linkedCursor.color.black")
}

pub(crate) fn checked(settings: Settings, id: &str) -> Option<bool> {
    if !handles(id) {
        return None;
    }
    Some(if id == "view.linkedCursor" {
        settings.enabled
    } else if let Some(size) = id.strip_prefix("view.linkedCursor.size.").and_then(|s| s.parse::<f32>().ok()) {
        settings.diameter == size
    } else {
        match settings.color {
            Color::Red => id.ends_with(".red"),
            Color::White => id.ends_with(".white"),
            Color::Black => id.ends_with(".black"),
        }
    })
}

/// Existing menu/control dispatch owns settings; validate everything before changing state.
pub(crate) fn command(app: &mut PhotocraftApp, ctx: &Context, id: &str, params: &Value) -> Result<Value, String> {
    let mut settings = app.ui.view.linked_cursor;
    if id == "view.linkedCursor" {
        settings.enabled = match params.get("on") {
            Some(v) => v.as_bool().ok_or("on must be a boolean")?,
            None => !settings.enabled,
        };
    }
    if let Some(value) = params.get("diameter") {
        settings.diameter = value.as_f64().filter(|n| n.is_finite() && (2.0..=20.0).contains(n)).ok_or("diameter must be 2–20 screen pixels")? as f32;
    }
    if let Some(size) = id.strip_prefix("view.linkedCursor.size.").and_then(|s| s.parse::<f32>().ok()) {
        settings.diameter = size;
    }
    let color = id.strip_prefix("view.linkedCursor.color.").or_else(|| params.get("color").and_then(Value::as_str));
    if params.get("color").is_some_and(|v| !v.is_string()) {
        return Err("color must be red, white or black".into());
    }
    if let Some(color) = color {
        settings.color = match color {
            "red" => Color::Red,
            "white" => Color::White,
            "black" => Color::Black,
            _ => return Err("color must be red, white or black".into()),
        };
    }
    let previous = app.ui.linked_cursor_pointer;
    app.ui.view.linked_cursor = settings;
    if !settings.enabled {
        app.ui.linked_cursor_pointer = None;
    }
    repaint(app, ctx, previous, app.ui.linked_cursor_pointer);
    ctx.request_repaint();
    Ok(json!({"enabled": settings.enabled, "diameter": settings.diameter, "color": settings.color}))
}

fn repaint(app: &PhotocraftApp, ctx: &Context, before: Option<Pointer>, after: Option<Pointer>) {
    if before.is_none() && after.is_none() {
        return;
    }
    if ctx.viewport_id() != ViewportId::ROOT {
        ctx.request_repaint_of(ViewportId::ROOT);
    }
    for window in app.ui.windows.iter().filter(|w| w.open) {
        if let Some(doc) = app.session.documents().get(window.document)
            && [before, after].into_iter().flatten().any(|p| p.document == doc.doc.id)
        {
            let viewport = ViewportId::from_hash_of(("docwin", window.id));
            if viewport != ctx.viewport_id() {
                ctx.request_repaint_of(viewport);
            }
        }
    }
}

fn set_pointer(app: &mut PhotocraftApp, ctx: &Context, pointer: Option<Pointer>) {
    let previous = app.ui.linked_cursor_pointer;
    if previous != pointer {
        app.ui.linked_cursor_pointer = pointer;
        repaint(app, ctx, previous, pointer);
    }
}

/// Root-frame cleanup also covers a source canvas that no longer gets a draw callback.
pub(crate) fn prune(app: &mut PhotocraftApp, ctx: &Context) {
    let Some(pointer) = app.ui.linked_cursor_pointer else { return };
    let document = app.session.documents().iter().position(|st| st.doc.id == pointer.document);
    let live = document.is_some_and(|index| {
        if pointer.viewport == ViewportId::ROOT {
            if app.ui.view.arrange == "tabs" {
                crate::canvas::main_canvas_document(app) == Some(index)
            } else {
                !crate::view_cmds::document_is_floating(app, index)
            }
        } else {
            app.ui.windows.iter().any(|w| w.open && w.document == index && ViewportId::from_hash_of(("docwin", w.id)) == pointer.viewport)
        }
    });
    let lost_focus = pointer.was_focused && ctx.input(|i| i.raw.viewports.get(&pointer.viewport).is_some_and(|v| v.focused == Some(false)));
    if !app.ui.view.linked_cursor.enabled || !live || lost_focus {
        set_pointer(app, ctx, None);
    }
}

fn projection(settings: Settings, pointer: Option<Pointer>, document: DocId, canvas: Id, xf: &ViewXform, ppp: f32) -> Option<(Pos2, f32, Color32)> {
    let pointer = pointer.filter(|p| settings.enabled && p.document == document && p.canvas != canvas)?;
    if !ppp.is_finite() || ppp <= 0.0 {
        return None;
    }
    let position = xf.to_screen(pointer.position[0] as f32, pointer.position[1] as f32);
    if !position.is_finite() || !xf.rect.contains(position) {
        return None;
    }
    let diameter = if settings.diameter.is_finite() { settings.diameter.clamp(2.0, 20.0) } else { 6.0 };
    Some((position, diameter / (2.0 * ppp), settings.color.paint()))
}

/// Runs after canvas/tool overlays. Read raw pointer hover, never brush/stabilizer state.
pub(crate) fn overlay(app: &mut PhotocraftApp, ui: &egui::Ui, document: DocId, xf: &ViewXform) {
    if !app.ui.view.linked_cursor.enabled {
        return;
    }
    let ctx = ui.ctx();
    let viewport = ctx.viewport_id();
    let canvas = Id::unique(("linked-cursor", viewport, document.0, ui.scope_id()));
    let owns = app.ui.linked_cursor_pointer.is_some_and(|p| p.canvas == canvas);
    let (moved, gone, lost, focused, position) = ctx.input(|i| {
        (
            i.events.iter().any(|e| matches!(e, egui::Event::PointerMoved(_))),
            i.events.iter().any(|e| matches!(e, egui::Event::PointerGone)),
            i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(false))),
            i.focused,
            i.pointer.hover_pos(),
        )
    });
    let hovering = !egui::Popup::is_any_open(ctx) && ui.rect_contains_pointer(xf.rect);
    if owns && (gone || lost || !hovering || position.is_none_or(|p| !xf.rect.contains(p))) {
        set_pointer(app, ctx, None);
    } else if (moved || owns)
        && hovering
        && let Some(position) = position.filter(|p| xf.rect.contains(*p))
    {
        let position = xf.to_doc(position);
        if position.iter().all(|p| p.is_finite()) {
            set_pointer(app, ctx, Some(Pointer { document, viewport, canvas, position, was_focused: focused }));
        }
    }
    if let Some((position, radius, color)) = projection(app.ui.view.linked_cursor, app.ui.linked_cursor_pointer, document, canvas, xf, ctx.pixels_per_point()) {
        ui.painter().with_clip_rect(xf.rect).circle_filled(position, radius, color);
    }
}

#[cfg(test)]
mod tests;
