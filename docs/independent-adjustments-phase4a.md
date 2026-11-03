# Independent adjustment parameters: Phase 4A

Posterize and Levels use the existing Properties controls with the Layers panel's **View**
selector. Main View edits canonical parameters and history. Secondary views edit temporary
display settings. Other adjustment editors are read-only while a secondary target is selected;
select Main View to edit them. Independent mask properties are not implemented.

## Architecture

`DocWindow.visibility` now also stores complete typed adjustment snapshots by stable LayerId.
Snapshots hold `Arc<Adjustment>` values; embedded LUT/source resources retain their existing
shared handles. Opening a window captures canonical values, regardless of the currently
focused secondary window. New layers inherit canonical parameters when first encountered.
Renaming/reordering preserves values. Deletion or conversion to a non-adjustment removes them.
An adjustment-type or document-color-mode change replaces the relevant snapshots and changes
their editor epoch; stale editing targets are rejected. Same-type canonical parameter edits
leave existing secondary snapshots unchanged. All adjustment types can be captured by this
mechanism, but Phase 4A only permits Posterize/Levels parameter editing.

The existing CPU render-only metadata projection applies visibility, opacity, blending and
effective adjustment values together. It never replaces Session's document or enters history,
save/export, GPU compositing or document-scoped background jobs. Canonical raster and mask
tiles remain shared. No layer IDs, masks or pixel surfaces are independently created for views.

Projection caches compare immutable adjustment handles rather than repeatedly comparing or
hashing their full payloads. New effective values affect only the owning view's cache identity.
No-op updates retain handles. Committing the same value as a live preview reuses its handle;
cancellation returns to the original committed snapshot. The established conservative CPU
refresh strategy remains unchanged. Large documents can still be slow during slider movement.

## Editor sessions and previews

An explicit target identifies document, layer, main/secondary window, adjustment kind, color
mode and snapshot epoch. Properties captures the target before editing. Widget scope and Levels
channel/drag state are namespaced by that identity. Switching targets cancels an unfinished
preview; values are never redirected to the new view. A cancellation also changes the widget
interaction scope to prevent a held pointer or uncommitted text edit from migrating.

Main View retains `app.live_adjust` previews and canonical `layer.setAdjustment` commits.
Secondary previews use a separate typed draft tagged with the captured target; they never
write `app.live_adjust`. Other secondary snapshots also shield their existing parameters from
Main View's live adjustment-layer previews. Escape cancels an active preview. Layer/type/mode,
view and document lifecycle changes discard invalid drafts.

The existing Posterize slider/numeric field and Levels input/output sliders, numeric fields
and channel selector are reused. Reset to defaults targets the selected view and uses the
existing `adjust_params::default_for`; ordinary edits merge over that view's effective typed
value using `from_params`. Main View's other adjustment editors and Color Balance session
reset behavior retain their existing hosts. The adjustment visibility toggle now uses the
existing per-view visibility routing when a secondary is selected.

## Sampling and other limits

Secondary Levels now shows a view-specific input histogram; **Auto and all three eyedroppers
remain disabled** with an explanation. Main View retains its histogram, Auto and eyedroppers.
Existing document/tool sampling remains canonical.

The focused histogram correction reuses `tone::compute_histograms`, its color-space bins,
banded CPU compositing and automatic proxy cache levels. It supplies the existing per-window
metadata projection and hides the selected adjustment and layers above it, including upper
siblings of enclosing groups. This preserves existing group, clipping, blending and mask
compositing. It is not a new intermediate-backdrop compositor: isolated group output can still
be affected by enclosing group properties. Main's historical `BelowLayer` implementation only
hides the selected layer (upper layers can affect its histogram); Main is deliberately unchanged.

One bounded histogram entry per open secondary window records document identity/revision,
weak source identity, selected layer, channel space, visibility/opacity/blend metadata and shared
effective adjustment handles. The selected adjustment's own parameters are excluded, so its
preview, commit and reset reuse the bins. Other metadata/source changes conservatively refresh
the histogram, including changes above the target even though they cannot affect its pixels.
Calculation is synchronous, as in Main: no asynchronous result can arrive for a previous target.
Closing a view/document or removing the target releases its entry. No canonical pixels, history,
preview state, serialization or raster/mask tile storage are changed by histogram generation.
Large images retain the existing proxy approximation; exact-only inputs can still be costly
on a cache miss. No GPU acceleration or full intermediate-composite cache is added.

Correction files: `view_visibility/histograms.rs`, `view_visibility.rs`, `widgets/tone.rs`,
`dialogs/adjust_editors/session.rs`, `dialogs/adjust_editors/levels.rs`,
`view_visibility/tests/adjustment_tests.rs` and this report.

Manual check: create an underlying Posterize layer and a Levels layer above it; open two
windows, give Posterize different per-view parameters, and switch the Layers View selector.
Check that Levels histograms differ, changing only Levels leaves its input histogram stable,
channel selection is retained, and underlying painting/visibility/opacity/blend changes refresh
the selected view's histogram. Check Main Auto/eyedroppers and undo, then close/reopen windows.

Clipping, mask painting/refinement, mask enable/density/feather controls, image adjustments,
layer creation/deletion, recorded actions and engine commands remain canonical editing paths.
Other adjustment types require a later editor-integration phase; LUT browsing, special reset
sessions, color pickers and their asynchronous callbacks are not added here. The established
flattened-preview limitation remains: secondary views use a layered source until flattened
editing operations commit. Settings are transient and are not serialized in document files.

## Automation

The shell command `view.layerAdjustment` accepts a secondary window, layer ID, action and
parameter object. Omit `window` to use the selected secondary view. Actions are `commit`
(default), `preview`, `cancel`, and `reset`.

```json
{"id":"view.layerAdjustment","params":{"window":123,"layer":456,"params":{"levels":4}}}
{"id":"view.layerAdjustment","params":{"window":123,"layer":456,"action":"preview","params":{"gamma":1.4,"red":{"outBlack":20}}}}
{"id":"view.layerAdjustment","params":{"window":123,"layer":456,"action":"cancel"}}
{"id":"view.layerAdjustment","params":{"window":123,"layer":456,"action":"reset"}}
```

Interactive previews follow the selected Properties target; changing selection cancels an
unfinished preview. Use `ui.focus` to select the intended window before testing preview actions.
`ui.inspect.viewVisibility[].adjustments` exposes committed editor parameter values by layer ID.
Engine `layer.setAdjustment` remains a canonical document command.

## Manual testing

1. Open a small colored document, add Posterize, and set Main View to 8 levels.
2. Open two secondary views. Select their targets in Layers > View and set 4 and 16 levels
   through Properties. Main stays at 8. Change Main to 12; existing secondary values remain.
3. Drag a secondary slider while watching all windows. Release to retain the value; use Escape
   or switch View during an unfinished drag to cancel it. Check Reset to defaults on each target.
4. Repeat with Levels: input/output values and channel-specific settings, including changing
   channels and switching views. Confirm secondary Auto/eyedroppers are disabled.
5. Check Main View's normal editing, undo/redo and eyedropper behavior. Save/export and reopen
   to confirm canonical parameters, then test layer reorder/rename/delete, color-mode changes,
   mask painting, shared painting and window/document closure.

## Files changed

- `crates/ui-egui/src/view_visibility.rs`: extend state and projection identity/application.
- `crates/ui-egui/src/view_visibility/adjustments.rs`: typed snapshots, lifecycle, targets,
  preview/cancel/commit/reset and shell command.
- `crates/ui-egui/src/state.rs`: presentation-state documentation.
- `crates/ui-egui/src/app/frame.rs`: cancel obsolete editor targets before input dispatch.
- `crates/ui-egui/src/dialogs/adjust_editors/session.rs`: targeted Posterize/Levels host.
- `crates/ui-egui/src/dialogs/adjust_editors/levels.rs`: disable secondary Auto independently of histogram availability.
- `crates/ui-egui/src/dialogs/layer_picker.rs`: guard canonical eyedroppers on secondary targets.
- `crates/ui-egui/src/panels/properties.rs`: target-aware reset and visibility controls.
- `crates/ui-egui/src/menus/view_cmds/dispatch.rs`: enable the shell command.
- `crates/ui-egui/src/view_visibility/tests.rs`: register focused adjustment tests.
- `crates/ui-egui/src/view_visibility/tests/adjustment_tests.rs`: unit and actual-control tests.
- `docs/independent-adjustments-phase4a.md`: architecture, limitations and manual instructions.

## Validation and delivery

- After the histogram correction, 30 focused `view_visibility` tests passed: 21 existing tests
  and nine adjustment/histogram tests. Two new histogram tests cover different per-view bins,
  own-parameter preview/commit cache reuse, underlying adjustment/visibility/opacity/blend and
  shared-pixel invalidation, exclusion of upper layers, channel spaces, lifecycle cleanup and
  canonical history/dirty isolation. Existing mode/group/mask and UI tests now cover histograms.
- The existing Main View Levels white-point eyedropper UI regression passed (one test).
- Coverage includes snapshot inheritance, canonical changes, Posterize/Levels projection,
  RGB/Gray/CMYK/Lab Levels parameters, group/clipping/mask behavior, tile identity, LUT sharing,
  cache no-ops and preview-to-commit reuse, preview cancellation, type/mode/conversion lifecycle,
  target-specific channel memory, actual Posterize value switching and held-drag cancellation,
  disabled secondary sampling, canonical history/dirty state and `.pcraft`/PSD/PNG export bytes.
- Windows build passed: `cargo build --offline --locked -p photocraft --features heif`.
  Executable: `target/debug/photocraft.exe` (optimized dev profile).
- No comprehensive suite, Clippy, wasm build or benchmark was run. Native visual/manual
  verification remains pending. Existing unused Cargo patch warnings remain unchanged.

Branch: `feature/independent-views`; HEAD: `cc265aed1`. Fourteen files changed, including this
report. All Phase 4A work is uncommitted. No commit, push, merge, rebase, dependency upgrade or
serialization-format change was performed.
