# Architecture

GPUI owns application/window lifetime, layout, input dispatch and presentation. Each viewport has a worker that owns a Nightshade `WgpuRenderer`, `SingleViewHost`, GPU ring and scene. GPU objects are constructed on that worker; `RenderHooks` are not assumed Send. The host composes render inputs with the real `compose_single_view` API, executes `render_frame`, then calls `restore_single_view`, including on returned render errors.

The cube host registers the actual Shading, Geometry and Outline plugins. Shader publication precedes graph configuration; presentation passes are installed after geometry. The first render materializes the graph before layout-driven resizing. This avoids a reproduced blank frame when a fresh graph is resized before its first execution. This is an explicit initialization render, not a hidden window. The host reserves entity zero because the upstream outline shader treats zero as background.

`App::into_schedules` is not a complete application runner: it checks plugin requirements, orders stages and returns world/startup/stages, dropping other App fields. Nightshade's `commands::into_parts` extracts render hooks before converting the app. The current integration uses that path for shader/graph hooks, and `SingleViewHost` for rendering. It does **not** yet run the full trunk viewer's simulation/plugin schedule. In particular, cube rendering is not evidence that glTF, animation, gizmos or the MCP dispatcher have been ported.

## Frame and resource ownership

The control mailbox holds only the newest camera/size request. CPU presentation holds at most three staging buffers and one unpublished completed image. GPUI holds the current image and frame scene references. Replacing an image calls `Window::drop_image`; entity release calls `App::drop_image`. External images report one frame so atlas eviction reaches their GPU entries too.

A resize advances a generation. CPU callbacks cannot publish an obsolete generation. A ring is replaced only after its producer work is ready; readback drains mapping callbacks before replacement. Shared images can outlive their ring through owned NT handles and GPUI's imported COM resources. Pixel storage is capped at 256 MiB per presentation ring; Nightshade scene/depth/material/pipeline caches are additional memory, not included in that cap.

The GPU slot lease lasts through the generic image, atlas entry and scene references. Reuse requires all leases to expire plus the consumer fence to complete. Unsampled dropped frames require producer completion only. The worker does not submit more render work when no presentation slot is available. No GPUI UI callback waits for a fence or maps GPU memory.

Workers sleep on a condition variable when idle. UI polling checks for updates at 16 ms intervals but only invalidates when a frame or diagnostic changed. Rendering is on demand; continuous simulation scheduling remains future work. Application quit signals workers and joins them through GPUI's background executor, subject to GPUI's shutdown timeout.

## Input

Canvas prepaint reports logical bounds and scale. Physical dimensions use rounded logical size times scale. Pointer picking subtracts the viewport origin before scaling. Camera orbit/pan/zoom configuration is coalesced. A pick dispatch uses Nightshade's depth/entity-ID compute readback, then selects an entity and enables the existing outline pass. Keyboard focus is assigned on pointer down. Shift-drag currently pans in world X/Y; view-plane pan, pointer capture beyond element boundaries, shortcuts and drag-and-drop still require work.

Multiple viewports currently use independent worlds/devices. A shared ECS with multiple camera producers is a separate lifecycle integration and is not claimed here.
