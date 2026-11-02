# Independent layer visibility, opacity and blending (Phases 1–3)

Open another view with Window > Arrange > New Window for Document. The existing Layers
panel follows the last focused document view. Its **View** target menu (the Phase 1 visibility
selector) can explicitly select Main View or a numbered secondary window. Main View's eyes
and opacity/blend controls edit normal document properties. A secondary target shows a display-only notice:
its eyes, eye sweep, Alt-eye solo, opacity and blending affect only that window. Other layer controls
still edit the shared document. The existing opacity number field, popup slider, scrub label
and multiple-layer selection are reused, along with the existing blending dropdown. Secondary Background opacity/blending are display-only and
do not convert the canonical Background into Layer 0.

Each secondary window captures canonical visibility, opacity and blending when it opens, even if
another secondary window is focused. Later canonical changes do not replace existing
secondary values. New layers inherit canonical values when first encountered; removed layer IDs
are discarded; reordered layers retain their values. Undoing a deletion reintroduces the
layer with its then-current canonical values. Overrides are transient and are not saved
in document files or restored across sessions.

## Architecture

- `DocWindow.visibility` holds layer-ID/boolean values, layer-ID/f32 opacity values in 0..1,
  layer-ID/`BlendMode` values,
  and its own visibility-only solo/restore snapshot. It is excluded from serialization.
  Main views have no overrides. The existing type/field names are retained for compatibility.
- `view_visibility.rs` builds a memoized render-only metadata projection from the current
  display snapshot. Surface tile payloads remain shared `Arc<Tile>` allocations. The
  projection is never installed in Session or passed to save/export/history commands.
- `PhotocraftApp.view_composites` holds caches under window identity, then canonical
  document identity and monitor output. CPU detail textures, pixel grids and channel overlay
  caches use the same window context as the canvas. Canonical document IDs are unchanged.
- The existing scoped document-window camera context also selects the render context.
  The main view retains its normal GPU/CPU path. Secondary views use the existing CPU
  compositor, preserving groups, clipping, masks, blending, color management and reflection.
- Projection identity includes the source snapshot, canonical revision, live-preview key
  and visibility/opacity/blend values. Shared edits invalidate the affected secondary composites lazily.
  A secondary setting change only changes that window's projection identity. Equal effective
  values preserve the projection and texture; repeated slider/dropdown events create no history.
  Damage rectangles are conservatively disabled for secondary composites: a layer hidden
  canonically can have visible effects in a secondary view.
- Secondary composites are synchronous and never start or poll document-scoped GPU
  background redraw jobs. Those jobs cannot write into a secondary texture/cache.
- Closing a window or document releases its projection, canvas and channel caches.

## Current limits

Secondary views can be slower on large multilayer images: each changed composite uses the
CPU path and a conservative refresh. Stationary views reuse textures; zoom/pan retain the
existing detail-texture behavior. GPU variants and partial-damage optimization are future work.

Flattened filter-dialog previews cannot preserve per-layer visibility because they have
already lost layer identity. Secondary views retain a layered image until the edit commits.
Layered live paint snapshots continue to update secondary views. Navigator/Histogram and
sampling remain document-based; they are not independent view analyses. The Layer menu's
canonical editing commands remain document operations; use the Layers-panel eyes for
secondary display visibility.

The Properties panel's existing layer-opacity slider follows the same selected view and
shows a secondary-window notice. Its Fill and other controls remain canonical. Number-key
layer-opacity shortcuts, Properties adjustment parameters, layer-style dialogs, recorded
actions and engine `layer.setProps` remain canonical editing paths. Native secondary-window
number keys retain the existing shortcut-routing limitation. Use Layers/Properties opacity
controls or `view.layerOpacity` for display-only opacity. Tool/brush opacity is unchanged.

Adjustment layers use the existing CPU compositor's opacity semantics exactly; no adjustment
algorithm or blend interpretation is changed. Group, mask, clipping and artboard metadata
are preserved by the same projection used in Phase 1.

Secondary blend dropdowns apply on a choice (click, arrow key or wheel); hovering a mode does
not preview it. PhotoCraft's existing hover preview is document-wide and is suppressed when
targeting a secondary view so it cannot temporarily change the main canvas. Main-view hover
preview remains unchanged; a secondary projection also preserves its own modes while the main
view previews a different mode. Layer-style dialogs, keyboard blend shortcuts and engine
commands remain canonical editing paths. Use the Layers dropdown or `view.layerBlend` for
display-only blending. Pass Through remains group-only, matching the canonical command.
Special layer types use exactly the existing CPU compositor semantics; no new blend behavior
is added for adjustment layers, nested groups or artboards. Masks remain canonical and shared.

## Automation

`ui.menu.invoke` accepts these shell commands:

```json
{"id":"view.layerVisibility","params":{"window":123,"layer":456,"visible":false}}
{"id":"view.layerVisibility.showOnly","params":{"window":123,"layer":456}}
{"id":"view.layerOpacity","params":{"window":123,"layer":456,"opacity":0.25}}
{"id":"view.layerOpacity","params":{"window":123,"layers":[456,789],"opacity":0.75}}
{"id":"view.layerBlend","params":{"window":123,"layer":456,"blend":"Soft Light"}}
{"id":"view.layerBlend","params":{"window":123,"layers":[456,789],"blend":"Overlay"}}
```

Omit `window` to target the last focused secondary view. Invalid/closed windows and missing
layers return errors. Opacity requires a finite number from 0 to 1; invalid batch targets
are rejected before any values change. `ui.inspect.viewVisibility` lists transient visibility
under `layers`, opacity under `opacity`, and blending under `blend`, keyed by layer ID for
each window. Blend strings use the canonical command's parser and aliases; inspection uses
serialized `BlendMode` names. Invalid modes or mixed Pass Through/non-group batches are
rejected before any value changes.
Engine `layer.setProps` remains a canonical document-editing command.

## Phase 1 focused verification

- UI crate compilation check passed.
- Ten new `view_visibility` tests passed: independent pixels and visibility; canonical
  revision/history and save/export bytes; raster/mask tile sharing; cache reuse and shared
  edits; focus routing and camera preservation; added/removed/reordered layers; group solo;
  preview invalidation; separate channel caches; closing/reopening documents and views.
- Existing Independent Views tests: 14 passed.
- Existing Linked Cursor tests: 13 passed.
- Existing channel-view tests: 13 passed; one benchmark remained ignored.
- No full test suite, Clippy, WebAssembly build, layering audit or benchmark was run.

The focus test's first run exposed missing texture-delta cleanup in the new headless test
harness. That cleanup was corrected; the final focused run passed. MSVC linking requires
execution outside the sandbox because it creates temporary files in AppData.

Manual Windows testing should exercise the visibility target menu, individual eyes and eye
sweeps, Alt-eye solo/restore, edits visible in multiple views, document/window closure, and
visibility while zoomed, panned or horizontally mirrored. Native focus ordering, performance
on large multilayer artwork and live editing previews still need manual assessment.

## Phase 1 files changed

New implementation and tests:

- `crates/ui-egui/src/view_visibility.rs`
- `crates/ui-egui/src/view_visibility/tests.rs`

State, initialization, scoped context and cleanup:

- `crates/ui-egui/src/state.rs`
- `crates/ui-egui/src/lib.rs`
- `crates/ui-egui/src/app.rs`
- `crates/ui-egui/src/app/view.rs`
- `crates/ui-egui/src/app/commands.rs`
- `crates/ui-egui/src/menus/view_cmds/arrange.rs`
- `crates/ui-egui/src/canvas/tabs.rs`

Rendering, panel and command integration:

- `crates/ui-egui/src/canvas.rs`
- `crates/ui-egui/src/canvas/textures.rs`
- `crates/ui-egui/src/canvas/channel_view.rs`
- `crates/ui-egui/src/canvas/pixel_grid.rs`
- `crates/ui-egui/src/panels/layers.rs`
- `crates/ui-egui/src/menus/view_cmds/dispatch.rs`
- `crates/ui-egui/src/control/inspect.rs`

Existing test fixture initializers and documentation:

- `crates/ui-egui/src/canvas/zoom_window_fit.rs` (test initializers only)
- `crates/ui-egui/src/file_open/tests.rs`
- `docs/independent-visibility.md`

Windows desktop build passed: cargo build --offline --locked -p photocraft --features heif (optimized dev profile). Executable: target/debug/photocraft.exe. Manual native-window verification is pending. No commit or push was made.

## Phase 2 verification and files

The focused `view_visibility` group passed all 15 tests: ten Phase 1 tests and five new
opacity tests. New coverage includes independent inherited values, no document dirty/history
changes, identical canonical `.pcraft`/PSD/PNG exports, per-view cache isolation, equal-value
cache reuse, shared raster tile pointers, Background and multiple-layer opacity routing,
atomic validation, shared pixel edits, renamed/reordered/grouped/deleted layers, closed views,
and Black & White/Posterize/Hue-Saturation adjustment results compared with the existing
compositor at 0%, 25%, 50% and 100%. No adjustment algorithm changed. Formatting was applied;
no full suite, Clippy, wasm build or benchmark was run.

Phase 2 changes these files only:

- `crates/ui-egui/src/view_visibility.rs`: opacity values, projection/cache comparisons,
  command validation and shared panel routing; inspection exposes values through the existing
  `viewVisibility` structure.
- `crates/ui-egui/src/state.rs`: update the display-state field documentation.
- `crates/ui-egui/src/menus/view_cmds/dispatch.rs`: enable the opacity shell command.
- `crates/ui-egui/src/panels/layers.rs`: retain the selector and focus behavior, display effective
  opacity, route single/multiple targets, allow display-only Background opacity.
- `crates/ui-egui/src/panels/properties.rs`: use the same selected-view opacity routing and notice.
- `crates/ui-egui/src/view_visibility/tests.rs`: register opacity tests and update the canonical
  non-opacity-control assertion.
- `crates/ui-egui/src/view_visibility/tests/opacity_tests.rs`: five focused opacity tests.
- `docs/independent-visibility.md`: usage, architecture, API, limitations and verification notes.

Manual Windows verification should check the View selector, numeric opacity entry, popup
slider and label scrubbing, Properties opacity, multiple layers, Background, adjustment
layers, simultaneous windows and shared painting with independent opacity/visibility.

Phase 2 Windows build passed with cargo build --offline --locked -p photocraft --features heif (optimized dev profile). The updated executable is target/debug/photocraft.exe. Native interaction and visual testing remain manual. Phase 2 changes are uncommitted; nothing was pushed.

## Phase 3 verification and files

Phase 2 was verified committed at `2b34711cb` with a clean tree before Phase 3 editing.
All 21 focused `view_visibility` tests passed: 15 existing tests and six new blend tests.
New coverage includes independent modes and canonical inheritance, history/dirty-state and
`.pcraft`/PSD/PNG export isolation, same-mode/alias cache reuse, per-view cache invalidation,
shared source tiles and shared mask edits, main/secondary hover isolation, batch validation,
Pass Through restrictions, independent visibility/opacity and camera preservation, nested
groups, artboards, adjustments, clipping/masks, and layer/document/window lifecycle cleanup.
Special layer results are compared with the existing CPU compositor using canonical test
documents with equivalent properties. No blending or adjustment calculation was changed.

Phase 3 changes these seven files:

- `crates/ui-egui/src/view_visibility.rs`: blend values, projection/cache comparison,
  command validation, shared layer-target validation, panel routing and hover isolation.
- `crates/ui-egui/src/state.rs`: presentation-state documentation.
- `crates/ui-egui/src/menus/view_cmds/dispatch.rs`: enable `view.layerBlend`.
- `crates/ui-egui/src/panels/layers.rs`: effective-mode dropdown, secondary batch/Background
  routing, Pass Through option eligibility and scoped hover handling.
- `crates/ui-egui/src/view_visibility/tests.rs`: register blend tests.
- `crates/ui-egui/src/view_visibility/tests/blend_tests.rs`: six focused blend tests.
- `docs/independent-visibility.md`: usage, automation, limitations and verification notes.

Formatting was applied. No comprehensive suite, Clippy, wasm build, benchmark or dependency
audit was run. Native manual testing should check different modes in multiple windows,
switching View/layer selection, click/arrow/wheel dropdown choices, visibility and opacity
combinations, masked/clipped groups and adjustments, shared editing and window/document
closure. Secondary hover preview is intentionally disabled; main hover preview is retained.

Phase 3 Windows build passed: cargo build --offline --locked -p photocraft --features heif (optimized dev profile). Executable: target/debug/photocraft.exe. Phase 3 changes are uncommitted on feature/independent-views; HEAD remains 2b34711cb. Nothing was pushed. Native visual/interaction verification is pending.
