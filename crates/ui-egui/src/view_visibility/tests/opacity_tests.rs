use super::*;

fn opacity(app: &mut PhotocraftApp, ctx: &egui::Context, w: u64, layer: LayerId, value: f64) -> Value {
    menus::invoke(app, ctx, "view.layerOpacity", json!({"window": w, "layer": layer.0, "opacity": value})).unwrap()
}

#[test]
fn opacity_is_independent_and_does_not_dirty_history_or_canonical_exports() {
    let (mut app, ctx, _, blue) = fixture();
    app.run("layer.setProps", json!({"layer": blue.0, "opacity": 0.6})).unwrap();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    assert_eq!(shown(&mut app, a).layer(blue).unwrap().opacity, 0.6);
    let st = app.session.active_mut().unwrap();
    st.saved_revision = st.revision;
    let before = app.session.active().unwrap().clone();
    let history = serde_json::to_value(before.history.entries()).unwrap();
    let opts = photocraft_io::ExportOptions::default();
    let formats = ["pcraft", "psd", "png"];
    let bytes: Vec<_> = formats.iter().map(|ext| photocraft_io::export(&before.doc, ext, &opts).unwrap().bytes).collect();
    opacity(&mut app, &ctx, a, blue, 0.25);
    opacity(&mut app, &ctx, b, blue, 0.75);
    assert_ne!(pixel(&shown(&mut app, a)), pixel(&shown(&mut app, b)));
    assert_eq!(shown(&mut app, a).layer(blue).unwrap().opacity, 0.25);
    assert_eq!(shown(&mut app, b).layer(blue).unwrap().opacity, 0.75);
    app.ui.focused_window = Some(a);
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!((after.revision, after.saved_revision, after.last_damage), (before.revision, before.saved_revision, before.last_damage));
    assert_eq!(after.revision, after.saved_revision, "view changes do not dirty a clean document");
    assert_eq!(history, serde_json::to_value(after.history.entries()).unwrap());
    for (ext, encoded) in formats.into_iter().zip(bytes) {
        assert_eq!(photocraft_io::export(&after.doc, ext, &opts).unwrap().bytes, encoded, "{ext} stays canonical");
    }
    app.run("layer.setProps", json!({"layer": blue.0, "opacity": 0.4})).unwrap();
    assert_eq!(shown(&mut app, a).layer(blue).unwrap().opacity, 0.25);
    assert_eq!(shown(&mut app, b).layer(blue).unwrap().opacity, 0.75);
    let c = window(&mut app, &ctx);
    assert_eq!(shown(&mut app, c).layer(blue).unwrap().opacity, 0.4, "new view inherits canonical, not focused view");
}

#[test]
fn opacity_cache_changes_only_its_view_and_equal_effective_values_reuse_it() {
    let (mut app, ctx, _, blue) = fixture();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    let canonical = app.session.active().unwrap().doc.clone();
    let main = crate::canvas::ensure_texture(&mut app, &ctx, 0, None).unwrap().0;
    let texture = |app: &mut PhotocraftApp, id| app.with_document_window(id, |app| crate::canvas::ensure_texture(app, &ctx, 0, None).unwrap().0).unwrap();
    texture(&mut app, a);
    texture(&mut app, b);
    let b_before = shown(&mut app, b);
    let key = app.view_composites.caches[&b][&(canonical.id, 0)].tex_preview_key;
    opacity(&mut app, &ctx, a, blue, 0.5);
    texture(&mut app, a);
    app.perf.last_refresh = "";
    assert!(!opacity(&mut app, &ctx, a, blue, 0.50000000001)["changed"].as_bool().unwrap());
    texture(&mut app, a);
    texture(&mut app, b);
    assert_eq!(crate::canvas::ensure_texture(&mut app, &ctx, 0, None).unwrap().0, main);
    assert_eq!(app.perf.last_refresh, "", "no unchanged composite is recomputed");
    assert!(Arc::ptr_eq(&b_before, &shown(&mut app, b)));
    assert_eq!(app.view_composites.caches[&b][&(canonical.id, 0)].tex_preview_key, key);
    let projected = shown(&mut app, a);
    for (_, _, l) in canonical.walk() {
        if let Some(s) = l.surface() {
            for ((ta, a), (tb, b)) in s.tiles().zip(projected.layer(l.id).unwrap().surface().unwrap().tiles()) {
                assert_eq!(ta, tb);
                assert!(Arc::ptr_eq(a, b));
            }
        }
    }
    app.run("edit.fill", json!({"color": "#00ff00"})).unwrap();
    texture(&mut app, a);
    texture(&mut app, b);
    let revision = app.session.active().unwrap().revision;
    for w in [a, b] {
        assert_eq!(app.view_composites.caches[&w][&(canonical.id, 0)].tex_revision, revision);
    }
    assert_ne!(pixel(&shown(&mut app, a)), pixel(&shown(&mut app, b)));
    assert!(app.redraws.is_empty());
}

#[test]
fn panel_batch_opacity_including_background_is_display_only_and_validation_is_atomic() {
    let (mut app, ctx, red, blue) = fixture();
    let w = window(&mut app, &ctx);
    app.ui.focused_window = Some(w);
    let canonical = app.session.active().unwrap().doc.clone();
    let background = canonical.layers[0].id;
    let cameras = (app.ui.views.clone(), app.ui.windows[0].view.clone());
    assert!(panel_action(&mut app, &ctx, Some(w), "layer.setProps", &json!({"layers": [background.0, red.0, blue.0], "opacity": 0.25, "coalesce": "slider"})));
    for layer in [background, red, blue] {
        assert_eq!(focused_opacity(&app, layer), Some(0.25));
        assert_eq!(shown(&mut app, w).layer(layer).unwrap().opacity, 0.25);
    }
    assert!(Arc::ptr_eq(&canonical, &app.session.active().unwrap().doc));
    assert_eq!(cameras, (app.ui.views.clone(), app.ui.windows[0].view.clone()));
    let before = shown(&mut app, w);
    for params in [
        json!({"window": w, "layer": blue.0, "opacity": -0.01}),
        json!({"window": w, "layer": blue.0, "opacity": 1.01}),
        json!({"window": w, "layer": blue.0, "opacity": "bad"}),
        json!({"window": w, "layers": [blue.0, u64::MAX], "opacity": 0.9}),
        json!({"window": w, "layers": [], "opacity": 0.9}),
        json!({"window": w, "layer": blue.0, "opacity": null}),
    ] {
        assert!(command(&mut app, &ctx, "view.layerOpacity", &params).is_err());
        assert!(Arc::ptr_eq(&before, &shown(&mut app, w)));
    }
    app.ui.focused_window = None;
    assert_eq!(focused_opacity(&app, blue), None);
    assert!(!panel_action(&mut app, &ctx, None, "layer.setProps", &json!({"layer": blue.0, "opacity": 0.6})));
    app.run("layer.setProps", json!({"layer": blue.0, "opacity": 0.6})).unwrap();
    assert_eq!(app.session.active().unwrap().doc.layer(blue).unwrap().opacity, 0.6);
    assert_eq!(shown(&mut app, w).layer(blue).unwrap().opacity, 0.25);
}

#[test]
fn adjustments_match_existing_compositor_at_each_view_opacity() {
    use photocraft_doc::{Adjustment, Layer, LayerContent};
    let mut color = Adjustment::default_hue_saturation();
    if let Adjustment::HueSaturation { hue, saturation, .. } = &mut color {
        *hue = 30.0;
        *saturation = -25.0;
    }
    let adjustments = [Adjustment::BlackWhite { weights: [0.4, 0.6, 0.4, 0.6, 0.2, 0.8], tint: None }, Adjustment::Posterize { levels: 3 }, color];
    for adjustment in adjustments {
        let (mut app, ctx, _, _) = fixture();
        app.run("edit.fill", json!({"color": "#3975ad"})).unwrap();
        let mut layer = Layer::raster("Adjustment", app.session.active().unwrap().doc.pixel_format());
        let id = layer.id;
        layer.content = LayerContent::Adjustment(adjustment);
        app.session
            .edit("Adjustment fixture", |doc, _| {
                doc.layers.push(layer.clone());
                Ok(())
            })
            .unwrap();
        app.sync_views();
        let canonical = app.session.active().unwrap().doc.clone();
        let a = window(&mut app, &ctx);
        let b = window(&mut app, &ctx);
        let mut results = Vec::new();
        for value in [0.0, 0.25, 0.5, 1.0] {
            opacity(&mut app, &ctx, a, id, value);
            let mut reference = (*canonical).clone();
            reference.layer_mut(id).unwrap().opacity = value as f32;
            let actual = pixel(&shown(&mut app, a));
            assert_eq!(actual, pixel(&reference));
            results.push(actual);
            assert_eq!(shown(&mut app, b).layer(id).unwrap().opacity, 1.0);
        }
        assert_ne!(results[0], results[3], "non-identity adjustment verifies the endpoint difference");
        set(&mut app, &ctx, a, id, false);
        let mut hidden = (*canonical).clone();
        hidden.layer_mut(id).unwrap().visible = false;
        assert_eq!(pixel(&shown(&mut app, a)), pixel(&hidden), "visibility remains independent of opacity");
        assert!(Arc::ptr_eq(&canonical, &app.session.active().unwrap().doc));
    }
}

#[test]
fn opacity_follows_ids_through_tree_edits_and_is_removed_with_views() {
    let (mut app, ctx, red, blue) = fixture();
    let w = window(&mut app, &ctx);
    opacity(&mut app, &ctx, w, blue, 0.3);
    app.run("layer.new.layer", json!({"name": "added"})).unwrap();
    let added = app.session.active().unwrap().active_layer.unwrap();
    assert_eq!(shown(&mut app, w).layer(added).unwrap().opacity, 1.0);
    app.session
        .edit("Tree fixture", |doc, _| {
            doc.layer_mut(blue).unwrap().name = "Renamed".into();
            let layers = std::mem::take(&mut doc.layers);
            doc.layers = vec![photocraft_doc::Layer::group("Group", layers.into_iter().rev().collect())];
            Ok(())
        })
        .unwrap();
    app.sync_views();
    assert_eq!(shown(&mut app, w).layer(blue).unwrap().opacity, 0.3);
    let group = app.session.active().unwrap().doc.layers[0].id;
    opacity(&mut app, &ctx, w, group, 0.4);
    let canonical = app.session.active().unwrap().doc.clone();
    let mut expected = (*canonical).clone();
    expected.layer_mut(group).unwrap().opacity = 0.4;
    expected.layer_mut(blue).unwrap().opacity = 0.3;
    assert_eq!(pixel(&shown(&mut app, w)), pixel(&expected));
    app.run("layer.delete", json!({"layer": red.0})).unwrap();
    assert!(!app.ui.windows[0].visibility.opacity.contains_key(&red));
    app.with_document_window(w, |app| {
        crate::canvas::ensure_texture(app, &ctx, 0, None);
    });
    let persisted = serde_json::to_value(&app.ui).unwrap();
    assert!(persisted["windows"][0].get("visibility").is_none());
    app.ui.windows[0].open = false;
    retain(&mut app);
    assert!(app.view_composites.caches.is_empty());
    assert!(app.view_composites.projections.is_empty());
    assert!(command(&mut app, &ctx, "view.layerOpacity", &json!({"window": w, "layer": blue.0, "opacity": 0.2})).is_err());
}
