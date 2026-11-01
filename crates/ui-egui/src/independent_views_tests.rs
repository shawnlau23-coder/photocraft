//! Independent cameras share artwork, never presentation state or view command targets.
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, ViewportId, ViewportInfo, pos2, vec2};
use serde_json::json;
use std::sync::Arc;

use crate::{PhotocraftApp, Services, canvas::ViewXform, menus};

fn app() -> (PhotocraftApp, egui::Context) {
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    app.run("file.new", json!({"width": 64, "height": 48, "background": "white"})).unwrap();
    app.sync_views();
    app.ui.views[0].zoom = 1.0;
    app.ui.views[0].center = [32.0, 24.0];
    app.ui.views[0].fit_pending = false;
    app.last_canvas_rect = Rect::from_min_size(pos2(0.0, 50.0), vec2(800.0, 600.0));
    (app, egui::Context::default())
}

fn window(app: &mut PhotocraftApp, ctx: &egui::Context) -> u64 {
    menus::invoke(app, ctx, "window.arrange.newWindowForDocument", json!({})).unwrap()["window"].as_u64().unwrap()
}

fn flip(app: &mut PhotocraftApp, ctx: &egui::Context) -> bool {
    menus::invoke(app, ctx, "view.flipHorizontal", json!({})).unwrap().as_bool().unwrap()
}

#[test]
fn independent_flip_commands_target_one_window_and_leave_artwork_and_history_untouched() {
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    let before = app.session.active().unwrap().clone();
    let history = serde_json::to_value(before.history.entries()).unwrap();
    app.ui.focused_window = Some(a);
    assert!(flip(&mut app, &ctx));
    assert!(!app.ui.views[0].flip_horizontal);
    assert!(app.ui.windows[0].view.flip_horizontal);
    assert!(!app.ui.windows[1].view.flip_horizontal);
    assert_eq!(crate::view_cmds::checked(&app, "view.flipHorizontal"), Some(true));
    app.ui.focused_window = Some(b);
    assert_eq!(crate::view_cmds::checked(&app, "view.flipHorizontal"), Some(false));
    assert!(flip(&mut app, &ctx));
    app.ui.focused_window = Some(a);
    assert!(!flip(&mut app, &ctx));
    assert!(app.ui.windows[1].view.flip_horizontal);
    app.ui.focused_window = None;
    assert!(flip(&mut app, &ctx));
    assert!(!app.ui.windows[0].view.flip_horizontal);
    assert!(app.ui.windows[1].view.flip_horizontal);
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(before.doc, after.doc);
    assert_eq!((before.revision, before.saved_revision, before.last_damage), (after.revision, after.saved_revision, after.last_damage));
    assert_eq!(history, serde_json::to_value(after.history.entries()).unwrap());
    assert_eq!(
        (before.history.can_undo(), before.history.can_redo(), before.is_dirty()),
        (after.history.can_undo(), after.history.can_redo(), after.is_dirty())
    );
}

#[test]
fn independent_flip_single_window_and_explicit_on_parameter_keep_the_existing_command() {
    let (mut app, ctx) = app();
    assert!(flip(&mut app, &ctx));
    assert!(app.ui.views[0].flip_horizontal);
    assert_eq!(menus::invoke(&mut app, &ctx, "view.flipHorizontal", json!({"on": true})).unwrap(), json!(true));
    assert_eq!(menus::invoke(&mut app, &ctx, "view.flipHorizontal", json!({"on": false})).unwrap(), json!(false));
    assert!(!app.ui.views[0].flip_horizontal);
}

#[test]
fn independent_flip_new_windows_inherit_the_source_camera_but_own_their_copy() {
    let (mut app, ctx) = app();
    app.ui.views[0].flip_horizontal = true;
    let a = window(&mut app, &ctx);
    app.ui.focused_window = Some(a);
    app.ui.windows[0].view.zoom = 2.0;
    app.ui.windows[0].view.center = [9.0, 12.0];
    app.ui.windows[0].view.rotation = 37.0;
    let b = window(&mut app, &ctx);
    assert_eq!(app.ui.windows[0].view, app.ui.windows[1].view);
    app.ui.focused_window = Some(b);
    assert!(!flip(&mut app, &ctx));
    assert!(app.ui.windows[0].view.flip_horizontal);
    assert!(app.ui.views[0].flip_horizontal);
}

#[test]
fn independent_flip_secondary_camera_writeback_keeps_main_zoom_pan_rotation_and_flip() {
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    let main = app.ui.views[0].clone();
    let other = app.ui.windows[1].view.clone();
    let rect = app.last_canvas_rect;
    let ppp = app.canvas_ppp();
    app.with_document_window(a, |app| {
        let view = &mut app.ui.views[0];
        view.zoom = 3.0;
        view.center = [11.0, 17.0];
        view.rotation = 42.0;
        view.flip_horizontal = true;
        app.last_canvas_rect = Rect::from_min_size(Pos2::ZERO, vec2(300.0, 200.0));
        app.ppp = 2.0;
        assert!(app.canvas_flip_horizontal());
    })
    .unwrap();
    assert_eq!(app.ui.views[0], main);
    assert_eq!(app.ui.windows.iter().find(|w| w.id == b).unwrap().view, other);
    let child = &app.ui.windows[0].view;
    assert_eq!((child.zoom, child.center, child.rotation, child.flip_horizontal), (3.0, [11.0, 17.0], 42.0, true));
    assert_eq!(app.last_canvas_rect, rect);
    assert_eq!(app.canvas_ppp(), ppp);
    assert!(!app.canvas_flip_horizontal());
}

#[test]
fn independent_flip_child_inverse_mapping_and_brush_axes_follow_its_own_camera() {
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    app.ui.windows[0].view.flip_horizontal = true;
    app.ui.windows[0].view.zoom = 2.0;
    app.ui.windows[0].view.center = [20.0, 16.0];
    app.ui.windows[0].view.rotation = 31.0;
    let main = ViewXform::active(&app).unwrap();
    let original = app.ui.views[0].clone();
    app.with_document_window(a, |app| {
        let xf = ViewXform::active(app).unwrap();
        assert!(xf.flip);
        let p = [12.0, 9.0];
        let screen = xf.to_screen(p[0], p[1]);
        let got = xf.to_doc(screen);
        assert!((got[0] - f64::from(p[0])).abs() < 1e-4);
        assert!((got[1] - f64::from(p[1])).abs() < 1e-4);
        let axes = crate::brush_cursor::tip_axes(&xf, 10.0);
        assert_eq!(axes[0], xf.map_vec(vec2(5.0, 0.0)));
        assert!(axes[0].x < 0.0, "brush outline reflects with the child view");
        assert_ne!(screen, main.to_screen(p[0], p[1]));
    })
    .unwrap();
    assert_eq!(app.ui.views[0], original);
    assert!(!ViewXform::active(&app).unwrap().flip);
}

fn focus_pass(app: &mut PhotocraftApp, ctx: &egui::Context, raw: RawInput) {
    ctx.begin_pass(raw);
    app.update_view_focus(ctx);
    ctx.end_pass().textures_delta.clear();
}

#[test]
fn independent_flip_native_focus_is_resolved_before_command_dispatch() {
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    let mut raw = RawInput::default();
    raw.viewports.get_mut(&ViewportId::ROOT).unwrap().focused = Some(false);
    raw.viewports.insert(ViewportId::from_hash_of(("docwin", a)), ViewportInfo { focused: Some(true), ..Default::default() });
    focus_pass(&mut app, &ctx, raw);
    assert_eq!(app.ui.focused_window, Some(a));
    assert!(flip(&mut app, &ctx));
    assert!(!app.ui.views[0].flip_horizontal);
}

#[test]
fn independent_flip_root_menu_focus_keeps_child_target_but_main_canvas_click_selects_main() {
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    app.ui.focused_window = Some(a);
    let mut raw = RawInput::default();
    raw.viewports.get_mut(&ViewportId::ROOT).unwrap().focused = Some(true);
    raw.events = vec![
        Event::WindowFocused(true),
        Event::PointerMoved(pos2(20.0, 10.0)),
        Event::PointerButton { pos: pos2(20.0, 10.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ];
    focus_pass(&mut app, &ctx, raw);
    assert_eq!(app.ui.focused_window, Some(a));
    let mut raw = RawInput::default();
    raw.viewports.get_mut(&ViewportId::ROOT).unwrap().focused = Some(true);
    raw.events = vec![
        Event::PointerButton { pos: pos2(20.0, 10.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        Event::PointerMoved(pos2(40.0, 100.0)),
        Event::PointerButton { pos: pos2(40.0, 100.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ];
    focus_pass(&mut app, &ctx, raw);
    assert_eq!(app.ui.focused_window, None);
    assert!(flip(&mut app, &ctx));
    assert!(!app.ui.windows[0].view.flip_horizontal);
}

#[test]
fn independent_flip_closed_window_falls_back_to_main_and_old_state_defaults_unflipped() {
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    app.ui.focused_window = Some(a);
    app.ui.windows[0].open = false;
    assert!(flip(&mut app, &ctx));
    assert!(app.ui.views[0].flip_horizontal);
    let mut saved = serde_json::to_value(&app.ui).unwrap();
    saved["views"][0].as_object_mut().unwrap().remove("flip_horizontal");
    saved["view"]["flip_horizontal"] = json!(true); // Legacy shared field is ignored.
    let restored: crate::UiState = serde_json::from_value(saved).unwrap();
    assert!(!restored.views[0].flip_horizontal);
    assert_eq!(restored.focused_window, None);
}

#[test]
fn independent_flip_menu_cache_changes_with_focused_camera_and_serializes_each_view() {
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    let before = crate::native_menu::state_hash(&app, None);
    app.ui.focused_window = Some(a);
    assert!(flip(&mut app, &ctx));
    assert_ne!(before, crate::native_menu::state_hash(&app, None));
    let saved = serde_json::to_value(&app.ui).unwrap();
    let restored: crate::UiState = serde_json::from_value(saved).unwrap();
    assert!(!restored.views[0].flip_horizontal);
    assert!(restored.windows[0].view.flip_horizontal);
    assert_eq!(restored.focused_window, None);
}

#[test]
fn independent_flip_real_view_menu_preserves_secondary_target() {
    use egui_kittest::{Harness, kittest::Queryable};
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    app.ui.focused_window = Some(a);
    let mut harness = Harness::builder().with_size(vec2(1200.0, 700.0)).build_ui_state(
        |ui, app: &mut PhotocraftApp| {
            if !ui.ctx().fonts(|f| f.families().contains(&egui::FontFamily::Name("medium".into()))) {
                return;
            }
            app.update_view_focus(ui.ctx());
            menus::menu_bar(app, ui);
            crate::canvas::document_area(app, ui);
        },
        app,
    );
    PhotocraftApp::setup_context(&harness.ctx, crate::theme::ThemeKind::ALL[0]);
    harness.run_steps(3);
    harness.get_by_label("View").click();
    harness.run_steps(3);
    harness.get_by_label("Flip Horizontal").click();
    harness.run_steps(3);
    assert_eq!(harness.state().ui.focused_window, Some(a));
    assert!(harness.state().ui.windows[0].view.flip_horizontal);
    assert!(!harness.state().ui.views[0].flip_horizontal);
}

#[test]
fn independent_flip_control_focus_validates_ids_and_targets_the_window() {
    use crate::control::{ControlRequest, Outcome};
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    let (request, _rx) = ControlRequest::new("ui.focus", json!({"window": a}));
    assert!(matches!(crate::control::handle(&mut app, &ctx, &request), Outcome::Done(v) if v["ok"] == true));
    assert!(flip(&mut app, &ctx));
    assert!(!app.ui.views[0].flip_horizontal);
    for bad in [json!({"window": -1}), json!({"window": "bad"}), json!({"window": a + 99})] {
        let (request, _rx) = ControlRequest::new("ui.focus", bad);
        assert!(matches!(crate::control::handle(&mut app, &ctx, &request), Outcome::Done(v) if v["ok"] == false));
        assert_eq!(app.ui.focused_window, Some(a));
    }
    let (request, _rx) = ControlRequest::new("ui.focus", json!({}));
    assert!(matches!(crate::control::handle(&mut app, &ctx, &request), Outcome::Done(v) if v["ok"] == true));
    assert_eq!(app.ui.focused_window, None);
}

#[test]
fn independent_flip_child_canvas_pan_and_zoom_do_not_change_the_main_camera() {
    let (mut app, ctx) = app();
    app.run("prefs.set", json!({"values": {"general.animatedZoom": false, "tools.overscroll": true}})).unwrap();
    app.ui.tool = crate::state::Tool::Hand;
    let a = window(&mut app, &ctx);
    app.ui.windows[0].view.flip_horizontal = true;
    let main = app.ui.views[0].clone();
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(600.0, 400.0)).build_ui_state(
        move |ui, app: &mut PhotocraftApp| {
            app.with_document_window(a, |app| {
                let rect = ui.max_rect();
                app.last_canvas_rect = rect;
                app.ppp = ui.ctx().pixels_per_point();
                let camera = app.ui.views[0].clone();
                crate::canvas::canvas_view(app, ui, 0, rect, camera, true);
            });
        },
        app,
    );
    harness.run_steps(2);
    let p = pos2(250.0, 180.0);
    harness.event(Event::PointerMoved(p));
    harness.step();
    harness.event(Event::Zoom(1.25));
    harness.step();
    assert!(harness.state().ui.windows[0].view.zoom > main.zoom);
    let before = harness.state().ui.windows[0].view.center;
    harness.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    harness.step();
    let end = p + vec2(40.0, 20.0);
    harness.event(Event::PointerMoved(end));
    harness.step();
    harness.event(Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    harness.step();
    let child = &harness.state().ui.windows[0].view;
    assert_ne!(child.center, before);
    assert!(child.flip_horizontal);
    assert_eq!(harness.state().ui.views[0], main);
}

fn keyboard_zoom_pass(app: &mut PhotocraftApp, ctx: &egui::Context, window: Option<u64>, key: egui::Key, shift: bool, focused: bool) -> bool {
    let viewport = window.map_or(ViewportId::ROOT, |id| ViewportId::from_hash_of(("docwin", id)));
    let modifiers = Modifiers { ctrl: true, command: true, shift, ..Modifiers::NONE };
    let mut raw = RawInput { viewport_id: viewport, focused, ..Default::default() };
    raw.viewports.insert(viewport, ViewportInfo { focused: Some(focused), ..Default::default() });
    raw.events = vec![Event::ModifiersChanged(modifiers), Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }];
    let mut consumed = false;
    let mut out = ctx.run_ui(raw, |ui| {
        if let Some(id) = window {
            app.with_document_window(id, |app| crate::shortcuts::document_window_zoom_keys(app, ui.ctx())).unwrap();
        } else {
            crate::shortcuts::handle(app, ui.ctx());
        }
        consumed = !ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Key { pressed: true, .. })));
    });
    out.textures_delta.clear();
    consumed
}

#[test]
fn keyboard_zoom_steps_both_window_types_without_changing_the_other_camera() {
    for animated in [false, true] {
        for (key, shift) in [(egui::Key::Equals, false), (egui::Key::Plus, false), (egui::Key::Plus, true)] {
            for secondary in [false, true] {
                let (mut app, ctx) = app();
                app.run("prefs.set", json!({"values": {"general.animatedZoom": animated}})).unwrap();
                let id = window(&mut app, &ctx);
                app.ui.windows[0].view.flip_horizontal = true;
                app.ui.windows[0].view.center = [21.0, 17.0];
                app.ui.windows[0].view.rotation = 23.0;
                let main = app.ui.views[0].clone();
                let child = app.ui.windows[0].view.clone();
                let document = app.session.active().unwrap().clone();
                let target = secondary.then_some(id);
                assert!(keyboard_zoom_pass(&mut app, &ctx, target, key, shift, true));
                let camera = if secondary { &app.ui.windows[0].view } else { &app.ui.views[0] };
                assert!(camera.zoom > 1.0, "{secondary}: {key:?}");
                if secondary {
                    assert_eq!(app.ui.views[0], main);
                } else {
                    assert_eq!(app.ui.windows[0].view, child);
                }
                assert!(keyboard_zoom_pass(&mut app, &ctx, target, egui::Key::Minus, false, true));
                let camera = if secondary { &app.ui.windows[0].view } else { &app.ui.views[0] };
                assert_eq!(camera.zoom, 1.0);
                if secondary {
                    assert_eq!(app.ui.views[0], main);
                } else {
                    assert_eq!(app.ui.windows[0].view, child);
                }
                let after = app.session.active().unwrap();
                assert!(Arc::ptr_eq(&document.doc, &after.doc));
                assert_eq!(document.revision, after.revision);
                assert_eq!(document.history.past_len(), after.history.past_len());
            }
        }
    }
}

#[test]
fn keyboard_zoom_secondary_respects_bindings_focus_and_leaves_other_view_shortcuts_alone() {
    let (mut app, ctx) = app();
    let id = window(&mut app, &ctx);
    let before = app.ui.windows[0].view.clone();
    assert!(!keyboard_zoom_pass(&mut app, &ctx, Some(id), egui::Key::Plus, false, false));
    for key in [egui::Key::Num0, egui::Key::Num1, egui::Key::PageDown] {
        assert!(!keyboard_zoom_pass(&mut app, &ctx, Some(id), key, false, true));
        assert_eq!(app.ui.windows[0].view, before);
    }
    app.session.edit_prefs(|p| {
        p.shortcuts.insert("view.zoomIn".into(), "Cmd+K".into());
    });
    assert!(!keyboard_zoom_pass(&mut app, &ctx, Some(id), egui::Key::Equals, false, true));
    assert!(keyboard_zoom_pass(&mut app, &ctx, Some(id), egui::Key::K, false, true));
    assert!(app.ui.windows[0].view.zoom > before.zoom);
    assert_eq!(app.ui.views[0].zoom, 1.0);
}
