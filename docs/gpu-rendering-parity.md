# GPU parity follow-up

Baseline f74f5ed68 on feature/independent-views. Tests ran on Windows, RTX 5090,
Vulkan, with RGBA32Float accumulation unless otherwise stated. The CPU compositor
remains the reference. No application routing or compositor changes are included.

## Posterize boundary diagnosis

The standalone example's probe and a one-pixel fixture reproduce the failure at
the original canvas coordinate (3002, 142). Before Posterize, both alpha values
are 1.0. The first red discrepancy appears during Multiply. The relevant red
and alpha inputs agree when the source is rendered alone. Screen and Normal
carry the discrepancy to the Posterize input.

| Stage | CPU red | GPU red |
| --- | ---: | ---: |
| Base | 0.7294118 | 0.7294118 |
| Multiply | 0.69872093 | 0.698721 |
| Screen | 0.7814528 | 0.7814529 |
| Before Posterize, scaled by 255 | 149.49998 | 149.5 |
| After 12-level Posterize, scaled by 255 | 139 | 162 |

The same result occurred on DX12 on this adapter. Both 8-bit Posterize paths
clamp, scale by 255, round positive half values upward and bin the integer.
Here the inputs round to 149 and 150, which straddle the bin boundary. This
happens in unprofiled RGB, with a fully opaque result and 32-bit float
accumulation, before Curves. Neither color conversion, half-float target
storage, nor a different adjustment order is needed to reproduce it.

The first Multiply difference is consistent with contraction or reassociation
of floating-point blend arithmetic. Using the same float32 source values,
separately rounded products reproduce the CPU red value and fusing a
multiply-add reproduces the GPU red value. The precise driver instruction
sequence was not inspected. Temporarily aligning WGSL blend operation order to
the CPU did not change the Vulkan result and was reverted. A Posterize epsilon
would also misclassify genuinely exact or just-above ties because the shader
has no record of which side the CPU reached. The one-pixel diagnostic asserts
the canonical 139 code and reports the GPU discrepancy explicitly.

A portable bit-equivalent blend path would require controlling arithmetic and
texture conversions across backends, including precision when sampling
16-bit sources. That change is outside this localized investigation. Keep
Posterize over unverified GPU backdrops on CPU in any future integration
until this boundary is resolved. No fallback policy was added here.

## Representative adjustment matrix

The standalone diagnostic test reuses the existing adjustment parameter
fixtures and adds extra Threshold and Posterize levels. Each parameter set
runs over a 256 × 128 RGB8 gradient with transparent edges and deterministic
noise detail, both in full Normal mode and with Multiply, partial opacity and
a pixel mask. A case compares the entire frame in straight RGBA before display
color management. Max is the largest normalized absolute channel error across
cases; mean is weighted across all compared channels, including alpha. One
and two 8-bit code values are 1/255 and 2/255. Counts allow four float32 epsilons
to avoid counting representation noise at an exact boundary. More than two
codes fails the diagnostic parity gate; a pass is specific to these fixtures.
This sweep does not include a photographic corpus, HDR, native Lab/CMYK,
or different GPU vendors.

| Adjustment | Cases | Support in sweep | Max | Mean | >1 code | >2 codes | Result |
| --- | ---: | --- | ---: | ---: | ---: | ---: | --- |
| Invert | 2 | CPU and GPU | 3.921628e-3 | 1.008755e-7 | 0 | 0 | Pass |
| Threshold | 6 | CPU and GPU | 3.921576e-3 | 5.194137e-8 | 0 | 0 | Pass |
| Posterize | 6 | CPU and GPU | 3.921509e-3 | 4.431153e-8 | 0 | 0 | Pass |
| Brightness/Contrast | 4 | CPU and GPU | 3.921628e-3 | 8.418355e-7 | 0 | 0 | Pass |
| Exposure | 2 | CPU and GPU | 3.921509e-3 | 6.059814e-8 | 0 | 0 | Pass |
| Levels | 2 | CPU and GPU | 3.921583e-3 | 8.685919e-8 | 0 | 0 | Pass |
| Curves | 2 | CPU and GPU | 6.000000e-8 | 4.529053e-8 | 0 | 0 | Pass |
| Hue/Saturation | 8 | CPU and GPU | 3.921628e-3 | 1.395590e-6 | 0 | 0 | Pass |
| Vibrance | 6 | CPU and GPU | 3.921509e-3 | 7.351323e-8 | 0 | 0 | Pass |
| Channel Mixer | 4 | CPU and GPU | 3.921628e-3 | 2.746494e-7 | 0 | 0 | Pass |
| Photo Filter | 4 | CPU and GPU | 3.921553e-3 | 4.853065e-8 | 0 | 0 | Pass |
| Black & White | 4 | CPU and GPU | 3.921539e-3 | 4.909134e-8 | 0 | 0 | Pass |
| Gradient Map | 4 | CPU and GPU | 3.921628e-3 | 3.030375e-7 | 0 | 0 | Pass |
| Color Balance | 4 | CPU and GPU | 3.921509e-3 | 4.893571e-8 | 0 | 0 | Pass |
| Selective Color | 4 | CPU and GPU | 3.921598e-3 | 7.893791e-8 | 0 | 0 | Pass |
| Color Lookup | 6 | CPU and GPU | 3.921628e-3 | 1.438159e-6 | 0 | 0 | Pass |

The separate one-pixel Posterize boundary case **fails** (max 0.090196013,
mean 0.022549041, one channel over both thresholds). The 24 MP stress case also
fails for both views, with maxima 0.09803918 and 0.07450974 and 192 channels
per view over two codes. The representative Posterize row does not supersede
these results. Gradient Map with coincident stops is **CPU only**, with the
planner reporting its hard-stop fallback. It is excluded from numeric results.

## Blend and group matrix

Each mode uses the same RGB8 fixture with partial opacity, transparent pixels
and a mask, plus the unadjusted source and an isolated masked clipping group.

| Case | Cases | Max | Mean | >1 code | >2 codes | Result |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Unadjusted source | 1 | 2.980000e-7 | 1.548619e-8 | 0 | 0 | Pass |
| Normal | 1 | 2.980000e-7 | 1.663432e-8 | 0 | 0 | Pass |
| Multiply | 1 | 3.580000e-7 | 1.650085e-8 | 0 | 0 | Pass |
| Screen | 1 | 2.980000e-7 | 1.833977e-8 | 0 | 0 | Pass |
| Overlay | 1 | 3.580000e-7 | 1.763262e-8 | 0 | 0 | Pass |
| Soft Light | 1 | 3.580000e-7 | 1.762800e-8 | 0 | 0 | Pass |
| Color Dodge | 1 | 2.980000e-7 | 1.820427e-8 | 0 | 0 | Pass |
| Difference | 1 | 2.980000e-7 | 1.675568e-8 | 0 | 0 | Pass |
| Masked clipping group | 1 | 3.580000e-7 | 2.246509e-8 | 0 | 0 | Pass |

These small blend differences are far below a code value in the output of
the blends themselves. A downstream discontinuity such as Posterize can
amplify a much smaller difference into a visibly different code.

## Isolation and timing

The standalone example now checks A and B before and after changing only A's
Curves, then checks that the same shared tile edit changes both views. The
separate small-region readbacks use a separate compositor so they do not alter
the timed prefix-cache state. Both checks passed. Full-frame A–B–A fingerprints
and source-upload assertions still pass. The 24 MP rerun recorded median GPU
wall times of 258.170 ms cold, 4.905 ms resident B, 5.049 ms for a Curves edit,
and 10.482 ms after editing shared pixels. Corresponding CPU medians were
421.315, 422.358, 425.192 and 432.934 ms. Warm B and the Curves edit uploaded
zero source bytes. The shared edit uploaded one 256 KiB tile on A and none
on B. These are repeat measurements, not a speedup from a shader change.

## Reproduction

Run these commands in the optimized Debug build environment with the local
TEMP/TMP workaround:

    cargo run --offline --locked -p photocraft-gpu --example independent_stacks -- --probe
    cargo run --offline --locked -p photocraft-gpu --example independent_stacks
    cargo test --offline --locked -p photocraft-gpu --test it independent_stack_parity_matrix -- --ignored --nocapture

The matrix emits per-case lines for aggregation. An unsupported operation
and an out-of-tolerance operation are distinct outcomes. The test run itself
can succeed while recording a measured parity failure. The focused existing
parity tests retain their original assertions.

The four focused existing tests for adjustment layers, blend modes, groups/masks/clipping, and hard-stop fallback passed.
