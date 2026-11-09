use egui::{CursorIcon, Id, Rect, Sense, Ui, UiBuilder, ViewportId};

use crate::{PhotocraftApp, state::DocWindow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy)]
struct Pane {
    id: u64,
    rect: Option<Rect>,
}

pub(crate) struct DockedGroup {
    panes: Vec<Pane>,
    side: Side,
    outer: f32,
    inner: f32,
}

impl DockedGroup {
    pub(crate) fn contains(&self, id: u64) -> bool {
        self.panes.iter().any(|pane| pane.id == id)
    }
}

pub(crate) fn contains(app: &PhotocraftApp, id: u64) -> bool {
    app.docked_view.as_ref().is_some_and(|group| group.contains(id))
}

fn visible(app: &PhotocraftApp) -> impl Iterator<Item = Pane> + '_ {
    let doc = super::main_canvas_document(app).filter(|doc| app.session.active_index() == Some(*doc) && app.jobs.focus.is_none());
    app.docked_view.iter().flat_map(|group| group.panes.iter().copied()).filter(move |pane| {
        app.ui.windows.iter().any(|w| w.id == pane.id && w.open && Some(w.document) == doc)
    })
}

pub(crate) fn hit(app: &PhotocraftApp, position: egui::Pos2) -> Option<u64> {
    visible(app).find(|pane| pane.rect.is_some_and(|rect| rect.contains(position))).map(|pane| pane.id)
}

pub(crate) fn focused(app: &PhotocraftApp) -> Option<u64> {
    let id = app.ui.focused_window?;
    visible(app).any(|pane| pane.id == id && pane.rect.is_some()).then_some(id)
}

fn can_dock(app: &PhotocraftApp, w: &DocWindow) -> bool {
    w.open && !w.floating && app.docked_view.as_ref().is_none_or(|group| {
        group.contains(w.id)
            || (group.panes.len() < 2 && group.panes.iter().all(|pane| {
                app.ui.windows.iter().any(|other| other.id == pane.id && other.document == w.document && other.open)
            }))
    })
}

pub(super) fn dock(app: &mut PhotocraftApp, ctx: &egui::Context, w: &DocWindow, side: Side) {
    if !can_dock(app, w) {
        return;
    }
    let group = app.docked_view.get_or_insert_with(|| DockedGroup { panes: Vec::new(), side, outer: 0.35, inner: 0.5 });
    if !group.contains(w.id) {
        group.panes.push(Pane { id: w.id, rect: None });
    }
    group.side = side;
    for pane in &mut group.panes {
        pane.rect = None;
    }
    app.ui.focused_window = Some(w.id);
    app.session.set_active(w.document);
    crate::linked_cursor::clear(app, ctx);
    ctx.request_repaint_of(ViewportId::ROOT);
}

pub(super) fn retain(app: &mut PhotocraftApp) {
    if let Some(group) = &mut app.docked_view {
        group.panes.retain(|pane| app.ui.windows.iter().any(|w| w.id == pane.id && w.open));
        if group.panes.is_empty() {
            app.docked_view = None;
        }
    }
}

pub(super) fn detach(app: &mut PhotocraftApp, ctx: &egui::Context, id: u64, close: bool) {
    if let Some(group) = &mut app.docked_view {
        group.panes.retain(|pane| pane.id != id);
        if group.panes.is_empty() {
            app.docked_view = None;
        }
    }
    if let Some(w) = app.ui.windows.iter_mut().find(|w| w.id == id) {
        w.open = !close;
        w.focused = false;
    }
    if app.ui.focused_window == Some(id) {
        app.ui.focused_window = None;
    }
    crate::linked_cursor::clear(app, ctx);
    ctx.request_repaint_of(ViewportId::ROOT);
}

pub(super) fn controls(app: &mut PhotocraftApp, ui: &mut Ui, w: &DocWindow, embedded: bool) {
    let compact = ui.available_width() < 210.0;
    ui.horizontal(|ui| {
        ui.menu_button(if compact { tl!("Dock") } else { tl!("Dock Position") }, |ui| {
            ui.add_enabled_ui(can_dock(app, w), |ui| {
                for (side, label) in [(Side::Left, tl!("Left")), (Side::Right, tl!("Right")), (Side::Top, tl!("Top")), (Side::Bottom, tl!("Bottom"))] {
                    if ui.button(label).clicked() {
                        dock(app, ui.ctx(), w, side);
                        ui.close();
                    }
                }
            });
            ui.separator();
            let mut independent = !w.follow_main_flip;
            if ui.checkbox(&mut independent, tl!("Independent Flip")).changed()
                && let Some(win) = app.ui.windows.iter_mut().find(|win| win.id == w.id)
            {
                win.follow_main_flip = !independent;
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
            let mut settings = !w.follow_main_settings;
            if ui.checkbox(&mut settings, tl!("Independent Settings")).changed() {
                crate::view_visibility::set_follow_main_settings(app, ui.ctx(), w.id, !settings);
            }
        });
        if embedded && ui.button(tl!("Float")).clicked() {
            detach(app, ui.ctx(), w.id, false);
        }
        if ui.button(if compact { "×" } else { tl!("Close View") }).on_hover_text(tl!("Close View")).clicked() {
            detach(app, ui.ctx(), w.id, true);
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn other(self) -> Self {
        if self == Self::X { Self::Y } else { Self::X }
    }

    fn component(self, size: egui::Vec2) -> f32 {
        if self == Self::X { size.x } else { size.y }
    }
}

impl Side {
    fn axis(self) -> Axis {
        if matches!(self, Self::Left | Self::Right) { Axis::X } else { Axis::Y }
    }

    fn first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

/// Returns the ratio-owned pane, divider, and remaining pane.
fn split(rect: Rect, axis: Axis, first: bool, share: f32, minimum: [f32; 2]) -> (Rect, Rect, Rect) {
    let length = axis.component(rect.size()).max(0.0);
    let gap = 6.0_f32.min(length * 0.1);
    let available = length - gap;
    let scale = (available / (minimum[0] + minimum[1])).min(1.0);
    let low = minimum[0] * scale;
    let high = (available - minimum[1] * scale).max(low);
    let owned = (available * share).clamp(low, high);
    let offset = if first { owned } else { available - owned };
    let (mut before, mut divider, mut after) = (rect, rect, rect);
    match axis {
        Axis::X => {
            before.max.x = rect.min.x + offset;
            divider.min.x = before.max.x;
            divider.max.x = divider.min.x + gap;
            after.min.x = divider.max.x;
        }
        Axis::Y => {
            before.max.y = rect.min.y + offset;
            divider.min.y = before.max.y;
            divider.max.y = divider.min.y + gap;
            after.min.y = divider.max.y;
        }
    }
    if first { (before, divider, after) } else { (after, divider, before) }
}

fn resize(ui: &mut Ui, id: Id, rect: Rect, axis: Axis, first: bool, share: &mut f32, minimum: [f32; 2]) -> (Rect, Rect) {
    let (owned, bar, _) = split(rect, axis, first, *share, minimum);
    let cursor = if axis == Axis::X { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical };
    let response = ui.interact(bar, id.with(axis), Sense::drag()).on_hover_cursor(cursor);
    let available = axis.component(rect.size() - bar.size());
    if response.dragged() && available > 0.0 {
        let sign = if first { 1.0 } else { -1.0 };
        let desired = (axis.component(owned.size()) + sign * axis.component(response.drag_delta())) / available;
        let (clamped, _, _) = split(rect, axis, first, desired, minimum);
        *share = axis.component(clamped.size()) / available;
    }
    let (owned, bar, rest) = split(rect, axis, first, *share, minimum);
    ui.painter().rect_filled(bar, 0.0, crate::theme::Tokens::get(ui.ctx()).separator);
    (owned, rest)
}

/// Each pane paints on the document area's ordinary UI layer, clipped to its own rectangle.
pub(super) fn layout(app: &mut PhotocraftApp, ui: &mut Ui, rect: Rect) -> Rect {
    let ids: Vec<_> = visible(app).map(|pane| pane.id).collect();
    let Some(group) = &mut app.docked_view else { return rect };
    for pane in &mut group.panes {
        pane.rect = None;
    }
    if ids.is_empty() || !rect.is_positive() {
        return rect;
    }
    let axis = group.side.axis();
    let minimum = if axis == Axis::X { [160.0, 160.0] } else { [96.0, 120.0] };
    let (secondary, main) = resize(ui, Id::unique("document-split-outer"), rect, axis, group.side.first(), &mut group.outer, minimum);
    let (first, second) = if ids.len() == 2 {
        let inner = axis.other();
        let min = if inner == Axis::X { 140.0 } else { 96.0 };
        resize(ui, Id::unique("document-split-inner"), secondary, inner, true, &mut group.inner, [min; 2])
    } else {
        (secondary, Rect::NOTHING)
    };
    for (id, rect) in ids.into_iter().zip([first, second]) {
        if let Some(pane) = group.panes.iter_mut().find(|pane| pane.id == id) {
            pane.rect = rect.is_positive().then_some(rect);
        }
    }
    let panes = group.panes.clone();
    for pane in panes {
        let Some(rect) = pane.rect else { continue };
        if !visible(app).any(|current| current.id == pane.id && current.rect == Some(rect)) {
            continue;
        }
        let Some(w) = app.ui.windows.iter().find(|w| w.id == pane.id).cloned() else { continue };
        let mut child = ui.new_child(UiBuilder::new().id_salt(("docked-document", w.id)).max_rect(rect));
        child.set_clip_rect(child.clip_rect().intersect(rect));
        controls(app, &mut child, &w, true);
        let canvas = child.available_rect_before_wrap().intersect(rect);
        if canvas.is_positive() && visible(app).any(|current| current.id == w.id && current.rect == Some(rect)) {
            super::tabs::secondary_canvas(app, &mut child, &w, canvas, None);
            if focused(app) == Some(w.id) {
                let accent = crate::theme::Tokens::get(ui.ctx()).accent;
                child.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, accent), egui::StrokeKind::Inside);
            }
        }
    }
    main
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, RawInput, ViewportInfo, pos2};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn docking_split_reserves_non_overlapping_space_on_all_sides() {
        let area = Rect::from_min_max(pos2(12.0, 40.0), pos2(812.0, 440.0));
        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            let axis = side.axis();
            let (secondary, bar, main) = split(area, axis, side.first(), 0.35, [80.0; 2]);
            assert!(area.contains_rect(secondary) && area.contains_rect(main));
            assert!(!secondary.intersect(main).is_positive());
            assert_eq!(axis.component(secondary.size() + bar.size() + main.size()), axis.component(area.size()));
            assert!(axis.component(split(area, axis, side.first(), 0.55, [80.0; 2]).0.size()) > axis.component(secondary.size()));
            let (a, _, b) = split(secondary, axis.other(), true, 0.5, [80.0; 2]);
            assert!(secondary.contains_rect(a) && secondary.contains_rect(b));
            assert!(!a.intersect(b).is_positive());
        }
    }

    #[test]
    fn docking_keeps_view_identity_and_isolates_root_shortcuts_and_menus() {
        let ctx = egui::Context::default();
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": 96, "height": 64, "background": "white"})).unwrap();
        app.sync_views();
        let id = crate::menus::invoke(&mut app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap()["window"].as_u64().unwrap();
        let window = app.ui.windows[0].clone();
        dock(&mut app, &ctx, &window, Side::Right);
        app.docked_view.as_mut().unwrap().panes[0].rect = Some(Rect::from_min_max(pos2(400.0, 0.0), pos2(640.0, 480.0)));
        assert_eq!(focused(&app), Some(id));
        let before = app.session.active().unwrap().clone();
        let main = app.ui.views[0].clone();
        assert!(crate::menus::invoke(&mut app, &ctx, "edit.fill", json!({"color":"#ff0000"})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "select.all", json!({})).is_err());

        let press = |key, modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        let mods = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        let mut input = RawInput { events: vec![Event::ModifiersChanged(mods), press(Key::A, mods), press(Key::Equals, mods)], ..Default::default() };
        input.viewports.insert(ViewportId::ROOT, ViewportInfo { focused: Some(true), ..Default::default() });
        let mut output = ctx.run_ui(input, |ui| crate::shortcuts::handle(&mut app, ui.ctx()));
        output.textures_delta.clear();
        assert!(app.ui.windows[0].view.zoom > window.view.zoom);
        assert_eq!(app.ui.views[0], main);
        assert_eq!(crate::menus::invoke(&mut app, &ctx, "view.flipVertical", json!({})).unwrap(), json!(true));
        assert!(app.ui.windows[0].view.flip_vertical);
        assert!(!app.ui.views[0].flip_vertical);

        let view = app.ui.windows[0].view.clone();
        let visibility = app.ui.windows[0].visibility.clone();
        detach(&mut app, &ctx, id, false);
        assert!(app.docked_view.is_none());
        assert_eq!(app.ui.windows[0].id, id);
        assert_eq!(app.ui.windows[0].view, view);
        assert_eq!(app.ui.windows[0].visibility, visibility);
        dock(&mut app, &ctx, &window, Side::Left);
        detach(&mut app, &ctx, id, true);
        assert!(!app.ui.windows[0].open);
        let after = app.session.active().unwrap();
        assert!(Arc::ptr_eq(&before.doc, &after.doc));
        assert_eq!(before.history.past_len(), after.history.past_len());
    }

    #[test]
    fn docking_gpu_presentation_identity_is_distinct_and_survives_host_change() {
        let key = super::super::gpu_presentation_key;
        let root = ViewportId::ROOT;
        let native = ViewportId::from_hash_of(("docwin", 7_u64));
        assert_eq!(key(root, 0, Some(7), false), key(native, 0, Some(7), false));
        assert_ne!(key(root, 0, None, false), key(root, 0, Some(7), false));
        assert_ne!(key(root, 0, Some(8), false), key(root, 0, Some(7), false));
    }
}

#[cfg(test)]
mod tests_pane {
    use super::*;
    use serde_json::json;

    #[test]
    fn docking_layout_clips_the_secondary_and_keeps_main_editable() {
        let ctx = egui::Context::default();
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": 96, "height": 64, "background": "white"})).unwrap();
        app.sync_views();
        crate::menus::invoke(&mut app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap();
        let window = app.ui.windows[0].clone();
        dock(&mut app, &ctx, &window, Side::Left);
        app.ui.focused_window = None;
        crate::menus::invoke(&mut app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap();
        let other = app.ui.windows[1].clone();
        dock(&mut app, &ctx, &other, Side::Left);
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(580.0, 440.0)).with_pixels_per_point(2.0).build_ui_state(
            |ui, app| {
                if ui.ctx().fonts(|f| f.families().contains(&egui::FontFamily::Name("medium".into()))) {
                    crate::canvas::document_area(app, ui);
                }
            },
            app,
        );
        PhotocraftApp::setup_context(&h.ctx, crate::theme::ThemeKind::ALL[0]);
        h.run_steps(2);
        let drag = |h: &mut egui_kittest::Harness<'_, PhotocraftApp>, p, delta| {
            use egui::{Event, Modifiers, PointerButton};
            h.event(Event::PointerMoved(p));
            h.step();
            h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            h.step();
            h.event(Event::PointerMoved(p + delta));
            h.step();
            h.event(Event::PointerButton { pos: p + delta, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            h.run_steps(2);
        };
        let geometry = |app: &PhotocraftApp| {
            let group = app.docked_view.as_ref().unwrap();
            (app.last_canvas_rect, group.panes[0].rect.unwrap(), group.panes[1].rect.unwrap())
        };
        let revision = h.state().session.active().unwrap().revision;
        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            dock(h.state_mut(), &ctx, &window, side);
            h.run_steps(2);
            let (main, a, b) = geometry(h.state());
            assert!(main.is_positive() && a.is_positive() && b.is_positive());
            assert_eq!(hit(h.state(), a.center()), Some(window.id));
            assert_eq!(hit(h.state(), b.center()), Some(other.id));
            assert_eq!(hit(h.state(), main.center()), None);
            assert_eq!(h.state().drop_canvas_rect, Some(main));
            assert!(!a.intersect(main).is_positive() && !b.intersect(main).is_positive() && !a.intersect(b).is_positive());

            let axis = side.axis();
            let group = a.union(b);
            let outer_at = if axis == Axis::X {
                egui::pos2(if side.first() { (group.right() + main.left()) / 2.0 } else { (main.right() + group.left()) / 2.0 }, group.center().y)
            } else {
                egui::pos2(group.center().x, if side.first() { (group.bottom() + main.top()) / 2.0 } else { (main.bottom() + group.top()) / 2.0 })
            };
            let delta = if axis == Axis::X { egui::vec2(24.0, 0.0) } else { egui::vec2(0.0, 24.0) };
            let delta = if side.first() { delta } else { -delta };
            let inner_before = h.state().docked_view.as_ref().unwrap().inner;
            drag(&mut h, outer_at, delta);
            let (main_after, a, b) = geometry(h.state());
            assert!(axis.component(main_after.size()) < axis.component(main.size()), "{side:?}");
            assert_eq!(h.state().docked_view.as_ref().unwrap().inner, inner_before);

            let inner_at = if axis == Axis::X {
                egui::pos2(a.center().x, (a.bottom() + b.top()) / 2.0)
            } else {
                egui::pos2((a.right() + b.left()) / 2.0, a.center().y)
            };
            assert_eq!(hit(h.state(), inner_at), None);
            drag(&mut h, inner_at, if axis == Axis::X { egui::vec2(0.0, 24.0) } else { egui::vec2(24.0, 0.0) });
            let (main_after_inner, a_after, _) = geometry(h.state());
            assert_eq!(main_after_inner, main_after);
            assert!(axis.other().component(a_after.size()) > axis.other().component(a.size()), "{side:?}");
        }
        assert_eq!(h.state().session.active().unwrap().revision, revision);
        assert!(h.state().drag.is_none());
        let view = h.state().ui.windows[1].view.clone();
        detach(h.state_mut(), &ctx, other.id, false);
        assert_eq!(h.state().ui.windows[1].view, view);
        assert!(contains(h.state(), window.id));
        assert!(!contains(h.state(), other.id));
    }
}
