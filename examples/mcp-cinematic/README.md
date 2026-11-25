# MCP cinematic choreography

A Python standard-library example that generates randomly sized and placed faceted crystals, loads them into the native viewer through MCP, then animates them with staggered quintic tweens. Colored faces use an embedded unlit material for predictable results across saved lighting settings. A free glTF camera follows a Catmull–Rom dolly path with an establishing view, low pass, elevated reveal and close landing. It continually aims at the subjects.

**This replaces the scene in the connected viewer.** Use a separate viewer instance for the demo. The final scene remains open; Ctrl+C freezes it at its current pose. No network assets, Blender, Python packages or API keys are needed.

From the repository root, start the editor:

```powershell
cargo run -p gpui-nightshade-viewer -- --agent-port=8791 --presentation=windows-shared
```

Use `--presentation=readback` for the portable path. Keep the viewport visible during the demo. In a second terminal:

```powershell
python examples/mcp-cinematic/cinematic.py
```

Try a different cast or a longer camera move:

```powershell
python examples/mcp-cinematic/cinematic.py --seed 42 --objects 10 --duration 40 --fps 20 --screenshot target/mcp-cinematic/finale.png
```

`--shape box` or `--shape pyramid` changes the shared geometry (the default is `diamond`). `--url http://127.0.0.1:PORT/` selects another endpoint. The client initializes MCP, checks the tool catalogue, loads a generated self-contained glTF, waits for named nodes, disables turntable/gizmos/grid, and activates the imported camera. Imported camera transforms are independent of the editor's orbit controller. `set_transforms` applies each camera/object update in one scene stage, validating the complete batch before editing. The example requires the batch-tool patch installed by `scripts/init.ps1`.

The cast shares one mesh and material, with randomized proportions, spin and placement. Live checks exposed missing/incorrect draws for a scene containing several distinct meshes on the pinned renderer. Sharing geometry avoids that issue; this example does not change global GPU culling or batching settings. A stage mesh is deliberately omitted.

The timeline runs in this client using monotonic wall-clock time. It sends evaluated poses over MCP; these are procedural tweens, not baked glTF animation clips. Slow requests lower the update frequency instead of building a backlog or slowing the timeline. `--fps` is a ceiling, not a render-frame-rate guarantee. The final output reports measured update frequency. Each tick records a grouped transform edit in ordinary inspector history; consecutive matching batches can merge using trunk's existing history rules.

Edit `choreography()` to change the seeded cast, `object_pose()` for tween timings/spin, and `camera_pose()` for the path and target. The glTF is written to `target/mcp-cinematic/scene.gltf`; `--scene PATH` changes it. File paths are resolved on the client machine and must be accessible to the viewer on the same machine. Screenshots wait for an output file, so use a fresh output path when checking a new capture.

To generate the scene without connecting, or run the math/geometry checks:

```powershell
python examples/mcp-cinematic/cinematic.py --dry-run
python -m unittest discover -s examples/mcp-cinematic -p 'test_*.py'
```

## Validation

A live Windows shared-GPU run with the normal culling/batching defaults completed 166 updates in 12.09 seconds (13.7 Hz) for six objects plus the camera, using 175 MCP tool calls including setup. The final engine screenshot showed all six colored faceted objects. Update frequency depends on render workload and MCP latency. Python tests check camera aim over the full path, outward triangle normals, deterministic randomization, tween endpoints and generated buffer bounds. Native HTTP tests check batch validation, no partial writes and grouped undo/redo.
