# Vulkan GPU sharing on Windows

The viewer and reusable viewport default to Vulkan rendering and prefer shared GPU presentation on supported Windows adapters. `--presentation=vulkan-shared` requires this path without readback fallback. `--presentation=readback` retains the portable reference transport. `windows-shared` and `windows-shared-auto` explicitly select the older DX12 producer.

## Pixel and synchronization path

1. Match Vulkan's physical-device LUID to a DXGI adapter. Reject non-primary linked-adapter nodes, then check `VK_KHR_external_memory_win32`, importable BGRA format properties, dimensions and device-local memory types.
2. Allocate a three-slot shared BGRA ring on that adapter. D3D12 is used for shared allocation/fence creation, not scene rendering. Import each allocation as a dedicated Vulkan image using `D3D12_RESOURCE` Win32 external memory.
3. Put the source texture into `COPY_SRC` using wgpu's tracked transition API. A separate wgpu-owned HAL command buffer acquires external image ownership, copies the source into the shared image, and releases ownership to `QUEUE_FAMILY_EXTERNAL` in `GENERAL` layout. Submit both buffers together through wgpu. There are no independent raw queue submissions or manually managed Vulkan command pools.
4. A wgpu completion callback retains the source and imported allocation through actual GPU completion. Normal worker polling drives it; publication relays completed readiness through an existing shared D3D fence. This is CPU synchronization bookkeeping, not a CPU pixel copy. No pending foreign GPU wait is placed on GPUI's queue.
5. GPUI opens the completed resource through the existing D3D11 importer, checks adapter/descriptor compatibility and samples it through the ordinary sprite compositor. Its existing consumer-only fence and frame leases gate slot reuse and retirement.

There is one GPU image copy, no CPU pixel readback/upload, no image encoding, no filesystem frame intermediary, and no additional scene render. Resize generations retain old rings through producer completion, consumer completion and lease expiry. Ring allocations are bounded; two retired generations are retained at most. Pending producer callbacks are drained on worker-owned teardown, not during UI painting. Native memory/device owners stay alive through imported frame leases.

The native interop code is isolated in `crates/gpui-nightshade-components/src/windows/vulkan.rs`. Existing GPUI import/retirement patches are reused unchanged. Unsupported imports in `Auto` fall back to Vulkan readback and preserve the reason; forced `VulkanShared` reports failure. Device recreation is not implemented.

## Validation

The native GPU regression compares every copied pixel against the Vulkan source using a separate D3D11 consumer over six changing camera views, including reused allocations. It also checks live-frame lease backpressure and consumer-fence gating. Worker regressions verify actual Vulkan sharing in Auto mode and real pixel readback after a simulated consumer-import failure for both Vulkan and DX12.

```powershell
cargo test -p gpui-nightshade-components --lib --locked -- --include-ignored --test-threads=1
cargo build -p gpui-nightshade-viewer --locked
pwsh -NoProfile -File scripts/verify-windows-shared.ps1 -Presentation vulkan-shared -CameraViews 4 -Frames 600 -Stress
```

The compositor acceptance run forces Vulkan sharing so failures cannot pass through CPU fallback. It exercises a visible editor, four cameras, Fox model replacement/animation, resizing, dock resets and minimize/restore. Results are recorded in `target/shared-validation`.

Validation currently covers an NVIDIA RTX 4080 on Windows 11. Other vendors, multiple adapters and device-loss recovery remain unverified. The earlier offscreen backend benchmark excludes this transport; no new end-to-end presentation performance claim is made.

## API references

- [Vulkan external image layouts](https://docs.vulkan.org/spec/latest/chapters/resources.html#resources-image-layouts).
- [Vulkan external queue ownership](https://docs.vulkan.org/spec/latest/chapters/synchronization.html#synchronization-queue-transfers).
- [Win32 external-memory import](https://docs.vulkan.org/refpages/latest/refpages/source/VkImportMemoryWin32HandleInfoKHR.html).
- [Microsoft shared-resource creation](https://learn.microsoft.com/en-us/windows/win32/direct3d12/shared-heaps).

The final 2026-11-25 four-camera acceptance run published 600 GPU frames and recorded 900 compositor Present submissions through 28 resize generations and four suspension/resumption cycles, with animated Fox replacement. See the [retained report](validation/2026-11-25-vulkan-sharing.json).
