# Reusable viewport integration

`gpui-nightshade-components::NightshadeViewport` owns its render worker and opaque GPUI image. The embedding application owns the GPUI window and positions the viewport like another view.

```rust,ignore
use gpui_nightshade_components::{BackendPreference, NightshadeViewport};

let viewport = cx.new(|cx| {
    NightshadeViewport::with_backend(BackendPreference::Auto, cx)
});
// Add viewport.clone() to a GPUI container.
let status = viewport.read(cx).presentation_status();
let frame = viewport.read(cx).current_frame_info();
```

`presentation_status()` returns an owned snapshot: requested preference, initialized transport, actual Nightshade graphics backend/adapter, effective visibility, fallback reason and error. `active` is absent during initialization. A recorded error means the transport must not be treated as usable; the API does not claim device recovery. Auto can select readback, recording why; forced `WindowsShared` fails rather than falling back. `PresentationBackend::capabilities()` describes whether normal pixels remain GPU-resident and whether GPUI must use the same adapter. Shared mode currently copies once into shared resources; GPU residency does not mean zero-copy.

`current_frame_info()` describes the immutable image currently retained by the GPUI view: producer sequence, resize generation, physical dimensions, transfer function and alpha. It is absent before the view adopts a frame. Metadata and the owned image travel through the mailbox together for both transports, so a pending newer render cannot relabel an older image. This is not a Present acknowledgement or timing sample. Existing CPU frame fields remain available through `CpuFrame`; `info()` exposes the same metadata contract.

No native handles, borrowed ECS state or CPU-only frame assumption appears in this inspection API. GPU resource ownership and producer/consumer synchronization stay inside the concrete transport and GPUI importer; eviction retains imports through consumer completion. The former unused `GpuFrame`/`PresentationFrame` abstractions were removed rather than maintained beside the working path.

For a custom scene, pass a `SceneFactory` to `NightshadeViewport::from_factory`. It runs on the render worker and returns a `Box<dyn ViewportHost>` built with the requested backend and dimensions. The scene host itself need not be Send. Honor backend selection so forced shared mode can use DX12. Use `with_scene_input()` for trunk picking/gizmo routing, and `set_visible()` to report container visibility independently of platform-window visibility. Quit joins the worker through GPUI's background executor.

Current limits: opaque viewports, Windows-only shared import, no device recreation, independent renderer/world per viewport. Readback remains the default editor transport. macOS/Linux sharing should extend this actual ownership contract when implemented, rather than introducing a parallel unused frame interface.

The view starts a window-bound polling task on first render. Besides checking the frame mailbox, this task calls GPUI's nonblocking `Window::poll_external_images()` even when no redraw is needed. Completed retired imports can therefore release their producer leases without waiting for another frame; this prevents a bounded producer pool from waiting on its own next redraw. Other users of the external-image extension should poll retirement while waiting for frames too. Polling never waits for GPU completion or reads pixels.
