# GPU compositing investigation for Independent Views

Investigated on feature/independent-views at f74f5ed68 from a clean checkout.
The scope is interactive canvas compositing. Canonical save, export, history and
document mutations continue to use their existing paths.

## Current pipeline

| Stage | CPU responsibility | GPU responsibility |
| --- | --- | --- |
| Document and view | Canonical tiled surfaces, layer metadata, damage and preview tracking. The secondary projection clones metadata and shares raster tiles. | None. |
| Stack evaluation | The CPU reference traverses the stack, renders content, evaluates adjustments against the backdrop, applies masks, and blends groups and clipped layers. Reduced/detail rendering uses the same compositor. | The existing wgpu compositor plans passes on the CPU and evaluates eligible blends, adjustments, groups, clipping, effects and mask sampling in WGSL. Planning can still rasterize content or build combined masks on the CPU. |
| Upload and presentation | The fallback prepares display pixels and egui texture updates. The GPU path stages changed raster pages, masks and auxiliary resources. | The custom canvas consumes resident composite chunks, updates mips and draws the viewport. CPU-composited textures are also presented by egui. The normal resident GPU path needs no per-frame CPU readback. |

Source paths are relative to the repository:

- CPU stack and content: [composite.rs](../crates/compose/src/composite.rs), [content.rs](../crates/compose/src/content.rs), [reduced.rs](../crates/compose/src/reduced.rs), [masks.rs](../crates/compose/src/masks.rs). A pixel or vector mask can contribute coverage, feathering and bounds. Fill placement can depend on mask bounds. Imported fill caches can provide the original raster result. Adjustment layers transform the accumulated backdrop, so dispatching each adjustment in isolation on the CPU and transferring its result would usually introduce costly boundaries.
- GPU planner and execution: [planner.rs](../crates/gpu/src/plan/planner.rs), [compose.wgsl](../crates/gpu/src/compose.wgsl), [encode.rs](../crates/gpu/src/encode.rs), [paging.rs](../crates/gpu/src/paging.rs), [prefix.rs](../crates/gpu/src/prefix.rs). The planner already supports a substantial subset. It can reject operations such as unsupported Advanced Blending combinations, native CMYK fills and adjustments needing native CMYK/Lab evaluation. Keep the existing CPU fallback at view level initially.
- Canvas routing and residency: [canvas.rs](../crates/ui-egui/src/canvas.rs#L383), [textures.rs](../crates/ui-egui/src/canvas/textures.rs), [resources.rs](../crates/ui-egui/src/canvas/gpu_canvas/resources.rs), [doc_textures.rs](../crates/ui-egui/src/canvas/gpu_canvas/doc_textures.rs). The custom canvas gate excludes every secondary viewport and horizontal reflection or non-square pixel aspect. Rotation is already a view uniform. GPU viewport uniforms are already distinct from document textures, which are keyed by document-oriented identifiers.
- Secondary projection and mode switch: [view_visibility.rs](../crates/ui-egui/src/view_visibility.rs), [adjustments.rs](../crates/ui-egui/src/view_visibility/adjustments.rs). ON uses the stored snapshot plus that view's valid draft. OFF uses the canonical display source directly. The latter still gets its own CPU texture cache and remains subject to the secondary viewport GPU gate. Main and secondary camera state are separate.

## Measured evidence and open performance questions

Fresh CPU-only Windows results from this checkout, using the optimized dev build
and five fresh processes per size after an initial 4900 × 4900 check:

| Fixture | Pixels | Median | Min–max |
| --- | ---: | ---: | ---: |
| 4900 × 4900, five layers | 24.01 MP | 283.56 ms | 278.08–285.52 ms |
| 6000 × 6000, five layers | 36 MP | 416.40 ms | 412.09–428.20 ms |

These are full-size flatten times from the example below. They do not measure
viewport refreshes, adjustment dragging, concurrent windows or GPU speedup.

Historical local Windows captures from the earlier view-flip investigation are below.
They are from the existing scripts under the workspace's work/view-flip-perf directory,
before the current Independent Settings implementation. Each row is the first horizontal
**View** flip into the CPU path, with the composite timer reported by the app and a separate
harness render barrier. The barrier includes other frame and synchronization work and its
difference from the composite timer cannot be attributed to upload alone.

| Fixture | Layers | Windows | CPU composite | Render barrier |
| --- | ---: | ---: | ---: | ---: |
| 1024 × 1024 dense | 4 | 1 | 6.13 ms | 48.51 ms |
| 6000 × 4800 dense | 1 | 1 | 103.29 ms | 138.34 ms |
| 6000 × 4800 dense | 4 | 1 | 167.81 ms | 212.51 ms |
| 6000 × 4800 dense | 8 | 1 | 232.00 ms | 292.73 ms |
| 6000 × 4800 dense | 4 | 2 | 162.78 ms | 207.16 ms |
| 6000 × 4800 sparse | 4 | 1 | 17.63 ms | 57.44 ms |

The historical 8-layer fixture's later flip barriers were about 27–57 ms, while the
reported composite timer still read 232 ms. The timer was retained from the last
refresh, so repeated samples cannot be counted as fresh composites. The two-window
fixture does not isolate per-window work and predates divergent adjustment snapshots.
Artwork flips from Image Rotation are document mutations and are excluded.

Source-derived expectations, pending a current interactive trace:

| Operation | Expected work |
| --- | --- |
| Paint or erase | Shared live and committed pixels can change every view. Secondary damage is deliberately conservative. Each affected secondary may refresh the full reduced image plus visible detail, even if a dab touched a small region. |
| Visibility, opacity, blend or adjustment edit | An independent edit changes that view's effective key. A canonical edit changes the source revision. Followers use new canonical values; independent snapshots preserve their values while still seeing shared content. A canonical metadata edit may conservatively refresh a view whose effective values did not change. |
| Toggle Settings | Current mode switch drops the target window's projection, textures, channels and histogram cache, so the next frame is cold for that window. The independent metadata remains stored. |
| Pan, zoom or horizontal flip | Stationary composite textures can be reused. Pan or zoom can request new detail regions or levels. Main flip can switch backend; secondaries already use CPU-composited textures. A GPU flip should be a presentation transform and should leave the composite key unchanged. |

Reproduce the CPU-only baseline with the existing
[bench_flatten](../crates/compose/examples/bench_flatten.rs) example in the normal
optimized dev profile. Run cargo run --offline --locked -p photocraft-compose --example bench_flatten -- 4900 in the local build environment, then repeat the executable with arguments 4900 and 6000. The timer excludes document generation,
GPU upload, display conversion and frame presentation. The example uses a background,
three dense raster layers, a pixel mask, blends and one Curves adjustment.
For representative multi-adjustment interactive runs, use 4000 × 6000 and
6000 × 4800, dense and sparse fixtures, at least 12 layers and several non-identity
adjustments. Compare Main only, Main plus one follower, and Main plus two divergent
secondaries. Hold viewport size and zoom fixed, then repeat at 100% with panning.
Capture cold and warm samples, painting/erasing, edits to high and low adjustments,
mode switches and flips. Report medians and p95 after warmup, with CPU planning,
composition, conversion and upload, GPU completion and end-to-end frame timing separated.
The existing [canvas timers](../crates/ui-egui/src/canvas/gpu_canvas/perf.rs) and
[GPU timers](../crates/gpu/src/timing.rs) provide starting points. A submit duration
measures host work, so use GPU timestamps where supported and synchronized completion
for stand-alone measurements. Keep readback for correctness runs separate.

## Cache correctness

Current secondary caches are keyed first by window, then document/output. The CPU
texture path checks revision and preview/display keys, and projections additionally
check the source Arc, layer metadata and typed adjustment handles. Layer sync drops
deleted IDs, initializes additions from canonical values and retains stable IDs through
reorder. Edits to masks share the canonical source and are not overridden. CPU damage
returns no partial region for a secondary, including followers. This avoids applying
canonical visibility-dependent damage to a different stack. It also makes even small
live brush changes candidates for a full secondary refresh. Closing a view drops its
cache namespace. Switching modes clears it before canonical and projection keys can
meet in the same texture slot. These are code-path findings, not a current interactive
repaint or stale-frame measurement.

Important risks for GPU integration:

- A distinct viewport uniform does not give a distinct composite. Sending two divergent
  projections with the same document ID through the existing document texture path can
  replace the image another viewport samples. Use an explicit composite key or target
  identity rather than manufacturing canonical document IDs.
- Separate immutable resource identity from composite identity. Share raster/mask pages
  only when content identity, format and transform semantics match. Pin uploads until
  queued draws are done. Partition stack-prefix and output caches by effective stack
  and document/source generation. Include preview identity, depth, mode/profile and
  output variant wherever those affect the stored result.
- Include old and new bounds for edits and source-generation changes. Never reuse a
  canonical damage region for an independent stack until dependency tracking proves it
  safe. In-flight work must be discarded on mode switch, close, source change or device
  reset. Add fast paint/erase, undo/redo, mask edit and window-churn tests.
- A follower can share the Main composite only if the source, preview, resolution,
  coverage and display contract are compatible. Different pan/zoom may require different
  detail residency, even when the underlying stack is identical.

## Options, policy and platform

Reuse the existing planner and GPU executor. Start with compatible followers and an
independent RGB view. Keep composite results GPU-resident and let each viewport sample
the appropriate target with its own camera. Share input pages where validated. Maintain
one bounded budget across views with LRU and explicit fallback under pressure. For scale,
a dense 24 MP RGBA32F image alone is about 366 MiB, or about 183 MiB in RGBA16F, before
mips, input pages, scratch slots or other views. These are storage estimates, not
measured VRAM. Tiled residency and budget limits matter even on the RTX 5090.

A small GPU display-only change could remove the mirror gate for eligible Main views
and validate coordinate mapping independently. It cannot address the repeated CPU
composites of divergent secondary adjustment stacks on its own. Likewise, selectively
moving a mask or adjustment to GPU helps when its inputs and outputs remain resident.
Keep vector rasterization and feather caches on the existing path initially. Per-pass
CPU fallback within a GPU stack adds synchronization and transfer costs. Whole-view
fallback is the simpler first boundary. A replacement compositor would duplicate an
existing planner, shaders and correctness suite with no measured justification.

The existing Rendering Mode preference offers Automatic, GPU and CPU / Compatibility
and applies at next launch. See [graphics.rs](../crates/ui-egui/src/prefs/prefs_ui/graphics.rs).
The app also has a startup recovery policy.
Reuse or clarify that policy before adding a second selector. **CPU** can force CPU
composition while allowing normal GPU texture presentation where the app supports it.
**GPU Preferred** can attempt any supported, budget-safe path with CPU fallback.
**Auto** is meaningfully different only if it uses demonstrated workload or adapter
thresholds, such as avoiding cold uploads for small or transient stacks. Until then
Auto and GPU Preferred may select the same canvas path. Expose actual engine and
fallback reason per view in existing diagnostics, with adapter/backend and recent
timing. Avoid a notification on every frame.

Use wgpu/WGSL and the existing [startup backend policy](../apps/photocraft/src/gpu_startup.rs).
Validate Windows DX12/Vulkan, Linux Vulkan and macOS Metal, plus the available
lower-capability and CPU fallbacks. Test integrated hardware, a discrete adapter,
and a no-compatible-device case. Capability checks must include texture limits,
formats, memory and timing-query support. CUDA and vendor-specific APIs are unnecessary.

## Proof of concept and correctness gates

1. After approval, extend the existing stand-alone
   [stack-reuse example](../crates/gpu/examples/bench_stack_reuse.rs) or a focused
   companion example. Use the same 24 MP source surfaces for a canonical RGB stack and
   two divergent view stacks, with several non-identity adjustments, blend modes and
   masks. Compare CPU reference timings with cold GPU, warm repeated adjustment changes,
   and alternating targets. Start without application routing or preference changes.
   Record uploads, resident bytes, planning and completion time. Establish which
   change yields a meaningful p95 improvement over the CPU path on both high-end and
   integrated hardware before expanding scope.
2. Compare chunk readbacks using [GPU readback](../crates/gpu/src/readback.rs) and
   [existing parity tests](../crates/gpu/src/tests.rs) against CPU renders of the
   exact same effective stack. Compare pre-display pixels in the same working space,
   then separately compare ICC/display transforms and HDR/SDR output. Check straight
   versus premultiplied alpha and transparent edges, per-depth quantization, blend
   modes, clipping, pass-through and isolated groups, pixel and vector masks with
   feathering, gradient and imported cached fills, and native adjustment precision.
   Use existing per-format tolerances and add edge cases, rather than widening the
   tolerance to admit visible differences. Keep unsupported stacks on CPU.
3. If the measurements justify it, wire one eligible secondary into the existing
   renderer behind a reversible internal gate. Verify simultaneous contrasting outputs,
   camera-only mirror/zoom/pan, per-view device fallback, switches, repaint latency,
   and canonical history/save/export invariance. Then extend sharing and damage reuse
   based on measured misses. Add user-facing policy only after reliability and platform
   coverage are demonstrated.

The unresolved question is how much current multi-window latency comes from repeated
CPU stack work versus conversion, upload and viewport scheduling. The historical
captures establish a large dense-stack CPU cost, but they cannot establish GPU speedup
or current multi-adjustment responsiveness.
