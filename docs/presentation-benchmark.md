# Visible presentation benchmark

Build `cargo build -p gpui-nightshade-viewer --release --locked`, then run from the repository root:

```powershell
python examples/mcp-cinematic/presentation_benchmark.py --manifest target/backend-benchmark/long-animation/manifest.json --output target/presentation-benchmark-measured --duration 24
```

The asset manifest is produced by `benchmark.py --prepare-only` (see [backend benchmark](backend-benchmark.md)). It freezes asset hashes and the RNG state used for clip/speed choices. The presentation runner verifies hashes and identical playback choices between passes. It opens the real editor, builds each scene through MCP, and uses the same cube, skinned coiled spring, morph sphere and Fox sequence. All four cameras view one animated scene. The cinematic camera and explicit morph edits are driven at 12 Hz; native clips advance independently.

Pass order is forced Vulkan sharing, Vulkan readback, readback, sharing. Each act runs for 24 seconds with its initial two seconds excluded. The runner uses isolated dock-layout paths and disables native settings persistence and remote catalog fetching. Do not resize, minimize or cover the benchmark window during measurement. Physical viewport dimensions are recorded and must agree across every act/pass. Normal worker pacing and UI polling remain enabled.

`GPUI_NIGHTSHADE_BENCHMARK=1` enables diagnostic stderr records for frame publication, viewport adoption and successful GPUI swap-chain Present returns. The default application has no event logging. On Windows, all event timestamps and Python act boundaries share the QueryPerformanceCounter clock. The runner excludes frames whose render began before the measured interval.

Published FPS counts frames made available by the render worker. Adopted FPS counts frames taken by the viewport. Unique presented FPS associates each adopted frame with the first subsequent successful Present return, counting that scene frame once. Total Present submissions can include repeated scene images and must be reported separately. This association measures application/compositor submission progress; it does not prove physical scanout, visibility of every adopted image, or GPU completion. Estimated first-Present age runs from native render start to that associated API return. CPU render time includes scene advance and command submission, not GPU execution completion.

Each run retains stderr, measurement intervals, four scene captures, GPU telemetry and per-act distributions. `results.json` contains throughput and mean/median/p95 age. Two repetitions per mode on one active Windows desktop offer limited repeat evidence, not a general hardware comparison. Diagnostic logging also has overhead in both modes. The separate renderer benchmark measures completed offscreen work and answers a different question.
