# Windows shared-GPU validation

Test machine: Windows, NVIDIA GeForce RTX 4080, pinned Rust 1.100.0 and debug builds. This is correctness investigation, not a release performance benchmark.

## Current result: imported-resource retirement

The visible full viewer now passes fresh forced-shared runs on this RTX 4080 after retaining evicted D3D11 imports until their consumer fence completes. The editor still defaults to readback; broader adapters, visual automation and device recreation remain unverified. The investigation history and temporary sampling probes are preserved in commit `7e21fed`; those probes have been removed from the active code.

The atlas previously dropped its imported D3D11 texture, shader view and synchronization owner as soon as the image key was evicted. The producer checked consumer fences before reuse, but that did not explicitly retain the consumer-side import objects. Removed external entries now leave the drawable atlas immediately and enter a retirement queue. Their native objects and producer frame leases stay alive until consumer completion; the device-removal sentinel cannot release them. Collection runs before each GPUI render and before importing another frame. Retained leases apply backpressure through the existing three-slot producer ring and bounded retired generations. There is no CPU wait on the UI thread and no additional pixel copy.

A native WARP regression holds queued consumer completion behind a shared fence, evicts the atlas key, verifies the producer lease remains alive and the tile is no longer drawable, releases the gate from a second device, and verifies collection releases the lease. Its gate is released during unwinding. `just test-gpu` includes it.

Fresh visible runs after the correction:

| Report directory under `target/shared-validation/` | Validation |
| --- | --- |
| `20261125-032447` | 600 publications, helmet-to-Fox switch and advancing animation |
| `20261125-032608` | 1,200 publications, fresh launch and both scenes |
| `20261125-033038` | 1,200 publications and 1,500 successful Present submissions |
| `20261125-033221` | 24 requested resizes, three dock resets, four minimize/restore cycles; 1,203 publications and 1,500 Present submissions |
| `20261125-033435` | Final assertions: 25 worker resize generations, four suspensions and four resumptions; 1,200 publications and 1,800 Present submissions |
| `20261125-034249` | After removing investigation probes/harness: 26 worker resize generations, four suspensions/resumptions; 1,200 publications and 1,800 Present submissions |

These runs force `windows-shared`, use the normal scene shader and do not fall back to readback. Successful Present calls count submissions, not DWM display or presentation FPS. Desktop capture remains unavailable after the earlier resets, so visual automation coverage is limited. Peak process working set in reports is not VRAM.

`just test-shared` now requires publication and successful Present submission thresholds, scene replacement and advancing animation. `just test-shared-stress` additionally drives native GPUI resizing/dock reset/minimize/restore and verifies at least eight worker resize generations and four suspension/resumption pairs. `GPUI_NIGHTSHADE_GPU_STRESS` is an explicit diagnostic switch; normal launches do not resize themselves.

## Maintained checks

- `just test-gpu`: native pixel/import, producer-publication gating, last-consumer reuse and retirement, visibility suspension, and atlas eviction through blocked consumer completion. Tests run serially to avoid independent GPU workloads contending.
- `just test-shared 1200 19790`: fresh visible editor, forced shared presentation, helmet-to-Fox replacement, advancing animation, publication and successful Present submission thresholds, and failure-log checks.
- `just test-shared-stress 1200 19791`: the same acceptance check plus native window resizing, dock reset, minimize/restore, and worker-side lifecycle assertions.

`GPUI_NIGHTSHADE_TRACE_GPU=1` enables bounded heartbeat/publication/resize/visibility and Present submission counters. The standalone six-stage shader sampling harness, its viewer-core test dependency and worker frame tap, compiler/debug switches, and no-draw/signal-only/frozen/fresh/aligned runtime probes were removed after isolating the fix. Draw synchronization hooks are internal again. The lifecycle driver is isolated in `apps/viewer/src/editor/gpu_validation.rs` and runs only when explicitly enabled by the stress script.

## Transport and remaining coverage

Nightshade renders on DX12 and copies its output once into a free shared BGRA texture, then signals a producer-only fence. Publication waits for completed copies. GPUI checks completion, samples through its normal sprite pipeline, unbinds the shared SRV and signals a separate consumer-only timeline. Evicted imports retain their native objects and producer leases through consumer completion. Slot reuse requires expired leases and completed producer/consumer timelines; the device-removal sentinel never counts as completion.

This is one GPU-to-GPU copy with no production CPU pixel readback, not zero-copy rendering into GPUI's backbuffer. There are three live slots and at most two retired producer rings per viewport, with a 256 MiB allocation limit per ring. Tests intentionally read pixels for validation.

Broader adapters/drivers, release-mode timing, actual display/GPU timings and VRAM measurement, multi-viewport stress, independent graphics capture and device recreation remain unverified. Desktop capture still times out after the investigation's earlier GPU resets, limiting visual automation coverage. Readback remains the default backend.
