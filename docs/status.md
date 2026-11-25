# Implementation status and remaining work

| Milestone | Evidence / status |
| --- | --- |
| 1. Inspect and pin | Complete for source investigation; component-compatible GPUI snapshot exact pin recorded |
| 2. Feasibility report | Source-backed report in presentation.md |
| 3. Surface-free cube | Compiled and executed; GPU pixel diversity and picking checked |
| 4. GPUI readback viewport | Compiled, visible cube and camera orbit verified in the native UI |
| 5. Input/resize/DPI/picking | Implemented core coordinates, orbit/pan/zoom, focus, picking and outlines; pointer capture, DnD and visibility lifecycle unfinished |
| 6. Windows GPU presentation | Shared texture import/copy/fences visibly verified on RTX 4080, including two viewports; long stress/device loss and broader drivers unverified |
| 7. Viewer core extraction | Not implemented |
| 8–9. Full GPUI editor / parity | Not implemented; current viewer-named binary is a viewport demonstrator |
| 10. macOS/Linux GPU sharing | Investigated, not implemented |
| 11. Benchmark/optimize | Actual debug readback measurements only; GPU-path timing and optimization incomplete |
| 12. Patch/API docs | Current integration documented; API is experimental |

## Viewer extraction work identified from actual trunk

The original `apps/viewer` source is retained unchanged. `data.rs::ViewerAction` remains the dispatcher to reuse. No competing full-viewer action protocol has been introduced.

`resources.rs::Stage` requires `RetainedUiRuntime` alongside scene/material/load/selection resources. `systems/actions.rs::apply_actions` mixes panel/browser state and core scene commands. `systems/setup.rs::initialize` uses that Stage; `systems/settings.rs::load_settings` directly changes runtime visibility. `plugin.rs` combines UI construction/relayout/poll/list rebuild/refresh with picking, gizmos, loading, animation and agent serving. `commands/agent.rs` and `queries/agent.rs` already expose native control and should feed the same action queue and immutable snapshots after extraction.

The remaining refactor must split Stage/resource access and UI-only actions, introduce ViewerCorePlugin/NativeViewerUiPlugin while keeping ViewerPlugin as the composition used by the original app, then expose action submission and owned scene/selection/material/animation snapshots. Merely re-exporting ViewerAction without those schedules would not preserve functionality.

All requested glTF/GLB loading, hierarchy/multi-selection, editing, material/texture operations, animation/morph/skeleton tools, lights/cameras, undo/redo, libraries/Khronos/Polyhaven, atmosphere, screenshots/export, FBX conversion and MCP features remain available in the untouched upstream viewer, but **are not available in the GPUI demonstrator**. Their GPUI frontend and native MCP integration remain required work, not completed parity.

No `just gltf` command or fake feature panels were added. A meaningful glTF recipe must load into the GPUI-owned scene through the extracted viewer core rather than launch the old native viewer or silently show a cube.

Other outstanding work: continuous simulation scheduling, mouse capture outside the viewport, keyboard shortcuts/palette, file dialogs/DnD, docking, shared-world camera viewports, visibility suspension, device recreation, HDR/exact-sRGB validation, detailed GPU/upload/present metrics and upstream application regression builds.
