# GPUI viewer controls

Run `just viewer` for the bundled model or `just gltf C:/models/scene.glb`. The full editor currently defaults to asynchronous readback. `--presentation=windows-shared` passes repeated full-scene and lifecycle checks on the tested RTX 4080 after fixing imported-resource retirement; broader hardware coverage remains unverified.

Open **Commands** or press **Ctrl+Shift+P** (Cmd+Shift+P on macOS) for the searchable command palette. Type to filter, use Up/Down and Enter to choose, and Escape to clear the query or dismiss. It dispatches the same trunk actions as the toolbar and MCP-controlled scene: browsing/catalog/library commands, all six primitives and three light types, framing/editor camera, selection tools, gizmo modes/snapping/grid, playback/skeleton, screenshot/export and undo/redo. Save/reset layout are frontend commands. Unavailable commands are disabled, and confirmation rechecks the latest snapshot rather than relying on state captured when the palette opened.

Trunk keyboard bindings now include W/E/R for move/rotate/scale, F to frame, X/Delete to delete selection, Space to toggle playback when clips exist, Ctrl+S for screenshot and Ctrl+Shift+S for GLB export, alongside Ctrl+O, Ctrl+B and undo/redo. Scene shortcuts yield to focused inspector/weight fields; the palette shortcut remains accessible while editing. Full keyboard/interface parity remains unfinished.

The hierarchy supports selection and Ctrl-click multi-selection. Inspector text fields apply on Enter. Changing selection releases focused entity fields so a pending value cannot apply to a different entity. Editor shortcuts yield to focused text fields.

Drop one GLB, glTF, HDR, ZIP or FBX file onto the editor to open it through trunk's existing native path loader. A supported drag highlights the editor border. Unsupported, relative, unrepresentable or multiple paths produce a status message without loading. Paths retain their full directory location so glTF sibling buffers and textures remain resolvable. File reads, archive handling and Blender conversion are delegated to the scene worker; FBX still requires trunk's Blender setup. Folder drops, loose multi-file bundles and texture-slot drops are not implemented.

File-drop tests cover case-insensitive supported extensions, Unicode/spaced paths, preserved directory location and rejected ambiguous/unsupported paths. The GPUI external-path listener uses the existing bounded action bridge. After the Explorer drag attempt, the live editor displayed the textured Fox model from the test folder with 27 hierarchy nodes and Survey/Walk/Run clips. The desktop input tool reported a cross-window coordinate error during that attempt, so repeatable automated external-drop coverage remains incomplete. HDR/archive/FBX and invalid-drop UI flows have not been validated end to end.

Viewport controls: left-click selects using trunk GPU picking; drag a gizmo handle with the left button to move/rotate/scale. Right-drag orbits, Shift+right-drag pans in world X/Y, and the wheel zooms. The active gizmo mode is marked in the toolbar. Snap toggles trunk snapping; holding Ctrl also enables it. Escape, focus loss and viewport size/DPI changes cancel an unfinished gizmo gesture and restore its starting transforms without adding history. Pointer release is captured across the viewport boundary so a completed drag is recorded and undoable even when released over another pane.

Input uses a bounded ordered edge queue; adjacent moves coalesce while press/release positions survive slow frames. Queue overflow cancels the gesture. Completed frames continue publishing during input; resize generations remain checked. Live Windows checks verified move/scale, release over the neighboring pane, Ctrl+Z and background GPU picking. Unit tests verify unchanged camera targets during gizmo movement, cancel restoration and queue overflow. Broader rotation/snapping, cross-window capture and cross-platform input stress remain unverified.

The inspector exposes transforms/name, visibility/delete, base color RGBA, metallic/roughness, emissive RGB/strength, transmission, IOR, unlit/double-sided flags, and all eight texture slots with replacement and removal. Texture replacement uses the original native file dialog and asynchronous texture loader. These controls edit the selected material; meshes sharing that material change together, as in trunk.

Point/spot lights can be created from the toolbar. Light inspectors expose RGB, intensity, range, cone angles in degrees and shadows. Light-marker materials are hidden so the actual light controls remain accessible. Selected cameras expose perspective FOV, near/far clipping and camera activation; perspective far clip zero means infinite. A button restores the editor camera.

Playback controls include clip selection, play/pause, stop, seek, positive speed, looping and blend duration. Each clip can be added as a layer; layer weights apply on Enter and layers can be removed. Duplicate layer additions are disabled. Dynamic fields are bound to the current scene root, clip and entity generation, and stale fields are released when the model or layer changes.

Select a mesh with morph targets to edit its indexed weights (0–1) in the inspector. Snapshots read current ECS weights, including changes made by animation. Pause playback before manual edits; active animation can overwrite them. Skeleton display reuses trunk's existing overlay and reports the model's unique joint count. The hierarchy remains available for selecting joints. Apply held clips invokes trunk's existing retargeting action and is enabled when held clips and a skeleton are present; cross-model retargeting still needs end-to-end validation.

Core tests exercise player/layer actions, actual morph state, owned snapshots and skeleton toggling. UI binding tests reject invalid weights and stale scene/layer edits. Live Windows checks loaded AnimatedMorphCube, added a layer and verified a GPUI weight edit through MCP, paused playback and visibly changed the morph, then loaded SimpleSkin and verified its skeleton toggle and two-joint count. Undo/Redo buttons reflect trunk history availability; creation is not recorded by trunk's current history implementation. Core regression tests verify transform, light and camera edit undo.

## Docking

Scene, Viewport, Inspector and Animation are retained native dock panels. Drag a panel title into another panel's body to create a tab group, or towards an edge to split it. Drag dividers to resize. The title's ellipsis menu offers Zoom In/Out; **Reset layout** restores the default split arrangement and exits panel zoom.

Moving, tabbing and resetting retain the same viewport entity, worker, scene, selection and input fields. Panels observe editor changes through weak references so tabbed inspectors continue receiving current snapshots without retaining the editor after window closure. Core panels cannot be closed. The asset browser stays above the docking area.

Live Windows checks covered inspector-to-viewport tab grouping, switching back to the rendered scene, splitter resizing, panel zoom and layout reset. A cube created while the viewport was hidden appeared on return, and MCP confirmed identical entity/selection/transform before and after reset.

Presentation pauses when the Viewport is behind another tab, outside the zoomed group, or in a closed side dock. The status bar reports “Viewport paused; scene live.” The retained worker continues simulation, loading, animation, snapshots and MCP requests. It drains outstanding copies without publishing more GPUI images, cancels pointer gestures, and defers viewport resize until presentation resumes. Explicit render jobs (including screenshots, texture reloads and sky commands) can produce offscreen frames while hidden. The last displayed image and bounded transport allocations remain retained.

The viewport also observes GPUI platform-window visibility. Minimizing or hiding the window pauses presentation independently of dock state; restoring it resumes only panels that are still displayed. Scene service and explicit render jobs continue. Focus and visibility changes cancel unfinished pointer gestures. Windows reports shown/non-minimized state, so covering the editor with another window does not suspend it.

The GPU worker regression verifies that presentation frame counts stop while hidden, service ticks continue, a hidden resize is handled on resume, and the host is constructed only once. It also verifies that restoring the window while its panel remains hidden does not resume rendering. Native readback checks created a cube and wrote a screenshot while hidden, preserved selection/transform on resume, then loaded AnimatedMorphCube while hidden, observed advancing animation time, and rendered the replacement model on return. A separate native minimize/restore check created a cube through MCP while minimized and confirmed its selection/transform and rendered scene after restoring. Visibility tests cover active tabs, zoomed groups and independent window/panel state changes. Readback diagnostics include a frame sequence for lifecycle inspection. Full-occlusion detection on Windows, broader GPU drivers remain unfinished. Forced shared-mode resize/minimize stress now passes; see [GPU validation](windows-gpu-validation.md).

Panel arrangement, split sizes and active tabs restore across launches. **Save layout** writes immediately; normal window close saves automatically. On Windows the default file is `%APPDATA%/gpui-nightshade/layout.json`; other platforms use `$XDG_CONFIG_HOME/gpui-nightshade/layout.json` or `$HOME/.config/gpui-nightshade/layout.json`. `GPUI_NIGHTSHADE_LAYOUT` overrides the file path; an empty value disables persistence. Reset layout affects the current arrangement, which is persisted by Save layout or normal close. Forced process termination does not save unsaved changes. Panel zoom is transient.

The versioned layout must contain each editor panel exactly once. File size, nesting, tab indices and split/dock geometry are bounded and validated before loading. Corrupt, incompatible or incomplete layouts fall back to the default arrangement with a stderr diagnostic. Saving replaces the file through a temporary sibling, and errors from explicit saves appear in the status bar. Tests cover replacement, corruption, oversize files, invalid geometry and panel identities. Native Windows checks verified manual save, relaunch restoration with the active tab, automatic save on normal close, and corrupt-file fallback. Scene/model state is not stored in the layout.

## Rendering and output

The inspector has fixed Selection and Rendering tabs. Rendering exposes exposure/scale, native dropdowns for shading/PBR debug/tonemapping/AA/atmosphere, grid/normals/bounds, bloom/SSAO/SSR/SSGI, sky, sun direction/intensity/shadows, fog color/range and depth-of-field focus settings. Fields apply on Enter. HDR atmosphere remains disabled until an HDR sky is loaded. The underlying dispatcher retains trunk's clamps and settings behavior.

Screenshot opens trunk's native save dialog and writes a scene PNG using the existing asynchronous GPU readback/writer. Export GLB opens the native model dialog and invokes trunk's scene exporter. No native Nightshade presentation window is required. Export is disabled while no scene nodes are available.

Validation: a live GPUI tonemap-menu selection was confirmed through MCP; both native save dialogs wrote files into the workspace. The textured helmet export contained five embedded images/textures and reloaded/rendered successfully (three nodes, 1,290 distinct colors). A separate SimpleSkin export retained its skin and animation, then reloaded and rendered surface-free (five scene nodes, 911 distinct colors). Core tests check effective settings, fog/scale clamping, HDR availability and actual camera/sun components. Exhaustive atmosphere/AA/effect combinations and multiple GPU drivers remain unverified.

## Asset browser

Open **Assets** or press Ctrl+B. Local library, Khronos, Polyhaven models and Polyhaven skies use trunk's catalog/library queries and loading actions. Search applies on Enter; changing tabs clears the search. Results are paged at sixty entries, with Previous/Next and explicit loading/failure states. Add folder/files uses trunk's native dialogs. Polyhaven tabs expose 1K/2K/4K/8K selection.

Click a card to load its model or sky into the current scene. Remote thumbnails use native HTTP and image decoding off the UI thread. Four requests may run at once; decoded images are at most 128×96, oversized source images are rejected, and leaving a page releases its GPUI image handles. Local files currently have text cards; local preview generation remains unfinished. Failed previews can be retried by leaving and reopening the page.

Live Windows validation displayed Khronos thumbnails and loaded the textured Avocado model by clicking its card. Core tests cover case-insensitive search, page bounds, catalog failures and owned snapshots. Polyhaven model/sky downloads and native library add-dialog flows have not yet received end-to-end automation coverage.

## Native MCP

The original agent plugin runs on the same worker-owned world used by the editor. Pass `--agent-port=PORT` to select its localhost port or `--no-agent` to disable it. The endpoint accepts HTTP POST JSON-RPC (`initialize`, `tools/list`, `tools/call`) and retains trunk's tool names with stricter argument schemas and additional animation/morph tools. See the [MCP contract](mcp.md) for mutation results, history and compatibility. No browser or second scene is involved.

`just test-core` runs the UI-free core, native frontend startup, light/camera history and localhost MCP regression tests. The MCP test creates a primitive through frontend actions, reads it over HTTP, selects/renames/transforms it, and checks the resulting owned snapshot. It needs permission to open/connect localhost sockets. A separate live GPUI smoke check verified that MCP selection/rename updated the hierarchy and inspector while the textured model was rendering. A keyboard edit of spotlight intensity in GPUI, applied with Enter, was read back as the new value through MCP; undo was then dispatched through that same endpoint.

## Remaining work

See the [feature-by-feature parity checklist](viewer-parity.md) for the distinction between exposed actions, GPUI workflows and live verification.

Local preview generation, complete PBR controls, advanced skeleton/retargeting coverage, broader render-mode validation, conversion UI, asset drag/drop and broader gizmo validation remain unfinished. Camera clipping and light editing have core regression coverage; native dialog texture replacement and the expanded camera UI have not yet received end-to-end automation coverage. Shared-mode full-scene and resize/minimize checks pass on the RTX 4080; broader adapters, device recreation and visual automation remain incomplete.

## Shared scene cameras

Use **1 view**, **2 views**, **4 views** or Ctrl+Alt+1/2/4 to change camera tiles in the Viewport panel. `--camera-views=4` starts with four views. Click or scroll a tile to direct camera input to it. Scene edits, animation, selection and MCP remain shared. See [controls, implementation and limits](shared-cameras.md).
