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
