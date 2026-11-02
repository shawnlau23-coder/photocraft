use super::*;
use photocraft_color::BlendMode;

fn blend(app: &mut PhotocraftApp, ctx: &egui::Context, w: u64, layer: LayerId, mode: &str) -> Value {
    menus::invoke(app, ctx, "view.layerBlend", json!({"window": w, "layer": layer.0, "blend": mode})).unwrap()
}

#[test]
fn blend_views_inherit_canonical_and_diverge_without_history_dirty_or_export_changes() {
    let (mut app, ctx, _, blue) = fixture();
    app.run("edit.fill", json!({"color": "#3975ad"})).unwrap();
    app.run("layer.setProps", json!({"layer": blue.0, "blend": "Multiply"})).unwrap();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    assert_eq!(shown(&mut app, a).layer(blue).unwrap().blend, BlendMode::Multiply);
    let st = app.session.active_mut().unwrap();
    st.saved_revision = st.revision;
    let before = app.session.active().unwrap().clone();
    let history = serde_json::to_value(before.history.entries()).unwrap();
    let opts = photocraft_io::ExportOptions::default();
    let formats = ["pcraft", "psd", "png"];
    let bytes: Vec<_> = formats.iter().map(|ext| photocraft_io::export(&before.doc, ext, &opts).unwrap().bytes).collect();
    blend(&mut app, &ctx, a, blue, "Screen");
    blend(&mut app, &ctx, b, blue, "Difference");
    assert_ne!(pixel(&shown(&mut app, a)), pixel(&shown(&mut app, b)));
    let after = app.session.active().unwrap();
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!((after.revision, after.saved_revision, after.last_damage), (before.revision, before.saved_revision, before.last_damage));
    assert_eq!(after.revision, after.saved_revision);
    assert_eq!(history, serde_json::to_value(after.history.entries()).unwrap());
    for (ext, expected) in formats.into_iter().zip(bytes) {
        assert_eq!(photocraft_io::export(&after.doc, ext, &opts).unwrap().bytes, expected, "{ext} stays canonical");
    }
    // Main edits retain their original history behavior, without replacing existing view modes.
    assert!(!panel_action(&mut app, &ctx, None, "layer.setProps", &json!({"layer": blue.0, "blend": "Overlay"})));
    app.run("layer.setProps", json!({"layer": blue.0, "blend": "Overlay"})).unwrap();
    assert_ne!(app.session.active().unwrap().revision, before.revision);
    assert_eq!(shown(&mut app, a).layer(blue).unwrap().blend, BlendMode::Screen);
    assert_eq!(shown(&mut app, b).layer(blue).unwrap().blend, BlendMode::Difference);
    app.ui.focused_window = Some(a);
    let c = window(&mut app, &ctx);
    assert_eq!(shown(&mut app, c).layer(blue).unwrap().blend, BlendMode::Overlay);
}

#[test]
fn blend_caches_are_isolated_and_aliases_and_repeated_choices_reuse_composites() {
    let (mut app, ctx, _, blue) = fixture();
    let a = window(&mut app, &ctx);
    let b = window(&mut app, &ctx);
    let canonical = app.session.active().unwrap().doc.clone();
    let main = crate::canvas::ensure_texture(&mut app, &ctx, 0, None).unwrap().0;
    let texture = |app: &mut PhotocraftApp, id| app.with_document_window(id, |app| crate::canvas::ensure_texture(app, &ctx, 0, None).unwrap().0).unwrap();
    texture(&mut app, a);
    texture(&mut app, b);
    let key_a = app.view_composites.caches[&a][&(canonical.id, 0)].tex_preview_key;
    let key_b = app.view_composites.caches[&b][&(canonical.id, 0)].tex_preview_key;
    let b_before = shown(&mut app, b);
    blend(&mut app, &ctx, a, blue, "Soft Light");
    texture(&mut app, a);
    assert_ne!(app.view_composites.caches[&a][&(canonical.id, 0)].tex_preview_key, key_a);
    app.perf.last_refresh = "";
    assert_eq!(blend(&mut app, &ctx, a, blue, "soft-light")["changed"], false);
    texture(&mut app, a);
    texture(&mut app, b);
    assert_eq!(crate::canvas::ensure_texture(&mut app, &ctx, 0, None).unwrap().0, main);
    assert_eq!(app.perf.last_refresh, "");
    assert!(Arc::ptr_eq(&b_before, &shown(&mut app, b)));
    assert_eq!(app.view_composites.caches[&b][&(canonical.id, 0)].tex_preview_key, key_b);
    let projected = shown(&mut app, a);
    for (_, _, layer) in canonical.walk() {
        if let Some(surface) = layer.surface() {
            for ((ca, ta), (cb, tb)) in surface.tiles().zip(projected.layer(layer.id).unwrap().surface().unwrap().tiles()) {
                assert_eq!(ca, cb);
                assert!(Arc::ptr_eq(ta, tb));
            }
        }
    }
    app.run("edit.fill", json!({"color": "#3975ad"})).unwrap();
    texture(&mut app, a);
    texture(&mut app, b);
    let revision = app.session.active().unwrap().revision;
    for w in [a, b] {
        assert_eq!(app.view_composites.caches[&w][&(canonical.id, 0)].tex_revision, revision);
    }
    assert!(app.redraws.is_empty());
}

#[test]
fn blend_panel_batches_are_atomic_and_preserve_opacity_visibility_and_cameras() {
    let (mut app, ctx, red, blue) = fixture();
    let w = window(&mut app, &ctx);
    app.ui.focused_window = Some(w);
    let cameras = (app.ui.views.clone(), app.ui.windows[0].view.clone());
    set(&mut app, &ctx, w, red, false);
    menus::invoke(&mut app, &ctx, "view.layerOpacity", json!({"window": w, "layer": blue.0, "opacity": 0.25})).unwrap();
    assert!(panel_action(&mut app, &ctx, Some(w), "layer.setProps", &json!({"layers": [red.0, blue.0], "blend": "Overlay"})));
    let before = shown(&mut app, w);
    assert_eq!(before.layer(red).unwrap().blend, BlendMode::Overlay);
    assert_eq!(before.layer(blue).unwrap().blend, BlendMode::Overlay);
    assert!(!before.layer(red).unwrap().visible);
    assert_eq!(before.layer(blue).unwrap().opacity, 0.25);
    assert_eq!(cameras, (app.ui.views.clone(), app.ui.windows[0].view.clone()));
    for params in [
        json!({"window": w, "layer": blue.0, "blend": "not-a-mode"}),
        json!({"window": w, "layer": blue.0, "blend": 42}),
        json!({"window": w, "layer": blue.0, "blend": "Pass Through"}),
        json!({"window": w, "layers": [red.0, u64::MAX], "blend": "Screen"}),
        json!({"window": w, "layers": [], "blend": "Screen"}),
    ] {
        assert!(command(&mut app, &ctx, "view.layerBlend", &params).is_err());
        assert!(Arc::ptr_eq(&before, &shown(&mut app, w)));
    }
    let background = app.session.active().unwrap().doc.layers[0].id;
    let canonical = app.session.active().unwrap().doc.clone();
    blend(&mut app, &ctx, w, background, "Multiply");
    assert!(Arc::ptr_eq(&canonical, &app.session.active().unwrap().doc));
    assert_eq!(shown(&mut app, w).layer(background).unwrap().name, canonical.layer(background).unwrap().name);
}

#[test]
fn blend_hover_never_previews_a_secondary_choice_in_main_or_other_views() {
    let (mut app, ctx, _, blue) = fixture();
    let w = window(&mut app, &ctx);
    let canonical = app.session.active().unwrap().doc.clone();
    blend_hover(&mut app, None, &[blue], Some(BlendMode::Screen));
    let main_preview = crate::blend_preview::display_doc(&mut app, 0).unwrap().0;
    assert_eq!(main_preview.layer(blue).unwrap().blend, BlendMode::Screen);
    let secondary = app
        .with_document_window(w, |app| {
            let (source, key) = crate::canvas::display_doc(app, 0);
            render_document(app, 0, source, key).0
        })
        .unwrap();
    assert_eq!(secondary.layer(blue).unwrap().blend, BlendMode::Normal, "main hover does not replace the secondary mode");
    blend_hover(&mut app, Some(w), &[blue], Some(BlendMode::Multiply));
    assert!(crate::blend_preview::display_doc(&mut app, 0).is_none(), "secondary hover clears document-wide preview instead of leaking to main");
    assert_eq!(shown(&mut app, w).layer(blue).unwrap().blend, BlendMode::Normal);
    assert!(Arc::ptr_eq(&canonical, &app.session.active().unwrap().doc));
}

#[test]
fn blend_special_layers_match_existing_cpu_semantics_and_masks_stay_shared() {
    use photocraft_doc::{Adjustment, Artboard, Layer, LayerContent, LayerMask};
    // Exercise the same projection with raster, nested group, artboard and adjustment targets.
    for kind in ["raster", "group", "artboard", "adjustment"] {
        let (mut app, ctx, _, blue) = fixture();
        app.run("edit.fill", json!({"color": "#3975ad"})).unwrap();
        let mut target = blue;
        app.session
            .edit("Special layer fixture", |doc, _| {
                let upper = doc.layer_mut(blue).unwrap();
                upper.clipped = true;
                let mut mask = LayerMask::reveal_all();
                mask.surface.write_pixel(0, 0, &[0.5]);
                upper.mask = Some(mask);
                if kind == "group" || kind == "artboard" {
                    let mut layers = std::mem::take(&mut doc.layers);
                    let background = layers.remove(0);
                    let nested = Layer::group("Nested", layers);
                    let mut outer = Layer::group("Outer", vec![nested]);
                    if kind == "artboard"
                        && let LayerContent::Group(g) = &mut outer.content
                    {
                        g.artboard = Some(Artboard::new(doc.bounds()));
                    }
                    target = outer.id;
                    doc.layers = vec![background, outer];
                } else if kind == "adjustment" {
                    let mut layer = Layer::raster("Adjustment", doc.pixel_format());
                    layer.content = LayerContent::Adjustment(Adjustment::BlackWhite { weights: [0.4, 0.6, 0.4, 0.6, 0.2, 0.8], tint: None });
                    layer.mask = Some(LayerMask::reveal_all());
                    target = layer.id;
                    doc.layers.push(layer);
                }
                Ok(())
            })
            .unwrap();
        app.sync_views();
        let w = window(&mut app, &ctx);
        let canonical = app.session.active().unwrap().doc.clone();
        let modes: &[BlendMode] = if kind == "group" || kind == "artboard" {
            &[BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen, BlendMode::PassThrough]
        } else {
            &[BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen, BlendMode::Color]
        };
        for mode in modes {
            blend(&mut app, &ctx, w, target, mode.label());
            let projected = shown(&mut app, w);
            let mut reference = (*canonical).clone();
            reference.layer_mut(target).unwrap().blend = *mode;
            let render = |doc: &Document| photocraft_compose::render(doc, doc.bounds()).px;
            assert_eq!(render(&projected), render(&reference), "{kind}: {mode:?}");
            let original_mask = &canonical.layer(blue).unwrap().mask.as_ref().unwrap().surface;
            let shown_mask = &projected.layer(blue).unwrap().mask.as_ref().unwrap().surface;
            for ((ca, ta), (cb, tb)) in original_mask.tiles().zip(shown_mask.tiles()) {
                assert_eq!(ca, cb);
                assert!(Arc::ptr_eq(ta, tb));
            }
        }
        // Mask edits remain canonical, visible in every subsequent projection.
        app.session
            .edit("Shared mask edit", |doc, _| {
                doc.layer_mut(blue).unwrap().mask.as_mut().unwrap().surface.write_pixel(0, 0, &[0.0]);
                Ok(())
            })
            .unwrap();
        let projected = shown(&mut app, w);
        let source = app.session.active().unwrap().doc.clone();
        let a = source.layer(blue).unwrap().mask.as_ref().unwrap().surface.tiles().next().unwrap().1;
        let b = projected.layer(blue).unwrap().mask.as_ref().unwrap().surface.tiles().next().unwrap().1;
        assert!(Arc::ptr_eq(a, b));
    }
}

#[test]
fn blend_layer_tree_document_and_window_lifecycles_release_old_overrides() {
    let (mut app, ctx, red, blue) = fixture();
    let w = window(&mut app, &ctx);
    blend(&mut app, &ctx, w, blue, "Screen");
    app.run("layer.new.layer", json!({"name": "Added"})).unwrap();
    let added = app.session.active().unwrap().active_layer.unwrap();
    assert_eq!(shown(&mut app, w).layer(added).unwrap().blend, BlendMode::Normal);
    app.session
        .edit("Tree fixture", |doc, _| {
            doc.layer_mut(blue).unwrap().name = "Renamed".into();
            doc.layers.reverse();
            Ok(())
        })
        .unwrap();
    app.sync_views();
    assert_eq!(shown(&mut app, w).layer(blue).unwrap().blend, BlendMode::Screen);
    app.run("layer.delete", json!({"layer": red.0})).unwrap();
    assert!(!app.ui.windows[0].visibility.blend.contains_key(&red));
    app.with_document_window(w, |app| {
        crate::canvas::ensure_texture(app, &ctx, 0, None);
    });
    let canonical = app.session.active().unwrap().doc.clone();
    drop(app.session.close(0));
    app.sync_views();
    assert!(app.ui.windows.is_empty());
    assert!(app.view_composites.projections.is_empty());
    assert!(app.view_composites.caches.is_empty());
    assert!(command(&mut app, &ctx, "view.layerBlend", &json!({"window": w, "layer": blue.0, "blend": "Screen"})).is_err());
    app.session.add_document((*canonical).clone(), None);
    app.sync_views();
    let reopened = window(&mut app, &ctx);
    assert_eq!(shown(&mut app, reopened).layer(blue).unwrap().blend, BlendMode::Normal);
    app.ui.windows[0].open = false;
    retain(&mut app);
    assert!(app.view_composites.projections.is_empty());
}
