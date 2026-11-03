use super::adjustment_tests::{change, fixture_adjustment, value};
use super::*;
use crate::view_visibility::adjustments as edit;
use photocraft_doc::LayerContent;

fn cases() -> Vec<(&'static str, Value)> {
    vec![
        ("brightnessContrast", json!({"brightness": 35, "contrast": 20, "legacy": true})),
        ("exposure", json!({"exposure": 1.5, "offset": 0.02, "gamma": 1.3})),
        ("vibrance", json!({"vibrance": 40, "saturation": -15})),
        ("hueSaturation", json!({"hue": 35, "saturation": 20, "lightness": -10, "colorize": true})),
        ("colorBalance", json!({"midtones": [20, -10, 30], "preserveLuminosity": false})),
        ("blackWhite", json!({"reds": 80, "tint": true, "tintColor": "#887744"})),
        ("photoFilter", json!({"color": "#ff2222", "density": 60, "preserveLuminosity": false})),
        ("channelMixer", json!({"red": [60, 30, 20, 10], "monochrome": true})),
        ("curves", json!({"points": [[0, 15], [128, 190], [255, 240]], "red": [[0, 0], [255, 180]]})),
        ("colorLookup", json!({"lut": photocraft_engine::adjust_cmds::LOOKS[0].0, "tetrahedral": true, "dither": true})),
        ("threshold", json!({"level": 190})),
        ("gradientMap", json!({"stops": [[0, "#550000"], [0.4, "#00ff00"], [1, "#7777ff"]], "reverse": true})),
        ("selectiveColor", json!({"method": "absolute", "reds": [25, -15, 35, 10], "neutrals": [10, 5, -15, 20]})),
        ("invert", json!({})),
        ("levels", json!({"gamma": 1.5})),
        ("posterize", json!({"levels": 3})),
    ]
}

#[test]
fn phase4b_all_types_isolate_parameters_rendering_history_and_exports() {
    for (kind, params) in cases() {
        let (mut app, ctx, layer) = fixture_adjustment(kind);
        let a = window(&mut app, &ctx);
        let b = window(&mut app, &ctx);
        let before = app.session.active().unwrap().clone();
        let base = value(&mut app, b, layer);
        let expected = photocraft_engine::adjust_params::from_params(kind, &params, Some(&base), before.doc.mode).unwrap();
        let options = photocraft_io::ExportOptions::default();
        let saved = photocraft_io::export(&before.doc, "pcraft", &options).unwrap().bytes;
        let png = photocraft_io::export(&before.doc, "png", &options).unwrap().bytes;
        let b_projection = shown(&mut app, b);
        change(&mut app, &ctx, a, layer, "commit", params.clone());
        assert_eq!(value(&mut app, a, layer), expected, "{kind}");
        assert_eq!(value(&mut app, b, layer), base, "{kind}");
        assert!(Arc::ptr_eq(&b_projection, &shown(&mut app, b)));
        let mut reference = (*before.doc).clone();
        reference.layer_mut(layer).unwrap().content = LayerContent::Adjustment(expected.clone());
        let projected = shown(&mut app, a);
        assert_eq!(photocraft_compose::render(&projected, projected.bounds()).px, photocraft_compose::render(&reference, reference.bounds()).px, "{kind}");
        let after = app.session.active().unwrap();
        assert!(Arc::ptr_eq(&before.doc, &after.doc));
        assert_eq!((before.revision, before.saved_revision, before.last_damage), (after.revision, after.saved_revision, after.last_damage));
        assert_eq!(serde_json::to_value(before.history.entries()).unwrap(), serde_json::to_value(after.history.entries()).unwrap());
        assert_eq!(saved, photocraft_io::export(&after.doc, "pcraft", &options).unwrap().bytes);
        assert_eq!(png, photocraft_io::export(&after.doc, "png", &options).unwrap().bytes);
        app.ui.focused_window = None;
        app.run("layer.setAdjustment", {
            let mut p = params;
            p["layer"] = json!(layer.0);
            p
        })
        .unwrap();
        if expected != base {
            assert_eq!(app.session.active().unwrap().history.past_len(), before.history.past_len() + 1);
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.session.active().unwrap().doc.layer(layer).unwrap().content, LayerContent::Adjustment(base.clone()));
            app.run("edit.redo", json!({})).unwrap();
        }
        assert_eq!(value(&mut app, b, layer), base, "Main edits do not replace {kind} snapshots");
        let c = window(&mut app, &ctx);
        assert_eq!(value(&mut app, c, layer), expected);
        app.ui.windows.iter_mut().find(|w| w.id == a).unwrap().open = false;
        retain(&mut app);
        assert!(!app.view_composites.projections.contains_key(&a));
        let reopened = window(&mut app, &ctx);
        assert_ne!(reopened, a);
        assert_eq!(value(&mut app, reopened, layer), expected, "reopened {kind} view inherits current Main");
    }
}

#[test]
fn phase4b_previews_cancel_for_every_type_and_stale_completion_is_rejected() {
    for (kind, params) in cases() {
        let (mut app, ctx, layer) = fixture_adjustment(kind);
        let a = window(&mut app, &ctx);
        let b = window(&mut app, &ctx);
        app.ui.focused_window = Some(a);
        let target = edit::Target::current(&mut app, layer).unwrap();
        edit::select(&mut app, Some(target));
        let epoch = app.view_composites.editor.ui_epoch;
        let base = value(&mut app, b, layer);
        edit::preview(&mut app, target, &params).unwrap();
        assert!(app.live_adjust.is_none());
        assert!(edit::accepts_completion(&app, target, epoch));
        app.ui.focused_window = Some(b);
        edit::follow(&mut app, &ctx);
        assert_eq!(value(&mut app, a, layer), base);
        assert!(!edit::accepts_completion(&app, target, epoch));
        app.ui.focused_window = Some(a);
        edit::select(&mut app, Some(target));
        assert!(!edit::accepts_completion(&app, target, epoch), "returning to a view cannot revive an async result");
    }
}

#[test]
fn phase4b_color_balance_reset_restores_session_then_defaults() {
    let (mut app, ctx, id) = fixture_adjustment("colorBalance");
    app.run("layer.setAdjustment", json!({"layer": id.0, "midtones": [12, 20, -5]})).unwrap();
    let a = window(&mut app, &ctx);
    app.ui.focused_window = Some(a);
    let target = edit::Target::current(&mut app, id).unwrap();
    edit::select(&mut app, Some(target));
    let start = value(&mut app, a, id);
    edit::commit(&mut app, target, &json!({"midtones": [50,0,0]}), false).unwrap();
    edit::commit(&mut app, target, &json!({}), true).unwrap();
    assert_eq!(value(&mut app, a, id), start);
    edit::commit(&mut app, target, &json!({}), true).unwrap();
    assert_eq!(value(&mut app, a, id), photocraft_engine::adjust_params::default_for("colorBalance", target.mode()).unwrap());
}

#[test]
fn phase4b_color_picker_requests_results_and_previews_have_an_owner() {
    let (mut app, ctx, id) = fixture_adjustment("photoFilter");
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    app.ui.focused_window = Some(a);
    let target = edit::Target::current(&mut app, id).unwrap();
    edit::select(&mut app, Some(target));
    let epoch = app.view_composites.editor.ui_epoch;
    let widget = target.memory().with(epoch);
    crate::color_picker_ui::with_widget_owner(&ctx, target, epoch, || {
        crate::color_picker_ui::request_for_widget(&ctx, widget, "Filter Color", [0.2, 0.4, 0.6])
    });
    crate::color_picker_ui::widget_tick(&mut app, &ctx);
    let fields = app.ui.dialogs.last().unwrap().fields.clone();
    assert_eq!(fields["__adjustmentSecondary"], json!(true));
    app.ui.dialogs.last_mut().unwrap().fields.insert("color".into(), json!("#ff0000"));
    crate::color_picker_ui::widget_tick(&mut app, &ctx);
    assert_eq!(crate::color_picker_ui::take_widget_event(&ctx, widget), Some(crate::color_picker_ui::WidgetEvent::Preview([1.0, 0.0, 0.0])));
    let mut confirmed = fields.clone();
    confirmed.insert("color".into(), json!("#00ff00"));
    crate::color_picker_ui::confirm(&mut app, &confirmed).unwrap();
    crate::color_picker_ui::widget_tick(&mut app, &ctx);
    assert_eq!(crate::color_picker_ui::take_widget_color(&ctx, widget), Some([0.0, 1.0, 0.0]));
    edit::preview(&mut app, target, &json!({"color":"#ff0000"})).unwrap();
    app.ui.focused_window = Some(b);
    edit::follow(&mut app, &ctx);
    assert!(app.ui.dialogs.is_empty());
    assert!(crate::color_picker_ui::confirm(&mut app, &confirmed).is_err());
    assert!(app.live_adjust.is_none());
    assert_eq!(value(&mut app, a, id), value(&mut app, b, id));
    // A click queued by the old frame cannot open a picker after the view changes.
    crate::color_picker_ui::with_widget_owner(&ctx, target, epoch, || {
        crate::color_picker_ui::request_for_widget(&ctx, widget, "Filter Color", [0.2, 0.4, 0.6])
    });
    crate::color_picker_ui::widget_tick(&mut app, &ctx);
    assert!(app.ui.dialogs.is_empty());
}

#[test]
fn phase4b_curves_and_gradient_graphical_controls_edit_only_the_selected_view() {
    use egui_kittest::{Harness, kittest::Queryable};
    for kind in ["curves", "gradientMap", "photoFilter"] {
        let (mut app, ctx, id) = fixture_adjustment(kind);
        let a = window(&mut app, &ctx);
        let b = window(&mut app, &ctx);
        app.ui.focused_window = Some(a);
        let before = app.session.active().unwrap().clone();
        let base = value(&mut app, b, id);
        let mut h = Harness::builder()
            .with_context_setup(|ctx| PhotocraftApp::setup_context(ctx, Default::default()))
            .with_size(egui::vec2(1000.0, 900.0))
            .build_ui_state(
                move |ui, app: &mut PhotocraftApp| {
                    edit::follow(app, ui.ctx());
                    crate::color_picker_ui::widget_tick(app, ui.ctx());
                    let doc = app.session.active().unwrap().doc.clone();
                    let LayerContent::Adjustment(adj) = &doc.layer(id).unwrap().content else { return };
                    crate::adjust_editors::layer_editor(app, ui, id, adj);
                    crate::dialogs::show(app, ui.ctx());
                },
                app,
            );
        h.run_steps(4);
        let target = edit::Target::current(h.state_mut(), id).unwrap();
        if kind == "curves" {
            let graph = h.ctx.data(|d| d.get_temp::<egui::Rect>(target.memory().with("curves-graph"))).unwrap();
            let point = graph.center() + egui::vec2(0.0, -35.0);
            h.hover_at(point);
            h.drag_at(point);
            h.run_steps(1);
            h.drop_at(point);
            h.run_steps(3);
        } else if kind == "gradientMap" {
            h.get_by_label("Add Stop").click();
            h.run_steps(3);
        } else {
            h.get_by_label("Color:").click();
            h.run_steps(2);
            h.get_by_label("Color Picker (Photo Filter)").click();
            h.run_steps(3);
            h.state_mut().ui.dialogs.last_mut().unwrap().fields.insert("color".into(), json!("#00ff00"));
            h.run_steps(3);
            assert_ne!(value(h.state_mut(), a, id), base);
            assert_eq!(*edit::committed(h.state(), target).unwrap(), base, "picker previews are not commits");
            h.get_by_label("Cancel").click();
            h.run_steps(4);
            assert!(h.state().ui.dialogs.is_empty(), "the picker must actually receive Cancel");
            assert_eq!(value(h.state_mut(), a, id), base);
            h.get_by_label("Color Picker (Photo Filter)").click();
            h.run_steps(3);
            h.state_mut().ui.dialogs.last_mut().unwrap().fields.insert("color".into(), json!("#ff0000"));
            h.run_steps(3);
            h.get_by_label("OK").click();
            h.run_steps(4);
        }
        assert_ne!(value(h.state_mut(), a, id), base, "{kind}");
        assert_eq!(value(h.state_mut(), b, id), base, "{kind}");
        assert!(Arc::ptr_eq(&before.doc, &h.state().session.active().unwrap().doc));
        assert_eq!(before.history.past_len(), h.state().session.active().unwrap().history.past_len());
        assert!(h.state().live_adjust.is_none());
        if kind == "curves" {
            let graph = h.ctx.data(|d| d.get_temp::<egui::Rect>(target.memory().with("curves-graph"))).unwrap();
            let committed = value(h.state_mut(), a, id);
            h.drag_at(graph.center());
            h.run_steps(1);
            h.hover_at(graph.center() + egui::vec2(20.0, -60.0));
            h.run_steps(1);
            h.state_mut().ui.focused_window = Some(b);
            h.run_steps(2);
            h.drop_at(graph.center());
            h.run_steps(2);
            assert_eq!(value(h.state_mut(), a, id), committed);
            assert_eq!(value(h.state_mut(), b, id), base);
        }
    }
}
