# Measurements

## Longer animation: Vulkan vs DX12 (2026-11-25)

Optimized Rust build, RTX 4080, NVIDIA driver 617.42, 1920x1080, four shared scene cameras. Frozen seed-7 assets and playback choices; four acts at 24 seconds each. Pass order DX12/Vulkan/Vulkan/DX12, two-second per-act warmup omitted. 15,091 measured frames. Values below are completed offscreen render time including scene advance and GPU completion, excluding CPU pixel readback. Each cell gives equal weight to the two pass means.

| Act | DX12 render ms | Vulkan render ms | Vulkan speedup |
| --- | ---: | ---: | ---: |
| AnimatedMorphCube | 193.14 | 7.10 | 27.2x |
| CoiledSpring | 191.88 | 8.40 | 22.8x |
| AnimatedMorphSphere | 197.88 | 7.15 | 27.7x |
| Fox | 194.23 | 6.75 | 28.8x |

Equal-weight means across acts and passes: completed rendering **194.28 ms DX12 vs 7.35 ms Vulkan** (26.4x). Additional serialized readback/conversion: **8.56 vs 4.67 ms**. Total completed render plus readback: **202.85 vs 12.02 ms** (16.9x).

CPU dispatch/preparation costs were similar (7.45 vs 6.51 ms); the large difference appeared in the completed render/queue span. The engine GPU timestamp span averaged 189.42 vs 6.31 ms and includes gaps between submissions. This establishes the current offscreen Vulkan path as substantially faster for this scene; it does not isolate the cause. DX12 currently disables indirect-count draws to work around a correctness bug in pinned wgpu 29, while Vulkan retains them.

Both DX12 passes were slow: act means ranged 186.76–203.36 ms. Vulkan act means ranged 6.16–9.32 ms. Two passes are limited repeat evidence, not a broad hardware/API survey. The active desktop retained about 12.1 GiB GPU allocation and 35% reported GPU activity before the first measured pass despite other apps being paused. Background load and residency pressure remain confounders.

These are **not displayed editor FPS**. The benchmark serializes completion and readback, excludes GPUI upload/display and does not benchmark the Windows shared-texture transport. Output captures were inspected and every act had changing sampled pixels. See [method and reproduction](backend-benchmark.md) and [retained per-pass results, percentiles, asset hashes and telemetry](benchmarks/2026-11-25-backends.json). Raw CSVs and captures are in `target/backend-benchmark/long-animation-measured`; `target/backend-benchmark/long-animation-results.zip` packages the measured evidence.

## Historical debug readback measurements

Actual Windows execution on NVIDIA GeForce RTX 4080, wgpu Vulkan, Rust 1.100.0, **unoptimized debug build**. Each resolution submits 30 cube frames through a three-buffer ring. CPU frames are de-padded/swizzled; stale completed frames are dropped. These timings exclude GPUI atlas upload and native presentation and are not GPU frame timings.

Command: `cargo run --example presentation_benchmark` (`BENCH_FRAMES` changes the submission count).

| Size | Delivered | Elapsed ms | Mean render CPU ms | Mean readback latency ms | Delivered FPS | Dropped | Staging bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1280×720 | 23 | 2147.824 | 10.802 | 110.374 | 10.709 | 7 | 11059200 |
| 1920×1080 | 25 | 4500.547 | 11.883 | 264.692 | 5.555 | 5 | 24883200 |
| 2560×1440 | 30 | 8211.772 | 10.987 | 522.215 | 3.653 | 0 | 44236800 |
| 3840×2160 | 30 | 18636.466 | 10.694 | 1206.658 | 1.610 | 0 | 99532800 |

Debug scalar pixel conversion and GPU/CPU overlap affect these results substantially. Do not use them as release performance predictions. The rows were measured before later pick/outline and shutdown changes; repeat the command for the current tree. At that milestone, no release benchmark or GPU-copy/shared-resource throughput comparison had been measured.

Manual native UI observations: a 2688×1308 readback viewport, a 2688×1308 shared GPU viewport with orbit, GPUI overlay above both transports, and two 1344×1308 shared GPU viewports. This verifies visible compositing, not an FPS claim. Repeated resizing/minimize/restore, docking/popovers and forced device loss are not yet stress-tested.

Automated checks: row alignment/overflow, logical-to-physical coordinates/capture conversion, asynchronous BGRA readback of an unaligned 65×2 texture, ring saturation, stale-frame dropping, generation metadata, channel order and opaque alpha. The headless example checks nonuniform rendered pixels after resize and a center GPU pick.

Regression compile: `cargo check --manifest-path upstream/nightshade/Cargo.toml -p viewer --locked --target-dir target/upstream-check` passed for the original trunk viewer (upstream manifest warnings remain).
