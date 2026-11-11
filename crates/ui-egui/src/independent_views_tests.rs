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
    let opts = Default::default();
    let png = photocraft_io::export(&before.doc, "png", &opts).unwrap().bytes;
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
    assert_eq!(menus::invoke(&mut app, &ctx, "view.flipVertical", json!({})).unwrap(), json!(true));
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(before.doc, after.doc);
    assert_eq!((before.revision, before.saved_revision, before.last_damage), (after.revision, after.saved_revision, after.last_damage));
    assert_eq!(history, serde_json::to_value(after.history.entries()).unwrap());
    assert_eq!(photocraft_io::export(&after.doc, "png", &opts).unwrap().bytes, png);
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
fn independent_flip_new_windows_inherit_framing_from_source_but_flip_from_main() {
    let (mut app, ctx) = app();
    app.ui.views[0].flip_horizontal = true;
    let a = window(&mut app, &ctx);
    app.ui.focused_window = Some(a);
    app.ui.windows[0].view.zoom = 2.0;
    app.ui.windows[0].view.center = [9.0, 12.0];
    app.ui.windows[0].view.rotation = 37.0;
    app.ui.windows[0].view.flip_horizontal = false;
    let b = window(&mut app, &ctx);
    let expected = crate::state::View { flip_horizontal: true, flip_vertical: false, ..app.ui.windows[0].view.clone() };
    assert_eq!(app.ui.windows[1].view, expected);
    app.ui.focused_window = Some(b);
    assert!(!flip(&mut app, &ctx));
    assert!(!app.ui.windows[0].view.flip_horizontal);
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
    let mut out = ctx.run_ui(raw, |ui| {
        app.update_view_focus(ui.ctx());
        let doc = app.session.active().unwrap().doc.id;
        ui.interact(app.last_canvas_rect, crate::canvas::canvas_widget_id(doc, None), egui::Sense::click_and_drag());
    });
    out.textures_delta.clear();
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
    ];
    focus_pass(&mut app, &ctx, raw.clone());
    raw.events = vec![Event::PointerButton { pos: pos2(40.0, 100.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }];
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
                app.ui.views[0] = crate::canvas::canvas_view(app, ui, 0, rect, camera, true);
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
            app.with_document_window(id, |app| crate::shortcuts::document_window_view_keys(app, ui.ctx())).unwrap();
        } else {
            crate::shortcuts::handle(app, ui.ctx());
        }
        consumed = !ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Key { pressed: true, .. })));
    });
    out.textures_delta.clear();
    consumed
}

#[test]
fn independent_views_focus_main_from_floating_view_menu_and_custom_shortcut() {
    let (mut app, ctx) = app();
    let id = window(&mut app, &ctx);
    let command = "window.focusMainCanvas";
    app.ui.focused_window = Some(id);
    let before = app.session.active().unwrap().clone();
    let main = app.ui.views[0].clone();
    menus::invoke(&mut app, &ctx, command, json!({})).unwrap();
    assert_eq!(crate::view_visibility::panel_window(&app), None);

    app.session.edit_prefs(|p| {
        p.shortcuts.insert(command.into(), "Cmd+Shift+F11".into());
    });
    app.ui.focused_window = Some(id);
    assert!(keyboard_zoom_pass(&mut app, &ctx, Some(id), egui::Key::F11, true, true));
    assert_eq!(app.ui.focused_window, None);
    assert_eq!(crate::view_visibility::panel_window(&app), None);
    assert_eq!(app.ui.views[0], main);
    let after = app.session.active().unwrap();
    assert!(std::sync::Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(before.active_layer, after.active_layer);
    assert_eq!(before.history.past_len(), after.history.past_len());
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

#[test]
fn following_main_is_live_without_overwriting_the_saved_orientation_or_camera() {
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    let before = app.session.active().unwrap().clone();
    assert!(app.ui.windows.iter().all(|w| !w.follow_main_flip));
    app.ui.focused_window = Some(a);
    assert!(flip(&mut app, &ctx));
    app.ui.windows[0].follow_main_flip = true;
    assert!(!app.flip_enabled());
    assert_eq!(crate::view_cmds::checked(&app, "view.flipHorizontal"), Some(false));
    assert!(!flip(&mut app, &ctx));
    assert!(app.ui.windows[0].view.flip_horizontal);
    let child = app.ui.windows[0].view.clone();

    for main_flip in [true, false, true, false] {
        app.ui.focused_window = None;
        assert_eq!(flip(&mut app, &ctx), main_flip);
        let main = app.ui.views[0].clone();
        assert_eq!(app.with_document_window(a, |app| app.canvas_flip_horizontal()), Some(main_flip));
        assert_eq!(app.with_document_window(b, |app| app.canvas_flip_horizontal()), Some(false));
        assert_eq!(app.ui.windows[0].view, child);
        assert_eq!(app.ui.views[0], main);
    }

    app.with_document_window(a, |app| {
        app.ui.views[0].zoom = 2.0;
        app.ui.views[0].center = [27.0, 21.0];
    });
    assert_eq!(app.ui.views[0].zoom, 1.0);
    assert_eq!(app.ui.views[0].center, [32.0, 24.0]);
    assert_eq!(app.ui.windows[1].view.zoom, 1.0);
    assert_eq!(app.ui.windows[0].view.zoom, 2.0);
    assert_eq!(app.ui.windows[0].view.center, [27.0, 21.0]);

    app.ui.windows[0].follow_main_flip = false;
    assert_eq!(app.with_document_window(a, |app| app.canvas_flip_horizontal()), Some(true));
    app.ui.windows.retain(|w| w.id != a);
    let c = window(&mut app, &ctx);
    assert_ne!(a, c);
    assert!(!app.ui.windows.last().unwrap().follow_main_flip);
    assert_eq!(app.with_document_window(c, |app| app.canvas_flip_horizontal()), Some(false));
    assert_eq!(menus::invoke(&mut app, &ctx, "view.flipVertical", json!({})).unwrap(), json!(true));
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.history.past_len(), after.history.past_len());
}

#[test]
fn configured_flip_shortcut_targets_the_native_camera_and_is_consumed_while_following() {
    let (mut app, ctx) = app();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    app.session.edit_prefs(|p| {
        p.shortcuts.insert("view.flipHorizontal".into(), "Cmd+K".into());
    });
    let key = egui::Key::K;
    assert!(!keyboard_zoom_pass(&mut app, &ctx, Some(a), key, false, false));
    // The Layers panel may have explicitly selected a different target.
    app.ui.focused_window = Some(b);
    assert!(keyboard_zoom_pass(&mut app, &ctx, Some(a), key, false, true));
    assert!(app.ui.windows[0].view.flip_horizontal);
    assert!(!app.ui.windows[1].view.flip_horizontal);
    assert!(!app.ui.views[0].flip_horizontal);

    app.ui.windows[0].follow_main_flip = true;
    assert!(keyboard_zoom_pass(&mut app, &ctx, Some(a), key, false, true));
    assert!(app.ui.windows[0].view.flip_horizontal);
    assert!(!app.ui.views[0].flip_horizontal);

    app.ui.focused_window = None;
    assert!(keyboard_zoom_pass(&mut app, &ctx, None, key, false, true));
    assert!(app.ui.views[0].flip_horizontal);
    assert_eq!(app.with_document_window(a, |app| app.canvas_flip_horizontal()), Some(true));
    assert_eq!(app.with_document_window(b, |app| app.canvas_flip_horizontal()), Some(false));
}

#[test]
fn older_window_state_defaults_to_independent_flip() {
    let (mut app, ctx) = app();
    window(&mut app, &ctx);
    app.ui.windows[0].follow_main_flip = true;
    let mut saved = serde_json::to_value(&app.ui.windows[0]).unwrap();
    let restored: crate::state::DocWindow = serde_json::from_value(saved.clone()).unwrap();
    assert!(restored.follow_main_flip);
    saved.as_object_mut().unwrap().remove("follow_main_flip");
    let restored: crate::state::DocWindow = serde_json::from_value(saved).unwrap();
    assert!(!restored.follow_main_flip);
}

#[test]
fn vertical_flip_follows_main_and_restores_both_independent_axes() {
    let (mut app, ctx) = app();
    app.ui.views[0].flip_vertical = true;
    let id = window(&mut app, &ctx);
    assert!(app.ui.windows[0].view.flip_vertical);
    app.ui.focused_window = Some(id);
    assert_eq!(menus::invoke(&mut app, &ctx, "view.flipVertical", json!({})).unwrap(), json!(false));
    assert!(flip(&mut app, &ctx));
    app.ui.windows[0].follow_main_flip = true;
    assert!(!app.flip_enabled());
    assert_eq!(app.with_document_window(id, |a| (a.canvas_flip_horizontal(), a.canvas_flip_vertical())), Some((false, true)));
    app.ui.focused_window = None;
    assert_eq!(menus::invoke(&mut app, &ctx, "view.flipVertical", json!({})).unwrap(), json!(false));
    assert_eq!(app.with_document_window(id, |a| (a.canvas_flip_horizontal(), a.canvas_flip_vertical())), Some((false, false)));
    app.ui.windows[0].follow_main_flip = false;
    assert_eq!(app.with_document_window(id, |a| (a.canvas_flip_horizontal(), a.canvas_flip_vertical())), Some((true, false)));
}

#[test]
fn vertical_flip_round_trips_points_and_vectors_after_rotation_and_aspect() {
    let (app, _) = app();
    let base = ViewXform { rotation: 31.0, zoom: 2.3, aspect: 1.6, center: [12.5, 21.25], ..ViewXform::active(&app).unwrap() };
    let point = pos2(28.0, 19.0);
    let delta = base.to_screen(point.x, point.y) - base.rect.center();
    for (flip, flip_vertical) in [(false, false), (true, false), (false, true), (true, true)] {
        let xf = ViewXform { flip, flip_vertical, ..base };
        let screen = xf.to_screen(point.x, point.y);
        let want = base.rect.center() + vec2(if flip { -delta.x } else { delta.x }, if flip_vertical { -delta.y } else { delta.y });
        assert!(screen.distance(want) < 0.001);
        let [x, y] = xf.to_doc(screen);
        assert!(point.distance(pos2(x as f32, y as f32)) < 0.001);
        let v = vec2(7.0, -5.0);
        assert!((xf.unmap_vec(xf.map_vec(v) / xf.zoom) - v).length() < 0.001);
        assert_eq!(xf.reflected(), flip ^ flip_vertical);
    }
}

#[test]
fn secondary_document_input_rejects_edits_but_main_paints_and_undoes() {
    use crate::canvas::{ToolEvent, tool_event};

    let (mut app, ctx) = app();
    app.run("layer.new.layer", json!({})).unwrap();
    let id = window(&mut app, &ctx);
    let before = app.session.active().unwrap().clone();
    let stroke = |app: &mut PhotocraftApp| {
        tool_event(app, ToolEvent::Down { x: 12.0, y: 12.0, pressure: 1.0 }, Modifiers::NONE);
        tool_event(app, ToolEvent::Move { x: 25.0, y: 20.0, pressure: 1.0 }, Modifiers::NONE);
        tool_event(app, ToolEvent::Up { x: 25.0, y: 20.0 }, Modifiers::NONE);
    };
    app.with_document_window(id, |app| {
        stroke(app);
        assert!(app.drag.is_none() && app.live_stroke.is_none());
        for cmd in [
            "paint.stroke",
            "edit.fill",
            "edit.clear",
            "select.all",
            "layer.new.layer",
            "layer.layerMask.revealAll",
            "edit.freeTransform",
            "image.crop",
            "edit.undo",
        ] {
            assert!(app.run(cmd, json!({})).is_err(), "{cmd}");
            assert!(menus::invoke(app, &ctx, cmd, json!({})).is_err(), "{cmd}");
        }
        assert_eq!(menus::invoke(app, &ctx, "view.flipHorizontal", json!({})).unwrap(), json!(true));
        assert_eq!(menus::invoke(app, &ctx, "view.flipVertical", json!({})).unwrap(), json!(true));
        app.run("tools.setColors", json!({"foreground": [1.0, 0.0, 0.0, 1.0]})).unwrap();
        assert!(app.run("document.sampleColor", json!({"x":12.0, "y":12.0, "sampleLayer":"all"})).is_ok());
    })
    .unwrap();
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.history.past_len(), after.history.past_len());

    app.ui.tool = crate::state::Tool::Brush;
    stroke(&mut app);
    let painted = app.session.active().unwrap();
    assert_ne!(painted.doc, before.doc);
    assert_eq!(painted.history.past_len(), before.history.past_len() + 1);
    assert!(app.session.undo());
    assert_eq!(app.session.active().unwrap().doc, before.doc);
}

#[test]
fn secondary_document_canvas_samples_without_painting() {
    let (mut app, ctx) = app();
    app.run("layer.new.layer", json!({})).unwrap();
    app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
    let id = window(&mut app, &ctx);
    app.ui.windows[0].view.flip_vertical = true;
    app.ui.windows[0].view.rotation = 25.0;
    let before = app.session.active().unwrap().clone();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(400.0, 300.0)).build_ui_state(
        move |ui, app: &mut PhotocraftApp| {
            app.with_document_window(id, |app| {
                let rect = ui.max_rect();
                let view = app.ui.views[0].clone();
                // Even an editable flag from a caller cannot make a secondary editable.
                app.ui.views[0] = crate::canvas::canvas_view(app, ui, 0, rect, view, true);
            });
        },
        app,
    );
    let p = pos2(200.0, 150.0);
    let gesture = |h: &mut egui_kittest::Harness<'_, PhotocraftApp>, mods| {
        h.event(Event::PointerMoved(p));
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: mods });
        h.step();
        h.event(Event::PointerMoved(p + vec2(12.0, 10.0)));
        h.step();
        h.event(Event::PointerButton { pos: p + vec2(12.0, 10.0), button: PointerButton::Primary, pressed: false, modifiers: mods });
        h.step();
    };
    for tool in [crate::state::Tool::Brush, crate::state::Tool::Eraser, crate::state::Tool::RectMarquee, crate::state::Tool::Crop] {
        h.state_mut().ui.tool = tool;
        gesture(&mut h, Modifiers::NONE);
    }
    h.state_mut().ui.tool = crate::state::Tool::Eyedropper;
    gesture(&mut h, Modifiers::NONE);
    assert_eq!(h.state().session.tools.foreground, [1.0, 0.0, 0.0, 1.0]);
    assert!(h.state().ui.crop_rect.is_none() && h.state().drag.is_none());
    let after = h.state().session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(before.history.past_len(), after.history.past_len());
}
