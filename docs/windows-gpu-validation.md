# Windows shared-GPU validation

Test machine: Windows, NVIDIA GeForce RTX 4080, pinned Rust 1.100.0 and debug builds. This is correctness investigation, not a release performance benchmark.

## Reproduction and isolation

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

The long manual run was not sufficient evidence of a reliable fix. The unsuccessful extra-copy experiment was removed. Current changes harden the single-copy transport and provide an automated failure detector; **the full-scene shared-presentation blocker remains open**. The editor still defaults to readback.

A separate headless test explicitly selected DX12, rendered the full viewer for 120 additional frames, and read 8,833 distinct colors at 640×480 without a native Nightshade window. At the actual unaligned viewport size, 1351×715, another 120-frame run passed with 15,599 colors. The example prints its actual adapter/backend; environment backend selection alone had selected Vulkan in an earlier run, so that earlier result was not counted as DX12 validation.

## Current handoff

1. Nightshade renders its final output on DX12.
2. A GPU copy writes a free shared BGRA texture; the producer signals its own fence.
3. The bounded pending queue publishes only completed copies, dropping older completed candidates.
4. GPUI checks completion again through the imported fence before sampling. No foreign-queue GPU wait is enqueued.
5. D3D11 signals its separate consumer timeline after every sprite draw, including repeated cached draws. Normal scene ordering and clipping remain in use.
6. Reuse requires expired CPU leases and both timelines complete. Frames retain producer textures/fences; up to two retired rings are retained until their queued consumer work and leases retire. Further resizing applies backpressure at that cap. The removal sentinel never counts as successful completion.

There is **one GPU-to-GPU copy** into shared resources and no production CPU pixel transfer. This is not zero-copy. Tests deliberately read pixels back for validation; that readback is not part of presentation. The three-slot live ring and at most two retired rings bound transport generations, with a 256 MiB allocation limit per ring. Actual peak allocation/VRAM behavior across resizing still needs measurement.

## Repeatable checks

`just test-gpu` includes native D3D12/D3D11 import validation. The regression gates the producer queue to verify unfinished copies are withheld, reads rendered cube pixels through the actual importer, verifies reuse waits for the latest consumer read, verifies the timelines do not rewind one another, and retains a frame after ring retirement.

`just test-shared 1200 19790` builds and launches the real viewer in forced shared mode with an isolated layout path, switches from the default helmet to the bundled Fox, checks animation advancement, and fails on logged device errors or insufficient published frames. **It is a reproducer, not a currently passing acceptance claim.** It opens a visible viewer window because hiding it correctly suspends presentation. Reports and stderr are written beneath `target/shared-validation/`. The child process is stopped at the end. Its peak process working set is not a GPU-memory measurement.

`GPUI_NIGHTSHADE_TRACE_GPU=1` enables periodic worker heartbeat and published/submitted/pending counters. Those counters measure publication to GPUI, not successful swapchain presents or presentation FPS. The headless example accepts `VIEWER_TEST_BACKEND=dx12|vulkan`, `VIEWER_TEST_FRAMES`, `VIEWER_TEST_WIDTH` and `VIEWER_TEST_HEIGHT`.

## Remaining validation

- Sustained resize/docking/minimize/restore and multiple shared viewports with the final handoff.
- Resolve the visible-window D3D11 hang. A native debugger/graphics capture of the consumer queue is the next diagnostic step; do not infer success from worker publication counters alone.
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

The compositor now obtains the shader view and matching synchronization owner under one atlas lock and unbinds shared SRVs before signaling completion. Fresh visible runs still failed with those changes. The next isolation boundary is pipelined native sampling versus GPUI atlas/swapchain presentation; the production blocker remains open and readback remains the default.

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
