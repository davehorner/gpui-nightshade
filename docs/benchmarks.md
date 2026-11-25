# Measurements

Actual Windows execution on NVIDIA GeForce RTX 4080, wgpu Vulkan, Rust 1.100.0, **unoptimized debug build**. Each resolution submits 30 cube frames through a three-buffer ring. CPU frames are de-padded/swizzled; stale completed frames are dropped. These timings exclude GPUI atlas upload and native presentation and are not GPU frame timings.

Command: `cargo run --example presentation_benchmark` (`BENCH_FRAMES` changes the submission count).

| Size | Delivered | Elapsed ms | Mean render CPU ms | Mean readback latency ms | Delivered FPS | Dropped | Staging bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1280×720 | 23 | 2147.824 | 10.802 | 110.374 | 10.709 | 7 | 11059200 |
| 1920×1080 | 25 | 4500.547 | 11.883 | 264.692 | 5.555 | 5 | 24883200 |
| 2560×1440 | 30 | 8211.772 | 10.987 | 522.215 | 3.653 | 0 | 44236800 |
| 3840×2160 | 30 | 18636.466 | 10.694 | 1206.658 | 1.610 | 0 | 99532800 |

Debug scalar pixel conversion and GPU/CPU overlap affect these results substantially. Do not use them as release performance predictions. The rows were measured before later pick/outline and shutdown changes; repeat the command for the current tree. No release benchmark or GPU-copy/shared-resource throughput comparison has been measured.

Manual native UI observations: a 2688×1308 readback viewport, a 2688×1308 shared GPU viewport with orbit, GPUI overlay above both transports, and two 1344×1308 shared GPU viewports. This verifies visible compositing, not an FPS claim. Repeated resizing/minimize/restore, docking/popovers and forced device loss are not yet stress-tested.

Automated checks: row alignment/overflow, logical-to-physical coordinates/capture conversion, asynchronous BGRA readback of an unaligned 65×2 texture, ring saturation, stale-frame dropping, generation metadata, channel order and opaque alpha. The headless example checks nonuniform rendered pixels after resize and a center GPU pick.

Regression compile: `cargo check --manifest-path upstream/nightshade/Cargo.toml -p viewer --locked --target-dir target/upstream-check` passed for the original trunk viewer (upstream manifest warnings remain).
