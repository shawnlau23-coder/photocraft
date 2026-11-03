# Independent adjustment parameters, Phase 4B

Branch: `feature/independent-views`, starting commit `c9df52436`.

## Scope and storage

The document model has 16 known adjustment types: Brightness/Contrast, Levels, Curves,
Exposure, Vibrance, Hue/Saturation, Color Balance, Black & White, Photo Filter, Channel Mixer,
Color Lookup, Invert, Posterize, Threshold, Gradient Map and Selective Color. All now participate
in target-aware independent editing. Invert has no parameters; its per-view visibility uses
the existing layer controls. Imported `Unsupported` adjustments remain preserved and read-only.

No second adjustment architecture was added. `DocWindow.visibility.adjustments` retains the
Phase 4A typed `Arc<Adjustment>` snapshots by stable LayerId. New windows inherit Main. Existing
windows retain their own values after Main edits. Existing metadata projections, CPU rendering,
histogram bins, tile sharing, cache identity, lifecycle cleanup and serialization remain in use.
Secondary edits change only snapshot/draft handles, never Session, revision, history, pixels or
save/export inputs. A change in one secondary view does not invalidate another view's projection.

## Editors and session safety

The shared graphical editors are reused, including Curves points/pencil/channel controls,
Gradient Map strip/stops/presets/colors, color swatches and full Color Picker, dropdowns,
checkboxes, sliders and numeric fields. Gradient preset imports update the existing shared
preset library; selecting a preset applies only to the targeted adjustment.

Selective Color and Color Lookup retain their existing hosts and controls, with writes routed
to the same Phase 4A preview/commit functions for secondary targets. Color Lookup supports
built-ins, configured-folder files, Load 3D LUT, interpolation and dither. Tables and source
payloads remain shared through the document model's existing Arcs. Wheel previews keep the
existing 0.75-second commit delay, scoped to the target and session generation.

Main keeps its canonical editor/command/history behavior (including Color Balance coalescing
and two-step reset). Secondary Color Balance also supports reset to its session-opening values,
then defaults. Main's legacy hosts now have immutable target scopes for picker safety.

The target contains document, layer, Main/window, kind, mode and snapshot epoch. The editor's
generation changes after switching targets or aborting. Gesture state is reset when the generation
changes while channel/range selections remain per-target. Owned color pickers and queued requests
are tagged with this identity; stale requests/results are rejected. Picker previews remain drafts
until OK, and Cancel aborts rather than committing a preview. Switching away closes an owned picker.

Async LUT completions must still match the captured target and generation, active document/layer,
selected view, mode and layer type. Returning to a previous view cannot revive an old callback.
Stale completions report an error and do not refocus another document or edit a different target.
Closed windows release snapshots, projections, histograms and editor-owned resources through the
existing cleanup. Shared LUT-folder discovery is app-level list data, not a per-view adjustment.

## Restrictions retained

- Secondary Levels Auto and Levels/Curves canvas eyedroppers remain disabled. Main retains them.
- Secondary adjustment Color Pickers retain their graphical and numeric color controls, but canvas
  eyedropper sampling is explicitly unavailable. The picker and status text explain this restriction;
  canonical Main sampling is not silently used as the secondary view's image.
- Curves/Threshold use the same secondary input histogram infrastructure as Levels. Existing
  proxy sampling, enclosing-group-output semantics and conservative source invalidation remain.
- Mask properties/pixels, clipping and other document edits remain shared. No independent masks,
  new GPU compositor, file-format changes, dependency changes or additional view features were added.
- Large secondary composites retain the existing CPU fallback and its performance limitations.
- View settings are temporary and do not become saved/exported document adjustment properties.

## Manual checks

1. Open Main and two windows for one document. Select each through the Layers View selector.
   Edit every adjustment type; verify the other views retain their values and shared painting updates.
2. For Curves, add/move/delete points, select channels and use pencil mode. Switch views during a
   held gesture. For Gradient Map, drag/cross stops, select presets and edit stop colors.
3. Open a Photo Filter or tint Color Picker. Preview, Cancel and OK; switch views/layers while it
   is open. Check the preview is canceled and cannot reappear on the next target. Main should still
   make one undo step for a confirmed edit and no edit for Cancel.
4. In Color Lookup, choose built-ins, wheel-preview them, change interpolation/dither and load a
   custom LUT. Change targets or close the window before a pending file selection completes.
5. Check Main undo/redo and save/export; close/reopen secondary windows to verify fresh inheritance.

## Validation

- 35 focused `view_visibility` tests passed, including all-type parameter/render/history/export
  isolation, preview cancellation, target generations, owned color-pickers, actual graphical
  Curves point editing/target switching, Gradient Map stop insertion and Photo Filter Cancel/OK.
- 23 existing `adjust_editors::ui_tests` passed, preserving Main's actual controls, mode/depth
  coverage, Curves/Levels eyedroppers, Color Balance history/reset, gradient imports and LUT wheel use.
- Five `lookup_picker_tests` passed, including custom LUT embedding, folder lookup, invalid/canceled
  files and a new target-lifetime/generation test covering accepted, switched, returned, closed
  and replaced secondary adjustment targets. Total: 63 focused tests.
- `cargo build --offline --locked -p photocraft --features heif` passed. The updated optimized-dev
  Windows executable is `target/debug/photocraft.exe`. No release rebuild or package update was made.
- A native isolated-profile smoke test opened two document windows, applied an isolated secondary
  Curves override, checked canonical revision/history, captured and inspected its Properties panel,
  then closed the test instance. Evidence: `log/phase4b-native/secondary-curves-properties.png`.
- `cargo fmt -p photocraft-ui-egui` and `git diff --check` passed. No full suite, Clippy, wasm build,
  dependency change or benchmark was performed. The existing unused Cargo-patch warnings remain.

## Files changed

- `view_visibility/adjustments.rs`: supported types, session generations, baseline reset and stale-result checks.
- `dialogs/adjust_editors/session.rs`: shared graphical host routing, gesture and picker transaction scopes.
- `panels/adjust_ui.rs`: Selective Color and Color Lookup routing and async completion checks.
- `dialogs/color_picker_ui.rs`, `dialogs.rs`: scoped picker requests/results, explicit Cancel and sampling restriction.
- `dialogs/adjust_editors/gradient_map.rs`: gesture cancellation without replacing its controls.
- `panels/properties.rs`: canonical reset preservation and view-specific Color Balance reset memory.
- `view_visibility/tests.rs`, `view_visibility/tests/adjustment_tests.rs`,
  `view_visibility/tests/phase4b_tests.rs`: focused regression coverage and fixture reuse.
- This report. All source paths above are relative to `crates/ui-egui/src`.

All 11 changed/new files are left for review; no commits or pushes were made.
