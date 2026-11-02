use super::*;
use egui::{Event, Modifiers, PointerButton, RawInput, Rect, ViewportInfo, pos2, vec2};
use std::sync::Arc;

fn app() -> PhotocraftApp {
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
    app.run("file.new", json!({"width": 1000, "height": 800, "background": "white"})).unwrap();
    app.sync_views();
    app.ui.view.linked_cursor.enabled = true;
    app
}

fn camera(flip: bool, rotation: f32, zoom: f32, center: [f32; 2], size: [f32; 2]) -> ViewXform {
    ViewXform { rect: Rect::from_min_size(pos2(20.0, 40.0), vec2(size[0], size[1])), zoom, center, flip, rotation, aspect: 1.0 }
}

fn sample(doc: DocId, viewport: ViewportId, canvas: Id, position: [f64; 2]) -> Pointer {
    Pointer { document: doc, viewport, canvas, position, was_focused: true }
}

fn pass(app: &mut PhotocraftApp, ctx: &Context, viewport: ViewportId, xf: &ViewXform, events: Vec<Event>) -> Vec<egui::epaint::ClippedShape> {
    let mut raw = RawInput { viewport_id: viewport, events, ..Default::default() };
    raw.viewports.insert(viewport, ViewportInfo { focused: Some(true), ..Default::default() });
    let doc = app.session.active().unwrap().doc.id;
    let mut out = ctx.run_ui(raw, |ui| {
        ui.allocate_rect(xf.rect, egui::Sense::hover());
        overlay(app, ui, doc, xf);
    });
    out.textures_delta.clear();
    out.shapes
}

fn circles(shapes: &[egui::epaint::ClippedShape]) -> Vec<&egui::epaint::CircleShape> {
    shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Circle(c) => Some(c),
            _ => None,
        })
        .collect()
}

#[test]
fn linked_cursor_defaults_persist_settings_but_not_pointer() {
    let mut app = app();
    let defaults = Settings::default();
    assert!(!defaults.enabled);
    assert_eq!((defaults.diameter, defaults.color), (6.0, Color::Red));
    let doc = app.session.active().unwrap().doc.id;
    app.ui.linked_cursor_pointer = Some(sample(doc, ViewportId::ROOT, Id::unique("source"), [1.0, 2.0]));
    app.ui.view.linked_cursor = Settings { enabled: true, diameter: 12.0, color: Color::White };
    let saved = serde_json::to_value(&app.ui).unwrap();
    assert!(saved.get("linked_cursor_pointer").is_none());
    let restored: crate::UiState = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(restored.view.linked_cursor, app.ui.view.linked_cursor);
    assert!(restored.linked_cursor_pointer.is_none());
    let mut old = saved;
    old["view"].as_object_mut().unwrap().remove("linked_cursor");
    let old: crate::UiState = serde_json::from_value(old).unwrap();
    assert_eq!(old.view.linked_cursor, defaults);
}

#[test]
fn linked_cursor_projects_both_directions_with_independent_cameras_and_dpi() {
    let app = app();
    let doc = app.session.active().unwrap().doc.id;
    for source_ppp in [1.0, 2.0] {
        for target_ppp in [1.0, 1.5, 2.0] {
            for flip_a in [false, true] {
                for flip_b in [false, true] {
                    for rotation in [0.0, 37.0, 90.0, -25.0] {
                        let a = camera(flip_a, rotation, 1.25 / source_ppp, [510.0, 390.0], [800.0, 600.0]);
                        let b = camera(flip_b, -rotation, 0.75 / target_ppp, [480.0, 420.0], [600.0, 400.0]);
                        for (source, receiver, ppp) in [(a, b, target_ppp), (b, a, source_ppp)] {
                            let document_point = [525.0, 405.0];
                            let actual_mouse = source.to_screen(document_point[0], document_point[1]);
                            let pointer = sample(doc, ViewportId::ROOT, Id::unique("source"), source.to_doc(actual_mouse));
                            let (screen, radius, color) =
                                projection(app.ui.view.linked_cursor, Some(pointer), doc, Id::unique("receiver"), &receiver, ppp).unwrap();
                            assert!(screen.distance(receiver.to_screen(document_point[0], document_point[1])) < 0.001);
                            assert!((radius * 2.0 * ppp - 6.0).abs() < 0.001);
                            assert_eq!(color, Color32::RED);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn linked_cursor_clips_and_filters_document_and_origin() {
    let mut app = app();
    let doc = app.session.active().unwrap().doc.id;
    let xf = camera(false, 0.0, 1.0, [500.0, 400.0], [800.0, 600.0]);
    let source = Id::unique("source");
    let target = Id::unique("target");
    let p = Some(sample(doc, ViewportId::ROOT, source, [500.0, 400.0]));
    let settings = app.ui.view.linked_cursor;
    assert!(projection(settings, p, doc, source, &xf, 1.0).is_none());
    assert!(projection(settings, p, DocId(doc.0 + 1), target, &xf, 1.0).is_none());
    assert!(projection(settings, p, doc, target, &xf, 0.0).is_none());
    assert!(projection(settings, Some(sample(doc, ViewportId::ROOT, source, [2000.0, 400.0])), doc, target, &xf, 1.0).is_none());
    app.ui.view.linked_cursor.enabled = false;
    assert!(projection(app.ui.view.linked_cursor, p, doc, target, &xf, 1.0).is_none());
}

#[test]
fn linked_cursor_menu_controls_validate_and_do_not_edit_artwork() {
    let mut app = app();
    let ctx = Context::default();
    let before = app.session.active().unwrap().clone();
    app.ui.view.linked_cursor.enabled = false;
    for (id, params) in [("view.linkedCursor", json!({})), ("view.linkedCursor.size.20", json!({})), ("view.linkedCursor.color.black", json!({}))] {
        assert!(crate::menus::invoke(&mut app, &ctx, id, params).is_ok());
        assert_eq!(crate::view_cmds::checked(&app, id), Some(true));
    }
    let settings = app.ui.view.linked_cursor;
    for params in [json!({"diameter": 0}), json!({"diameter": 21}), json!({"diameter": "bad"}), json!({"color": "blue"}), json!({"color": 7}), json!({"on": 3})]
    {
        assert!(crate::menus::invoke(&mut app, &ctx, "view.linkedCursor", params).is_err());
        assert_eq!(app.ui.view.linked_cursor, settings);
    }
    let doc = app.session.active().unwrap().doc.id;
    app.ui.linked_cursor_pointer = Some(sample(doc, ViewportId::ROOT, Id::unique("source"), [1.0, 2.0]));
    crate::menus::invoke(&mut app, &ctx, "view.linkedCursor", json!({"on": false})).unwrap();
    assert!(app.ui.linked_cursor_pointer.is_none());
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.history.past_len(), after.history.past_len());
    assert_eq!(before.is_dirty(), after.is_dirty());
}

#[test]
fn linked_cursor_raw_pointer_handoff_and_leave_draw_only_in_other_views() {
    let mut app = app();
    let a = Context::default();
    let b = Context::default();
    let child = ViewportId::from_hash_of(("docwin", 10));
    let xf = camera(false, 0.0, 1.0, [500.0, 400.0], [800.0, 600.0]);
    let mirrored = camera(true, 20.0, 0.5, [480.0, 410.0], [600.0, 400.0]);
    pass(&mut app, &a, ViewportId::ROOT, &xf, vec![]);
    pass(&mut app, &b, child, &mirrored, vec![]);
    let source = pass(&mut app, &a, ViewportId::ROOT, &xf, vec![Event::PointerMoved(xf.to_screen(520.0, 405.0))]);
    assert!(app.ui.linked_cursor_pointer.is_some());
    assert!(circles(&source).is_empty(), "the source keeps its own ordinary cursor");
    let receiver = pass(&mut app, &b, child, &mirrored, vec![]);
    let dots = circles(&receiver);
    assert_eq!(dots.len(), 1);
    assert!(dots[0].center.distance(mirrored.to_screen(520.0, 405.0)) < 0.001);
    // The other window becomes source from its actual raw pointer, with no paint event.
    pass(&mut app, &a, ViewportId::ROOT, &xf, vec![Event::PointerGone]);
    assert!(app.ui.linked_cursor_pointer.is_none());
    pass(&mut app, &b, child, &mirrored, vec![Event::PointerMoved(mirrored.to_screen(490.0, 415.0))]);
    let receiver = pass(&mut app, &a, ViewportId::ROOT, &xf, vec![]);
    assert_eq!(circles(&receiver).len(), 1);
    assert!(circles(&receiver)[0].center.distance(xf.to_screen(490.0, 415.0)) < 0.001);
    pass(&mut app, &b, child, &mirrored, vec![Event::PointerMoved(pos2(0.0, 0.0))]);
    assert!(app.ui.linked_cursor_pointer.is_none());
}

#[test]
fn linked_cursor_multiple_receivers_and_closed_source_cleanup() {
    let mut app = app();
    let ctx = Context::default();
    let first = crate::menus::invoke(&mut app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap()["window"].as_u64().unwrap();
    crate::menus::invoke(&mut app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap();
    let doc = app.session.active().unwrap().doc.id;
    let viewport = ViewportId::from_hash_of(("docwin", first));
    let xf = camera(false, 0.0, 1.0, [500.0, 400.0], [800.0, 600.0]);
    app.ui.linked_cursor_pointer = Some(sample(doc, viewport, Id::unique("source"), [500.0, 400.0]));
    for target in [Id::unique("root"), Id::unique("second-child"), Id::unique("third-child")] {
        assert!(projection(app.ui.view.linked_cursor, app.ui.linked_cursor_pointer, doc, target, &xf, 1.0).is_some());
    }
    app.ui.windows[0].open = false;
    prune(&mut app, &ctx);
    assert!(app.ui.linked_cursor_pointer.is_none());
    app.ui.linked_cursor_pointer = Some(sample(doc, ViewportId::ROOT, Id::unique("root"), [500.0, 400.0]));
    app.run("file.new", json!({"width": 64, "height": 64})).unwrap();
    app.sync_views();
    prune(&mut app, &ctx);
    assert!(app.ui.linked_cursor_pointer.is_none(), "switching away from a source tab clears it");
}

#[test]
fn linked_cursor_motion_is_overlay_only_and_stationary_pointer_is_unchanged() {
    let mut app = app();
    let before = app.session.active().unwrap().clone();
    let cameras = app.ui.views.clone();
    let ctx = Context::default();
    let xf = camera(false, 0.0, 1.0, [500.0, 400.0], [800.0, 600.0]);
    pass(&mut app, &ctx, ViewportId::ROOT, &xf, vec![]);
    let point = xf.to_screen(520.0, 410.0);
    pass(&mut app, &ctx, ViewportId::ROOT, &xf, vec![Event::PointerMoved(point)]);
    let pointer = app.ui.linked_cursor_pointer;
    assert!(pointer.is_some());
    pass(&mut app, &ctx, ViewportId::ROOT, &xf, vec![]);
    assert_eq!(app.ui.linked_cursor_pointer, pointer);
    // The overlay does not consume pointer buttons, tool input, or keyboard input.
    let mut out = ctx.run_ui(
        RawInput {
            events: vec![Event::PointerButton { pos: point, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }],
            ..Default::default()
        },
        |ui| {
            ui.allocate_rect(xf.rect, egui::Sense::hover());
            overlay(&mut app, ui, before.doc.id, &xf);
            assert!(ui.input(|i| i.events.iter().any(|e| matches!(e, Event::PointerButton { pressed: true, .. }))));
        },
    );
    out.textures_delta.clear();
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.history.past_len(), after.history.past_len());
    assert!(app.canvases.is_empty());
    assert_eq!(app.ui.views, cameras);
}

#[test]
fn linked_cursor_colors_and_diameter_are_physical_pixels() {
    let app = app();
    let doc = app.session.active().unwrap().doc.id;
    let pointer = Some(sample(doc, ViewportId::ROOT, Id::unique("source"), [500.0, 400.0]));
    for (color, expected) in [(Color::Red, Color32::RED), (Color::White, Color32::WHITE), (Color::Black, Color32::BLACK)] {
        for diameter in [2.0, 6.0, 20.0] {
            for zoom in [0.1, 1.0, 20.0] {
                for ppp in [1.0, 1.5, 2.0] {
                    let xf = camera(false, 0.0, zoom / ppp, [500.0, 400.0], [600.0, 400.0]);
                    let (_, radius, paint) = projection(Settings { enabled: true, diameter, color }, pointer, doc, Id::unique("receiver"), &xf, ppp).unwrap();
                    assert!((radius * 2.0 * ppp - diameter).abs() < 0.001);
                    assert_eq!(paint, expected);
                }
            }
        }
    }
}

#[test]
fn linked_cursor_focus_loss_clears_source_and_closing_receiver_preserves_source() {
    let mut app = app();
    let ctx = Context::default();
    let xf = camera(false, 0.0, 1.0, [500.0, 400.0], [800.0, 600.0]);
    pass(&mut app, &ctx, ViewportId::ROOT, &xf, vec![]);
    pass(&mut app, &ctx, ViewportId::ROOT, &xf, vec![Event::PointerMoved(xf.rect.center())]);
    assert!(app.ui.linked_cursor_pointer.is_some());
    pass(&mut app, &ctx, ViewportId::ROOT, &xf, vec![Event::WindowFocused(false)]);
    assert!(app.ui.linked_cursor_pointer.is_none());
    crate::menus::invoke(&mut app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap();
    let doc = app.session.active().unwrap().doc.id;
    app.ui.linked_cursor_pointer = Some(sample(doc, ViewportId::ROOT, Id::unique("root"), [500.0, 400.0]));
    app.ui.windows[0].open = false;
    let before = app.ui.linked_cursor_pointer;
    prune(&mut app, &Context::default());
    assert_eq!(app.ui.linked_cursor_pointer, before);
}

#[test]
fn linked_cursor_stationary_sample_does_not_request_more_repaints() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut app = app();
    let ctx = Context::default();
    crate::menus::invoke(&mut app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = requests.clone();
    ctx.set_request_repaint_callback(move |_| {
        observed.fetch_add(1, Ordering::Relaxed);
    });
    let doc = app.session.active().unwrap().doc.id;
    let pointer = Some(sample(doc, ViewportId::ROOT, Id::unique("root"), [500.0, 400.0]));
    set_pointer(&mut app, &ctx, pointer);
    let first = requests.load(Ordering::Relaxed);
    for _ in 0..20 {
        set_pointer(&mut app, &ctx, pointer);
    }
    assert_eq!(requests.load(Ordering::Relaxed), first);
}

#[test]
fn linked_cursor_canvas_motion_reuses_existing_composite() {
    let mut app = app();
    app.ui.views[0].fit_pending = false;
    app.ui.views[0].center = [500.0, 400.0];
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(800.0, 600.0)).build_ui_state(
        |ui, app: &mut PhotocraftApp| {
            let view = app.ui.views[0].clone();
            crate::canvas::canvas_view(app, ui, 0, ui.max_rect(), view, true);
        },
        app,
    );
    harness.run_steps(2);
    assert!(!harness.state().canvases.is_empty());
    let textures: Vec<_> = harness.state().canvases.iter().map(|(key, c)| (*key, c.tex_revision, c.texture.as_ref().map(|t| t.id()))).collect();
    harness.state_mut().perf.composite_ms = -123.0;
    for step in 0..5 {
        harness.event(Event::PointerMoved(pos2(300.0 + step as f32 * 5.0, 250.0)));
        harness.step();
    }
    assert_eq!(harness.state().perf.composite_ms, -123.0, "pointer movement must not composite again");
    for (key, revision, texture) in textures {
        let current = &harness.state().canvases[&key];
        assert_eq!(current.tex_revision, revision);
        assert_eq!(current.texture.as_ref().map(|t| t.id()), texture);
    }
}

#[test]
fn linked_cursor_real_view_menu_toggles_and_catalog_exposes_settings() {
    use egui_kittest::{Harness, kittest::Queryable};
    let mut app = app();
    app.ui.view.linked_cursor.enabled = false;
    let mut harness = Harness::builder().with_size(vec2(1200.0, 700.0)).build_ui_state(
        |ui, app| {
            crate::menus::menu_bar(app, ui);
        },
        app,
    );
    PhotocraftApp::setup_context(&harness.ctx, crate::theme::ThemeKind::ALL[0]);
    harness.run_steps(3);
    harness.get_by_label("View").click();
    harness.run_steps(3);
    harness.get_by_label("Linked Cursor").click();
    harness.run_steps(3);
    assert!(harness.state().ui.view.linked_cursor.enabled);
    let items = crate::menus::menu_items(harness.state());
    for id in ["view.linkedCursor", "view.linkedCursor.size.2", "view.linkedCursor.size.6", "view.linkedCursor.size.20", "view.linkedCursor.color.white"] {
        assert!(items.iter().any(|item| item.id == id && item.enabled), "{id}");
    }
}

#[test]
fn linked_cursor_enabled_preserves_real_brush_strokes_and_one_undo_step() {
    let mut app = app();
    app.ui.tool = crate::state::Tool::Brush;
    app.ui.views[0].fit_pending = false;
    app.ui.views[0].center = [500.0, 400.0];
    app.run("tools.setBrush", json!({"brush": {"size": 10, "hardness": 1.0}, "smoothing": {"amount": 0.0}})).unwrap();
    let history = app.session.active().unwrap().history.past_len();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(800.0, 600.0)).build_ui_state(
        |ui, app: &mut PhotocraftApp| {
            let view = app.ui.views[0].clone();
            crate::canvas::canvas_view(app, ui, 0, ui.max_rect(), view, true);
        },
        app,
    );
    h.run_steps(2);
    let p = pos2(400.0, 300.0);
    h.event(Event::PointerMoved(p));
    h.step();
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerMoved(p + vec2(15.0, 0.0)));
    h.step();
    h.event(Event::PointerButton { pos: p + vec2(15.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let doc = h.state().session.active().unwrap();
    assert_eq!(doc.history.past_len(), history + 1);
    let layer = doc.doc.layer(doc.active_layer.unwrap()).unwrap();
    assert!(layer.surface().unwrap().rgba(500, 400)[0] < 0.9, "the normal black brush paints with Linked Cursor on");
    assert!(h.state().ui.linked_cursor_pointer.is_some());
}
