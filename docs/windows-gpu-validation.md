# Windows shared-GPU validation

## Vulkan sharing: current validation

The default Windows path now prefers Vulkan GPU sharing; `--presentation=vulkan-shared` requires it and `readback` forces the CPU reference path. Vulkan performs one GPU copy into imported shared memory and GPUI uses its existing D3D11 compositor. No CPU pixel mapping or upload is used by this transport.

On the tested RTX 4080, the final wgpu-managed submission implementation passed a forced-sharing four-camera compositor run: 600 published GPU frames, 900 Present submissions, 28 resize generations, four suspensions/restorations, and animated Fox model replacement. Peak process working set was 516,878,336 bytes. The native pixel regression matches six changing/reused-slot images byte-for-byte against the Vulkan source and checks lease/consumer gating. Both Vulkan and DX12 worker import-failure regressions produce real readback pixels. Workspace tests and strict Clippy pass.

See [Vulkan sharing architecture and commands](vulkan-sharing.md) and the [retained validation report](validation/2026-11-25-vulkan-sharing.json). This adds correctness/lifecycle evidence on one adapter, not a new end-to-end FPS benchmark. The DX12 investigation and earlier results below remain relevant to the shared GPUI importer and explicit DX12 path.


Test machine: Windows, NVIDIA GeForce RTX 4080, pinned Rust 1.100.0 and debug builds. This is correctness investigation, not a release performance benchmark.

## Current result: imported-resource retirement

The visible full viewer now passes fresh forced-shared runs on this RTX 4080 after retaining evicted D3D11 imports until their consumer fence completes. The editor and demos now prefer Vulkan GPU sharing with readback fallback; broader adapters, visual automation and device recreation remain unverified. The investigation history and temporary sampling probes are preserved in commit `7e21fed`; those probes have been removed from the active code.

The atlas previously dropped its imported D3D11 texture, shader view and synchronization owner as soon as the image key was evicted. The producer checked consumer fences before reuse, but that did not explicitly retain the consumer-side import objects. Removed external entries now leave the drawable atlas immediately and enter a retirement queue. Their native objects and producer frame leases stay alive until consumer completion; the device-removal sentinel cannot release them. Collection runs before each GPUI render/import and from the viewport polling task even when no draw is scheduled. Retained leases apply backpressure through the existing three-slot producer ring and bounded retired generations. There is no CPU wait on the UI thread and no additional pixel copy.

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

## Retirement progress while idle

A longer native editor session exposed a scheduling cycle after 3,774 publications: the worker remained visible with no pending producer copies and no free slots, without a device-error log. Retired imports were collected only on a new draw/import, but their leases could prevent the next producer frame that would request that draw. The viewport now polls completed external-image retirement from its existing 16 ms window-bound task. It neither forces continuous redraw nor waits on a fence. The WARP lifetime regression also invokes the public atlas polling hook after releasing its gate, with no draw/import to drive collection.

The acceptance script's `-StaticScene` option keeps the initial model instead of switching to animated Fox, making idle progress testable without animation or input. It still requires the publication/Present thresholds, a loaded scene and clean GPU logs. Default acceptance continues checking both models and advancing animation.

After adding polling, the static-scene check `-Frames 5000 -StaticScene -Port 19873` passed with **5,008 publications and 6,000 successful Present submissions**, zero animation clips and no CPU fallback (`target/shared-validation/20261125-041524/report.json`). No input or animation was needed to maintain progress. This is an idle-retirement progress result on the same adapter, not a release FPS or VRAM benchmark.

## Animated shared-camera rendering

Native DirectX 12 regressions now verify visible morph deformation and continuously playing AnimatedMorphCube pixels in each of four camera tiles. The investigation found that the pinned wgpu 29 DirectX 12 indirect-count path loses emulated vertex/instance base offsets. Patch `0007-dx12-indirect-builtins.patch` selects the existing non-count indirect fallback on DirectX 12. GPU culling remains enabled. Feature negotiation has a DirectX 12/Vulkan unit regression; fallback performance and broader adapters have not been benchmarked. See [shared cameras](shared-cameras.md).
