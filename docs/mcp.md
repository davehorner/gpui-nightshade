# Native MCP contract

The editor uses trunk's native localhost HTTP JSON-RPC endpoint and worker-owned scene. `--agent-port=PORT` selects the port; `--no-agent` disables it. No second server, scene, or editor command protocol is introduced. Existing tool names remain available; `tools/list` is the authoritative argument catalogue.

## Results and history

`get_material` and the effective result of `set_material` expose every scalar in trunk's editable material view, including `normal_scale`, `occlusion_strength`, `dispersion`, `clearcoat`, `clearcoat_roughness`, `sheen_roughness`, `iridescence` and `iridescence_ior`. These added query/result fields do not expand `set_material`'s accepted arguments. Advanced inspector edits can be checked through `get_material` against the same native registry values.

`set_transform`, `set_transforms`, `set_material`, and `set_light` return actual post-edit values. They share the inspector's edit helpers, clamping and undo history. Omitted fields are preserved; one multi-field call creates one history edit (subject to trunk's existing nearby-edit merging). Targeted edits preserve selection. Material edits affect every mesh sharing that material and refresh its selected inspector values.

Actions dispatched for a later scene stage return `queued: true`. Existing acknowledgement fields remain for compatibility; they describe the requested operation, not proof of completion. Query the scene/selection/animation afterward. `set_render` returns `{queued: true, state_before: ...}` instead of presenting the pre-edit state as a completed result. Async loading and screenshot acknowledgements likewise do not guarantee a successful download or written file. Inspect viewer notices/status and output files as appropriate.

Successful calls use `isError: false`; execution or argument errors use `isError: true` with an error message in both text content and `structuredContent`. Invalid JSON, malformed requests, unknown tools, non-object arguments, and unavailable/timed-out queues produce JSON-RPC errors. This follows the MCP distinction between [tool execution errors and protocol errors](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2025-11-25/server/tools.mdx).

Argument schemas reject unknown properties. Required arguments cannot be skipped using null. Numeric arguments must fit finite 32-bit values. Clip/layer/morph indices are checked before actions are queued; weights must be in `[0,1]`, and animation times must be nonnegative. Empty undo/redo history produces a tool error. JSON-RPC notifications never enqueue tool mutations.

## Tracked scene application

Queued scene edits now retain their existing response fields and add `operation_id` and `operation_status: "queued"`. Call `get_operation` with that ID until its status is `applied` or `failed`. `queued` and `dispatched` are nonterminal. Results include the tool name, submitted/finished scene-update tick, a retained post-update state snapshot, and an error for failed operations.

The completion boundary is explicitly `scene_actions_applied`. It means the native action batch and subsequent viewer update chain ran. It does **not** prove that a GPU frame rendered, reached the compositor, or became visible. Snapshots represent state at the end of the update and can include other edits made during that update; they are not rollback transactions. Native clamps and action semantics still apply. Tick numbers identify viewer updates, not scene revisions or frame IDs.

Tracking covers selection, creation/deletion, visibility/rename/isolation, undo/redo, playback/animation/layers/morphs, render settings, camera activation and framing. Transform/material/light edits already execute synchronously and return effective values; they retain that contract. File loading, screenshots, export and dialogs do **not** receive operation IDs yet: their workers need correlated success/error reporting before those outcomes can be tracked truthfully.

Operation history retains up to 256 records. Pending records are never evicted; a full pending tracker rejects new tracked edits before they change state. Terminal records are evicted oldest-first as new work arrives. IDs become invalid after eviction or restart; unknown IDs return tool errors. Each retained result is limited to 64 KiB; larger state snapshots return `state_omitted: true` and clients must query current scene state separately. Explicit targets retain their ECS generation while queued; disappearing targets fail the batch before its actions run. Invalid arguments remain immediate tool errors and do not leave a pending operation.

```json
{"name":"set_playback","arguments":{"playing":false}}
```

The response includes an `operation_id`. Use it in:

```json
{"name":"get_operation","arguments":{"operation_id":"<returned ID>"}}
```

The cinematic client's `call_applied` helper performs this bounded wait for tracked scene actions. Native clip setup and pause use it; loading still uses the existing scene readiness checks. Continuous morph and camera updates can continue using `call` when no completion wait is needed.

## Shared camera tiles

`get_camera_views` reports whether the frontend supports shared tiles, the requested layout count, zero-based active tile, physical output dimensions, and enabled tiles with camera entity IDs and pixel rectangles. The GPUI editor publishes its actual shared scene cameras; the original native frontend reports `supported: false` and rejects tile mutations.

| Tool | Arguments | Behavior |
| --- | --- | --- |
| `get_camera_views` | `{}` | Read actual shared camera layout and assignments. |
| `set_camera_views` | `count`: 1, 2 or 4; optional `active` | Change the tile layout. Omitted active preserves focus where possible, clamping when shrinking. |
| `focus_camera_view` | `index` | Focus a visible tile for navigation, existing `frame_model` and `set_active_camera` actions. |
| `set_view_camera` | `index`; optional `entity` | Assign an existing camera. Omit entity for the persistent editor camera. Reusing a managed camera swaps its two assignments. |

Mutations return operation IDs. Their retained result is the actual camera layout after frontend application, with the existing `scene_actions_applied` boundary. Wait for layout application before requesting an index that only exists in the new layout. Invalid counts, indices and non-camera targets are tool errors. Targets that disappear before application fail the operation; imported cameras destroyed by model replacement are repaired with independent editor cameras.

MCP changes synchronize back into the viewport's tile labels and input focus. A newer frontend event takes precedence over feedback from an older worker request. Per-view input anchors prevent a remote focus change from applying another camera's orbit delta. Layout changes also work while the panel/window is hidden. At tiny output sizes the enabled tile list can be shorter than the requested count; indices must refer to an enabled tile.

```python
views = client.call_applied("set_camera_views", count=4, active=0)["result"]
client.call_applied("focus_camera_view", index=2)
client.call_applied("frame_model")
client.call_applied("set_view_camera", index=1, entity=views["tiles"][0]["camera"])
print(client.call("get_camera_views"))
```

These tools control tiles within the existing shared scene panel. Per-view screenshot selection, explicit camera lens/orbit tools and independently dockable camera panels remain unfinished. Existing `get_transform`/`set_transform` can address camera entities; native orbit controllers and animation can subsequently update their transforms.

## Additional tools

| Tool | Arguments | Behavior |
| --- | --- | --- |
| `set_transforms` | `transforms`: 1–128 transform argument objects | Validate all targets/values before applying them in one stage; return post-edit transforms and record grouped history. Duplicate entities are rejected. |
| `get_operation` | `operation_id` | Read tracked scene application status and retained effective state. |
| `set_playback` | `playing`: boolean | Queue an idempotent desired playback state; evaluated when the native action runs. Requires clips. |
| `toggle_playback` | `{}` | Queue pause/resume through the existing animation action; requires clips. |
| `set_animation` | Optional `time`, `speed`, `looping`, `blend_time` | Queue the existing animation actions. Time is seconds; blend time can be configured before clips load. |
| `set_animation_layer_weight` | `index`, `weight` | Queue an existing layer's weight change. |
| `get_morph_weights` | `entity` | Read actual ECS weights. |
| `set_morph_weight` | `entity`, `index`, `weight` | Queue the existing morph editing action. |

These tools reuse the same animation players and morph components as GPUI. Animation and morph actions retain trunk's history behavior; no new undo guarantee is added for them.

## Limits and validation

The endpoint still advertises trunk's `2024-11-05` protocol version and retains its POST JSON transport. This is not a complete modern Streamable HTTP implementation: version negotiation, sessions, streaming and transport lifecycle modernization remain separate work. `get_viewer_state` panel flags describe trunk's native UI state, not GPUI dock layout.

Calls have a 20-second deadline and the pending queue is bounded to 256 entries. Expired calls are discarded before starting; already executing work is not cancelled. The original HTTP server services requests serially.

`cargo test -p viewer --lib --locked` covers protocol errors, execution error envelopes, strict schemas, bounded/expired queues, native frontend startup, and HTTP edits/undo/redo of transforms, materials and lights in the frontend's world. It also checks selection preservation and invalid calls. Localhost socket permission is required. Existing core animation/morph tests cover the underlying actions. Workspace tests and strict Clippy pass; this milestone does not add new live shared-GPU or full client interoperability coverage.

## Cinematic example

[The dependency-free Python example](../examples/mcp-cinematic/README.md) resolves actual animated models remotely first, falls back to validated cache/bundled models, and then uses seeded RNG selection. It always includes AnimatedMorphCube and a real skinned coiled spring, then adds RNG-selected models. It plays native clips, edits morph weights and moves the camera through MCP. The coiled spring is a documented local reconstruction with 13 joints and compression/bending clips. Original skin/joint/animation data are preserved. The earlier procedural crystal demo is optional via `--procedural`.

Operation-tracking validation adds native HTTP checks for queued IDs, retained effective render state, unknown/malformed IDs and snapshot stability after later edits. Core regressions cover stale-target failure before batch execution, bounded retention and repeated explicit playback states. A separate rebuilt viewer was queried over localhost using the Python client: render settings, repeated play/pause, new light components and actual ECS morph weights matched retained applied results. This was scene-state validation; it did not assert GPU presentation or file-worker completion.

The default viewer presentation now prefers Vulkan GPU sharing on supported Windows adapters. MCP still controls the same native scene; `--presentation=vulkan-shared` requires that transport and `readback` remains the reference/fallback. See [Vulkan sharing](vulkan-sharing.md).
