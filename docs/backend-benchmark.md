# Vulkan / DirectX 12 animation benchmark

The maintained harness runs the real MCP model showcase against the native offscreen viewer core, with an explicitly selected graphics backend. It preserves original native clips, the skinned spring/Fox, MCP morph edits and the cinematic camera path.

## Reproduction

From the workspace root:

```powershell
cargo build -p nightshade-viewer-core --example backend_benchmark --release --locked
python examples/mcp-cinematic/benchmark.py --output target/backend-benchmark/frozen --prepare-only
python examples/mcp-cinematic/benchmark.py --manifest target/backend-benchmark/frozen/manifest.json --output target/backend-benchmark/measured --duration 24 --width 1920 --height 1080 --views 4 --order dx12,vulkan,vulkan,dx12
```

The first command prepares an optimized native Rust host. Preparation resolves remote-first/local-fallback assets once, then records chosen files, SHA-256 hashes and the RNG state. Timed runs read that frozen manifest and verify each hash. Reuse an existing manifest to retain the same assets; fresh network availability can otherwise change the selected pool. Output run directories must not already exist.

Each act runs for 24 seconds, with the first two seconds after clip/camera setup omitted. Every pass uses the same asset sequence, selected native clips, playback speeds and normalized camera path. MCP updates are capped at 12 Hz; native rendering is uncapped. This is the actual real-time MCP controller, not a frame-identical deterministic replay. Setup/response delays and scheduling can shift animation phase and camera sampling by different amounts on different backends. Non-cinematic views are framed on each replacement model. Snapshots after each act and sampled pixel hashes provide output evidence.

## Metrics and limits

The host permits one frame in flight and waits for GPU completion every frame. It writes raw frame samples to CSV and backend/adapter metadata to JSON:

- `dispatch_ms`: native scene advance, extraction, rendering preparation and command submission on the CPU.
- `render_complete_ms`: elapsed time from scene advance through GPU completion, excluding output readback.
- `gpu_span_ms`: the engine's GPU queue timestamps around its frame submissions. This includes gaps between submissions and is not an isolated sum of shader execution times.
- `readback_ms`: additional completed texture readback, mapping, row conversion and channel conversion using the maintained readback ring.
- `total_ms`: completed rendering plus that serialized readback, before benchmark CSV/hash bookkeeping.

Dispatch and GPU timestamp spans overlap; do not add them. The normal editor can overlap work and use a shared GPU transport, so reciprocal benchmark frame times are not displayed editor FPS. The benchmark does not measure GPUI upload, D3D11 shared-texture sampling, window composition or Present. Vulkan-to-GPUI sharing is not implemented. This comparison isolates the existing backend paths and a common readback workload; it cannot alone choose the best full application presentation path.

Idle/load telemetry is saved before and after each pass. The test runs on the user's active desktop, not an otherwise isolated GPU. Background allocation/activity, clocks, shader compilation, driver cache state and thermal conditions can influence results. The DX12/Vulkan/Vulkan/DX12 order provides two passes per backend with reversed outer order; it does not remove these effects. Report per-act repeated results instead of treating every frame as an independent experiment.

Loading and screenshots are requested during idle phases. Phase markers are polled at frame boundaries, so interval classification has one-frame granularity. Asset downloads, shader/model setup, the two-second warmup, MCP client sleeps and report generation are excluded. DX12 uses the correctness workaround disabling indirect-count draws in the pinned wgpu 29 backend; Vulkan retains that feature. This measures the actual current implementations, not a general claim about either API.

## Recorded results

See [the 2026-11-25 measurements](benchmarks.md#longer-animation-vulkan-vs-dx12-2026-11-25) for the four-pass results and limitations.
