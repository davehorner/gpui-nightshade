# Shared scene cameras

The editor supports one, two or four camera tiles in its existing Viewport panel. Select **1 view**, **2 views** or **4 views**, press Ctrl+Alt+1/2/4, or launch with `--camera-views=1|2|4`.

All tiles render one worker-owned ECS world. Animation advances once per scene tick; selection, inspector edits, undo/redo and MCP use the same entities and history. The native renderer produces camera targets and composes them into the existing presentation image. There is no additional CPU composition or tile readback in the shared GPU transport.

Click or scroll over a tile to make its camera active. Orbit, pan, zoom, framing and picking then use that camera. Each camera retains its own pose when switching focus or layouts. Labels identify the active camera. Native model cameras activated through the inspector or MCP bind to the focused tile. Enabled cameras are refreshed each presented scene tick so GPU morph and skeleton animation cannot leave cached tiles stale.

These are tiles within one dockable panel. Independently dockable camera panels and persistence of tile layout/camera poses are unfinished. Other independently created viewport widgets still own separate worlds. Secondary cameras initially use quarter-turn offsets from the editor camera; model auto-framing can subsequently affect the active camera alone.

## Validation

The native GPU regression checks four distinct camera targets, stable camera entities, independent orbit and actual picking in the second tile into the same scene. A separate rendered-pixel test loads the bundled AnimatedMorphCube and requires visible changes in every quadrant for both explicit morph weights and continuously playing native clips. These tests are opt-in because they require a native DirectX 12 adapter:

```powershell
cargo test -p nightshade-viewer-core --locked -- --ignored --test-threads=1
```

The animation investigation found incorrect vertex-base builtins in the pinned wgpu 29 DirectX 12 indirect-count draw path. Device feature negotiation now omits `MULTI_DRAW_INDIRECT_COUNT` on DirectX 12, selecting the existing offset-correct indirect fallback. GPU culling remains enabled; other backends retain their previous feature negotiation. Debug line rendering also checks the enabled device feature before selecting its count-draw path. A unit regression checks DirectX 12 and Vulkan negotiation. This is a correctness workaround; its performance cost has not been benchmarked.
