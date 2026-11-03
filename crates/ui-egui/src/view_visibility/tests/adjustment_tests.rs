use super::*;
use crate::view_visibility::adjustments as edit;
use photocraft_doc::{Adjustment, LayerContent};

#[test]
fn secondary_levels_histograms_are_targeted_cached_and_side_effect_free() {
    use photocraft_doc::adjust::ToneSpace;
    let (mut app, ctx, below) = fixture_adjustment("posterize");
    app.run("layer.newAdjustmentLayer.levels", json!({})).unwrap();
    let levels = app.session.active().unwrap().active_layer.unwrap();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    change(&mut app, &ctx, a, below, "commit", json!({"levels": 2}));
    change(&mut app, &ctx, b, below, "commit", json!({"levels": 16}));
    let before = app.session.active().unwrap().clone();
    let ha = histograms::get(&mut app, a, levels, ToneSpace::Rgb);
    let hb = histograms::get(&mut app, b, levels, ToneSpace::Rgb);
    assert_ne!(ha.ch, hb.ch);
    assert!(Arc::ptr_eq(&ha, &histograms::get(&mut app, a, levels, ToneSpace::Rgb)));
    change(&mut app, &ctx, a, levels, "commit", json!({"inBlack": 48, "outWhite": 190}));
    assert!(Arc::ptr_eq(&ha, &histograms::get(&mut app, a, levels, ToneSpace::Rgb)));
    app.ui.focused_window = Some(a);
    change(&mut app, &ctx, a, levels, "preview", json!({"gamma": 2.0}));
    assert!(Arc::ptr_eq(&ha, &histograms::get(&mut app, a, levels, ToneSpace::Rgb)));
    change(&mut app, &ctx, a, levels, "cancel", json!({}));
    change(&mut app, &ctx, a, below, "commit", json!({"levels": 4}));
    let changed = histograms::get(&mut app, a, levels, ToneSpace::Rgb);
    assert!(!Arc::ptr_eq(&ha, &changed));
    assert_ne!(ha.ch, changed.ch);
    assert!(Arc::ptr_eq(&hb, &histograms::get(&mut app, b, levels, ToneSpace::Rgb)));
    for (command, params) in [
        ("view.layerVisibility", json!({"window": a, "layer": below.0, "visible": false})),
        ("view.layerOpacity", json!({"window": a, "layer": below.0, "opacity": 0.5})),
        ("view.layerBlend", json!({"window": a, "layer": below.0, "blend": "multiply"})),
    ] {
        let previous = histograms::get(&mut app, a, levels, ToneSpace::Rgb);
        menus::invoke(&mut app, &ctx, command, params).unwrap();
        let next = histograms::get(&mut app, a, levels, ToneSpace::Rgb);
        assert!(!Arc::ptr_eq(&previous, &next));
        let expected = crate::tone::compute_histograms(&shown(&mut app, a), crate::tone::HistSource::BelowLayer(levels), ToneSpace::Rgb);
        assert_eq!(next.ch, expected.ch);
        assert!(Arc::ptr_eq(&hb, &histograms::get(&mut app, b, levels, ToneSpace::Rgb)));
    }
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!((before.revision, before.saved_revision, before.last_damage), (after.revision, after.saved_revision, after.last_damage));
    assert_eq!(serde_json::to_value(before.history.entries()).unwrap(), serde_json::to_value(after.history.entries()).unwrap());
    assert!(app.live_adjust.is_none());
    assert!(app.tone_hist.is_none());
    app.ui.windows.iter_mut().find(|w| w.id == a).unwrap().open = false;
    retain(&mut app);
    assert!(!app.view_composites.histograms.contains_key(&a));
    assert!(app.view_composites.histograms.contains_key(&b));
}

#[test]
fn secondary_histogram_excludes_upper_layers_and_refreshes_after_shared_pixels() {
    use photocraft_doc::adjust::ToneSpace;
    let (mut app, ctx, levels) = fixture_adjustment("levels");
    let a = window(&mut app, &ctx);
    let h = histograms::get(&mut app, a, levels, ToneSpace::Rgb);
    app.run("layer.new.layer", json!({})).unwrap();
    let upper = app.session.active().unwrap().active_layer.unwrap();
    app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
    let upper_hist = histograms::get(&mut app, a, levels, ToneSpace::Rgb);
    assert_eq!(h.ch, upper_hist.ch, "upper layers are not adjustment input");
    let expected = crate::tone::compute_histograms(&shown(&mut app, a), crate::tone::HistSource::BelowLayer(levels), ToneSpace::Rgb);
    assert_ne!(h.ch, expected.ch, "Main's historical BelowLayer semantics include upper layers");
    let bottom = app.session.active().unwrap().doc.walk().into_iter().rev().find(|(_, _, l)| l.id != upper && l.surface().is_some()).unwrap().2.id;
    app.run("layer.select", json!({"layer": bottom.0})).unwrap();
    app.run("edit.fill", json!({"color": "#00ff00"})).unwrap();
    let changed = histograms::get(&mut app, a, levels, ToneSpace::Rgb);
    assert_ne!(h.ch, changed.ch);
    for (space, channels) in [(ToneSpace::Rgb, 4), (ToneSpace::Cmyk, 5), (ToneSpace::Lab, 3)] {
        let h = histograms::get(&mut app, a, levels, space);
        assert_eq!(h.ch.len(), channels);
        assert!(h.ch.iter().all(|channel| channel.iter().sum::<u32>() == 16 * 12));
        assert!(Arc::ptr_eq(&h, &histograms::get(&mut app, a, levels, space)));
    }
}

pub(super) fn fixture_adjustment(kind: &str) -> (PhotocraftApp, egui::Context, LayerId) {
    let (mut app, ctx, _, _) = fixture();
    app.run("edit.fill", json!({"color": "#3975ad"})).unwrap();
    app.run(&format!("layer.newAdjustmentLayer.{kind}"), json!({})).unwrap();
    let layer = app.session.active().unwrap().active_layer.unwrap();
    if kind == "posterize" {
        app.run("layer.setAdjustment", json!({"layer": layer.0, "levels": 8})).unwrap();
    }
    (app, ctx, layer)
}

pub(super) fn change(app: &mut PhotocraftApp, ctx: &egui::Context, window: u64, layer: LayerId, action: &str, params: Value) -> Value {
    menus::invoke(app, ctx, "view.layerAdjustment", json!({"window": window, "layer": layer.0, "action": action, "params": params})).unwrap()
}

pub(super) fn value(app: &mut PhotocraftApp, window: u64, layer: LayerId) -> Adjustment {
    match &shown(app, window).layer(layer).unwrap().content {
        LayerContent::Adjustment(value) => value.clone(),
        _ => panic!("not an adjustment"),
    }
}

#[test]
fn adjustment_snapshots_inherit_main_and_isolate_history_dirty_save_export() {
    let (mut app, ctx, id) = fixture_adjustment("posterize");
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    let st = app.session.active_mut().unwrap();
    st.saved_revision = st.revision;
    let before = app.session.active().unwrap().clone();
    let history = serde_json::to_value(before.history.entries()).unwrap();
    let opts = photocraft_io::ExportOptions::default();
    let formats = ["pcraft", "psd", "png"];
    let encoded: Vec<_> = formats.iter().map(|ext| photocraft_io::export(&before.doc, ext, &opts).unwrap().bytes).collect();
    change(&mut app, &ctx, a, id, "commit", json!({"levels": 4}));
    change(&mut app, &ctx, b, id, "commit", json!({"levels": 16}));
    assert_eq!(value(&mut app, a, id), Adjustment::Posterize { levels: 4 });
    assert_eq!(value(&mut app, b, id), Adjustment::Posterize { levels: 16 });
    assert_ne!(pixel(&shown(&mut app, a)), pixel(&shown(&mut app, b)));
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!((before.revision, before.saved_revision, before.last_damage), (after.revision, after.saved_revision, after.last_damage));
    assert_eq!(history, serde_json::to_value(after.history.entries()).unwrap());
    for (ext, expected) in formats.into_iter().zip(encoded) {
        assert_eq!(photocraft_io::export(&after.doc, ext, &opts).unwrap().bytes, expected);
    }
    app.run("layer.setAdjustment", json!({"layer": id.0, "levels": 12})).unwrap();
    assert_eq!(value(&mut app, a, id), Adjustment::Posterize { levels: 4 });
    assert_eq!(value(&mut app, b, id), Adjustment::Posterize { levels: 16 });
    app.ui.focused_window = Some(a);
    let c = window(&mut app, &ctx);
    assert_eq!(value(&mut app, c, id), Adjustment::Posterize { levels: 12 });
    change(&mut app, &ctx, a, id, "reset", json!({}));
    assert_eq!(value(&mut app, a, id), photocraft_engine::adjust_params::default_for("posterize", photocraft_doc::ColorMode::Rgb).unwrap());
}

#[test]
fn adjustment_previews_cancel_or_commit_only_the_captured_target() {
    let (mut app, ctx, id) = fixture_adjustment("posterize");
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    app.ui.focused_window = Some(a);
    let ta = edit::Target::current(&mut app, id).unwrap();
    edit::preview(&mut app, ta, &json!({"levels": 3})).unwrap();
    assert_eq!(value(&mut app, a, id), Adjustment::Posterize { levels: 3 });
    assert_eq!(value(&mut app, b, id), Adjustment::Posterize { levels: 8 });
    assert!(app.live_adjust.is_none());
    assert_eq!(app.session.active().unwrap().doc.layer(id).unwrap().content, LayerContent::Adjustment(Adjustment::Posterize { levels: 8 }));
    app.ui.focused_window = Some(b);
    edit::follow(&mut app, &ctx);
    assert_eq!(value(&mut app, a, id), Adjustment::Posterize { levels: 8 });
    assert!(app.view_composites.editor.target.is_none());
    change(&mut app, &ctx, b, id, "preview", json!({"levels": 5}));
    change(&mut app, &ctx, b, id, "cancel", json!({}));
    assert_eq!(value(&mut app, b, id), Adjustment::Posterize { levels: 8 });
    app.ui.focused_window = None;
    let main = edit::Target::current(&mut app, id).unwrap();
    let steps = app.session.active().unwrap().history.past_len();
    edit::preview(&mut app, main, &json!({"levels": 6})).unwrap();
    assert!(app.live_adjust.is_some());
    let child = app
        .with_document_window(a, |app| {
            let (source, key) = crate::canvas::display_doc(app, 0);
            render_document(app, 0, source, key).0
        })
        .unwrap();
    assert_eq!(child.layer(id).unwrap().content, LayerContent::Adjustment(Adjustment::Posterize { levels: 8 }));
    edit::commit(&mut app, main, &json!({"levels": 6}), false).unwrap();
    assert_eq!(app.session.active().unwrap().history.past_len(), steps + 1);
    assert!(app.live_adjust.is_none());
    assert_ne!(main.memory(), ta.memory());
}

#[test]
fn adjustment_cache_noops_preview_commit_and_tile_identity() {
    let (mut app, ctx, id) = fixture_adjustment("posterize");
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    let canonical = app.session.active().unwrap().doc.clone();
    let main = crate::canvas::ensure_texture(&mut app, &ctx, 0, None).unwrap().0;
    let texture = |app: &mut PhotocraftApp, w| app.with_document_window(w, |app| crate::canvas::ensure_texture(app, &ctx, 0, None).unwrap().0).unwrap();
    texture(&mut app, a);
    texture(&mut app, b);
    let kb = app.view_composites.caches[&b][&(canonical.id, 0)].tex_preview_key;
    app.ui.focused_window = Some(a);
    change(&mut app, &ctx, a, id, "preview", json!({"levels": 4}));
    texture(&mut app, a);
    let preview = shown(&mut app, a);
    change(&mut app, &ctx, a, id, "commit", json!({"levels": 4}));
    app.perf.last_refresh = "";
    texture(&mut app, a);
    texture(&mut app, b);
    assert!(Arc::ptr_eq(&preview, &shown(&mut app, a)), "commit keeps the effective preview handle");
    change(&mut app, &ctx, a, id, "commit", json!({"levels": 4}));
    texture(&mut app, a);
    assert_eq!(app.perf.last_refresh, "");
    assert_eq!(app.view_composites.caches[&b][&(canonical.id, 0)].tex_preview_key, kb);
    assert_eq!(crate::canvas::ensure_texture(&mut app, &ctx, 0, None).unwrap().0, main);
    for (_, _, layer) in canonical.walk() {
        if let Some(surface) = layer.surface() {
            for ((ca, ta), (cb, tb)) in surface.tiles().zip(preview.layer(layer.id).unwrap().surface().unwrap().tiles()) {
                assert_eq!(ca, cb);
                assert!(Arc::ptr_eq(ta, tb));
            }
        }
    }
    assert!(app.redraws.is_empty());
}

#[test]
fn adjustment_levels_channels_modes_masks_and_groups_use_existing_compositor() {
    use photocraft_doc::{ColorMode, Layer, LayerMask};
    for mode in ["rgb", "gray", "cmyk", "lab"] {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": 8, "height": 6, "mode": mode})).unwrap();
        app.run("edit.fill", json!({"color": "#3975ad"})).unwrap();
        app.run("layer.newAdjustmentLayer.levels", json!({})).unwrap();
        let id = app.session.active().unwrap().active_layer.unwrap();
        app.session
            .edit("Mask/group fixture", |doc, _| {
                let layer = doc.layer_mut(id).unwrap();
                layer.clipped = true;
                let mut mask = LayerMask::reveal_all();
                mask.surface.write_pixel(0, 0, &[0.5]);
                layer.mask = Some(mask);
                let children = std::mem::take(&mut doc.layers);
                doc.layers = vec![Layer::group("Group", children)];
                Ok(())
            })
            .unwrap();
        app.sync_views();
        let ctx = egui::Context::default();
        let w = window(&mut app, &ctx);
        let params = match mode {
            "rgb" => json!({"inBlack": 12, "gamma": 1.2, "outWhite": 220, "red": {"outBlack": 25}, "blue": {"inWhite": 210}}),
            "cmyk" => json!({"cyan": {"gamma": 1.3}, "black": {"outWhite": 200}}),
            "lab" => json!({"inBlack": 10, "a": {"gamma": 1.3}, "b": {"outWhite": 200}}),
            _ => json!({"gamma": 1.4, "outBlack": 20}),
        };
        change(&mut app, &ctx, w, id, "commit", params.clone());
        let canonical = app.session.active().unwrap().doc.clone();
        let projected = shown(&mut app, w);
        let mut reference = (*canonical).clone();
        let LayerContent::Adjustment(base) = &reference.layer(id).unwrap().content else { panic!() };
        let expected = photocraft_engine::adjust_params::from_params("levels", &params, Some(base), canonical.mode).unwrap();
        reference.layer_mut(id).unwrap().content = LayerContent::Adjustment(expected.clone());
        assert_eq!(value(&mut app, w, id), expected);
        assert_eq!(photocraft_compose::render(&projected, projected.bounds()).px, photocraft_compose::render(&reference, reference.bounds()).px);
        let ma = canonical.layer(id).unwrap().mask.as_ref().unwrap().surface.tiles().next().unwrap().1;
        let mb = projected.layer(id).unwrap().mask.as_ref().unwrap().surface.tiles().next().unwrap().1;
        assert!(Arc::ptr_eq(ma, mb));
        let space = crate::dialogs::adjust_editors::space_of(&params);
        let input = histograms::get(&mut app, w, id, space);
        let expected_input = crate::tone::compute_histograms(&projected, crate::tone::HistSource::BelowLayer(id), space);
        assert_eq!(input.ch, expected_input.ch, "group, clipping, mask and color-mode semantics reuse the compositor");
        change(&mut app, &ctx, w, id, "reset", json!({}));
        assert!(Arc::ptr_eq(&input, &histograms::get(&mut app, w, id, space)));
        assert_eq!(value(&mut app, w, id), photocraft_engine::adjust_params::default_for("levels", canonical.mode).unwrap());
        assert!(matches!(canonical.mode, ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Cmyk | ColorMode::Lab));
    }
}

#[test]
fn adjustment_lifecycle_invalidates_stale_targets_and_color_domains() {
    let (mut app, ctx, id) = fixture_adjustment("levels");
    let w = window(&mut app, &ctx);
    app.ui.focused_window = Some(w);
    let old = edit::Target::current(&mut app, id).unwrap();
    change(&mut app, &ctx, w, id, "commit", json!({"gamma": 1.8}));
    app.session
        .edit("Rename/reorder", |doc, _| {
            doc.layer_mut(id).unwrap().name = "Renamed".into();
            doc.layers.reverse();
            Ok(())
        })
        .unwrap();
    app.sync_views();
    assert_eq!(edit::Target::current(&mut app, id), Some(old));
    app.session
        .edit("Replace type", |doc, _| {
            doc.layer_mut(id).unwrap().content = LayerContent::Adjustment(Adjustment::Posterize { levels: 11 });
            Ok(())
        })
        .unwrap();
    app.sync_views();
    assert!(edit::preview(&mut app, old, &json!({"gamma": 2.0})).is_err());
    assert_eq!(value(&mut app, w, id), Adjustment::Posterize { levels: 11 });
    let replaced = edit::Target::current(&mut app, id).unwrap();
    assert_ne!(replaced.memory(), old.memory());
    app.session
        .edit("Replace with Levels", |doc, _| {
            doc.layer_mut(id).unwrap().content = LayerContent::Adjustment(photocraft_engine::adjust_params::default_for("levels", doc.mode).unwrap());
            Ok(())
        })
        .unwrap();
    app.sync_views();
    let rgb = edit::Target::current(&mut app, id).unwrap();
    // Convert mode without flattening to retain the layer's identity.
    app.run("image.mode.grayscale", json!({"flatten": false})).unwrap();
    app.sync_views();
    assert!(edit::preview(&mut app, rgb, &json!({"gamma": 2.0})).is_err());
    if app.session.active().unwrap().doc.layer(id).is_some() {
        app.session
            .edit("Convert to raster", |doc, _| {
                doc.layer_mut(id).unwrap().content = LayerContent::Raster(photocraft_raster::Surface::new(doc.pixel_format()));
                Ok(())
            })
            .unwrap();
        app.sync_views();
        assert!(!app.ui.windows[0].visibility.adjustments.contains_key(&id));
    }
    app.ui.windows[0].open = false;
    retain(&mut app);
    assert!(app.view_composites.projections.is_empty());
    assert!(app.view_composites.editor.target.is_none());
}

#[test]
fn adjustment_resources_are_shared_and_unsupported_editors_are_rejected() {
    let (mut app, ctx, id) = fixture_adjustment("posterize");
    let lut = Arc::new(vec![0.5; 24]);
    app.session
        .edit("LUT fixture", |doc, _| {
            doc.layer_mut(id).unwrap().content = LayerContent::Adjustment(Adjustment::ColorLookup {
                name: "test".into(),
                lut: Some(lut.clone()),
                size: 2,
                tetrahedral: true,
                dither: false,
                source: None,
            });
            Ok(())
        })
        .unwrap();
    app.sync_views();
    let w = window(&mut app, &ctx);
    let shown = shown(&mut app, w);
    let LayerContent::Adjustment(Adjustment::ColorLookup { lut: Some(other), .. }) = &shown.layer(id).unwrap().content else { panic!() };
    assert!(Arc::ptr_eq(&lut, other));
    assert!(edit::command(&mut app, &ctx, &json!({"window": w, "layer": id.0, "params": {"lut": "other"}})).is_err());
}

#[test]
fn adjustment_editor_ui_switches_values_disables_sampling_and_cancels_held_drag() {
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    let (mut app, ctx, id) = fixture_adjustment("posterize");
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    change(&mut app, &ctx, a, id, "commit", json!({"levels": 4}));
    change(&mut app, &ctx, b, id, "commit", json!({"levels": 16}));
    app.ui.focused_window = Some(a);
    let mut h = Harness::builder().with_size(egui::vec2(360.0, 560.0)).build_ui_state(
        move |ui, app: &mut PhotocraftApp| {
            edit::follow(app, ui.ctx());
            let doc = app.session.active().unwrap().doc.clone();
            let LayerContent::Adjustment(adj) = &doc.layer(id).unwrap().content else { return };
            crate::adjust_editors::layer_editor(app, ui, id, adj);
        },
        app,
    );
    PhotocraftApp::setup_context(&h.ctx, Default::default());
    h.run_steps(4);
    let number =
        |h: &Harness<'_, PhotocraftApp>| h.get_all_by_role(egui::accesskit::Role::SpinButton).next().unwrap().accesskit_node().numeric_value().unwrap();
    assert_eq!(number(&h), 4.0);
    h.state_mut().ui.focused_window = Some(b);
    h.run_steps(2);
    assert_eq!(number(&h), 16.0);
    h.state_mut().ui.focused_window = Some(a);
    h.run_steps(2);
    let label = h
        .query_all(egui_kittest::kittest::by().label("Levels").include_labels())
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Label)
        .unwrap()
        .rect();
    let from = egui::pos2(label.left() + 30.0, label.bottom() + 14.0);
    h.hover_at(from);
    h.drag_at(from);
    h.run_steps(1);
    h.hover_at(from + egui::vec2(100.0, 0.0));
    h.run_steps(1);
    assert!(h.state().live_adjust.is_none());
    h.state_mut().ui.focused_window = Some(b);
    h.run_steps(2);
    h.drop_at(from + egui::vec2(100.0, 0.0));
    h.run_steps(2);
    assert_eq!(number(&h), 16.0);
    assert_eq!(value(h.state_mut(), a, id), Adjustment::Posterize { levels: 4 });
    h.state_mut().ui.focused_window = None;
    h.run_steps(2);
    assert_eq!(number(&h), 8.0);
    // Replace the fixture's type to draw Levels using the same actual editor host.
    h.state_mut()
        .session
        .edit("Levels fixture", |doc, _| {
            doc.layer_mut(id).unwrap().content = LayerContent::Adjustment(photocraft_engine::adjust_params::default_for("levels", doc.mode).unwrap());
            Ok(())
        })
        .unwrap();
    h.state_mut().sync_views();
    h.state_mut().ui.focused_window = Some(b);
    h.run_steps(3);
    assert!(h.state().view_composites.histograms.contains_key(&b));
    assert!(h.get_by_label("Auto").accesskit_node().is_disabled());
    for label in ["Set Black Point", "Set Neutral Gray Point", "Set White Point"] {
        assert!(h.get_by_label(label).accesskit_node().is_disabled());
    }
    assert!(crate::layer_picker::armed(h.state()).is_none());
    let tb = edit::Target::current(h.state_mut(), id).unwrap();
    h.ctx.data_mut(|d| d.insert_temp(tb.memory().with("levels-ch"), 2usize));
    h.state_mut().ui.focused_window = Some(a);
    h.run_steps(2);
    let ta = edit::Target::current(h.state_mut(), id).unwrap();
    assert_ne!(ta.memory(), tb.memory());
    assert_eq!(h.ctx.data(|d| d.get_temp::<usize>(ta.memory().with("levels-ch"))).unwrap_or(0), 0);
    h.state_mut().ui.focused_window = Some(b);
    h.run_steps(2);
    assert_eq!(h.ctx.data(|d| d.get_temp::<usize>(tb.memory().with("levels-ch"))), Some(2));
}
