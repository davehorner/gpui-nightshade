# Presentation feasibility and implementation

This report is based on the pinned source checkouts and the exported compatible GPUI snapshot. Runtime observations are restricted to the Windows RTX 4080 configuration in the README.

## Nightshade

Inspected `crates/nightshade/src/app.rs`, `nightshade-wgpu/src/renderer/wgpu.rs`, `initialization.rs`, `frame.rs`, `presentation.rs`, `render_configs.rs`, `camera_viewport.rs`, `paint_target.rs`, `rendergraph`, and trunk `apps/viewer`.

`WgpuRenderer.surface` was already optional, but `new_with_device` required a surface and queried its capabilities. The patch shares initialization through an optional-surface helper, preserves the existing constructor, adds surface-free creation, and retains an owned/external output texture. External output validation covers format, dimension, layers, mip count, sample count and render usage; allocation also checks device dimension limits. Cross-device textures are rejected by wgpu validation; callers must allocate on the renderer's device.

`render_frame` previously returned early without a surface when the camera list was empty. An attached external output now counts as an available presentation destination, allowing the normal single-view graph to run. The output is bound to the existing swapchain graph resource, with no actual swapchain or native present call.

Camera viewport textures can already be supplied through `set_external_camera_viewport`. Existing owned camera textures lack COPY_SRC usage. They preserve per-camera postprocess/depth state, and are a promising direct presentation source without an additional scene render. The current implementation exports the final composed output so existing postprocess and outline ordering is retained. It does not claim to eliminate the viewport compose pass. PaintTarget redirects paint, not renderer initialization, and is not a surface-free constructor by itself.

## GPUI

`Window::paint_image` normally inserts CPU bytes into a platform atlas; `RenderImage` bytes are BGRA. Its image bounds/content mask, `PolychromeSprite`, opacity, scene batches and atlas lifetime are already the right compositing machinery. The added `ExternalTexture` contract carries an immutable owned GPU frame, and `RenderImage::from_external` selects platform import rather than byte upload. Other atlas backends return an explicit unsupported error. No native handle type is in the generic trait.

This reuses the sprite primitive rather than inventing a parallel native visual. GPUI still owns its one window swapchain. Overlays are ordinary GPUI elements above the viewport; headless Nightshade never presents that HWND.

## Portable backend

The ring follows Nightshade screenshot readback's copy/map/poll structure, without its file writer or image encoder. It has 2–4 reusable MAP_READ/COPY_DST buffers, a 256-byte row stride, callback readiness atomics and nonblocking polling on the render worker. The newest ready sequence wins; older completed frames are unmapped and discarded. A monotonic last-delivered sequence prevents late older completions from replacing a newer frame. Resizing drains pending work and changes generation.

RGBA/BGRA ordering is normalized for the CPU frame, then converted to GPUI's BGRA off the UI thread. The opaque viewport forces alpha to 255; transparent scene output is not verified. Non-sRGB textures contain Nightshade's default gamma-2.2 display encoding and carry Gamma22 metadata; sRGB texture sources carry Srgb metadata. There is no extra sRGB decode/encode on presentation. Exact sRGB transfer-function matching, arbitrary color-grading gamma and HDR displays need additional color-management work.

## Windows shared GPU backend

GPUI uses D3D11 (`gpui_windows/src/directx_devices.rs` and `directx_renderer.rs`). Nightshade's wgpu uses D3D12 in shared mode. D3D12/11 compatibility is therefore explicit, not inferred from the word DirectX.

The producer allocates three BGRA8_UNORM shared committed D3D12 resources with ALLOW_SIMULTANEOUS_ACCESS and ALLOW_RENDER_TARGET, exports owned NT handles and imports those resources into the same wgpu device using `wgpu-hal::dx12::Device::texture_from_raw` and `Device::create_texture_from_hal`. Nightshade renders normally and wgpu copies its output into a free shared texture. An explicit transition to PRESENT maps to D3D12 COMMON in this pinned HAL. The native queue used to signal the producer fence is the same queue created by HAL `Adapter::open` and retained as `Device::present_queue`.

The GPUI importer compares adapter LUIDs, opens the shared texture and two fences, validates BGRA dimensions/sample count and creates a shader-resource view. The worker retains at most three pending frames and publishes only completed producer copies; GPUI checks completion again before sampling rather than enqueuing a foreign-queue GPU wait. D3D11 signals its separate monotonically increasing consumer timeline after every draw, including repeated cached draws, and flushes. Each fence has one signaling queue. The producer checks lease expiration and both completions before reuse. Frames retain producer objects, and up to two old rings are retained until consumer work and leases retire before destruction. Further resizing is backpressured at that cap.

This is **GPU-resident presentation with a GPU-to-GPU copy into shared resources**. It is not zero-copy rendering directly into GPUI's final backbuffer. Atlas eviction now retains imported D3D11 textures, shader views and producer leases through consumer completion. Fresh full-scene forced-shared runs and resize/minimize stress pass on the RTX 4080; the editor retains readback as its default pending broader coverage. [GPU validation](windows-gpu-validation.md) records passing checks and historical failures. A consumer-local extra copy was tested and removed because it did not fix earlier visible-window failures.

The API contracts are documented by Microsoft in [CreateSharedHandle](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12device-createsharedhandle), [OpenSharedResource1](https://learn.microsoft.com/en-us/windows/win32/api/d3d11_1/nf-d3d11_1-id3d11device1-opensharedresource1), and [resource flags](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/ne-d3d12-d3d12_resource_flags).

DirectComposition is already owned by GPUI's renderer. A separate visual would require interleaving engine visuals with GPUI scene batches, popovers and content masks; it is unnecessary for the implemented sprite import. Child HWND presentation was not implemented: it would introduce native airspace/focus/docking limitations that a composited sprite avoids.

## macOS

The current Apple renderer lives in `gpui_apple/src/metal_renderer.rs`, shared by macOS/iOS. Its paint-surface path accepts only `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange` (NV12 full-range), creates R8Unorm/RG8Unorm Metal plane textures through CVMetalTextureCache and retains wrappers through command completion. It does not directly accept an arbitrary BGRA CVPixelBuffer.

An IOSurface-backed BGRA pool plus wgpu Metal import could avoid CPU copies, but requires adding BGRA surface handling (or a Metal external-atlas importer), matching MTLDevice identity and explicit command-buffer/shared-event completion before pool reuse. Metal texture/HAL ownership must be verified on Apple hardware. No macOS GPU backend is implemented or cross-compiled; readback is the intended fallback, not a tested macOS claim.

## Linux

Inspected `gpui_linux/Cargo.toml`, `gpui_wgpu` and Zed's workspace dependency: current Linux GPUI uses its wgpu renderer with Vulkan/GLES support, not an assumed old Blade renderer. Nightshade and GPUI are separate wgpu versions/devices. A portable `wgpu::Texture` cannot cross that boundary.

Vulkan external memory requires matching physical devices and negotiated external image format/usage capabilities. DMA-BUF requires compatible format modifiers, plane metadata, owned FDs and external semaphore lifetime. Wayland and X11 change display/adapter selection and do not guarantee those extensions. None is implemented or verified here. The generic importer returns unsupported outside Windows; ordinary readback remains independent of native sharing.
