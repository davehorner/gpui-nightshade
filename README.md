# gpui-nightshade

Native Rust Nightshade rendering inside GPUI elements. Windows has a working D3D12-to-D3D11 shared-texture presentation path, and a separate asynchronous CPU readback reference backend. GPUI owns the window and event loop. No browser, hidden Nightshade window, or second presenter is used.

**Status: the trunk viewer core and an initial GPUI editor are implemented; feature parity remains unfinished.** The default editor loads glTF/GLB through trunk's original schedules, displays the hierarchy and inspector, and dispatches scene edits, undo/redo, rendering settings and animation actions through the original action queue. See [implementation status](docs/status.md).

The editor defaults to CPU readback while a full-scene D3D12 device hang is investigated. Shared presentation remains available explicitly and has previously passed cube/two-viewport checks; full-scene shared stability is not established.

## Build and run

Requires Rust 1.100.0, Windows SDK/MSVC C++ build tools and `just`. `basis-universal-sys` and `meshopt` compile native C++ upstream dependencies. All application and integration code is Rust.

```text
just init
just check
just test
just test-gpu
just headless
just viewer
just cube readback
just gltf C:/models/scene.glb readback
just multi-viewport
just benchmark
just fmt
just clippy
```

Equivalent editor launch: `cargo run -p gpui-nightshade-viewer -- --presentation=readback`. Use `--model=C:/models/scene.glb`, `--no-agent` or `--agent-port=PORT` as needed. The original native agent plugin is composed against the same worker-owned world; its end-to-end GPUI validation remains pending.
`--presentation=readback` forces CPU transfer. `windows-shared` requires the GPU backend and never falls back. `auto` selects shared presentation on Windows and falls back to readback after allocation/import failure; its diagnostics identify the active transport. `GPUI_NIGHTSHADE_PRESENTATION` supplies the default when no CLI selection is present.

Left drag orbits; Shift+left drag pans; wheel zooms; clicking picks using physical viewport-local coordinates. The overlay button demonstrates GPUI content above the scene. `--multi-viewport` creates two independent scene hosts, not two cameras sharing one ECS.

## Source pins and compatibility

| Source | Revision / package | Role |
| --- | --- | --- |
| Nightshade trunk | `630c74c973f1f1e047123f2fb3090be5a26ba6b9` | Patched local submodule, including `apps/viewer` reference |
| Zed trunk | `2664a0aa1eae5dabf8f8273c03f88d53061ca7dc` | Inspected reference checkout |
| gpui-component / gpui-kit | `82f1c1dc1f2970d93f1adfe10cbd772516c21ce7` | Local submodule; current upstream facade is `gpui-kit` |
| Compatible GPUI snapshot | `gpui-pre-* =0.3.14`, Zed `f126950f9d3b62666f97437ae5d4ccac2272c4db` | Exact family required by the component pin |
| wgpu | `29.0.4` in Cargo.lock | Nightshade backend; DX12, Vulkan and Metal compiled upstream |

Nightshade declares Rust 1.98 minimum; inspected Zed trunk pins 1.100.0. This workspace uses 1.100.0. GPUI Windows uses D3D11, not D3D12. Shared mode explicitly requests Nightshade DX12. Readback permits wgpu's environment-selected backend (Vulkan on the tested machine).

GPUI snapshot packages are exported upstream sources under `upstream/gpui-snapshot`, with licenses intact and two patched packages selected by `[patch.crates-io]`. Other GPUI snapshot crates remain exact registry packages, locked with checksums. Do not replace them with Zed trunk independently: gpui-component's exact snapshot contract matters. Zed trunk's wgpu Git dependency is not unified with Nightshade's wgpu; these are separate rendering devices.

## Integration and verification

`nightshade-viewer-core` runs the extracted trunk schedules and publishes owned snapshots through a bounded frontend bridge. `gpui-nightshade` owns the scene host, frame metadata, coordinate conversion and bounded readback ring. `gpui-nightshade-components::NightshadeViewport` is a reusable GPUI view. Custom scene hosts can be supplied with `NightshadeViewport::from_factory` and `SceneHost::from_single_view`. Each view constructs and owns its renderer on a dedicated worker; the UI receives immutable image handles. Camera configuration uses a coalesced mailbox.

The external GPU image extension reuses GPUI's regular polychrome sprite primitive. It preserves layout, content masks, scene ordering, opacity and atlas eviction rather than adding an independent native visual tree.

Verified on Windows with NVIDIA GeForce RTX 4080: headless pixels and picking, CPU viewport display, shared GPU display, camera orbit, GPUI overlay, two independent viewports, and GPU readback tests. The original trunk viewer also passes `cargo check` with the patch. Full upstream-application runtime regression, device-loss recovery, long resize/minimize stress, and non-Windows execution are not yet verified. [Architecture](docs/architecture.md), [presentation feasibility](docs/presentation.md), [patches](docs/upstream-patches.md), [measurements](docs/benchmarks.md).

## Licenses

Integration code: MIT OR Apache-2.0. Nightshade remains MIT OR Apache-2.0, copyright Matthew Jordan Berger. Zed/GPUI and gpui-component retain their Apache-2.0 licenses and notices in their upstream directories. No upstream source attribution has been removed.

