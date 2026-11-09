//! Background full CPU redraws (#4024). The GPU tests skip when there is no adapter.

use super::*;
use serde_json::json;

#[test]
fn blocks_cover_the_document_once() {
    let bounds = Rect::new(0, 0, 2500, 1100);
    let b = blocks(bounds);
    assert_eq!(b.len(), 3 * 2);
    assert_eq!(b.iter().map(|r| r.width() as u64 * r.height() as u64).sum::<u64>(), 2500 * 1100);
    for (i, r) in b.iter().enumerate() {
        assert!(r.x0 % BLOCK == 0 && r.y0 % BLOCK == 0, "block {i} starts on the grid: {r:?}");
        assert!(b[i + 1..].iter().all(|o| o.intersect(r).is_empty()), "block {i} overlaps another");
    }
}

#[test]
fn visible_blocks_come_first_nearest_the_view_centre() {
    let b = blocks(Rect::new(0, 0, 4096, 4096));
    // The view shows the right half of the second block row.
    let visible = Rect::new(2100, 1100, 4000, 1900);
    let o = order(&b, 0..b.len(), visible);
    assert_eq!(o.len(), 16);
    let shown: Vec<usize> = (0..b.len()).filter(|&i| !b[i].intersect(&visible).is_empty()).collect();
    assert_eq!(shown.len(), 2);
    assert!(o[..2].iter().all(|i| shown.contains(i)), "{o:?}");
    // Then the block row's neighbours, the far corners last.
    assert!(o[14..].contains(&12), "the bottom-left corner is among the last: {o:?}");
    // Only the blocks asked for.
    assert_eq!(order(&b, [3, 9].into_iter(), visible).len(), 2);
}

/// A kept composite of `doc`: `bytes` texels per block.
fn kept(doc: &Arc<Document>, bytes: usize) -> Kept {
    let all = blocks(doc.bounds());
    let texels: Vec<Vec<u8>> = all.iter().map(|_| vec![1u8; bytes]).collect();
    Kept {
        doc: Arc::downgrade(doc),
        output: doc.id.into(),
        display: 7,
        text_gamma: 1,
        format: eframe::wgpu::TextureFormat::Rgba8Unorm,
        bytes: (bytes * all.len()) as u64,
        blocks: all,
        texels,
    }
}

#[test]
fn kept_composites_come_back_only_for_the_exact_state_and_stay_bounded() {
    use eframe::wgpu::TextureFormat::{Rgba8Unorm, Rgba16Float};
    let mut a = Document::new("kept", photocraft_geom::Size::new(2000, 1500), photocraft_doc::ColorMode::Rgb, photocraft_doc::SampleType::U8);
    let mut layer = photocraft_doc::Layer::raster("one", a.pixel_format());
    if let photocraft_doc::LayerContent::Raster(s) = &mut layer.content {
        s.fill_rect(Rect::new(0, 0, 300, 300), &[0.2, 0.4, 0.6, 1.0]);
    }
    a.layers.push(layer);
    let a = Arc::new(a);
    let all = blocks(a.bounds());
    let changed = |f: &dyn Fn(&mut Document)| {
        let mut d = (*a).clone();
        f(&mut d);
        Arc::new(d)
    };
    let mut cache = KeptComposites::default();
    cache.store(kept(&a, 4), &a, u64::MAX);
    // The same state hits, also as an equal document that isn't the same Arc (a restore after a solo).
    assert!(cache.find(a.id.into(), &a, 7, 1, Rgba8Unorm, &all).is_some());
    assert!(cache.find(a.id.into(), &Arc::new((*a).clone()), 7, 1, Rgba8Unorm, &all).is_some());
    // Any change to what the composite depends on misses.
    let hidden = changed(&|d| d.layers[0].visible = false);
    let faded = changed(&|d| d.layers[0].opacity = 0.5);
    let multiplied = changed(&|d| d.layers[0].blend = photocraft_color::BlendMode::Multiply);
    let painted = changed(&|d| {
        if let photocraft_doc::LayerContent::Raster(s) = &mut d.layers[0].content {
            s.fill_rect(Rect::new(10, 10, 11, 11), &[1.0, 0.0, 0.0, 1.0]);
        }
    });
    for (what, d) in [("visibility", &hidden), ("opacity", &faded), ("blend mode", &multiplied), ("pixels", &painted)] {
        assert!(cache.find(a.id.into(), d, 7, 1, Rgba8Unorm, &all).is_none(), "{what} changed");
    }
    assert!(cache.find(a.id.into(), &a, 8, 1, Rgba8Unorm, &all).is_none(), "another display transform");
    assert!(cache.find(a.id.into(), &a, 7, 2, Rgba8Unorm, &all).is_none(), "another text gamma");
    assert!(cache.find(a.id.into(), &a, 7, 1, Rgba16Float, &all).is_none(), "another texture format");
    assert!(cache.find(a.id.into(), &a, 7, 1, Rgba8Unorm, &all[1..]).is_none(), "other blocks");
    // At most KEEP entries, the least recently used going first.
    cache.store(kept(&hidden, 4), &hidden, u64::MAX);
    assert!(cache.find(a.id.into(), &a, 7, 1, Rgba8Unorm, &all).is_some());
    cache.store(kept(&faded, 4), &faded, u64::MAX);
    assert_eq!(cache.entries.len(), KEEP);
    assert!(cache.find(a.id.into(), &hidden, 7, 1, Rgba8Unorm, &all).is_none(), "the least recently used went");
    assert!(cache.find(a.id.into(), &a, 7, 1, Rgba8Unorm, &all).is_some() && cache.find(a.id.into(), &faded, 7, 1, Rgba8Unorm, &all).is_some());
    // A stored entry makes the next frame take the cap again, over every kept document.
    cache.capped = (vec![(a.id, 1)], 1);
    cache.store(kept(&faded, 4), &faded, u64::MAX);
    assert_eq!(cache.capped, Default::default());
    // Storing a state again replaces its entry.
    cache.store(kept(&faded, 4), &faded, u64::MAX);
    assert_eq!(cache.entries.len(), 2);
    // The byte cap: a smaller one drops the oldest, and a composite over it isn't kept.
    let one = kept(&a, 4).bytes;
    cache.trim(one);
    assert_eq!(cache.entries.len(), 1);
    assert!(cache.bytes() <= one);
    cache.trim(0);
    cache.store(kept(&a, 4), &a, one - 1);
    assert!(cache.entries.is_empty(), "over the cap");
    // A closed document's composites go.
    cache.store(kept(&a, 4), &a, u64::MAX);
    cache.retain(|output| output.document != a.id);
    assert!(cache.entries.is_empty());
    // A state History no longer holds isn't kept alive: its entry is dead and goes.
    let gone = changed(&|d| d.layers[0].opacity = 0.25);
    cache.store(kept(&gone, 4), &gone, u64::MAX);
    let equal = Arc::new((*gone).clone());
    drop(gone);
    assert!(cache.find(a.id.into(), &equal, 7, 1, Rgba8Unorm, &all).is_none(), "no strong reference to the drawn state");
    cache.trim(u64::MAX);
    assert!(cache.entries.is_empty());
}

struct Gpu {
    harness: egui_kittest::Harness<'static, PhotocraftApp>,
    observer: GpuCanvas,
}

/// The app on a real device with background jobs, or `None` without a GPU adapter.
fn gpu_app() -> Option<Gpu> {
    let observer = Arc::new(std::sync::Mutex::new(None));
    let observer_out = observer.clone();
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        egui_kittest::Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .with_pixels_per_point(1.0)
            .with_max_steps(64)
            .wgpu_setup(crate::gpu_canvas::wgpu_setup())
            .build_eframe(move |cc| {
                PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
                let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
                if let Some(rs) = cc.wgpu_render_state.as_ref() {
                    *observer_out.lock().expect("observer") = Some(GpuCanvas::new(rs));
                    app.set_wgpu(rs.clone());
                }
                app.background_jobs = true;
                app
            })
    }));
    let Ok(harness) = built else {
        eprintln!("skipping: no GPU adapter");
        return None;
    };
    let observer = observer.lock().expect("observer").take()?;
    Some(Gpu { harness, observer })
}

/// A document the wgpu compositor leaves to the CPU (knockout on its top layer, which also has
/// Blend If).
fn cpu_only_document(app: &mut PhotocraftApp, w: u32, h: u32) -> DocId {
    app.run("file.new", json!({"width": w, "height": h, "background": "#336699"})).expect("new");
    app.run("layer.new.layer", json!({})).expect("layer");
    app.run("edit.fill", json!({"color": "#cc8844"})).expect("fill");
    app.run("layer.layerStyle.blendingOptions", json!({"opacity": 60, "knockout": "shallow", "blendIf": {"channel": "gray", "underlying": [0, 10, 245, 255]}}))
        .expect("blending options");
    app.session.active().expect("document").doc.id
}

/// Frames until no redraw is running.
fn settle(g: &mut Gpu) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        g.harness.step();
        if g.harness.state().redraws.is_empty() {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "the redraw didn't finish");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// The canvas texture of `doc` against the synchronous CPU refresh of the current document.
fn assert_matches_synchronous(g: &mut Gpu, doc: DocId) {
    let app = g.harness.state_mut();
    let d = app.session.active().expect("document").doc.clone();
    let (display, _) = crate::canvas::canvas_display(app, &d, None);
    let reference = doc.0 ^ (1u64 << 60);
    g.observer.upload_composite(reference, &d, display.as_deref());
    let (_, size, shown) = g.observer.read_texels(doc.0).expect("canvas texture");
    let (_, _, want) = g.observer.read_texels(reference).expect("reference texture");
    assert_eq!(size, [d.size.width, d.size.height]);
    let differ = shown.iter().zip(&want).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{differ} texels differ from the synchronous refresh");
}

#[test]
fn a_full_cpu_refresh_draws_in_the_background_with_the_synchronous_pixels() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let doc = cpu_only_document(g.harness.state_mut(), 2600, 1700);
    g.harness.step();
    assert!(g.harness.state().redraws.contains_key(&doc.into()), "the frame started a redraw instead of compositing inline");
    settle(&mut g);
    let app = g.harness.state();
    assert_eq!(app.perf.last_refresh, "background");
    assert!(app.perf.gpu_fallback.as_deref().is_some_and(|f| f.contains("Advanced Blending")), "{:?}", app.perf.gpu_fallback);
    assert_matches_synchronous(&mut g, doc);
}

#[test]
fn an_edit_during_a_redraw_is_not_overwritten_by_blocks_drawn_before_it() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    // Hold the worker before its first batch and wait until this very worker acknowledges that
    // hold: when the fill lands, all 12 blocks of this 4×3 document are still to draw by it.
    // Without the hold a fast machine finishes them before the next frame, so the edit never
    // races the worker and this test only checked a completed redraw; without the per-worker
    // acknowledgement an edit could beat the worker to the hold, and its restarted replacement
    // would satisfy the checks.
    let held = hold_redraws();
    let doc = cpu_only_document(g.harness.state_mut(), 4096, 3072);
    g.harness.step();
    let generation = g.harness.state().redraws.get(&doc.into()).expect("the frame started a redraw").worker_generation;
    assert!(
        wait_for_held_redraw(g.harness.state(), doc, generation, std::time::Duration::from_secs(10)),
        "the redraw worker never reached the pre-batch hold; an edit would race an unstarted redraw"
    );
    // A corner, drawn last (farthest from the view's centre), changes on a layer of its own while
    // the redraw runs: a small edit, drawn at once.
    g.harness.state_mut().run("layer.new.layer", json!({})).expect("layer");
    g.harness.step();
    g.harness.state_mut().run("select.rect", json!({"x": 3900, "y": 2900, "width": 150, "height": 150})).expect("select");
    g.harness.step();
    g.harness.state_mut().run("edit.fill", json!({"color": "#22ee22"})).expect("fill");
    g.harness.step();
    assert_eq!(g.harness.state().perf.last_refresh, "rect", "the fill's damage was drawn synchronously");
    // Still running with blocks to draw, and drawing the edited document, not the state before it.
    let current = g.harness.state().session.active().expect("document").doc.clone();
    let r = g.harness.state().redraws.get(&doc.into()).expect("the redraw goes on");
    assert!(r.pending.iter().any(|p| *p), "blocks are still to draw");
    assert!(Arc::ptr_eq(&r.doc, &current) || *r.doc == *current, "the blocks left draw the edit");
    drop(held);
    settle(&mut g);
    assert_matches_synchronous(&mut g, doc);
}

#[test]
fn an_edit_damaging_most_of_a_large_document_is_drawn_in_the_background_too() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let doc = cpu_only_document(g.harness.state_mut(), 2600, 1700);
    settle(&mut g);
    // Filling most of the Blend If layer touches tiles all over it: the whole canvas (a fill of a
    // small part damages only its own tiles, #9329).
    g.harness.state_mut().run("select.rect", json!({"x": 100, "y": 100, "width": 2500, "height": 1500})).expect("select");
    g.harness.step();
    g.harness.state_mut().run("edit.fill", json!({"color": "#22ee22"})).expect("fill");
    assert_eq!(g.harness.state().session.active().expect("document").last_damage, Some(Rect::new(0, 0, 2600, 1700)));
    g.harness.step();
    assert!(g.harness.state().redraws.contains_key(&doc.into()), "not composited inline: {}", g.harness.state().perf.last_refresh);
    settle(&mut g);
    assert_eq!(g.harness.state().perf.last_refresh, "background");
    assert_matches_synchronous(&mut g, doc);
}

#[test]
fn showing_a_hidden_layer_again_reuses_the_redraw_from_before_it_was_hidden() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let doc = cpu_only_document(g.harness.state_mut(), 2600, 1700);
    let layer = g.harness.state().session.active().and_then(|st| st.active_layer).expect("layer");
    settle(&mut g);
    assert_eq!(g.harness.state().perf.last_refresh, "background");
    g.harness.state_mut().run("layer.setProps", json!({"layer": layer.0, "visible": false})).expect("hide");
    settle(&mut g);
    // Back to the state drawn before: its blocks come back, nothing is composited.
    g.harness.state_mut().run("layer.setProps", json!({"layer": layer.0, "visible": true})).expect("show");
    g.harness.step();
    assert!(g.harness.state().redraws.is_empty(), "no redraw: {}", g.harness.state().perf.last_refresh);
    assert_eq!(g.harness.state().perf.last_refresh, KEPT);
    assert_matches_synchronous(&mut g, doc);
    // A real change composites again. `last_refresh` is only set when that redraw finishes, so
    // check that one started, then that it was a composite rather than kept blocks.
    g.harness.state_mut().run("layer.setProps", json!({"layer": layer.0, "opacity": 0.3})).expect("opacity");
    g.harness.step();
    assert!(g.harness.state().redraws.contains_key(&doc.into()), "not served from kept blocks: {}", g.harness.state().perf.last_refresh);
    settle(&mut g);
    assert_eq!(g.harness.state().perf.last_refresh, "background");
    assert_matches_synchronous(&mut g, doc);
}

/// Memory Usage lowered while a redraw runs: the finished composite is kept within what it leaves
/// now, not what it left when the redraw began.
#[test]
fn a_redraw_finishing_after_memory_usage_drops_keeps_nothing_over_it() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let doc = cpu_only_document(g.harness.state_mut(), 4096, 3072);
    g.harness.step();
    assert!(g.harness.state().redraws.contains_key(&doc.into()));
    // Less than the document's own pixels: nothing is left for kept composites.
    g.harness.state_mut().session.edit_prefs(|p| p.performance.memory_usage_mb = 1);
    settle(&mut g);
    assert!(g.harness.state().kept.entries.is_empty(), "a composite was kept over the current cap");
}

/// Color Settings › "Blend Text Colors Using Gamma" isn't part of the document, but type layers
/// composite with it: a state drawn before it changed is composited again, not uploaded from
/// blocks drawn with the old gamma. Only the session's setting changes here, not the process-wide
/// gamma compose reads, so tests compositing type layers at the same time aren't affected.
#[test]
fn a_state_drawn_before_the_text_gamma_changed_is_composited_again() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let doc = cpu_only_document(g.harness.state_mut(), 2600, 1700);
    let layer = g.harness.state().session.active().and_then(|st| st.active_layer).expect("layer");
    settle(&mut g);
    g.harness.state_mut().run("layer.setProps", json!({"layer": layer.0, "visible": false})).expect("hide");
    settle(&mut g);
    g.harness.state_mut().session.color.settings.blend_text_gamma = 1.0;
    g.harness.state_mut().run("layer.setProps", json!({"layer": layer.0, "visible": true})).expect("show");
    g.harness.step();
    assert!(g.harness.state().redraws.contains_key(&doc.into()), "served from blocks of the old gamma: {}", g.harness.state().perf.last_refresh);
    settle(&mut g);
    assert_eq!(g.harness.state().perf.last_refresh, "background");
    assert_matches_synchronous(&mut g, doc);
}

/// The text gamma changing while a redraw runs: its blocks were drawn with both, so it isn't kept.
#[test]
fn a_redraw_across_a_text_gamma_change_is_not_kept() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let doc = cpu_only_document(g.harness.state_mut(), 4096, 3072);
    g.harness.step();
    assert!(g.harness.state().redraws.contains_key(&doc.into()));
    g.harness.state_mut().session.color.settings.blend_text_gamma = 1.0;
    settle(&mut g);
    assert!(g.harness.state().kept.entries.is_empty(), "a composite of two gammas was kept");
}

/// Changed and changed back while a redraw runs: blocks drawn in between still mix both.
#[test]
fn a_redraw_across_a_text_gamma_change_and_back_is_not_kept() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let doc = cpu_only_document(g.harness.state_mut(), 4096, 3072);
    let gamma = g.harness.state().session.color.settings.blend_text_gamma;
    g.harness.step();
    g.harness.state_mut().session.color.settings.blend_text_gamma = 1.0;
    g.harness.step();
    g.harness.state_mut().session.color.settings.blend_text_gamma = gamma;
    assert!(g.harness.state().redraws.contains_key(&doc.into()), "the redraw finished before the change back");
    settle(&mut g);
    assert!(g.harness.state().kept.entries.is_empty(), "a composite of two gammas was kept");
}

/// The same while another document is shown: its redraw goes on without being polled, so the
/// change is seen every frame, not only when it shows again.
#[test]
fn a_hidden_documents_redraw_across_a_text_gamma_change_and_back_is_not_kept() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let doc = cpu_only_document(g.harness.state_mut(), 4096, 3072);
    let gamma = g.harness.state().session.color.settings.blend_text_gamma;
    g.harness.step();
    g.harness.state_mut().run("file.new", json!({"width": 64, "height": 64})).expect("another document");
    g.harness.step();
    g.harness.state_mut().session.color.settings.blend_text_gamma = 1.0;
    g.harness.step();
    g.harness.state_mut().session.color.settings.blend_text_gamma = gamma;
    g.harness.step();
    assert!(g.harness.state().redraws.contains_key(&doc.into()), "the hidden document's redraw stopped");
    let i = g.harness.state().session.documents().iter().position(|st| st.doc.id == doc).expect("document");
    g.harness.state_mut().session.set_active(i);
    settle(&mut g);
    assert!(g.harness.state().kept.entries.iter().all(|k| k.output.document != doc), "a composite of two gammas was kept");
}

/// Memory Usage lowered with no redraw running: the kept composites go at the next frame, not
/// only when the next redraw starts.
#[test]
fn kept_composites_go_when_memory_usage_drops_between_redraws() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    cpu_only_document(g.harness.state_mut(), 2600, 1700);
    settle(&mut g);
    assert!(!g.harness.state().kept.entries.is_empty(), "the finished redraw is kept");
    g.harness.state_mut().session.edit_prefs(|p| p.performance.memory_usage_mb = 1);
    g.harness.step();
    assert!(g.harness.state().redraws.is_empty());
    assert!(g.harness.state().kept.entries.is_empty(), "kept over the current cap");
}

/// An edit too small for a background redraw that grows the document past what Memory Usage
/// leaves: the kept composite goes at the next frame. The test holds the kept state itself, so
/// only the cap can drop it, not History letting go of the state.
#[test]
fn kept_composites_go_when_a_small_edit_grows_the_document_past_the_cap() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    cpu_only_document(g.harness.state_mut(), 2600, 1700);
    settle(&mut g);
    let app = g.harness.state_mut();
    let st = app.session.active().expect("document");
    let held = st.doc.clone();
    let (used, kept) = (st.memory_bytes() as u64, app.kept.bytes());
    assert!(kept > 0, "the finished redraw is kept");
    // Room for the kept composite and 2-3 MB more.
    let mb = (used + kept).div_ceil(1 << 20) + 2;
    app.session.edit_prefs(|p| p.performance.memory_usage_mb = mb as _);
    g.harness.step();
    assert_eq!(g.harness.state().kept.entries.len(), 1, "still within the cap");
    // 2 MP of new pixels (8 MB): under the size that redraws in the background.
    for (cmd, p) in
        [("layer.new.layer", json!({})), ("select.rect", json!({"x": 0, "y": 0, "width": 2000, "height": 1000})), ("edit.fill", json!({"color": "#cc2244"}))]
    {
        g.harness.state_mut().run(cmd, p).expect(cmd);
        g.harness.step();
        assert!(g.harness.state().redraws.is_empty(), "{cmd} redrew in the background");
    }
    assert!(g.harness.state().kept.entries.is_empty(), "kept over the cap after the edit");
    drop(held);
}

/// The same for an older kept composite of another document: every kept document's edits count,
/// not only the newest one's.
#[test]
fn an_older_kept_composite_goes_when_its_document_grows_past_the_cap() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let a = cpu_only_document(g.harness.state_mut(), 2600, 1700);
    settle(&mut g);
    let b = cpu_only_document(g.harness.state_mut(), 2600, 1700);
    settle(&mut g);
    let app = g.harness.state_mut();
    let held: Vec<_> = app.session.documents().iter().map(|st| st.doc.clone()).collect();
    assert_eq!(app.kept.entries.iter().map(|k| k.output.document).collect::<Vec<_>>(), [b, a], "both kept, the second newest");
    let used = app.session.documents().iter().map(|st| st.memory_bytes() as u64).max().unwrap_or(0);
    let mb = (used + app.kept.bytes()).div_ceil(1 << 20) + 2;
    app.session.edit_prefs(|p| p.performance.memory_usage_mb = mb as _);
    let i = app.session.documents().iter().position(|st| st.doc.id == a).expect("document");
    app.session.set_active(i);
    g.harness.step();
    assert_eq!(g.harness.state().kept.entries.len(), 2, "still within the cap");
    for (cmd, p) in
        [("layer.new.layer", json!({})), ("select.rect", json!({"x": 0, "y": 0, "width": 2000, "height": 1000})), ("edit.fill", json!({"color": "#cc2244"}))]
    {
        g.harness.state_mut().run(cmd, p).expect(cmd);
        g.harness.step();
        assert!(g.harness.state().redraws.is_empty(), "{cmd} redrew in the background");
    }
    assert!(g.harness.state().kept.entries.len() < 2, "kept over the cap after the older document grew");
    drop(held);
}

/// A kept state History no longer holds goes at the next frame: its texels aren't held until
/// some later redraw.
#[test]
fn a_kept_state_history_purged_goes_at_the_next_frame() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    cpu_only_document(g.harness.state_mut(), 2600, 1700);
    let layer = g.harness.state().session.active().and_then(|st| st.active_layer).expect("layer");
    settle(&mut g);
    // Hidden, the document is drawn on the GPU: the kept state is the one before, in History.
    g.harness.state_mut().run("layer.setProps", json!({"layer": layer.0, "visible": false})).expect("hide");
    settle(&mut g);
    assert_eq!(g.harness.state().kept.entries.len(), 1, "the state before hiding is kept");
    g.harness.state_mut().run("edit.purge.histories", json!({})).expect("purge");
    g.harness.step();
    assert!(g.harness.state().redraws.is_empty());
    assert!(g.harness.state().kept.entries.is_empty(), "the purged state's composite is still held");
}

#[test]
fn closing_the_document_stops_its_redraw() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    let doc = cpu_only_document(g.harness.state_mut(), 4096, 3072);
    g.harness.step();
    assert!(g.harness.state().redraws.contains_key(&doc.into()));
    g.harness.state_mut().run("file.close", json!({"save": false})).expect("close");
    g.harness.step();
    assert!(g.harness.state().redraws.is_empty());
}

#[test]
fn hiding_a_large_layer_over_the_gpu_budget_redraws_in_the_background() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    // A layer covering most, but not all, of the document: hiding it damages a large rect that
    // isn't the whole canvas.
    let app = g.harness.state_mut();
    app.run("file.new", json!({"width": 5000, "height": 2000, "background": "#336699"})).expect("new");
    app.run("layer.new.layer", json!({})).expect("layer");
    app.run("select.rect", json!({"x": 0, "y": 0, "width": 4800, "height": 1800})).expect("select");
    app.run("edit.fill", json!({"color": "#cc8844"})).expect("fill");
    app.run("select.deselect", json!({})).expect("deselect");
    let (doc, layer) = {
        let st = app.session.active().expect("document");
        (st.doc.id, st.active_layer.expect("layer"))
    };
    settle(&mut g);
    assert!(g.harness.state().perf.last_refresh.starts_with("gpu"), "the document fits the GPU: {}", g.harness.state().perf.last_refresh);
    // A budget the pages of one 2048 px page cell fit (about 33 MB here) but not those of the
    // whole width (80 MB), as on a multi-gigabyte PSB. Damage refreshes keep it: `gpu_budget`
    // recomputes it for full ones only. A small edit still draws on the GPU at once.
    g.harness.state_mut().run("layer.new.layer", json!({})).expect("layer");
    g.harness.step();
    g.harness.state_mut().run("select.rect", json!({"x": 100, "y": 100, "width": 64, "height": 64})).expect("select");
    g.harness.step();
    g.observer.set_memory_budget(48 << 20);
    g.harness.state_mut().run("edit.fill", json!({"color": "#22ee22"})).expect("fill");
    g.harness.step();
    assert_eq!(g.harness.state().perf.last_refresh, "gpu-rect", "{:?}", g.harness.state().perf.gpu_fallback);
    g.harness.state_mut().run("select.deselect", json!({})).expect("deselect");
    g.harness.step();
    assert!(g.harness.state().redraws.is_empty());
    g.observer.set_memory_budget(48 << 20);
    g.harness.state_mut().run("layer.setProps", json!({"layer": layer.0, "visible": false})).expect("hide");
    let damage = g.harness.state().session.active().expect("document").last_damage.expect("damage");
    assert!(damage != Rect::new(0, 0, 5000, 2000) && damage.width() as u64 * damage.height() as u64 >= crate::gpu_canvas::DEFER_PX, "{damage:?}");
    g.harness.step();
    assert!(g.harness.state().redraws.contains_key(&doc.into()), "not composited inline: {}", g.harness.state().perf.last_refresh);
    settle(&mut g);
    assert_eq!(g.harness.state().perf.last_refresh, "background");
    let app = g.harness.state();
    assert!(matches!(app.render_path.current(doc.0), Some(crate::render_path::CpuReason::OverBudget { .. })), "{:?}", app.perf.gpu_fallback);
    assert_matches_synchronous(&mut g, doc);
}

/// The active document's render path: its reason's kind and the status bar's label.
fn render_path_of(g: &Gpu) -> (Option<&'static str>, Option<&'static str>) {
    let app = g.harness.state();
    let doc = app.session.active().expect("document").doc.id.0;
    (app.render_path.current(doc).map(crate::render_path::CpuReason::kind), crate::chrome_ui::render_path_label(app).map(|l| l.0))
}

/// On the GPU canvas: no reason, or only a software adapter's (CI's runners have one).
fn on_gpu_canvas(g: &Gpu) -> bool {
    matches!(render_path_of(g).0, None | Some("softwareAdapter"))
}

#[test]
fn advanced_blending_fallback_is_shown_in_the_status_bar() {
    use egui_kittest::kittest::Queryable;
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    cpu_only_document(g.harness.state_mut(), 800, 600);
    g.harness.step();
    settle(&mut g);
    g.harness.run_steps(2);
    assert_eq!(render_path_of(&g), (Some("unsupported"), Some("CPU Rendering")));
    assert!(g.harness.query_by_label("CPU Rendering").is_some(), "the status bar says so");
    let tip = crate::chrome_ui::render_path_label(g.harness.state()).map(|l| l.1).unwrap_or_default();
    assert!(tip.contains("Advanced Blending"), "{tip}");
}

#[test]
fn over_budget_fallback_shows_working_set_and_budget() {
    use egui_kittest::kittest::Queryable;
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    // As in `hiding_a_large_layer_over_the_gpu_budget_redraws_in_the_background`: a large damage
    // rect over a budget the whole width's pages don't fit.
    let app = g.harness.state_mut();
    app.run("file.new", json!({"width": 5000, "height": 2000, "background": "#336699"})).expect("new");
    app.run("layer.new.layer", json!({})).expect("layer");
    app.run("select.rect", json!({"x": 0, "y": 0, "width": 4800, "height": 1800})).expect("select");
    app.run("edit.fill", json!({"color": "#cc8844"})).expect("fill");
    app.run("select.deselect", json!({})).expect("deselect");
    let layer = app.session.active().expect("document").active_layer.expect("layer");
    settle(&mut g);
    assert!(on_gpu_canvas(&g), "the document fits the GPU");
    g.observer.set_memory_budget(48 << 20);
    g.harness.state_mut().run("layer.setProps", json!({"layer": layer.0, "visible": false})).expect("hide");
    g.harness.step();
    settle(&mut g);
    g.harness.run_steps(2);
    assert_eq!(render_path_of(&g), (Some("overBudget"), Some("GPU Memory Full")));
    assert!(g.harness.query_by_label("GPU Memory Full").is_some());
    let text = g.harness.state().perf.gpu_fallback.clone().unwrap_or_default();
    assert!(text.contains("of GPU memory, budget") && text.contains('M'), "{text}");
}

#[test]
fn a_compositor_build_failure_shows_its_reason() {
    use egui_kittest::kittest::Queryable;
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    // The DX12 FXC failure of #8733, which no Mac reproduces.
    g.observer.fail_compositor("Shader compilation failed: error X3507: not all control paths return a value");
    g.harness.state_mut().run("file.new", json!({"width": 400, "height": 300, "background": "#336699"})).expect("new");
    g.harness.step();
    settle(&mut g);
    g.harness.run_steps(2);
    assert_eq!(render_path_of(&g), (Some("compositorBuild"), Some("CPU Rendering")));
    assert!(g.harness.query_by_label("CPU Rendering").is_some());
    assert!(g.harness.state().perf.gpu_fallback.as_deref().is_some_and(|f| f.contains("X3507")));
}

#[test]
fn a_canvas_over_the_texture_budget_records_its_reason() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    // The app's canvas, not the observer: each `GpuCanvas::new` shares a new budget.
    g.harness.state().gpu.as_ref().expect("gpu canvas").set_canvas_budget(1);
    g.harness.state_mut().run("file.new", json!({"width": 400, "height": 300, "background": "#336699"})).expect("new");
    g.harness.run_steps(3);
    assert_eq!(render_path_of(&g), (Some("canvasTooLarge"), Some("CPU Rendering")));
    let doc = g.harness.state().session.active().expect("document").doc.id.0;
    assert_eq!(g.harness.state().render_path.history().filter(|c| c.doc == doc).count(), 1, "once, not every frame");
}

#[test]
fn a_flipped_view_records_why_it_draws_on_the_cpu() {
    let _gpu = photocraft_testkit::gpu_lock();
    let Some(mut g) = gpu_app() else { return };
    g.harness.state_mut().run("file.new", json!({"width": 400, "height": 300, "background": "#336699"})).expect("new");
    g.harness.run_steps(3);
    assert!(on_gpu_canvas(&g));
    g.harness.state_mut().ui.views[0].flip_horizontal = true;
    g.harness.run_steps(3);
    assert_eq!(render_path_of(&g), (Some("viewTransform"), Some("CPU Rendering")));
    g.harness.state_mut().ui.views[0].flip_horizontal = false;
    g.harness.run_steps(3);
    assert!(on_gpu_canvas(&g), "back on the GPU");
}

#[test]
fn composite_output_storage_redraw_and_lifetimes_are_isolated() {
    use crate::gpu_canvas::CompositeOutput;

    let Some(mut g) = gpu_app() else { return };
    let gpu = g.observer.clone();
    let ctx = g.harness.ctx.clone();
    let app = g.harness.state_mut();
    app.run("file.new", json!({"width": 32, "height": 24, "background": "transparent"})).unwrap();
    app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
    app.sync_views();
    let doc = app.session.active().unwrap().doc.clone();
    let window = crate::menus::invoke(app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap()["window"].as_u64().unwrap();
    let main = CompositeOutput::from(doc.id);
    let secondary = CompositeOutput { document: doc.id, window: Some(window) };
    assert_ne!(crate::gpu_canvas::CanvasTextureKey::from(main), crate::gpu_canvas::CanvasTextureKey::from(secondary));

    let bounds = doc.bounds();
    let canonical = gpu.refresh_with(main, &doc, None, None, false);
    assert_eq!(canonical.kind, "gpu-full", "{:?}", canonical.fallback);
    let mut projected = (*doc).clone();
    projected.layers[0].opacity = 0.5;
    let projected = Arc::new(projected);
    let child = gpu.refresh_with(secondary, &projected, None, None, false);
    assert_eq!(child.kind, "gpu-full", "{:?}", child.fallback);
    assert_eq!(child.uploads, 0, "the secondary should reuse the canonical layer upload");
    let main_pixels = gpu.read_texels(main).unwrap();
    let child_pixels = gpu.read_texels(secondary).unwrap();
    assert_ne!(main_pixels, child_pixels);

    // An output-specific CPU fallback can replace its own texture without touching Main.
    start(app, &ctx, &gpu, secondary, projected.clone(), None, bounds, bounds, None);
    assert!(app.redraws.contains_key(&secondary));
    assert!(!app.redraws.contains_key(&main));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.redraws.contains_key(&secondary) {
        poll(app, &gpu, secondary);
        assert!(std::time::Instant::now() < deadline, "fallback redraw did not finish");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(gpu.read_texels(main).unwrap(), main_pixels);
    assert!(app.kept.entries.iter().any(|entry| entry.output == secondary));

    crate::view_visibility::set_follow_main_settings(app, &ctx, window, true);
    assert!(gpu.read_texels(secondary).is_none());
    assert!(app.kept.entries.iter().all(|entry| entry.output != secondary));

    start(app, &ctx, &gpu, secondary, projected, None, bounds, bounds, None);
    assert!(app.redraws.contains_key(&secondary));
    app.ui.windows.iter_mut().find(|w| w.id == window).unwrap().open = false;
    poll(app, &gpu, secondary);
    crate::canvas::retain_gpu_documents(app);
    assert!(!app.redraws.contains_key(&secondary));
    assert!(gpu.read_texels(secondary).is_none());
    assert_eq!(gpu.read_texels(main).unwrap(), main_pixels);
    assert_eq!(gpu.refresh_with(main, &doc, None, None, false).uploads, 0, "closing a view must preserve shared source pages");

    app.run("file.close", json!({"save": false})).unwrap();
    crate::canvas::retain_gpu_documents(app);
    assert!(gpu.read_texels(main).is_none());
}

#[test]
fn shared_output_budget_and_secondary_allocation_failure() {
    use crate::gpu_canvas::CompositeOutput;

    let Some(mut g) = gpu_app() else { return };
    let ctx = g.harness.ctx.clone();
    let app = g.harness.state_mut();
    let gpu = app.gpu.clone().unwrap();
    app.run("file.new", json!({"width": 32, "height": 24, "background": "transparent"})).unwrap();
    app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
    app.sync_views();
    let doc = app.session.active().unwrap().doc.clone();
    let mode = app.session.prefs().performance.effective_rendering_mode();
    let secondary = |app: &mut PhotocraftApp| {
        let id = crate::menus::invoke(app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap()["window"].as_u64().unwrap();
        CompositeOutput { document: doc.id, window: Some(id) }
    };
    let main = CompositeOutput::from(doc.id);
    let a = secondary(app);
    let b = secondary(app);
    let (bytes, _) = gpu.canvas_fit(&doc).unwrap();
    gpu.set_canvas_budget(bytes * 2);
    assert!(gpu.prepare_texture(main, &doc).is_some());
    assert!(gpu.prepare_texture(a, &doc).is_some());
    assert!(gpu.prepare_texture(b, &doc).is_none());
    app.ui.windows.iter_mut().find(|w| Some(w.id) == a.window).unwrap().open = false;
    crate::canvas::retain_gpu_documents(app);
    assert!(gpu.texture_info(a).is_none());
    assert!(gpu.prepare_texture(b, &doc).is_some());

    gpu.discard_output(main);
    let c = secondary(app);
    assert!(gpu.prepare_texture(c, &doc).is_some());
    assert!(gpu.prepare_texture(main, &doc).is_some(), "Main reclaims the secondary reservations");
    assert!(gpu.texture_info(b).is_none() && gpu.texture_info(c).is_none());

    gpu.set_canvas_budget(512 << 20);
    assert_eq!(gpu.refresh_with(main, &doc, None, None, false).kind, "gpu-full");
    let original = gpu.read_texels(main).unwrap();
    gpu.discard_output(b);
    gpu.fail_next_secondary_allocation();
    let mut projection = (*doc).clone();
    projection.layers[0].opacity = 0.5;
    let refresh = gpu.refresh_with(b, &projection, None, None, false);
    assert_eq!(refresh.kind, "full");
    assert!(refresh.fallback.is_some());
    let expected = photocraft_compose::flatten(&projection);
    let (_, _, actual) = gpu.read_texels(b).unwrap();
    assert!(actual.iter().zip(expected.px).all(|(actual, [r, g, b, a])| {
        actual.iter().zip([r * a, g * a, b * a, a]).all(|(x, y)| (x - y).abs() < 1.0 / 255.0)
    }));
    assert_eq!(gpu.read_texels(main).unwrap(), original);
    assert_eq!(gpu.canvas_fit(&doc).unwrap().1, 512 << 20);
    assert_eq!(gpu.refresh_with(main, &doc, None, None, false).uploads, 0);
    assert!(Arc::ptr_eq(&doc, &app.session.active().unwrap().doc));
    gpu.health().mark(photocraft_gpu::Fault::Lost("simulated device loss".into()));
    for output in [main, b, b] {
        assert_eq!(gpu.refresh_with(output, &doc, None, None, false).kind, "lost");
    }
    assert_eq!(app.session.prefs().performance.effective_rendering_mode(), mode);
}

#[test]
fn secondary_gpu_refreshes_divergent_settings_and_shared_paint() {
    use crate::gpu_canvas::CompositeOutput;

    let Some(mut g) = gpu_app() else { return };
    let ctx = g.harness.ctx.clone();
    let app = g.harness.state_mut();
    let gpu = app.gpu.clone().unwrap();
    app.run("file.new", json!({"width": 64, "height": 48, "background": "white"})).unwrap();
    app.run("layer.new.layer", json!({})).unwrap();
    app.run("edit.fill", json!({"color": "#617fa6"})).unwrap();
    let raster = app.session.active().unwrap().active_layer.unwrap();
    app.run("layer.newAdjustmentLayer.posterize", json!({})).unwrap();
    let adjustment = app.session.active().unwrap().active_layer.unwrap();
    let doc = app.session.active().unwrap().doc.id;
    app.sync_views();

    let open = |app: &mut PhotocraftApp| {
        crate::menus::invoke(app, &ctx, "window.arrange.newWindowForDocument", json!({})).unwrap()["window"].as_u64().unwrap()
    };
    let a = open(app);
    let b = open(app);
    let edit = |app: &mut PhotocraftApp, window, levels| {
        crate::menus::invoke(app, &ctx, "view.layerAdjustment", json!({"window": window, "layer": adjustment.0, "params": {"levels": levels}})).unwrap();
    };
    edit(app, a, 2);
    edit(app, b, 8);
    for (id, params) in [
        ("view.layerOpacity", json!({"window": b, "layer": raster.0, "opacity": 0.75})),
        ("view.layerBlend", json!({"window": b, "layer": raster.0, "blend": "multiply"})),
        ("view.layerVisibility", json!({"window": b, "layer": adjustment.0, "visible": true})),
    ] {
        crate::menus::invoke(app, &ctx, id, params).unwrap();
    }
    let output = |window| CompositeOutput { document: doc, window: Some(window) };
    let draw = |app: &mut PhotocraftApp, window| {
        let shown = app.with_document_window(window, |app| crate::canvas::ensure_secondary_gpu(app, 0)).unwrap().unwrap();
        let (_, _, pixels) = gpu.read_texels(output(window)).unwrap();
        let expected = photocraft_compose::flatten(&shown);
        assert!(pixels.iter().zip(expected.px).all(|(p, [r, g, b, a])| {
            p.iter().zip([r * a, g * a, b * a, a]).all(|(x, y)| (x - y).abs() <= 2.0 / 255.0)
        }));
        pixels
    };
    // Exercise the actual canvas dispatch with a child viewport ID as well as
    // checking its refresh helper below. Reflection must leave its GPU output intact.
    let viewport_id = egui::ViewportId::from_hash_of(("docwin", a));
    let canvas_pass = |app: &mut PhotocraftApp, viewport_id| {
        let mut input = egui::RawInput { viewport_id, ..Default::default() };
        input.viewports.insert(viewport_id, Default::default());
        let mut result = ctx.run_ui(input, |ui| {
            app.with_document_window(a, |app| {
                let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(512.0, 384.0));
                let camera = app.ui.views[0].clone();
                crate::canvas::canvas_view(app, ui, 0, rect, camera, false);
            });
        });
        result.textures_delta.clear();
    };
    app.ui.windows.iter_mut().find(|w| w.id == a).unwrap().view.flip_horizontal = true;
    canvas_pass(app, viewport_id);
    let window = app.ui.windows.iter().find(|w| w.id == a).unwrap().clone();
    crate::canvas::docked_view::dock(app, &ctx, &window, crate::canvas::docked_view::Side::Right);
    let other = app.ui.windows.iter().find(|w| w.id == b).unwrap().clone();
    crate::canvas::docked_view::dock(app, &ctx, &other, crate::canvas::docked_view::Side::Top);
    gpu.discard_output(output(a));
    gpu.discard_output(output(b));
    let mut root = ctx.run_ui(egui::RawInput::default(), |ui| {
        for (id, x) in [(a, 0.0), (b, 260.0)] {
            app.with_document_window(id, |app| {
                let rect = egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(256.0, 192.0));
                let camera = app.ui.views[0].clone();
                crate::canvas::canvas_view(app, ui, 0, rect, camera, false);
            });
        }
    });
    root.textures_delta.clear();
    assert_ne!(gpu.read_texels(output(a)).unwrap(), gpu.read_texels(output(b)).unwrap());
    let other_output = gpu.read_texels(output(b)).unwrap();
    crate::canvas::docked_view::detach(app, &ctx, b, false);
    assert_eq!(gpu.read_texels(output(b)).unwrap(), other_output);
    let mirrored = gpu.read_texels(output(a)).expect("mirrored docked GPU output in root viewport");
    crate::canvas::docked_view::detach(app, &ctx, a, false);
    canvas_pass(app, viewport_id);
    assert_eq!(gpu.read_texels(output(a)).unwrap(), mirrored, "floating must keep the composite output");
    assert!(gpu.upload_rect(output(a), [0, 0], [1, 1], &[3, 251, 29, 255]));
    let marked = gpu.read_texels(output(a)).unwrap();
    assert_ne!(marked, mirrored);
    app.ui.windows.iter_mut().find(|w| w.id == a).unwrap().view.flip_horizontal = false;
    canvas_pass(app, viewport_id);
    assert_eq!(gpu.read_texels(output(a)).unwrap(), marked, "a camera-only flip must not refresh the output");
    app.ui.windows.iter_mut().find(|w| w.id == a).unwrap().view.flip_vertical = true;
    canvas_pass(app, viewport_id);
    assert_eq!(gpu.read_texels(output(a)).unwrap(), marked, "vertical flip must reuse the output");
    app.ui.views[0].flip_horizontal = true;
    app.ui.views[0].flip_vertical = true;
    app.ui.windows.iter_mut().find(|w| w.id == a).unwrap().follow_main_flip = true;
    canvas_pass(app, viewport_id);
    assert_eq!(gpu.read_texels(output(a)).unwrap(), marked, "following Main must also reuse the output");
    let child = app.ui.windows.iter_mut().find(|w| w.id == a).unwrap();
    assert!(!child.view.flip_horizontal);
    assert!(child.view.flip_vertical);
    child.follow_main_flip = false;
    app.ui.views[0].flip_horizontal = false;
    app.ui.views[0].flip_vertical = false;
    gpu.discard_output(output(a));

    let pa = draw(app, a);
    let pb = draw(app, b);
    assert_ne!(pa, pb);
    edit(app, a, 5);
    assert_ne!(draw(app, a), pa);
    assert_eq!(draw(app, b), pb);

    crate::view_visibility::set_follow_main_settings(app, &ctx, a, true);
    let following = draw(app, a);
    app.run("layer.setAdjustment", json!({"layer": adjustment.0, "levels": 3})).unwrap();
    assert_ne!(draw(app, a), following);
    assert_eq!(draw(app, b), pb);
    crate::view_visibility::set_follow_main_settings(app, &ctx, a, false);
    let prior_a = draw(app, a);
    let prior_b = draw(app, b);
    app.run("layer.select", json!({"layer": raster.0})).unwrap();
    app.run("paint.stroke", json!({"points": [[12, 12]], "size": 10, "hardness": 1, "color": "#ff0000"})).unwrap();
    assert_ne!(draw(app, a), prior_a);
    assert_ne!(draw(app, b), prior_b);

    let b_after_paint = draw(app, b);
    app.ui.views[0].flip_vertical = false;
    gpu.discard_output(output(a));
    gpu.fail_next_secondary_allocation();
    draw(app, a); // The CPU fallback must publish A's composite to A's texture.
    assert_eq!(draw(app, b), b_after_paint);

    let (_, budget) = gpu.canvas_fit(&app.session.active().unwrap().doc).unwrap();
    gpu.discard_output(output(a));
    gpu.set_canvas_budget(0);
    app.ui.windows.iter_mut().find(|w| w.id == a).unwrap().view.flip_horizontal = true;
    canvas_pass(app, viewport_id);
    assert!(gpu.texture_info(output(a)).is_none());
    assert!(app.view_composites.caches[&a].values().any(|cache| cache.texture.is_some()));
    gpu.set_canvas_budget(budget);

    app.ui.windows.iter_mut().find(|w| w.id == a).unwrap().open = false;
    crate::canvas::retain_gpu_documents(app);
    crate::view_visibility::retain(app);
    assert!(gpu.read_texels(output(a)).is_none());
    assert!(gpu.read_texels(output(b)).is_some());
}
