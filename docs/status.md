# Implementation status and remaining work

The default executable now runs the trunk viewer core inside the GPUI-owned window. `--cube` retains the presentation demonstrator; `--model=PATH` loads into the actual viewer world.

Implemented this milestone:

- `ViewerCorePlugin` and `NativeViewerUiPlugin`, with the original `ViewerPlugin` composing both. Core startup and update schedules run without a retained UI runtime.
- Embedded composition with caller-owned arguments and no native Nightshade event loop or window.
- Bounded action submission and owned hierarchy, selection, material, animation, render settings and history snapshots. Existing viewer actions and native agent plugin are reused.
- A worker-owned full viewer frame lifecycle, loading/extraction/render hooks, continuous simulation and resize handling.
- GPUI hierarchy selection, transform/name fields, metallic/roughness, visibility/delete, primitive creation, undo/redo, exposure/grid/bloom and basic animation controls. Inspector fields apply on Enter.
- Material color/emissive/transmission/IOR and texture replacement/removal controls; light creation/properties, camera clipping/activation, playback speed/looping and history-aware Undo/Redo. See [viewer controls](viewer.md).
- Asset browser with local/Khronos/Polyhaven tabs, search, pagination, resolution controls and bounded native remote thumbnails. Live Khronos thumbnail display and card-to-scene Avocado loading verified. Local previews and broader download/dialog coverage remain unfinished.
- Deferred spawn registration fix: selection/rows/material/light state are read after queued ECS components are installed.
- Original native frontend startup, core edits/undo/snapshot ownership, light/camera history, native HTTP MCP round-trip and bounded frontend queue tests. Live MCP rename/selection was also observed in the rendering GPUI editor; a GPUI spotlight intensity edit applied with Enter was confirmed through MCP.

Validation: workspace check, tests and strict Clippy pass. A surface-free bundled DamagedHelmet load rendered 1,303 distinct colors in the initial headless check. The GPUI editor displayed its hierarchy and model, but the shared presentation run reported D3D device removal/HRESULT 0x887A0006. A fresh isolated Vulkan run also passed with 1,287 distinct colors, and a 120-frame run completed with 8,917 colors. Native GPUI validation confirmed textured model display, hierarchy selection, inspector population and primitive creation. Shared full-scene stability and a specific root cause remain unverified. The editor defaults to readback while this is investigated.

The previous cube milestones remain: surface-free rendering, asynchronous readback, Windows D3D12/D3D11 shared textures and fences, DPI coordinates, picking/outline, camera orbit/pan/zoom, GPUI overlay and independent multi-viewport display. Broader drivers, long stress and device recreation remain unverified.

Remaining viewer work includes docking, complete material/texture/light/camera/morph/skeleton inspectors, local asset previews and broader catalog/dialog validation, atmosphere, export/screenshots, conversion UI, drag/drop, gizmo pointer routing, capture outside the viewport, full shortcut coverage and feature parity checks. The core still compiles upstream UI dependencies although it does not instantiate their runtime. Non-Windows GPU sharing, visibility suspension, shared-world multi-camera views, HDR/color validation and detailed GPU timings remain unfinished.
