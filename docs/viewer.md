# GPUI viewer controls

Run `just viewer` for the bundled model or `just gltf C:/models/scene.glb`. The full editor currently defaults to asynchronous readback. `--presentation=windows-shared` remains experimental for full scenes because a D3D device hang was observed.

The hierarchy supports selection and Ctrl-click multi-selection. Inspector text fields apply on Enter. Changing selection releases focused entity fields so a pending value cannot apply to a different entity. Editor shortcuts yield to focused text fields.

The inspector exposes transforms/name, visibility/delete, base color RGBA, metallic/roughness, emissive RGB/strength, transmission, IOR, unlit/double-sided flags, and all eight texture slots with replacement and removal. Texture replacement uses the original native file dialog and asynchronous texture loader. These controls edit the selected material; meshes sharing that material change together, as in trunk.

Point/spot lights can be created from the toolbar. Light inspectors expose RGB, intensity, range, cone angles in degrees and shadows. Light-marker materials are hidden so the actual light controls remain accessible. Selected cameras expose perspective FOV, near/far clipping and camera activation; perspective far clip zero means infinite. A button restores the editor camera.

Playback controls include clip selection, play/pause, stop, seek, positive speed and looping. Advanced animation layers, morph/skeleton controls and blend settings remain to be built. Undo/Redo buttons reflect trunk history availability; creation is not recorded by trunk's current history implementation. Core regression tests verify transform, light and camera edit undo.

## Native MCP

The original agent plugin runs on the same worker-owned world used by the editor. Pass `--agent-port=PORT` to select its localhost port or `--no-agent` to disable it. The endpoint accepts HTTP POST JSON-RPC (`initialize`, `tools/list`, `tools/call`) and retains trunk's tool names and schemas. No browser or second scene is involved.

`just test-core` runs the UI-free core, native frontend startup, light/camera history and localhost MCP regression tests. The MCP test creates a primitive through frontend actions, reads it over HTTP, selects/renames/transforms it, and checks the resulting owned snapshot. It needs permission to open/connect localhost sockets. A separate live GPUI smoke check verified that MCP selection/rename updated the hierarchy and inspector while the textured model was rendering. A keyboard edit of spotlight intensity in GPUI, applied with Enter, was read back as the new value through MCP; undo was then dispatched through that same endpoint.

## Remaining work

Docking, asset-browser/search/thumbnails, complete PBR controls, morph/skeleton and animation layers, atmosphere, export/screenshot UI, conversion UI, drag/drop and gizmo pointer routing remain unfinished. Camera clipping and light editing have core regression coverage; native dialog texture replacement and the expanded camera UI have not yet received end-to-end automation coverage. Full-scene shared presentation stability is unresolved.
