use super::*;
use crate::{Services, menus};

mod adjustment_tests;
mod blend_tests;
mod opacity_tests;
mod phase4b_tests;

fn fixture() -> (PhotocraftApp, egui::Context, LayerId, LayerId) {
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    app.run("file.new", json!({"width": 16, "height": 12, "background": "white"})).unwrap();
    app.run("layer.new.layer", json!({"name": "red"})).unwrap();
    let red = app.session.active().unwrap().active_layer.unwrap();
    app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
    app.run("layer.new.layer", json!({"name": "blue"})).unwrap();
    let blue = app.session.active().unwrap().active_layer.unwrap();
    app.run("edit.fill", json!({"color": "#0000ff"})).unwrap();
    app.sync_views();
    (app, egui::Context::default(), red, blue)
}

fn window(app: &mut PhotocraftApp, ctx: &egui::Context) -> u64 {
    menus::invoke(app, ctx, "window.arrange.newWindowForDocument", json!({})).unwrap()["window"].as_u64().unwrap()
}

fn set(app: &mut PhotocraftApp, ctx: &egui::Context, window: u64, layer: LayerId, visible: bool) {
    menus::invoke(app, ctx, "view.layerVisibility", json!({"window": window, "layer": layer.0, "visible": visible})).unwrap();
}

fn shown(app: &mut PhotocraftApp, window: u64) -> Arc<Document> {
    let doc = app.session.active().unwrap().doc.clone();
    project(app, window, doc).0
}

fn pixel(doc: &Document) -> [f32; 4] {
    photocraft_compose::render(doc, photocraft_geom::Rect::new(0, 0, 1, 1)).px[0]
}

#[test]
fn adjustment_creation_from_canvas_focus_preserves_main_and_restores_secondary_visibility() {
    let (mut app, ctx, red, blue) = fixture();
    let command = "layer.newAdjustmentLayer.posterize";
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    let c = window(&mut app, &ctx);
    set_follow_main_settings(&mut app, &ctx, c, true);
    select_panel_window(&mut app, Some(b));
    app.ui.focused_window = Some(a);
    app.run("layer.select", json!({"layer":red.0,"mode":"add"})).unwrap();
    app.run("layer.layerMask.revealAll", json!({})).unwrap();
    app.run("select.all", json!({})).unwrap();
    assert!(app.ui.mask_target);
    let working = app.session.active().unwrap().clone();
    let masks = (app.ui.mask_target, app.ui.vector_mask_target);
    let result = menus::invoke(&mut app, &ctx, command, json!({})).unwrap();
    let layer = LayerId(result["layer"].as_u64().unwrap());
    let st = app.session.active().unwrap();
    assert_eq!(st.active_layer, working.active_layer);
    assert_eq!(st.selected_layers(), working.selected_layers());
    assert!(st.doc.selection == working.doc.selection, "creation keeps Main selection");
    assert_eq!((app.ui.mask_target, app.ui.vector_mask_target), masks);
    assert_eq!(st.history.past_len(), working.history.past_len() + 1);
    assert!(!st.doc.layer(layer).unwrap().visible);
    assert_eq!(panel_window(&app), Some(b));
    assert_eq!(created_properties(&app), Some((a, layer)));
    let target = adjustments::Target::current(&mut app, layer).unwrap();
    assert_eq!(target.window, Some(a));
    adjustments::select(&mut app, Some(target));
    assert!(adjustments::accepts_completion(&app, target, app.view_composites.editor.ui_epoch));
    adjustments::commit(&mut app, target, &json!({"levels": 5}), false).unwrap();
    let canonical = &app.session.active().unwrap().doc.layer(layer).unwrap().content;
    assert_ne!(canonical, &photocraft_doc::LayerContent::Adjustment((*adjustments::committed(&app, target).unwrap()).clone()));
    assert!(shown(&mut app, a).layer(layer).unwrap().visible);
    assert!(!shown(&mut app, b).layer(layer).unwrap().visible);
    assert!(!shown(&mut app, c).layer(layer).unwrap().visible);

    let opts = photocraft_io::ExportOptions::default();
    let png = |doc: &Document| photocraft_io::export(doc, "png", &opts).unwrap().bytes;
    assert_eq!(png(&working.doc), png(&app.session.active().unwrap().doc));
    let native = photocraft_io::export(&app.session.active().unwrap().doc, "pcraft", &opts).unwrap().bytes;
    let saved = photocraft_io::import("sample.pcraft", &native).unwrap().document;
    assert!(!saved.layer(layer).unwrap().visible);

    app.ui.focused_window = None;
    menus::invoke(&mut app, &ctx, "edit.undo", json!({})).unwrap();
    assert!(app.session.active().unwrap().doc.layer(layer).is_none());
    assert!(created_properties(&app).is_none());
    menus::invoke(&mut app, &ctx, "edit.redo", json!({})).unwrap();
    assert!(shown(&mut app, a).layer(layer).unwrap().visible);
    assert!(!shown(&mut app, b).layer(layer).unwrap().visible);
    assert!(!app.session.active().unwrap().doc.layer(layer).unwrap().visible);

    app.ui.windows.iter_mut().find(|w| w.id == a).unwrap().open = false;
    retain(&mut app);
    assert!(!app.view_composites.created_adjustments.contains_key(&(working.doc.id, layer)));

    app.ui.focused_window = Some(c);
    let shared = LayerId(menus::invoke(&mut app, &ctx, command, json!({})).unwrap()["layer"].as_u64().unwrap());
    assert!(app.session.active().unwrap().doc.layer(shared).unwrap().visible);
    assert!(shown(&mut app, b).layer(shared).unwrap().visible);
    assert!(settings_read_only(&app, Some(c)));
    assert_eq!(app.session.active().unwrap().active_layer, working.active_layer);

    app.ui.focused_window = None;
    let main = LayerId(menus::invoke(&mut app, &ctx, command, json!({})).unwrap()["layer"].as_u64().unwrap());
    assert_eq!(app.session.active().unwrap().active_layer, Some(main));
    assert!(app.session.active().unwrap().doc.layer(main).unwrap().visible);
    assert!(app.session.active().unwrap().doc.layer(blue).is_some());
}

#[test]
fn views_capture_canonical_visibility_and_diverge_without_document_or_history_edits() {
    let (mut app, ctx, red, blue) = fixture();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    let before = app.session.active().unwrap().clone();
    let history = serde_json::to_value(before.history.entries()).unwrap();
    set(&mut app, &ctx, a, blue, false);
    assert_eq!(pixel(&shown(&mut app, a)), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(pixel(&shown(&mut app, b)), [0.0, 0.0, 1.0, 1.0]);
    set(&mut app, &ctx, b, red, false);
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!((before.revision, before.saved_revision, before.last_damage), (after.revision, after.saved_revision, after.last_damage));
    assert_eq!(history, serde_json::to_value(after.history.entries()).unwrap());
    assert!(after.doc.layer(blue).unwrap().visible);
    assert!(after.doc.layer(red).unwrap().visible);
    // The authoritative image supplied to save/export remains blue.
    assert_eq!(pixel(&after.doc), [0.0, 0.0, 1.0, 1.0]);
    app.run("layer.setProps", json!({"layer": blue.0, "visible": false})).unwrap();
    assert_eq!(pixel(&app.session.active().unwrap().doc), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(pixel(&shown(&mut app, b)), [0.0, 0.0, 1.0, 1.0], "main visibility edits do not change an existing secondary view");
    app.ui.focused_window = Some(b);
    let c = window(&mut app, &ctx);
    assert!(!shown(&mut app, c).layer(blue).unwrap().visible, "new windows inherit main, even when another child was focused");
}

#[test]
fn metadata_projection_shares_all_existing_raster_and_mask_tiles() {
    let (mut app, ctx, _, blue) = fixture();
    let w = window(&mut app, &ctx);
    set(&mut app, &ctx, w, blue, false);
    let canonical = app.session.active().unwrap().doc.clone();
    let projected = shown(&mut app, w);
    let mut compared = 0;
    for (_, _, l) in canonical.walk() {
        if let Some(surface) = l.surface() {
            let other = projected.layer(l.id).unwrap().surface().unwrap();
            let a: Vec<_> = surface.tiles().collect();
            let b: Vec<_> = other.tiles().collect();
            assert_eq!(a.len(), b.len());
            for ((ca, ta), (cb, tb)) in a.into_iter().zip(b) {
                assert_eq!(ca, cb);
                assert!(Arc::ptr_eq(ta, tb));
                compared += 1;
            }
        }
    }
    assert!(compared > 0);
    assert!(Arc::ptr_eq(&projected, &shown(&mut app, w)), "unchanged projections are memoized");
}

#[test]
fn cpu_texture_caches_are_per_view_and_refresh_after_shared_edits() {
    let (mut app, ctx, _, blue) = fixture();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    set(&mut app, &ctx, a, blue, false);
    let main = crate::canvas::ensure_texture(&mut app, &ctx, 0, None).unwrap().0;
    let texture = |app: &mut PhotocraftApp, id| app.with_document_window(id, |app| crate::canvas::ensure_texture(app, &ctx, 0, None).unwrap().0).unwrap();
    let ta = texture(&mut app, a);
    let tb = texture(&mut app, b);
    assert_ne!(main, ta);
    assert_ne!(ta, tb);
    assert_ne!(main, tb);
    let doc = app.session.active().unwrap().doc.id;
    assert_ne!(app.view_composites.caches[&a][&(doc, 0)].tex_preview_key, app.view_composites.caches[&b][&(doc, 0)].tex_preview_key);
    app.perf.last_refresh = "";
    assert_eq!(texture(&mut app, a), ta);
    assert_eq!(texture(&mut app, b), tb);
    assert_eq!(crate::canvas::ensure_texture(&mut app, &ctx, 0, None).unwrap().0, main);
    assert_eq!(app.perf.last_refresh, "", "stationary views reuse composites");
    app.run("edit.fill", json!({"color": "#00ff00"})).unwrap();
    texture(&mut app, a);
    texture(&mut app, b);
    crate::canvas::ensure_texture(&mut app, &ctx, 0, None);
    let revision = app.session.active().unwrap().revision;
    assert_eq!(app.view_composites.caches[&a][&(doc, 0)].tex_revision, revision);
    assert_eq!(app.view_composites.caches[&b][&(doc, 0)].tex_revision, revision);
    assert_eq!(app.canvases[&(doc, 0)].tex_revision, revision);
    assert_eq!(pixel(&shown(&mut app, a)), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(pixel(&shown(&mut app, b)), [0.0, 1.0, 0.0, 1.0]);
    assert!(app.redraws.is_empty(), "secondary CPU composites never submit document-scoped background jobs");
}

#[test]
fn panel_routes_only_visibility_and_preserves_cameras_and_focus_target() {
    let (mut app, ctx, _, blue) = fixture();
    let w = window(&mut app, &ctx);
    let cameras = (app.ui.views.clone(), app.ui.windows[0].view.clone());
    app.ui.focused_window = Some(w);
    assert_eq!(focused_window(&app), Some(w));
    assert!(panel_action(&mut app, &ctx, Some(w), "layer.setProps", &json!({"layer": blue.0, "visible": false})));
    assert!(!shown(&mut app, w).layer(blue).unwrap().visible);
    assert!(app.session.active().unwrap().doc.layer(blue).unwrap().visible);
    assert!(!panel_action(&mut app, &ctx, Some(w), "layer.setProps", &json!({"layer": blue.0, "fill": 0.5})));
    assert!(!panel_action(&mut app, &ctx, None, "layer.setProps", &json!({"layer": blue.0, "visible": false})));
    assert_eq!(cameras, (app.ui.views.clone(), app.ui.windows[0].view.clone()));
    // Clicking root panel chrome retains the child's logical target; clicking canvas selects main.
    let rect = egui::Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(600.0, 500.0));
    app.last_canvas_rect = rect;
    let doc = app.session.active().unwrap().doc.id;
    let pass = |app: &mut PhotocraftApp, events| {
        let mut input = egui::RawInput { events, ..Default::default() };
        input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().focused = Some(true);
        let mut output = ctx.run_ui(input, |ui| {
            app.update_view_focus(ui.ctx());
            ui.interact(rect, crate::canvas::canvas_widget_id(doc, None), egui::Sense::click_and_drag());
        });
        output.textures_delta.clear();
    };
    for (pos, expected) in [(egui::pos2(700.0, 200.0), Some(w)), (rect.center(), None)] {
        pass(&mut app, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            pass(&mut app, vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]);
        }
        assert_eq!(app.ui.focused_window, expected);
    }
}

#[test]
fn added_removed_reordered_layers_and_closed_views_have_safe_lifecycles() {
    let (mut app, ctx, red, blue) = fixture();
    let w = window(&mut app, &ctx);
    set(&mut app, &ctx, w, blue, false);
    app.run("layer.new.layer", json!({"name": "new"})).unwrap();
    let added = app.session.active().unwrap().active_layer.unwrap();
    assert!(shown(&mut app, w).layer(added).unwrap().visible);
    // Structural edits use the canonical engine path and stable layer IDs.
    app.session
        .edit("Reorder fixture", |doc, _| {
            doc.layers.reverse();
            Ok(())
        })
        .unwrap();
    app.sync_views();
    assert!(!shown(&mut app, w).layer(blue).unwrap().visible);
    app.run("layer.delete", json!({"layer": red.0})).unwrap();
    assert!(shown(&mut app, w).layer(red).is_none());
    assert!(!app.ui.windows[0].visibility.layers.contains_key(&red));
    app.with_document_window(w, |app| {
        crate::canvas::ensure_texture(app, &ctx, 0, None);
    });
    assert!(app.view_composites.caches.contains_key(&w));
    app.ui.windows[0].open = false;
    retain(&mut app);
    assert!(app.view_composites.caches.is_empty());
    assert!(app.view_composites.projections.is_empty());
    assert!(app.view_composites.channels.is_empty());
    assert!(command(&mut app, &ctx, "view.layerVisibility", &json!({"window": w, "layer": blue.0, "visible": true})).is_err());
}

#[test]
fn group_solo_restores_view_eyes_and_invalid_commands_do_not_edit_anything() {
    let (mut app, ctx, red, blue) = fixture();
    app.session
        .edit("Group fixture", |doc, _| {
            let layers = std::mem::take(&mut doc.layers);
            doc.layers = vec![photocraft_doc::Layer::group("group", layers)];
            Ok(())
        })
        .unwrap();
    app.sync_views();
    let w = window(&mut app, &ctx);
    let canonical = app.session.active().unwrap().doc.clone();
    set(&mut app, &ctx, w, blue, false);
    let original = shown(&mut app, w);
    let solo = json!({"window": w, "layer": red.0});
    menus::invoke(&mut app, &ctx, "view.layerVisibility.showOnly", solo.clone()).unwrap();
    assert_eq!(pixel(&shown(&mut app, w)), [1.0, 0.0, 0.0, 1.0]);
    menus::invoke(&mut app, &ctx, "view.layerVisibility.showOnly", solo).unwrap();
    assert_eq!(*shown(&mut app, w), *original);
    for params in
        [json!({}), json!({"window": "bad"}), json!({"window": w, "layer": u64::MAX, "visible": true}), json!({"window": w, "layer": blue.0, "visible": "bad"})]
    {
        assert!(command(&mut app, &ctx, "view.layerVisibility", &params).is_err());
    }
    assert!(Arc::ptr_eq(&canonical, &app.session.active().unwrap().doc));
    assert_eq!(*shown(&mut app, w), *original);
    let state = serde_json::to_value(&app.ui).unwrap();
    assert!(state["windows"][0].get("visibility").is_none(), "display overrides never persist in document/workspace serialization");
}

#[test]
fn canonical_save_and_flat_export_bytes_ignore_secondary_visibility() {
    let (mut app, ctx, _, blue) = fixture();
    let before = app.session.active().unwrap().doc.clone();
    let opts = photocraft_io::ExportOptions::default();
    let formats = ["pcraft", "psd", "png"];
    let encoded: Vec<_> = formats.iter().map(|ext| photocraft_io::export(&before, ext, &opts).unwrap().bytes).collect();
    let w = window(&mut app, &ctx);
    app.ui.focused_window = Some(w);
    set(&mut app, &ctx, w, blue, false);
    app.with_document_window(w, |app| {
        crate::canvas::ensure_texture(app, &ctx, 0, None);
    });
    let after = &app.session.active().unwrap().doc;
    for (ext, bytes) in formats.into_iter().zip(encoded) {
        assert_eq!(photocraft_io::export(after, ext, &opts).unwrap().bytes, bytes, "{ext} stays canonical");
    }
}

#[test]
fn preview_keys_invalidate_projection_and_flattened_previews_cannot_bypass_overrides() {
    let (mut app, ctx, _, blue) = fixture();
    let w = window(&mut app, &ctx);
    set(&mut app, &ctx, w, blue, false);
    let canonical = app.session.active().unwrap().doc.clone();
    let (a, ka) = project_with_key(&mut app, w, canonical.clone(), 123);
    let (b, kb) = project_with_key(&mut app, w, canonical.clone(), 124);
    assert_ne!(ka, kb, "live-stroke preview changes invalidate even with the same source pointer");
    assert!(!Arc::ptr_eq(&a, &b));
    let mut flattened = (*canonical).clone();
    flattened.layers = vec![canonical.layers[0].clone()];
    let shown = app.with_document_window(w, |app| render_document(app, 0, Arc::new(flattened), 999).0).unwrap();
    assert_eq!(pixel(&shown), [1.0, 0.0, 0.0, 1.0]);
    assert!(app.view_composites.rendering_window.is_none());
}

#[test]
fn channel_overlays_have_distinct_view_caches_and_document_reopen_drops_old_state() {
    let (mut app, ctx, _, blue) = fixture();
    let w = window(&mut app, &ctx);
    set(&mut app, &ctx, w, blue, false);
    app.run("channel.target", json!({"channel": "red"})).unwrap();
    let main = crate::channel_view::ensure(&mut app, &ctx, 0).unwrap();
    let child = app.with_document_window(w, |app| crate::channel_view::ensure(app, &ctx, 0).unwrap()).unwrap();
    assert_ne!(main, child);
    assert_eq!(crate::channel_view::ensure(&mut app, &ctx, 0), Some(main));
    assert_eq!(app.with_document_window(w, |app| crate::channel_view::ensure(app, &ctx, 0)).unwrap(), Some(child));
    assert_eq!(app.view_composites.channels[&w].len(), 1);
    let canonical = app.session.active().unwrap().doc.clone();
    drop(app.session.close(0));
    app.sync_views();
    assert!(app.ui.windows.is_empty());
    assert!(app.view_composites.caches.is_empty());
    assert!(app.view_composites.channels.is_empty());
    assert!(app.view_composites.projections.is_empty());
    app.session.add_document((*canonical).clone(), None);
    app.sync_views();
    let reopened = window(&mut app, &ctx);
    assert!(shown(&mut app, reopened).layer(blue).unwrap().visible);
}

#[test]
fn visibility_projection_preserves_group_clipping_mask_and_blend_semantics() {
    let (mut app, ctx, red, blue) = fixture();
    app.session
        .edit("Mask and blend fixture", |doc, _| {
            let upper = doc.layer_mut(blue).unwrap();
            upper.opacity = 0.5;
            upper.blend = photocraft_color::BlendMode::Multiply;
            upper.clipped = true;
            let mut mask = photocraft_doc::LayerMask::reveal_all();
            mask.surface.write_pixel(0, 0, &[0.5]);
            upper.mask = Some(mask);
            let layers = std::mem::take(&mut doc.layers);
            doc.layers = vec![photocraft_doc::Layer::group("group", layers)];
            Ok(())
        })
        .unwrap();
    app.sync_views();
    let w = window(&mut app, &ctx);
    let original = app.session.active().unwrap().doc.clone();
    set(&mut app, &ctx, w, red, false);
    let projected = shown(&mut app, w);
    let mut expected = (*original).clone();
    expected.layer_mut(red).unwrap().visible = false;
    assert_eq!(pixel(&projected), pixel(&expected));
    let mask = &original.layer(blue).unwrap().mask.as_ref().unwrap().surface;
    let projected_mask = &projected.layer(blue).unwrap().mask.as_ref().unwrap().surface;
    for ((a, ta), (b, tb)) in mask.tiles().zip(projected_mask.tiles()) {
        assert_eq!(a, b);
        assert!(Arc::ptr_eq(ta, tb), "mask pixels are also shared");
    }
    assert!(Arc::ptr_eq(&original, &app.session.active().unwrap().doc));
}

#[test]
fn new_secondary_presentation_comes_from_main_regardless_of_focus_and_panel_target() {
    let (mut app, ctx, _, layer) = fixture();
    app.run("layer.newAdjustmentLayer.posterize", json!({})).unwrap();
    let adjustment = app.session.active().unwrap().active_layer.unwrap();
    let native = window(&mut app, &ctx);
    let selected = window(&mut app, &ctx);

    for (window, opacity, levels) in [(native, 0.25, 2), (selected, 0.5, 4)] {
        set(&mut app, &ctx, window, layer, true);
        for (id, params) in [
            ("view.layerOpacity", json!({"window": window, "layer": layer.0, "opacity": opacity})),
            ("view.layerBlend", json!({"window": window, "layer": layer.0, "blend": "multiply"})),
        ] {
            menus::invoke(&mut app, &ctx, id, params).unwrap();
        }
        adjustment_tests::change(&mut app, &ctx, window, adjustment, "commit", json!({"levels": levels}));
    }
    app.run("layer.setProps", json!({"layer": layer.0, "visible": false, "opacity": 0.75, "blend": "screen"})).unwrap();
    app.run("layer.setAdjustment", json!({"layer": adjustment.0, "levels": 12})).unwrap();
    app.ui.windows[0].view.flip_horizontal = true;
    app.ui.windows[1].view.flip_horizontal = true;
    let existing = app.ui.windows.clone();
    let canonical = app.session.active().unwrap().doc.clone();

    for (native_focus, panel_target) in [(None, None), (Some(native), Some(native)), (Some(native), Some(selected))] {
        let mut raw = egui::RawInput::default();
        raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().focused = Some(native_focus.is_none());
        if let Some(id) = native_focus {
            raw.viewports.insert(egui::ViewportId::from_hash_of(("docwin", id)), egui::ViewportInfo { focused: Some(true), ..Default::default() });
        }
        let mut output = ctx.run_ui(raw, |ui| app.update_view_focus(ui.ctx()));
        output.textures_delta.clear();
        assert_eq!(app.ui.focused_window, native_focus);
        // The Layers panel's View selector writes the same target field.
        app.ui.focused_window = panel_target;

        let new = window(&mut app, &ctx);
        let child = app.ui.windows.iter().find(|w| w.id == new).unwrap();
        assert!(!child.follow_main_flip);
        assert!(!child.view.flip_horizontal);
        let projected = shown(&mut app, new);
        for (_, _, expected) in canonical.walk() {
            let actual = projected.layer(expected.id).unwrap();
            assert_eq!((actual.visible, actual.opacity, actual.blend), (expected.visible, expected.opacity, expected.blend));
            if matches!(&expected.content, photocraft_doc::LayerContent::Adjustment(_)) {
                assert_eq!(actual.content, expected.content);
            }
        }
        assert_eq!(&app.ui.windows[..existing.len()], existing.as_slice());
    }
    assert!(Arc::ptr_eq(&canonical, &app.session.active().unwrap().doc));
}

#[test]
fn independent_settings_follow_canonical_and_restore_without_accepting_stale_edits() {
    use crate::view_visibility::adjustments as edit;
    let (mut app, ctx, layer, _) = fixture();
    app.run("layer.newAdjustmentLayer.posterize", json!({})).unwrap();
    let adjustment = app.session.active().unwrap().active_layer.unwrap();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    assert!(app.ui.windows.iter().all(|w| !w.follow_main_settings));
    let run = |app: &mut PhotocraftApp, id, p| menus::invoke(app, &ctx, id, p);
    set(&mut app, &ctx, a, layer, false);
    run(&mut app, "view.layerOpacity", json!({"window": a, "layer": layer.0, "opacity": 0.25})).unwrap();
    run(&mut app, "view.layerBlend", json!({"window": a, "layer": layer.0, "blend": "multiply"})).unwrap();
    adjustment_tests::change(&mut app, &ctx, a, adjustment, "commit", json!({"levels": 2}));
    let independent = shown(&mut app, a);
    let other = shown(&mut app, b);
    app.ui.windows[0].view.flip_horizontal = true;
    app.ui.focused_window = Some(a);
    let target = edit::Target::current(&mut app, adjustment).unwrap();
    edit::preview(&mut app, target, &json!({"levels": 3})).unwrap();
    let epoch = app.view_composites.editor.ui_epoch;
    app.ui.focused_window = Some(b); // Properties target differs from the checkbox's native window.
    set_follow_main_settings(&mut app, &ctx, a, true);
    assert_eq!(app.ui.focused_window, Some(b));
    assert!(app.view_composites.editor.target.is_none());

    app.ui.focused_window = Some(a);
    let before = app.session.active().unwrap().clone();
    assert!(settings_read_only(&app, Some(a)));
    assert!(edit::Target::current(&mut app, adjustment).is_none());
    for (id, p) in [
        ("view.layerVisibility", json!({"layer": layer.0, "visible": true})),
        ("view.layerVisibility.showOnly", json!({"layer": layer.0})),
        ("view.layerOpacity", json!({"layer": layer.0, "opacity": 0.5})),
        ("view.layerBlend", json!({"layer": layer.0, "blend": "screen"})),
        ("view.layerAdjustment", json!({"layer": adjustment.0, "params": {"levels": 6}})),
    ] {
        assert!(run(&mut app, id, p).is_err());
    }
    assert!(panel_action(&mut app, &ctx, Some(a), "layer.setProps", &json!({"layer": layer.0, "opacity": 0.5})));
    assert!(!edit::accepts_completion(&app, target, epoch));
    assert!(Arc::ptr_eq(&shown(&mut app, a), &before.doc));
    assert_eq!(before.history.past_len(), app.session.active().unwrap().history.past_len());

    app.run("layer.setProps", json!({"layer": layer.0, "visible": true, "opacity": 0.75, "blend": "screen"})).unwrap();
    app.run("layer.setAdjustment", json!({"layer": adjustment.0, "levels": 12})).unwrap();
    let canonical = app.session.active().unwrap().doc.clone();
    assert!(Arc::ptr_eq(&shown(&mut app, a), &canonical));
    let rendered = app.with_document_window(a, |app| render_document(app, 0, canonical.clone(), 23)).unwrap();
    assert!(Arc::ptr_eq(&rendered.0, &canonical));
    assert_eq!(rendered.1, 23);
    let c = window(&mut app, &ctx);
    assert!(!app.ui.windows.iter().find(|w| w.id == c).unwrap().follow_main_settings);
    assert_eq!(shown(&mut app, c).layer(adjustment).unwrap().content, canonical.layer(adjustment).unwrap().content);

    set_follow_main_settings(&mut app, &ctx, a, false);
    assert!(edit::commit(&mut app, target, &json!({"levels": 6}), false).is_err());
    let restored = shown(&mut app, a);
    let still_other = shown(&mut app, b);
    for id in [layer, adjustment] {
        assert_eq!(restored.layer(id), independent.layer(id));
        assert_eq!(still_other.layer(id), other.layer(id));
    }
    assert!(app.ui.windows[0].view.flip_horizontal);
    assert!(Arc::ptr_eq(&canonical, &app.session.active().unwrap().doc));
    app.ui.windows[0].open = false;
    retain(&mut app);
    assert!(!app.view_composites.projections.contains_key(&a));
}
