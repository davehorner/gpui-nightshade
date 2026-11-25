# Upstream patches

## Nightshade

`patches/nightshade/0001-external-output.patch` applies to the pinned trunk submodule. `scripts/init.ps1` verifies the exact revision and applies the patch idempotently; it refuses conflicts rather than overwriting local changes.

Changes are isolated to nightshade-wgpu: optional-surface initialization retaining the native constructor, surface-free creation with explicit backend selection, validated owned/external output attachment, external-target availability in frame dispatch, clearing external output on native surface recreation, and a public re-export of existing depth-pick readback.

The original viewer is not refactored yet, and the original trunk viewer passes a locked cargo check with this patch. Existing applications are not claimed runtime regression-tested. The native constructor still queries native surface capabilities and configures/presents normally. Output rendering reuses the existing graph instead of replacing postprocessing.

## GPUI

`patches/gpui/0001-shared-external-images.patch` is a reviewable diff against the pristine `gpui-pre` and `gpui-pre-windows` 0.3.14 source exports. Paths are relative to `upstream/gpui-snapshot`; the source changes are already applied there. Export provenance is Zed `f126950f9d3b62666f97437ae5d4ccac2272c4db`, as declared by those published packages. Root Cargo patches select the local exports. Their normalized manifests are kept so the exact matching registry family remains usable without cloning the entire Zed dependency workspace.

Generic changes: owned `ExternalTexture` images, external atlas import with explicit unsupported errors, and a cache insertion helper. The Windows atlas allocates a whole imported texture as a polychrome tile. Existing sprite shaders/order/masks handle it. Windows-only code owns NT handles, checks LUIDs and descriptors, and enqueues producer/consumer fence operations around sprite drawing. Last-frame eviction drops the imported resource lease.

## Unsafe review notes

Interop is confined to the Windows producer module and GPUI's `shared_texture.rs` importer. Native D3D allocations use BGRA8, one sample/layer/mip, shared heaps and simultaneous access. wgpu import occurs on the allocating device. The imported descriptor matches the native resource; native COM and NT handle ownership survive asynchronous commands. A normal wgpu copy is followed by a tracked transition to COMMON before signaling the shared fence.

The producer checks both lease expiration and fence completion before writing again. D3D11 waits on the producer value before drawing and signals consumption after the draw. The CPU never waits for those fences. Dropped unsampled frames cannot deadlock waiting for a nonexistent consumer signal. GPUI holds the frame owner with the imported atlas resource, so late cached draws cannot outlive the resource owner.

This is a source/lifetime self-review and runtime validation on one adapter. Independent review, debug-layer validation, device-loss handling and broad-driver testing remain outstanding. The GetCompletedValue device-removal sentinel stops shared presentation with an error; native device recreation is not implemented.

