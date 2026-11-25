# Upstream patches

## Nightshade

`patches/nightshade/0001-external-output.patch` applies to the pinned trunk submodule. `scripts/init.ps1` verifies the exact revision and applies the patch idempotently; it refuses conflicts rather than overwriting local changes.

Changes are isolated to nightshade-wgpu: optional-surface initialization retaining the native constructor, surface-free creation with explicit backend selection, validated owned/external output attachment, external-target availability in frame dispatch, clearing external output on native surface recreation, and a public re-export of existing depth-pick readback.

`0002-embedded-viewer-core.patch` separates the viewer schedules from native UI, introduces owned snapshots/action submission and embedded application composition, and retains `ViewerPlugin` as the original frontend composition. Tests exercise native startup and UI-free edits/undo. The original trunk viewer passes a cargo check with the rendering patch. `0003-deferred-spawn-selection.patch` fixes inspector registration before queued spawn components exist, so new lights expose their properties and correct names immediately. Core tests cover light/camera edits and history as well as native HTTP MCP servicing. Existing applications are not claimed runtime regression-tested. The native constructor still queries native surface capabilities and configures/presents normally. Output rendering reuses the existing graph instead of replacing postprocessing.

`0004-mcp-alignment.patch` aligns the existing native MCP tools with inspector transform/material/light history, preserves selection for targeted edits, reports tool failures accurately, validates arguments and bounds, expires unstarted calls, and adds animation/morph controls. Protocol, schema, queue and native HTTP regressions cover errors and frontend-world undo/redo. See [MCP contract](mcp.md). The existing legacy HTTP endpoint and protocol version are preserved.

`0005-mcp-transform-batches.patch` adds bounded, prevalidated transform batches through the shared inspector mutation helper. A batch records one grouped history edit and returns all post-edit transforms. HTTP regressions cover invalid/duplicate targets, numeric overflow, no partial writes and grouped undo/redo. Procedural clients can use this tool for synchronized updates; the [cinematic example](../examples/mcp-cinematic/README.md) now uses native animation clips for its object/camera tracks and MCP morph controls.

## GPUI

`patches/gpui/0001-shared-external-images.patch` is a reviewable diff against the pristine `gpui-pre` and `gpui-pre-windows` 0.3.14 source exports. Paths are relative to `upstream/gpui-snapshot`; the source changes are already applied there. Export provenance is Zed `f126950f9d3b62666f97437ae5d4ccac2272c4db`, as declared by those published packages. Root Cargo patches select the local exports. Their normalized manifests are kept so the exact matching registry family remains usable without cloning the entire Zed dependency workspace.

Generic changes: owned `ExternalTexture` images, external atlas import with explicit unsupported errors, and a cache insertion helper. The Windows atlas allocates a whole imported texture as a polychrome tile. Existing sprite shaders/order/masks handle it. Windows-only code owns NT handles, checks LUIDs and descriptors, and enqueues producer/consumer fence operations around sprite drawing. Last-frame eviction retires imported native objects and the producer lease until consumer completion.

## Unsafe review notes

Interop is confined to the Windows producer module and GPUI's `shared_texture.rs` importer. Native D3D allocations use BGRA8, one sample/layer/mip, shared heaps and simultaneous access. wgpu import occurs on the allocating device. The imported descriptor matches the native resource; native COM and NT handle ownership survive asynchronous commands. A normal wgpu copy is followed by a tracked transition to COMMON before signaling the shared fence.

`0002-shared-frame-synchronization.patch` follows the initial GPUI patch. It splits producer/consumer fence timelines, retains producer objects in exported frames, validates completed publication before drawing, and adds bounded D3D11 failure diagnostics. Matching producer changes retain at most two retired rings until consumer completion and lease expiration. D3D11 signals a fresh consumer value for each draw; no foreign-queue GPU wait is enqueued. CPU fence polling provides backpressure without blocking the UI thread. A native import test reads actual rendered pixels and checks pending publication, reuse and retirement. These corrections do not resolve the full-scene D3D11 hang; see [GPU validation](windows-gpu-validation.md).

This is a source/lifetime self-review and runtime validation on one adapter. Independent review, debug-layer validation, device-loss handling and broad-driver testing remain outstanding. The GetCompletedValue device-removal sentinel stops shared presentation with an error; native device recreation is not implemented.


`0003-consumer-sampling-isolation.patch` pins each sampled view with its matching synchronization owner in one atlas lookup, unbinds shared shader resources before signaling, and exposes draw hooks for native sampling validation. Opt-in interop probes isolate import/signaling/frozen sampling from live scene drawing; these modes are diagnostic only. A separate serialized fullscreen D3D11 sampling test passes 600 changing-camera cube frames, while full-viewer shared presentation remains failing.

`0004-retain-evicted-shared-imports.patch` keeps evicted external atlas entries out of drawable lookup while retaining native D3D11 objects and their producer lease until consumer fence completion. Collection runs before rendering/import; producer leases provide backpressure. A shared-fence-gated WARP regression verifies eviction/retention/release. Opt-in diagnostics count successful Present submissions. The matching application stress driver exercises native resize, dock reset and minimize/restore; fresh full-viewer checks now pass on the tested RTX 4080. See [current GPU validation](windows-gpu-validation.md).

`0005-remove-investigation-probes.patch` removes the temporary runtime probe modes and public sampling hooks while retaining normal sprite binding, unbinding, retirement and failure diagnostics. The standalone six-stage sampling harness and test-only worker frame tap were also removed from the integration crate. Maintained native lifetime regressions and visible-editor acceptance/stress checks replace those investigation-only controls.

`0006-poll-external-image-retirement.patch` adds a default nonblocking atlas retirement hook and `Window::poll_external_images()`. The Windows atlas releases only GPU-complete imports through it; a window-bound viewport polling task drives the hook even without a draw. This breaks the frame-pool/redraw scheduling cycle without an extra pixel copy or UI-thread GPU wait. The native eviction regression verifies release through the public hook without a subsequent render/import.
