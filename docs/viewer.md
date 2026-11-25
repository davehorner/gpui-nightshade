# GPUI viewer controls

Run `just viewer` for the bundled model or `just gltf C:/models/scene.glb`. The full editor currently defaults to asynchronous readback. `--presentation=windows-shared` remains experimental for full scenes because a D3D device hang was observed.

The hierarchy supports selection and Ctrl-click multi-selection. Inspector text fields apply on Enter. Changing selection releases focused entity fields so a pending value cannot apply to a different entity. Editor shortcuts yield to focused text fields.

The inspector exposes transforms/name, visibility/delete, base color RGBA, metallic/roughness, emissive RGB/strength, transmission, IOR, unlit/double-sided flags, and all eight texture slots with replacement and removal. Texture replacement uses the original native file dialog and asynchronous texture loader. These controls edit the selected material; meshes sharing that material change together, as in trunk.

Point/spot lights can be created from the toolbar. Light inspectors expose RGB, intensity, range, cone angles in degrees and shadows. Light-marker materials are hidden so the actual light controls remain accessible. Selected cameras expose perspective FOV, near/far clipping and camera activation; perspective far clip zero means infinite. A button restores the editor camera.

Playback controls include clip selection, play/pause, stop, seek, positive speed, looping and blend duration. Each clip can be added as a layer; layer weights apply on Enter and layers can be removed. Duplicate layer additions are disabled. Dynamic fields are bound to the current scene root, clip and entity generation, and stale fields are released when the model or layer changes.

Select a mesh with morph targets to edit its indexed weights (0–1) in the inspector. Snapshots read current ECS weights, including changes made by animation. Pause playback before manual edits; active animation can overwrite them. Skeleton display reuses trunk's existing overlay and reports the model's unique joint count. The hierarchy remains available for selecting joints. Apply held clips invokes trunk's existing retargeting action and is enabled when held clips and a skeleton are present; cross-model retargeting still needs end-to-end validation.

Core tests exercise player/layer actions, actual morph state, owned snapshots and skeleton toggling. UI binding tests reject invalid weights and stale scene/layer edits. Live Windows checks loaded AnimatedMorphCube, added a layer and verified a GPUI weight edit through MCP, paused playback and visibly changed the morph, then loaded SimpleSkin and verified its skeleton toggle and two-joint count. Undo/Redo buttons reflect trunk history availability; creation is not recorded by trunk's current history implementation. Core regression tests verify transform, light and camera edit undo.

## Asset browser

Open **Assets** or press Ctrl+B. Local library, Khronos, Polyhaven models and Polyhaven skies use trunk's catalog/library queries and loading actions. Search applies on Enter; changing tabs clears the search. Results are paged at sixty entries, with Previous/Next and explicit loading/failure states. Add folder/files uses trunk's native dialogs. Polyhaven tabs expose 1K/2K/4K/8K selection.

Click a card to load its model or sky into the current scene. Remote thumbnails use native HTTP and image decoding off the UI thread. Four requests may run at once; decoded images are at most 128×96, oversized source images are rejected, and leaving a page releases its GPUI image handles. Local files currently have text cards; local preview generation remains unfinished. Failed previews can be retried by leaving and reopening the page.

Live Windows validation displayed Khronos thumbnails and loaded the textured Avocado model by clicking its card. Core tests cover case-insensitive search, page bounds, catalog failures and owned snapshots. Polyhaven model/sky downloads and native library add-dialog flows have not yet received end-to-end automation coverage.

## Native MCP

The original agent plugin runs on the same worker-owned world used by the editor. Pass `--agent-port=PORT` to select its localhost port or `--no-agent` to disable it. The endpoint accepts HTTP POST JSON-RPC (`initialize`, `tools/list`, `tools/call`) and retains trunk's tool names and schemas. No browser or second scene is involved.

`just test-core` runs the UI-free core, native frontend startup, light/camera history and localhost MCP regression tests. The MCP test creates a primitive through frontend actions, reads it over HTTP, selects/renames/transforms it, and checks the resulting owned snapshot. It needs permission to open/connect localhost sockets. A separate live GPUI smoke check verified that MCP selection/rename updated the hierarchy and inspector while the textured model was rendering. A keyboard edit of spotlight intensity in GPUI, applied with Enter, was read back as the new value through MCP; undo was then dispatched through that same endpoint.

## Remaining work

Docking, local preview generation, complete PBR controls, advanced skeleton/retargeting coverage, atmosphere, export/screenshot UI, conversion UI, drag/drop and gizmo pointer routing remain unfinished. Camera clipping and light editing have core regression coverage; native dialog texture replacement and the expanded camera UI have not yet received end-to-end automation coverage. Full-scene shared presentation stability is unresolved.
