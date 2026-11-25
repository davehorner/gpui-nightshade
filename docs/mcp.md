# Native MCP contract

The editor uses trunk's native localhost HTTP JSON-RPC endpoint and worker-owned scene. `--agent-port=PORT` selects the port; `--no-agent` disables it. No second server, scene, or editor command protocol is introduced. Existing tool names remain available; `tools/list` is the authoritative argument catalogue.

## Results and history

`set_transform`, `set_transforms`, `set_material`, and `set_light` return actual post-edit values. They share the inspector's edit helpers, clamping and undo history. Omitted fields are preserved; one multi-field call creates one history edit (subject to trunk's existing nearby-edit merging). Targeted edits preserve selection. Material edits affect every mesh sharing that material and refresh its selected inspector values.

Actions dispatched for a later scene stage return `queued: true`. Existing acknowledgement fields remain for compatibility; they describe the requested operation, not proof of completion. Query the scene/selection/animation afterward. `set_render` returns `{queued: true, state_before: ...}` instead of presenting the pre-edit state as a completed result. Async loading and screenshot acknowledgements likewise do not guarantee a successful download or written file. Inspect viewer notices/status and output files as appropriate.

Successful calls use `isError: false`; execution or argument errors use `isError: true` with an error message in both text content and `structuredContent`. Invalid JSON, malformed requests, unknown tools, non-object arguments, and unavailable/timed-out queues produce JSON-RPC errors. This follows the MCP distinction between [tool execution errors and protocol errors](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2025-11-25/server/tools.mdx).

Argument schemas reject unknown properties. Required arguments cannot be skipped using null. Numeric arguments must fit finite 32-bit values. Clip/layer/morph indices are checked before actions are queued; weights must be in `[0,1]`, and animation times must be nonnegative. Empty undo/redo history produces a tool error. JSON-RPC notifications never enqueue tool mutations.

## Additional tools

| Tool | Arguments | Behavior |
| --- | --- | --- |
| `set_transforms` | `transforms`: 1–128 transform argument objects | Validate all targets/values before applying them in one stage; return post-edit transforms and record grouped history. Duplicate entities are rejected. |
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

[The dependency-free Python example](../examples/mcp-cinematic/README.md) creates a seeded cast of faceted crystals with two real morph targets and a native glTF clip containing object tweens and cinematic camera tracks. MCP starts/configures playback, reads clip time, and drives and verifies vertex morph weights. It replaces the connected scene and leaves the final pose open. Nightshade evaluates the object/camera animation clip; the client evaluates only the independent morph envelopes.
