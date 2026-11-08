# GPU proof of concept for independent adjustment stacks

Baseline f74f5ed68 on feature/independent-views. This experiment runs outside the
application. See the [architecture investigation](gpu-compositing-investigation.md)
for the current canvas and fallback paths.

## Reproduction and methodology

Run the [independent_stacks example](../crates/gpu/examples/independent_stacks.rs)
using the existing Windows build environment with the repository-local TEMP/TMP
workaround. These commands use the optimized Debug profile.

    cargo run --offline --locked -p photocraft-gpu --example independent_stacks
    cargo run --offline --locked -p photocraft-gpu --example independent_stacks -- --curves-only

The optional quick flag runs a 768 × 512 smoke fixture. The default 6000 × 4000
RGB8 fixture contains four dense patterned raster layers, Normal/Multiply/Screen,
varying source alpha, a pixel mask, a 12-level Posterize and a nonidentity master
Curves adjustment. The control omits Posterize. Views A and B use the same document
and layer IDs and clone the same COW surfaces. Their Curves midpoints start at 0.35
and 0.65. The timed edit changes A's midpoint. The timed shared-pixel edit modifies
a 32 × 32 patch of a raster layer, then both views reference the same edited tiles.
The timed render still covers the entire canvas.

Five samples per operation. Each round creates a fresh compositor on the same
device, renders A cold, renders B using the resident sources, changes A's Curves,
then changes the shared pixels. CPU times use full-size flatten on the same effective
stack and region. They exclude fixture creation and edits. GPU wall time starts before
render and ends after the device completes. Host submit time includes preparation,
allocation and staging. The existing GPU timestamp spans exclude queue-level
texture uploads and end-of-batch work. GPU output chunks have a no-op sink during
timing. The tests do not include canvas texture copies, mips, monitor color
conversion, UI scheduling or presentation. Full readbacks run separately.

## Measurements

Windows, RTX 5090, Vulkan, NVIDIA driver 610.88, RGBA32Float accumulation.
Numbers below are medians in milliseconds unless stated otherwise.

| Posterize + Curves, 24 MP | CPU flatten | GPU wall | Host submit | GPU timestamp span | Median upload |
| --- | ---: | ---: | ---: | ---: | ---: |
| Cold A | 415.227 | 229.007 | 159.352 | 3.380 | 415,236,096 bytes |
| Resident B | 410.018 | 5.278 | 3.614 | 1.157 | 0 bytes |
| Resident Curves edit A | 420.665 | 5.067 | 3.528 | 1.148 | 0 bytes |
| Shared pixel edit A, full canvas | 428.288 | 10.478 | 7.232 | 2.664 | 262,144 bytes |

Device startup took 537.280 ms in this run. Compositor setup took a median
706.529 ms separately from first rendering. The cold render staged 1,728 tiles.
The resident B and Curves-edit renders reused 24 stack-prefix chunks. The shared
edit uploaded one tile, and the subsequent B render uploaded zero bytes. The
measured cold overhead is substantial. The API reports bytes and timed render
spans separately, but it has no upload-only timing boundary. Cold wall minus GPU
span includes allocation, host staging, queue copies, synchronization and writeback.
It would be misleading to report that residual as transfer time.

| Curves-only control, 24 MP | CPU flatten | GPU wall | Host submit | GPU timestamp span |
| --- | ---: | ---: | ---: | ---: |
| Cold A | 326.093 | 256.712 | 206.983 | 2.742 |
| Resident B | 331.600 | 5.180 | 3.492 | 1.267 |
| Resident Curves edit A | 345.016 | 4.902 | 3.337 | 1.258 |
| Shared pixel edit A, full canvas | 348.436 | 8.413 | 5.796 | 2.016 |

The control showed the same upload counts and 24 prefix hits for resident B
and the Curves edit. Its setup median was 738.390 ms. These results compare the
existing full CPU flatten with the existing GPU compositor and its resident
prefix reuse. They do not predict the speedup of a reduced CPU viewport or
end-to-end painting in the app.

## Rendering accuracy and isolation

Full-frame readbacks after the shared edit were compared channel by channel in
working RGB, including alpha. Errors below are normalized absolute channel values
and the mean includes all 96 million channels. Two 8-bit codes is the diagnostic
threshold for counting outliers, not a production acceptance tolerance.

| Fixture and view | Maximum | Mean | Channels beyond 2/255 |
| --- | ---: | ---: | ---: |
| Posterize + Curves A | 0.09803918 | 0.00000024 | 192 |
| Posterize + Curves B | 0.07450974 | 0.00000020 | 192 |
| Curves-only A | 0.00000006 | 0.00000004 | 0 |
| Curves-only B | 0.00392163 | 0.00000007 | 0 |

Both variants gave distinct A and B fingerprints. Rendering A again after B
reproduced A's full-frame float-bit fingerprint. All three readback renders
used the already-resident sources without uploads. This checks sequential
reuse of the compositor, including its shared prefix. It does not test two
simultaneously displayed GPU textures or overlapping command submissions.

With Posterize, the worst discrepancies were about 25 and 19 8-bit codes for
A and B. The Curves-only control removes these outliers. That implicates the
Posterize-containing path in this fixture, though it does not establish which
upstream rounding or quantization step crosses the bin boundary. See the CPU
[Posterize calculation](../crates/compose/src/adjust/tone.rs#L275) and the
[WGSL calculation](../crates/gpu/src/compose.wgsl#L500). Reproduce the discrepant
pixel at (3002, 142) in a smaller test and resolve or explicitly fall back for
the affected combination before exposing it to users. The example prints
parity failures as measurements and fails for an isolation or resource-reuse
violation. CMYK/Lab, HDR, color-managed display, group/clipping parity, vector
masks and imported fills were outside this fixture.

## Memory and production implications

The compositor reported 415,236,096 resident source bytes (396 MiB) and a
402,653,184-byte shared prefix (384 MiB) for this case. These are its cache
accounting values, not measured total VRAM. One dense 24 MP RGBA32F output
requires about 366 MiB of raw pixel storage, or 183 MiB at RGBA16F. Two dedicated
32F outputs plus the measured sources and one prefix would total roughly 1.48 GiB
before scratch, mips, staging, display resources and driver overhead. The three
reported slots would represent about 48 MiB if each were a 1024-square 32F
scratch texture. The example streams output chunks and does not allocate two
persistent viewport outputs. Distinct cached prefixes would increase the budget.

Production would need separate output identities for divergent window stacks and
preview generations while preserving shared page residency. The existing canvas
textures are document-keyed. Introduce output identity at the canvas boundary,
scope intermediate reuse to compatible effective stack prefixes, and reject stale
work after edits, mode switches, window close or device reset. A secondary in
follow mode could share a compatible canonical output, subject to detail and
display requirements. Horizontal flipping belongs in the viewport sampling
transform, with matching pointer mapping and no composite invalidation. The
current secondary and mirror gates must stay until those paths are validated.

The warm timings justify a narrowly scoped production experiment with an
eligible RGB subset once parity is addressed. Start with one shared-source,
two-output case, a bounded memory budget and visible per-view fallback diagnostics.
Validate on another backend and on integrated hardware. Keep CPU fallback and
canonical save/export independent of whichever renderer the canvas uses.

Validation also ran 16 focused GPU planner and WGSL validation tests. All passed.

## Parity follow-up

The [GPU rendering parity report](gpu-rendering-parity.md) contains the
16-adjustment and blend matrices, the one-pixel Posterize boundary repro,
and the repeated 24 MP isolation and performance measurements.
