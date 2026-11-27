# gpui-nightshade

Native Rust Nightshade rendering inside GPUI elements. Windows supports Vulkan-to-D3D11 and D3D12-to-D3D11 shared-texture presentation, with a separate asynchronous CPU readback fallback. GPUI owns the window and event loop. No browser, hidden Nightshade window, or second presenter is used.

**Status: the trunk viewer core and an initial GPUI editor are implemented; feature parity remains unfinished.** The default editor loads glTF/GLB through trunk's original schedules, displays the hierarchy and inspector, and dispatches scene edits, undo/redo, rendering settings and animation actions through the original action queue. See [implementation status](docs/status.md).

The editor, demos and rendering helpers default to Vulkan with GPU sharing on supported Windows adapters and readback fallback, following the [long-animation benchmark](docs/benchmarks.md). Explicit Windows shared-GPU presentation now passes fresh full-viewer runs, model replacement and resize/minimize stress on the tested RTX 4080 after retaining evicted D3D11 imports through consumer completion. See [GPU validation](docs/windows-gpu-validation.md) for results and remaining coverage.

## Build and run

The music application lives in its own repository: [gpui-rustel](https://github.com/davehorner/gpui-rustel). It uses this project’s renderer and presentation components.

Requires Rust 1.100.0, Windows SDK/MSVC C++ build tools and `just`. `basis-universal-sys` and `meshopt` compile native C++ upstream dependencies. All application and integration code is Rust.

```text
just init
just check
just test
just test-core
just test-gpu
just test-shared 1200 19790
just headless
just viewer
just cube readback
just gltf C:/models/scene.glb readback
just multi-viewport
just benchmark
just fmt
just clippy
```

Equivalent editor launch: `cargo run -p gpui-nightshade-viewer`. Use `--model=C:/models/scene.glb`, `--no-agent` or `--agent-port=PORT` as needed. The original native agent plugin is composed against the same worker-owned world; live GPUI selection/rename and HTTP core regression tests pass. See [viewer controls](docs/viewer.md).
`--presentation=readback` forces CPU transfer. `windows-shared` requires the GPU backend and never falls back. `auto` prefers Vulkan GPU sharing on Windows and falls back to Vulkan readback when allocation/import is unsupported. `vulkan-shared` forces Vulkan GPU sharing and reports errors instead of falling back. `windows-shared-auto` explicitly opts into DX12 shared presentation with readback fallback after allocation/import failure. `GPUI_NIGHTSHADE_PRESENTATION` supplies the default when no CLI selection is present.

In the editor, left-click picks and left-drag manipulates gizmos; right-drag orbits, Shift+right-drag pans and the wheel zooms. The cube demo retains left-drag orbit and Shift+left-drag pan. On mobile, one-finger drag zooms, two-finger drag pans, pinching zooms, twisting two fingers rotates around the scene, and three-finger drag adjusts yaw and pitch. Multi-touch navigation starts inside the viewport and resets when the finger count changes. Pointer coordinates are viewport-local physical pixels. The overlay button demonstrates GPUI content above the scene. `--multi-viewport` creates two independent scene hosts, not two cameras sharing one ECS.

## Source pins and compatibility

| Source | Revision / package | Role |
| --- | --- | --- |
| Nightshade trunk | `630c74c973f1f1e047123f2fb3090be5a26ba6b9` | Patched local submodule, including `apps/viewer` reference |
| Zed trunk | `2664a0aa1eae5dabf8f8273c03f88d53061ca7dc` | Inspected reference checkout |
| gpui-component / gpui-kit | `82f1c1dc1f2970d93f1adfe10cbd772516c21ce7` | Local submodule; current upstream facade is `gpui-kit` |
| Compatible GPUI snapshot | `gpui-pre-* =0.3.14`, Zed `f126950f9d3b62666f97437ae5d4ccac2272c4db` | Exact family required by the component pin |
| wgpu | `29.0.4` in Cargo.lock | Nightshade backend; DX12, Vulkan and Metal compiled upstream |

Nightshade declares Rust 1.98 minimum; inspected Zed trunk pins 1.100.0. This workspace uses 1.100.0. GPUI Windows uses D3D11, not D3D12. `windows-shared` explicitly requests Nightshade DX12; `vulkan-shared` retains Vulkan rendering. Readback explicitly defaults to Vulkan. `WGPU_BACKEND` remains an explicit renderer override, for example `dx12` or `metal`; Windows GPU sharing requires a matching adapter and supports Vulkan or DX12. GPUI's Windows compositor continues to use D3D11.

GPUI snapshot packages are exported upstream sources under `upstream/gpui-snapshot`, with licenses intact and two patched packages selected by `[patch.crates-io]`. Other GPUI snapshot crates remain exact registry packages, locked with checksums. Do not replace them with Zed trunk independently: gpui-component's exact snapshot contract matters. Zed trunk's wgpu Git dependency is not unified with Nightshade's wgpu; these are separate rendering devices.

## Integration and verification

`nightshade-viewer-core` runs the extracted trunk schedules and publishes owned snapshots through a bounded frontend bridge. `gpui-nightshade` owns the scene host, frame metadata, coordinate conversion and bounded readback ring. `gpui-nightshade-components::NightshadeViewport` is a reusable GPUI view. Custom scene hosts can be supplied with `NightshadeViewport::from_factory` and `SceneHost::from_single_view`. Each view constructs and owns its renderer on a dedicated worker; the UI receives immutable image handles. Camera configuration uses a coalesced mailbox.

The external GPU image extension reuses GPUI's regular polychrome sprite primitive. It preserves layout, content masks, scene ordering, opacity and atlas eviction rather than adding an independent native visual tree.

Verified on Windows with NVIDIA GeForce RTX 4080: headless pixels and picking, CPU viewport display, shared GPU display, camera orbit, GPUI overlay, two independent viewports, and GPU readback tests. The original trunk viewer also passes `cargo check` with the patch. Full upstream-application runtime regression, device-loss recovery, long resize/minimize stress, and non-Windows execution are not yet verified. [Embedding API](docs/integration.md), [architecture](docs/architecture.md), [presentation feasibility](docs/presentation.md), [patches](docs/upstream-patches.md), [measurements](docs/benchmarks.md).

## Licenses

Integration code: MIT OR Apache-2.0. Nightshade remains MIT OR Apache-2.0, copyright Matthew Jordan Berger. Zed/GPUI and gpui-component retain their Apache-2.0 licenses and notices in their upstream directories. No upstream source attribution has been removed.


## Browser / GitHub Pages

The WASM target reuses the GPUI editor, Nightshade viewer core, scene actions, hierarchy, inspector, animation controls and shared-camera viewport. Nightshade renders real 3D through WebGPU. The browser presentation backend asynchronously maps a bounded readback ring and presents completed frames as GPUI images; it never blocks on a GPU wait or creates a native render thread. Windows Vulkan/D3D sharing remains the native path.

```text
python scripts/prepare.py
rustup toolchain install nightly-2026-10-31 --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked --root target/wasm-tools
python web/build.py
python -m http.server 18767 --directory target/nightshade-web
```

Open http://localhost:18767/ with a WebGPU-capable browser. Optional `?model=HTTPS_URL` loads a remote glTF/GLB through the original viewer loader. Browser filesystem paths, native MCP HTTP serving and Windows GPU sharing are unavailable. Remote assets need CORS. The browser app uses GPUI’s single-threaded web backend and needs HTTPS for WebGPU; it does not block startup on cross-origin-isolation service-worker registration.

The `Publish WASM to GitHub Pages` workflow builds from pinned source dependencies and deploys after pushes to main/master or manual dispatch. Site: https://davehorner.github.io/gpui-nightshade/.

The browser build includes a compatibility copy of wgpu 29.0.4 whose internal WebGPU enum bindings are private to Rust. This lets Nightshade’s wgpu 29 and GPUI’s wgpu 30 coexist in one WASM module without duplicate JavaScript exports; upstream licenses and Rust APIs are retained.

### Mobile startup diagnostics

Open `?diagnostics=1` to pause before requesting an adapter or downloading WASM.
After Start app, Copy diagnostics includes the startup stage, WebGPU features and
limits. `diagnostics.html` reads the last saved stage without GPU calls.
`gpu-check.html` runs explicit RGBA/BGRA copy/readback, indirect-draw and two-device
checks without GPUI, WASM or audio. Reports are copied manually, never sent automatically. The current renderer requires
`indirect-first-instance`. Startup stays visible until the first scene frame;
an interrupted startup can be retried explicitly. Narrow browser windows use
tabs for the viewport, scene, inspector and animation instead of side-by-side
panels. Browser shadow targets use a smaller budget; native targets are unchanged.

Browser SDF caching uses an 8 MiB atlas instead of the desktop 343 MiB atlas;
lookup buffers and environment-lighting filter sizes/sample counts are also bounded.
Browser particle pools hold at most 32,768 particles and sixteen 128-square textures.
The Nightshade paint image pool uses thirty-two 256-square layers. Together these
three pools request 13 MiB instead of about 314 MiB, including in empty scenes;
native particle and paint budgets are unchanged.
GPUI reuses empty web image-atlas textures as readback frames replace their image
keys; it avoids allocating a fresh atlas texture for each presented frame. Native
atlas eviction is unchanged.
Startup waits for GPU rendering completion before copying the first frame, so
a rendering failure is reported separately from copy/readback.

Mobile browsers and `?safe=1` select a compact material profile; the full
profile remains available explicitly.
The profile retains skinning/morphs, vertex/instance colors, base and emissive
textures, alpha mask/dither/blend, geometric normals and up to four diffuse
point/spot/directional lights. It uses simple ambient lighting; normal/occlusion
maps, PBR reflections, IBL, light cookies and advanced material lobes are omitted
to reduce fragment compilation complexity. Desktop browsers and native apps keep
the full shaders. `?materials=compact` or `?materials=full` overrides the browser
selection; diagnostics record when mobile material shaders are selected.

`mesh-check.html` reproduces the captured compact opaque mesh pipeline without
WASM/audio and compares actual vertex/fragment stages, target state and sampler
variants using small zero-filled resources. It preserves per-case GPU errors.

Browser mesh shaders use perspective varyings with explicit reconstruction of
screen-linear UVs and OIT depth. Compact opaque materials do not consume affine
UVs and skip their vertex correction. Adreno 6xx standalone variants pass while
the full app still reports pipeline creation failures; mobile startup remains
unresolved. Native interpolation is unchanged.
`interpolation-check.html` compares the correction against hardware-linear
interpolation at different vertex depths (at most one UNORM byte difference).
