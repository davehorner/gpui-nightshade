# Windows shared-GPU validation

Test machine: Windows, NVIDIA GeForce RTX 4080, pinned Rust 1.100.0 and debug builds. This is correctness investigation, not a release performance benchmark.


## Current result: imported-resource retirement

The visible full viewer now passes fresh forced-shared runs on this RTX 4080 after retaining evicted D3D11 imports until their consumer fence completes. The editor still defaults to readback; broader adapters, visual automation and device recreation remain unverified. Earlier failing results below describe the protocol before this lifetime correction.

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

These runs force `windows-shared`, use the normal scene shader and do not fall back to readback. Successful Present calls count submissions, not DWM display or presentation FPS. Desktop capture remains unavailable after the earlier resets, so visual automation coverage is limited. Peak process working set in reports is not VRAM.

`just test-shared` now requires publication and successful Present submission thresholds, scene replacement and advancing animation. `just test-shared-stress` additionally drives native GPUI resizing/dock reset/minimize/restore and verifies at least eight worker resize generations and four suspension/resumption pairs. `GPUI_NIGHTSHADE_GPU_STRESS` is an explicit diagnostic switch; normal launches do not resize themselves.

The real render-worker/native-consumer harness also passed 600 full-viewer samples on separate threads. A 120-sample run with the D3D11 debug layer and GPUI's unoptimized debug HLSL flags passed. `SHARED_SAMPLING_D3D11_DEBUG` and `SHARED_SAMPLING_UNOPTIMIZED` enable those controls. The test-only frame tap is absent from production builds.

Before the lifetime correction, viewport-only GPUI, isolated viewer settings, optimized HLSL, and the HWND swapchain with DirectComposition disabled each still reproduced the hang. Those unsuccessful temporary application/shader changes were removed.

## Historical reproduction and isolation

Forced `--presentation=windows-shared` reproduced the full-scene failure. Bounded D3D11 diagnostics identified `DXGI_ERROR_DEVICE_HUNG` (`0x887A0006`) and debug message 378 at the consumer. Shared fences then returned the removal sentinel while the producer's `GetDeviceRemovedReason` still returned `S_OK`. Calling this simply a D3D12 device failure hid that distinction.

The original alternating producer/consumer fence also allowed multiple queues to signal one timeline, including repeated cached draws signaling an older value. Microsoft warns that this can rewind fences and leave waits unsatisfied; see [multi-engine synchronization](https://learn.microsoft.com/en-us/windows/win32/direct3d12/user-mode-heap-synchronization). Producer and consumer now have separate single-writer timelines. This correction alone did not remove the full-scene hang.

Experiments were kept separate:

| Configuration | Observation |
| --- | --- |
| Original shared-resource sampling and alternating fence | Full-scene failure reproduced |
| Separate fence timelines and retained producer resources | Failure still reproduced |
| Publish completed copies, sample shared resource directly | Textured scene progressed, then D3D11 hang after hundreds of frames |
| Copy shared resource into a private D3D11 texture, retain GPU wait | Failure still reproduced |
| Completed publication + private D3D11 texture + checked completion without foreign GPU wait | Manual run reached 4,500 published frames at 327.4 seconds, including helmet-to-Fox replacement and active Fox animation, without a logged device failure; then intentionally stopped |
| Fresh visible-window script using that private-copy handoff | D3D11 hang reproduced after fewer than 300 published frames |
| Retain retired rings until leases and consumer reads complete | Necessary lifetime correction; a fresh private-copy run still reproduced the D3D11 hang |
| Final single-copy, completed-publication transport with separate timelines and bounded retirement | Fresh visible-window reproducer still detects the D3D11 hang; acceptance remains failing |

The long manual run was not sufficient evidence of a reliable fix. The unsuccessful extra-copy experiment was removed. At that stage the changes hardened the single-copy transport, but **full-scene acceptance still failed**; the newer import-retirement results above supersede that status. The editor still defaults to readback.

A separate headless test explicitly selected DX12, rendered the full viewer for 120 additional frames, and read 8,833 distinct colors at 640×480 without a native Nightshade window. At the actual unaligned viewport size, 1351×715, another 120-frame run passed with 15,599 colors. The example prints its actual adapter/backend; environment backend selection alone had selected Vulkan in an earlier run, so that earlier result was not counted as DX12 validation.

## Current handoff

1. Nightshade renders its final output on DX12.
2. A GPU copy writes a free shared BGRA texture; the producer signals its own fence.
3. The bounded pending queue publishes only completed copies, dropping older completed candidates.
4. GPUI checks completion again through the imported fence before sampling. No foreign-queue GPU wait is enqueued.
5. D3D11 signals its separate consumer timeline after every sprite draw, including repeated cached draws. Atlas eviction retains native imported objects and producer leases until that completion. Normal scene ordering and clipping remain in use.
6. Reuse requires expired CPU leases and both timelines complete. Frames retain producer textures/fences; up to two retired rings are retained until their queued consumer work and leases retire. Further resizing applies backpressure at that cap. The removal sentinel never counts as successful completion.

There is **one GPU-to-GPU copy** into shared resources and no production CPU pixel transfer. This is not zero-copy. Tests deliberately read pixels back for validation; that readback is not part of presentation. The three-slot live ring and at most two retired rings bound transport generations, with a 256 MiB allocation limit per ring. Actual peak allocation/VRAM behavior across resizing still needs measurement.

## Repeatable checks

`just test-gpu` includes native D3D12/D3D11 import validation. The regression gates the producer queue to verify unfinished copies are withheld, reads rendered cube pixels through the actual importer, verifies reuse waits for the latest consumer read, verifies the timelines do not rewind one another, and retains a frame after ring retirement.

`just test-shared 1200 19790` builds and launches the real viewer in forced shared mode with an isolated layout path, switches from the default helmet to the bundled Fox, checks animation advancement, and fails on logged device errors or insufficient published frames. It is both a failure detector and a passing check for the import-retirement correction on the tested adapter. It opens a visible viewer window because hiding it correctly suspends presentation. Reports and stderr are written beneath `target/shared-validation/`. The child process is stopped at the end. Its peak process working set is not a GPU-memory measurement.

`GPUI_NIGHTSHADE_TRACE_GPU=1` enables periodic worker heartbeat and published/submitted/pending counters. Those counters measure publication to GPUI, not successful swapchain presents or presentation FPS. The headless example accepts `VIEWER_TEST_BACKEND=dx12|vulkan`, `VIEWER_TEST_FRAMES`, `VIEWER_TEST_WIDTH` and `VIEWER_TEST_HEIGHT`.

## Remaining validation

- Sustained resize/docking/minimize/restore and multiple shared viewports with the final handoff.
- Independent graphics capture, broader adapters/drivers, and longer stress runs for the imported-resource retirement correction; publication counters alone do not prove visible display.
- Release-mode timing, actual presentation/GPU timings and allocation/VRAM measurements.
- Additional adapters/drivers and real device-loss recovery.
- Post-reset visual automation: desktop capture began returning `FrameArrived timed out` during this investigation, so later runs rely on native import pixel tests, publication counters and MCP scene state. This limits visual/lifecycle coverage.

## Consumer sampling isolation (2026-11-25)

Diagnostic `-Probe` modes on `scripts/verify-windows-shared.ps1` isolate import, signaling and sampling. They are not presentation acceptance modes:

| Probe / experiment | Observation |
| --- | --- |
| `no-draw`: import only, no scene shader or completion signal | 600 publications passed |
| `signal-only`: import and completion signal, no scene shader | 600 publications passed |
| `frozen`: repeatedly sample the initial published frame | 600 consumer draw submissions passed; only two frames published |
| `fresh`: retire each allocation after publication | Visible scene still hung; allocation reuse alone does not explain it |
| `aligned`: round both dimensions to multiples of 64 | Visible scene still hung |
| Explicit shader-resource unbinding, GPU waits in both directions, or a global submission gate | Each still reproduced the visible-scene hang |

Unsuccessful GPU-wait and global-gate implementations were removed. The frozen probe now services simulation without further rendering after publication, avoiding an unbounded producer queue in that diagnostic mode; the earlier frozen result predates that adjustment. Aligned output changes scaling and has no validated input mapping.

`windows::sampling_test::changing_shared_frames_sample_without_gpui_compositor` adds a separate native D3D11 fullscreen shader test. It uses the actual shared importer, rotates the cube camera, draws into a private 1351×715 render target, and waits for each consumer completion before reusing the allocation. A 600-frame DX12/D3D11 run passed and verified 42 distinct output colors. This is serialized shader-sampling validation, with no GPUI atlas, window or swapchain; it does not prove concurrent full-viewer presentation. Set `SHARED_SAMPLING_FRAMES` to change its default 120-frame duration.

The compositor now obtains the shader view and matching synchronization owner under one atlas lock and unbinds shared SRVs before signaling completion. Fresh visible runs still failed with those changes. The next isolation boundary is pipelined native sampling versus GPUI atlas/swapchain presentation; at that stage the production blocker remained open and readback remained the default.

## Pipelined sampling isolation (2026-11-25)

`just test-sampling` now runs serialized cube, three-slot cube, and three-slot full-viewer sampling tests. `SHARED_SAMPLING_FRAMES` controls their duration (120 by default). The full-viewer test disables agents, settings persistence and remote catalogs and waits for the default model to load through the real viewer schedules. Its viewer-core dependency is test-only.

The pipelined tests retain imported frames, textures, shader views and synchronization owners until the oldest consumer fence retires. A test-only fence gate holds the first three draws pending simultaneously, asserts that all three slots are unavailable with no consumer completion, and then releases them. The gate releases during unwinding as well. Subsequent production work is submitted while earlier consumer reads may still execute. All three allocations must be exercised. GPU recipes now run serially to avoid independent GPU regressions contending with this workload.

Observed on the same RTX 4080/debug configuration at 1351×715:

| Native sampling configuration | Result |
| --- | --- |
| Three-slot moving-camera cube, original point-sampling shader | 1,200 frames passed |
| Three-slot full viewer, original point-sampling shader | 1,200 frames passed; 15,599 output colors |
| Serialized moving-camera cube, vertex-stage dimensions + linear pixel sampling | 600 frames passed; 42 output colors |
| Gated three-slot moving-camera cube, vertex-stage dimensions + linear pixel sampling | 600 frames passed; 42 output colors |
| Gated three-slot full viewer, vertex-stage dimensions + linear pixel sampling | 600 frames passed; 15,599 output colors |

The latter shader binds the shared SRV to both vertex and pixel stages, queries its dimensions in the vertex shader, and uses the same linear/wrap sampler choices as GPUI. It is still a small fullscreen shader, not GPUI's actual polychrome sprite shader. These tests exercise overlapping GPU work and the full viewer's producer workload without a GPUI atlas, swapchain, or worker/UI thread split. They narrow the next investigation to those remaining differences; they neither identify a root cause nor validate visible presentation. The production path and default presentation choice are unchanged.

## Actual GPUI sprite shader and unattached swapchain (2026-11-25)

Two additional `just test-sampling` cases compile the checked-in GPUI `polychrome_sprite_vertex` and `polychrome_sprite_fragment` entries directly, inlining the checked-in alpha-correction include. They use a structured sprite buffer with a nonzero batch start; only instance 1 has nonempty bounds. Rust size/offset assertions verify the test buffer against the exported `PolychromeSprite` layout. Vertex/pixel constants, both texture-stage bindings, and the linear/wrap sampler are populated explicitly. This covers the shader's simple-sprite branch, not every mask, grayscale or rounded-corner variant.

- Actual GPUI sprite shader, gated three-slot full viewer, private render target: **600 samples passed**, 15,599 output colors at 1351×715.
- Same shader and scene with an **unattached** three-buffer BGRA composition swapchain, flip-sequential and premultiplied alpha: **600 samples passed**, including 597 successful `Present(0)` calls and 15,599 final output colors. The first two draws remain behind the initial consumer gate; the final draw is read before flipping, making pixel validation independent of flip rotation. The test counts and verifies the expected `frames - 3` presents and rebinds the render target after each flip.

This swapchain has no HWND or DirectComposition visual. Successful calls do **not** demonstrate DWM display, visible presents or presentation FPS. The test still does not use GPUI's atlas, live mixed UI batches, normal blend/depth setup, worker/UI thread split, or a window. At that stage the visible-editor D3D11 hang remained unresolved; those passing isolation tests are not substitutes for `just test-shared` acceptance. No production transport behavior or default presentation mode changed in this milestone.
