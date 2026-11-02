# Independent layer visibility (Phase 1)

Open another view with Window > Arrange > New Window for Document. The existing Layers
panel follows the last focused document view. Its **Visibility** target menu can explicitly
select Main View or a numbered secondary window. Main View's eyes edit normal document
visibility. A secondary target shows a display-only notice: its eyes, eye sweep and Alt-eye
solo affect only that window. All other layer controls still edit the shared document.

Each secondary window captures the canonical visibility when it opens, even if another
secondary window is focused. Later canonical visibility changes do not replace existing
secondary values. New layers inherit their initial canonical visibility; removed layer IDs
are discarded; reordered layers retain their values. Undoing a deletion reintroduces the
layer with its then-current canonical visibility. Overrides are transient and are not saved
in document files or restored across sessions. Independent opacity is not implemented.

## Architecture

- `DocWindow.visibility` holds layer-ID/boolean values and its own solo/restore snapshot.
  It is excluded from serialization. Main views have no visibility overrides.
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
  and visibility values. Shared edits invalidate the affected secondary composites lazily.
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

## Automation

`ui.menu.invoke` accepts these shell commands:

```json
{"id":"view.layerVisibility","params":{"window":123,"layer":456,"visible":false}}
{"id":"view.layerVisibility.showOnly","params":{"window":123,"layer":456}}
```

Omit `window` to target the last focused secondary view. Invalid/closed windows and missing
layers return errors. `ui.inspect.viewVisibility` lists the transient layer values by window.
Engine `layer.setProps` remains a canonical document-editing command.

## Focused verification

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

## Files changed

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
