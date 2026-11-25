# Trunk viewer parity checklist

Reference: pinned Nightshade trunk `630c74c973f1f1e047123f2fb3090be5a26ba6b9`, not the older standalone viewer. An available core action is not proof of a complete GPUI workflow. Live observations below are Windows-only; source inspection and compilation are distinguished from end-to-end checks.

| Requirement | GPUI frontend / current evidence | Remaining work |
| --- | --- | --- |
| glTF/GLB load | Native open, asset cards and single-path drop; helmet/Fox/Avocado observed | Broader invalid-file/dialog/drop coverage |
| Hierarchy and multi-selection | Hierarchy, Ctrl-click, rename/visibility and inspector | Larger scenes and multi-selection editing checks |
| Transforms, picking and gizmos | Move/scale, background GPU pick, undo and cancellation checked | Broader rotation/snapping, mixed DPI and outside-window capture |
| Materials and textures | Common PBR fields and eight texture replacement/removal slots | Alpha mode/cutoff, normal/occlusion strength, specular/thickness/dispersion, clearcoat/sheen/iridescence fields; live texture-dialog coverage |
| Animation playback/blend/layers | Existing controls, model clips/layer/morph observations | Editable timeline and broader multi-layer workflows |
| Morph targets | Indexed ECS weights and AnimatedMorphCube observed | Larger/animated model coverage |
| Skeleton/retargeting | Skeleton toggle/count observed; held-clip action exposed | Cross-model retargeting verification |
| Lights and cameras | Point/spot controls and camera activation/clipping; core history tests | Directional-light palette workflow and expanded camera UI validation |
| Undo/redo | Existing dispatcher/history; transform/light/camera regression coverage | History parity audit; trunk creation itself is not undoable |
| Local asset library | Search/page/add files/folder actions; palette access | Local previews, library dialogs and failure/retry flows |
| Khronos | Native thumbnails and Avocado card loading observed | Larger catalog/download coverage |
| Polyhaven model/HDRI | Catalog tabs, resolution and loader actions exposed | End-to-end download, HDR and failure flows |
| Rendering/atmosphere | Settings controls, effective-state tests and live tonemap menu | Atmosphere/AA/effect combinations and color/HDR validation |
| Screenshot/GLB export | Native output dialogs and static/skinned reload observations | Broader output/dialog combinations |
| Blender-assisted FBX | Single-path loader delegates to trunk conversion | Blender discovery/setup, progress/errors and complete UI validation |
| Native MCP | Shared transform/material/light history; truthful errors, strict arguments, animation/morph tools; HTTP undo/redo checks | Modern transport negotiation, more shared-mode scene-edit stress |
| Command palette | Search, keyboard selection, dynamic disabled state and current-snapshot confirmation | More command workflow coverage and shortcut hints |
| Keyboard shortcuts | Palette, import/assets/history, framing, W/E/R, playback, output and deletion | Remaining native interface/browser/navigation bindings |
| Dockable panels | Scene/viewport/inspector/animation, split/tab/reset/save/restore | Dedicated material/camera/asset/render/console arrangements; current inspector combines several roles |
| Console/diagnostics | Status text and structured viewport inspection API | Dedicated panel and full timing/memory counters |
| Multiple viewports | Independent-world demo | Multiple editor cameras into one scene |

## Latest parity work

The native gpui-component command palette dispatches existing `ViewerAction` values through `Editor::send`; layout operations stay frontend-local. It exposes all six primitive and all three light kinds, catalogs/library actions, scene/gizmo tools, playback/skeleton, output and history. It never substitutes a second scene-command protocol. Disabled state is rebuilt from the current snapshot and checked again at confirmation. Unknown snapshot state disables state-dependent toggles.

Live shared-GPU validation opened the palette using Ctrl+Shift+P, filtered Add sphere and confirmed a new Sphere through the native MCP scene query. Filtering Rotate gizmo changed the active toolbar mode; the R shortcut selected Scale. Reopening from inspector focus and Escape dismissal were observed through native accessibility. These are keyboard/accessibility and scene-state checks, not fresh screenshot validation: desktop capture remains unavailable after earlier GPU resets. The longer idle session exposed a retirement/redraw scheduling stall, tracked separately in [GPU validation](windows-gpu-validation.md).

## Next parity increments

1. Finish missing material fields using trunk's existing MaterialField actions and snapshots.
2. Add timeline scrubbing/clip-time display without introducing a competing animation state.
3. Complete local library previews and conversion setup/progress/error flows.
4. Split diagnostics and specialized editing panels where that improves the docked workflow.

Keep the portable/reference path and shared presentation checks available while implementing these workflows. Full viewer parity is not claimed.
