# GPUI Rustel

Native music-first GPUI application built on this workspace's reusable Nightshade viewport. It connects to the same music hub as [`bespoke_rustel`](https://github.com/davehorner/bespoke_rustel) and opens the live pattern field by default. There is no URL flag, browser, WebView, WASM worker or old Nightshade viewer dependency.

## Run

Keep the existing `bespoke_rustel` checkout beside `gpui-nightshade`. This development integration uses its `music-link` protocol crate and Hearsay TCP client directly:

```text
R:/w/
  bespoke_rustel/
    music-link/
    hearsay/
  gpui-nightshade/
    gpui-rustel/
```

This folder is a separate Cargo workspace so the original viewer remains buildable without the Rustel checkout. `just rustel` shares the root build cache.

After preparing that checkout with its normal setup, start its music hub and Rustel/Bespoke producer. The existing Windows launcher supports `-NoWeb`, and the Unix launcher supports `--no-web`; these suppress the old web viewer. The native app does not launch or replace the audio engine.

From `gpui-nightshade`:

```powershell
cargo run --manifest-path gpui-rustel/Cargo.toml --target-dir target --release --locked
# Alternate native TCP hub or forced presentation transport:
cargo run --manifest-path gpui-rustel/Cargo.toml --target-dir target --release --locked -- --hub=127.0.0.1:9612 --presentation=vulkan-shared
cargo run --manifest-path gpui-rustel/Cargo.toml --target-dir target --release --locked -- --presentation=readback
```

Vulkan sharing is the automatic Windows presentation preference, with the same capability-based readback fallback as the viewer. Audio stays in Rustel/Bespoke. Scope/spectrum/vectorscope use the hub's master analysis frames; the app does not play another copy of the audio. It reconnects if the hub starts later or disconnects, and expires stale snapshots/audio.

## Native workflows

- Stage (3D) is the default; Nightshade renders moving colored note blocks, lane rails and a playhead into an ordinary GPUI viewport. Drag to orbit and scroll to zoom. Orbit can also run automatically.
- Piano roll, Wordfall, Spiral, Pitch wheel, Scope, Spectrum, Vectorscope and Control curves use native GPUI drawing. Flat views support wheel zoom, drag pan and reset. Audio gain changes visualization gain only.
- Session and demo selection, play/pause/stop, four-cycle seek, tempo/time signature, lane filtering, solo and transpose use the original music commands and acknowledgements. Tempo entry is `BPM, numerator, denominator`, followed by Enter.
- Lane cards show source names, original line numbers, transpose and named parts. New scores automatically expand independently emitting named parts; manual collapses survive later snapshots of that score. Expansion uses the shared protocol's visual-lane projection. Click a lane to inspect its full score with active-event source-range highlighting; source text is selectable. Shared definition names and effective numeric values are shown.
- Commands retain session/generation checks, bounded queues and acknowledgement feedback. A queued send does not claim application. Rendering and UI never wait for hub I/O.

This is a native port of the music experience, not an embedding of the former web editor. Exact visual parity is not claimed: 3D mesh-text labels/picking-to-source, browser source token popovers and full shared-definition graph widgets are further work. Desktop packaging and removal of the sibling source-checkout requirement are also unfinished. The music client currently controls the hub rather than exposing a new music MCP server.

## Source provenance

Reference checkout: `davehorner/bespoke_rustel` at `223a9e4b9f3d51b76233e7bde31dc2df3b63572b` with its tracked vendor overlays applied; Hearsay published base `033cf8ba3ef87614f45ac397d5cb940898433a5d`. The protocol/types/command validation remain in that checkout. Native visual layouts adapt the stage and Canvas2D music algorithms, including radial geometry and audio analysis interpretation. Original project attribution and licenses remain with those dependencies. New GPUI integration uses this workspace's MIT OR Apache-2.0 license.

## Validation (Windows, 2026-11-25)

Release application ran on an RTX 4080 with forced `vulkan-shared` presentation. Live desktop observations covered the animated stage, named-part expansion, orbit, active source highlighting, native demo selection, Piano roll and master Scope. The real native Rustel runtime ran with `--output silent`; its hub published fresh score data and 512-sample scope, 512-bin spectrum and 1024 stereo-pair analysis frames. This validates the native telemetry/rendering path, not audible-device output. The final window leaves that producer and hub running.

`just rustel-check` runs five tests and strict Clippy. Tests cover a real TCP broker/client snapshot-command-acknowledgement round trip, startup before the hub is available, broker restart/reconnection, stale-generation rejection, audio ordering/expiration, UTF-8 source highlights, radial geometry and named-part expansion/collapse behavior. The original viewer workspace tests and strict Clippy also passed during integration. Runtime evidence is retained locally under `target/gpui-rustel-validation`.
