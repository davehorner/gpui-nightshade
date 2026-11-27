# Compatible GPUI source export

These are the published `gpui-pre`, `gpui-pre-windows` and `gpui-pre-wgpu` 0.3.14 source exports, derived from Zed commit `f126950f9d3b62666f97437ae5d4ccac2272c4db`. They are the exact snapshot family pinned by gpui-component revision `82f1c1dc1f2970d93f1adfe10cbd772516c21ce7`.

The local changes are described by `../../patches/gpui/0001-shared-external-images.patch`. They are already applied. The full Apache-2.0 licenses are retained in each package. Normalized published manifests intentionally reference the compatible registry snapshot family, rather than the independent Zed trunk checkout.

The WGPU export retains empty atlas textures on wasm32 so replacing video/readback
image keys reuses allocations instead of creating a new GPU texture per frame.
Native atlas eviction is unchanged. The published Apache-2.0 license is retained.
