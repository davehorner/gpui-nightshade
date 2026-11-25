# Remote-first MCP model showcase

Every run includes these two animated models first:

1. **AnimatedMorphCube**, with its original morph animation and MCP weight edits.
2. **CoiledSpring**, an actual six-turn helix with a 13-joint skin and native compression and bending clips.

The cube and spring cannot be skipped by RNG. Then seeded RNG chooses an additional animated morph model and an animated character with a skeleton. Duplicate cube selections are skipped. Extras currently come from AnimatedMorphSphere, Fox and CesiumMan.

Remote candidates are fetched and validated first. A remote pool falls back to validated cached/bundled assets when unavailable. Only after resolution does RNG choose the extras and clips. The included spring has no remote source and is always available locally. Without `--seed`, a fresh seed is generated and printed for replay. Availability changes can change random choices for the same seed.

**Spring provenance:** the earlier recorded skeleton-validation asset was SimpleSkin (a bending strip); the original coiled-spring file could not be located. `CoiledSpring.glb` is an explicit reconstruction, not a renamed sphere or strip. Its reproducible source is `make_spring.py`.


**This replaces the connected scene for each model (three or four acts).** Use a separate viewer instance. No Python packages or API keys are required. Remote models are downloaded into the ignored cache; original assets are preserved.

From the repository root, start a viewer:

```powershell
cargo run -p gpui-nightshade-viewer -- --agent-port=8791 --presentation=windows-shared
```

Use `--presentation=readback` for the portable path. Keep the viewport visible. In a second terminal:

```powershell
python examples/mcp-cinematic/cinematic.py --seed 7
```

`--duration` is the duration **of each act**. For example:

```powershell
python examples/mcp-cinematic/cinematic.py --seed 42 --duration 20 --fps 10 --screenshot target/mcp-cinematic/showcase.png
```

This writes `showcase-cube.png`, `showcase-spring.png`, `showcase-skeleton.png`, and `showcase-morph.png` when an extra morph model is selected. Use fresh output paths. `--url http://127.0.0.1:PORT/` selects another viewer. `--asset-cache PATH` changes the download cache; `--download-timeout SECONDS` sets the network timeout (default 8 seconds). `--dry-run` resolves sources and prints the RNG choices without contacting MCP.

## Animation, morphs and camera

The example retains the chosen model's original mesh, materials, joints, inverse bind matrices, skin weights and animation channels. It converts the self-contained GLB to an embedded glTF and appends a free camera without changing original node indices. Bind-pose scene bounds, including node transforms, scale the cinematic camera path to the model. This bound estimate does not account for all animated excursions.

MCP `play_clip` starts the chosen native clip; `set_animation` configures randomized playback speed and looping. For the morph act, the original weight-animation clip plays first. In the last 35% of the act the client pauses playback and drives actual weights using `set_morph_weight`, then reads them with `get_morph_weights`. Pausing prevents the model's weight tracks from overwriting MCP edits.

For the spring and skeleton acts, the native player drives the original skinned model and MCP enables the skeleton overlay. The source joint count and the selected native clip are printed. Skinning is evaluated on the GPU; unchanged ECS joint transforms are not used as a playback test. Camera dolly/orbit updates use MCP `set_transform`, independently of native model animation. `--fps` caps camera/morph MCP updates, not native animation/render frequency. The final model stays open; Ctrl+C pauses the current act.

To regenerate the local coiled spring:

```powershell
python examples/mcp-cinematic/make_spring.py
```

## Sources and local fallback credits

- [AnimatedMorphCube](https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/AnimatedMorphCube): Microsoft, CC0. A small original copy is included for offline fallback; see [assets/NOTICE.md](assets/NOTICE.md).
- [AnimatedMorphSphere](https://github.com/KhronosGroup/glTF-Sample-Models/tree/master/2.0/AnimatedMorphSphere): original archived Khronos sample repository, including its source credits/license.
- [Fox](https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/Fox): PixelMannen model (CC0), tomkranis rig/animation and AsoboStudio/scurest conversion (CC BY 4.0).
- [CesiumMan](https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/CesiumMan): source model credits/license apply.

Bundled Nightshade fallbacks retain their existing upstream files and licenses. Downloads are validated and limited to 32 MiB each. Failed or malformed remote data does not replace a valid cached model.

## Optional procedural demo and tests

The earlier generated crystal/tween/native-clip demo is available explicitly:

```powershell
python examples/mcp-cinematic/cinematic.py --procedural --seed 7 --objects 6
python -m unittest discover -s examples/mcp-cinematic -p 'test_*.py'
```

`--objects` and `--shape` apply only to that optional demo. The default uses downloaded or local actual model assets.

Tests cover remote-before-RNG ordering, deterministic choices, remote validation and local fallback, preservation of skin/animation data when adding a camera, and procedural geometry/animation checks. Live shared-GPU checks selected remote AnimatedMorphSphere + Fox and, with an empty cache and failed remote handshakes, bundled AnimatedMorphCube + Fox. Both acts loaded, played native clips, moved the camera and produced inspected engine captures. The local Fox contains 24 skin joints. A separate live run rendered the required cube and actual coiled spring, playing its native compression clip with 13 skin joints.

## Scene application acknowledgements

This client requires `get_operation` and `set_playback`. Native clip setup and pause wait for their returned operation IDs to reach `applied`; repeated pause requests cannot resume playback. `Mcp.call_applied` returns the retained operation result and raises on failure or a bounded wait timeout. This confirms native scene application, not GPU presentation. Model loading and screenshot writing remain asynchronous and do not yet have correlated completion IDs.

To control the same shared scene from several cameras, use `client.call_applied("set_camera_views", count=4, active=0)`, then `focus_camera_view` and `frame_model` for each desired tile. `get_camera_views` returns the actual camera IDs and output rectangles. Existing camera transforms and native clips continue using those entities. See the [camera tool example](../../docs/mcp.md#shared-camera-tiles).
