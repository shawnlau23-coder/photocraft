//! Per-window input histograms, using the existing CPU compositor and tone binning.
use super::*;
use crate::tone::{self, HistSource, Histograms};
use photocraft_doc::adjust::ToneSpace;

pub(crate) struct Cache {
    source: Weak<Document>,
    document: DocId,
    revision: u64,
    layer: LayerId,
    tag: u64,
    state: Visibility,
    values: BTreeMap<LayerId, Arc<photocraft_doc::Adjustment>>,
    histogram: Arc<Histograms>,
}

impl Cache {
    pub(super) fn document(&self) -> DocId {
        self.document
    }
    pub(super) fn layer(&self) -> LayerId {
        self.layer
    }
}

pub(crate) fn get(app: &mut PhotocraftApp, window: u64, layer: LayerId, space: ToneSpace) -> Arc<Histograms> {
    initialize(app, window);
    let Some(w) = app.ui.windows.iter().find(|w| w.id == window && w.open) else { return Arc::new(Histograms::default()) };
    let Some(st) = app.session.documents().get(w.document) else { return Arc::new(Histograms::default()) };
    if st.doc.layer(layer).is_none() {
        return Arc::new(Histograms::default());
    }
    let source = st.doc.clone();
    let revision = st.revision;
    let mut state = w.visibility.clone();
    // Parameter identity is compared via shared handles below, never by hashing LUT payloads.
    state.adjustments.clear();
    state.adjustment_serial = 0;
    state.solo = None;
    let mut values = adjustments::effective_snapshots(app, window);
    values.remove(&layer); // Its own preview/commit cannot change the input histogram.
    let tag = tone::tag(HistSource::BelowLayer(layer), space);
    if let Some(cache) = app.view_composites.histograms.get(&window)
        && cache.source.ptr_eq(&Arc::downgrade(&source))
        && cache.revision == revision
        && cache.layer == layer
        && cache.tag == tag
        && cache.state == state
        && cache.values.len() == values.len()
        && cache.values.iter().all(|(id, value)| values.get(id).is_some_and(|other| Arc::ptr_eq(value, other)))
    {
        return cache.histogram.clone();
    }
    let (projected, _) = project(app, window, source.clone());
    // Metadata only: pixel/mask tiles remain shared. Reuse the existing tree traversal and
    // render-bands/proxy histogram implementation, with the stack above the target excluded.
    let mut input = (*projected).clone();
    tone::hide_above(&mut input.layers, layer);
    let histogram = Arc::new(tone::compute_histograms(&input, HistSource::BelowLayer(layer), space));
    app.view_composites
        .histograms
        .insert(window, Cache { source: Arc::downgrade(&source), document: source.id, revision, layer, tag, state, values, histogram: histogram.clone() });
    histogram
}
