use etagere::BucketedAtlasAllocator;
use parking_lot::Mutex;
use windows::Win32::Graphics::{
    Direct3D11::{
        D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
        ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
    },
    Dxgi::Common::*,
};

use gpui::{
    AtlasBackend, AtlasKey, AtlasState, AtlasTextureId, AtlasTextureKind, AtlasTextureList,
    AtlasTile, Bounds, DevicePixels, PlatformAtlas, Point, Size,
};

pub(crate) struct DirectXAtlas(Mutex<AtlasState<DirectXAtlasTextures>>);
pub(crate) struct TextureBinding {
    pub(crate) views: [Option<ID3D11ShaderResourceView>; 1],
    pub(crate) external: Option<std::sync::Arc<crate::shared_texture::ImportedTexture>>,
}

struct DirectXAtlasTextures {
    device: ID3D11Device,
    device_context: ID3D11DeviceContext,
    monochrome_textures: AtlasTextureList<DirectXAtlasTexture>,
    polychrome_textures: AtlasTextureList<DirectXAtlasTexture>,
    subpixel_textures: AtlasTextureList<DirectXAtlasTexture>,
    retired_external: Vec<DirectXAtlasTexture>,
}

struct DirectXAtlasTexture {
    id: AtlasTextureId,
    bytes_per_pixel: u32,
    allocator: BucketedAtlasAllocator,
    texture: ID3D11Texture2D,
    view: [Option<ID3D11ShaderResourceView>; 1],
    live_atlas_keys: u32,
    external: Option<std::sync::Arc<crate::shared_texture::ImportedTexture>>,
}

impl DirectXAtlas {
    pub(crate) fn new(device: &ID3D11Device, device_context: &ID3D11DeviceContext) -> Self {
        DirectXAtlas(Mutex::new(AtlasState::new(DirectXAtlasTextures {
            device: device.clone(),
            device_context: device_context.clone(),
            monochrome_textures: Default::default(),
            polychrome_textures: Default::default(),
            subpixel_textures: Default::default(),
            retired_external: Vec::new(),
        })))
    }

    /// Returns the view backing `id`, or `None` once every tile in it has been
    /// removed. A scene can still reference such a texture when a cached view
    /// replays a paint from before the image was dropped, so callers must skip
    /// those sprites rather than assume the texture exists.
    pub(crate) fn get_texture_view(
        &self,
        id: AtlasTextureId,
    ) -> Option<[Option<ID3D11ShaderResourceView>; 1]> {
        let lock = self.0.lock();
        let texture = lock.backend.texture(id)?;
        Some(texture.view.clone())
    }

    /// Pin the view and its matching synchronization owner in one lookup.
    pub(crate) fn texture_binding(&self, id: AtlasTextureId) -> Option<TextureBinding> {
        let lock = self.0.lock();
        let texture = lock.backend.texture(id)?;
        Some(TextureBinding {
            views: texture.view.clone(),
            external: texture.external.clone(),
        })
    }
    pub(crate) fn collect_retired(&self) {
        self.0.lock().backend.collect_retired();
    }
    pub(crate) fn handle_device_lost(
        &self,
        device: &ID3D11Device,
        device_context: &ID3D11DeviceContext,
    ) {
        let mut lock = self.0.lock();
        lock.clear(|textures| {
            textures.device = device.clone();
            textures.device_context = device_context.clone();
            textures.monochrome_textures = AtlasTextureList::default();
            textures.polychrome_textures = AtlasTextureList::default();
            textures.subpixel_textures = AtlasTextureList::default();
            textures.retired_external.clear();
        });
    }
}

impl PlatformAtlas for DirectXAtlas {
    fn get_or_insert_external(
        &self,
        key: AtlasKey,
        owner: std::sync::Arc<dyn gpui::ExternalTexture>,
    ) -> anyhow::Result<AtlasTile> {
        self.0.lock().get_or_insert_tile(key, |backend| {
            backend.collect_retired();
            let size = owner.size();
            let (texture, imported) =
                crate::shared_texture::import(&backend.device, &backend.device_context, owner)?;
            let mut view = None;
            unsafe {
                backend
                    .device
                    .CreateShaderResourceView(&texture, None, Some(&mut view))
                    .map_err(|e| anyhow::anyhow!("CreateShaderResourceView shared: {e}"))?;
            }
            let list = &mut backend.polychrome_textures;
            let index = list.free_list.pop().unwrap_or(list.textures.len());
            let mut entry = DirectXAtlasTexture {
                id: AtlasTextureId {
                    index: index as u32,
                    kind: AtlasTextureKind::Polychrome,
                },
                bytes_per_pixel: 4,
                allocator: BucketedAtlasAllocator::new(device_size_to_etagere(size)),
                texture,
                view: [view],
                live_atlas_keys: 0,
                external: Some(std::sync::Arc::new(imported)),
            };
            let tile = entry
                .allocate(size)
                .ok_or_else(|| anyhow::anyhow!("external texture tile allocation failed"))?;
            if index == list.textures.len() {
                list.textures.push(Some(entry));
            } else {
                list.textures[index] = Some(entry);
            }
            Ok(tile)
        })
    }

    fn get_or_insert_with<'a>(
        &self,
        key: AtlasKey,
        build: &mut dyn FnMut() -> anyhow::Result<
            Option<(Size<DevicePixels>, std::borrow::Cow<'a, [u8]>)>,
        >,
    ) -> anyhow::Result<Option<AtlasTile>> {
        self.0.lock().get_or_insert_with(key, build)
    }

    fn remove(&self, key: &AtlasKey) {
        self.0.lock().remove(key);
    }
}

impl AtlasBackend for DirectXAtlasTextures {
    fn insert(
        &mut self,
        kind: AtlasTextureKind,
        size: Size<DevicePixels>,
        bytes: &[u8],
    ) -> anyhow::Result<AtlasTile> {
        let tile = self
            .allocate(size, kind)
            .ok_or_else(|| anyhow::anyhow!("failed to allocate"))?;
        let texture = self
            .texture(tile.texture_id)
            .ok_or_else(|| anyhow::anyhow!("allocated tile refers to a missing texture"))?;
        texture.upload(&self.device_context, tile.bounds, bytes);
        Ok(tile)
    }

    fn remove(&mut self, tile: AtlasTile) {
        let id = tile.texture_id;

        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
            AtlasTextureKind::Subpixel => &mut self.subpixel_textures,
        };

        let Some(texture_slot) = textures.textures.get_mut(id.index as usize) else {
            return;
        };

        if let Some(mut texture) = texture_slot.take() {
            texture.allocator.deallocate(tile.tile_id.into());
            texture.decrement_ref_count();
            if texture.is_unreferenced() {
                textures.free_list.push(texture.id.index as usize);
                if texture
                    .external
                    .as_ref()
                    .is_some_and(|owner| !owner.is_idle())
                {
                    self.retired_external.push(texture);
                }
            } else {
                *texture_slot = Some(texture);
            }
        }
    }
}

impl DirectXAtlasTextures {
    fn collect_retired(&mut self) {
        // Retain the producer frame lease as well as native D3D11 objects.
        // The producer's bounded ring then applies backpressure until retirement.
        self.retired_external.retain(|texture| {
            texture
                .external
                .as_ref()
                .is_some_and(|owner| !owner.is_idle())
        });
    }
    fn allocate(
        &mut self,
        size: Size<DevicePixels>,
        texture_kind: AtlasTextureKind,
    ) -> Option<AtlasTile> {
        {
            let textures = match texture_kind {
                AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
                AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
                AtlasTextureKind::Subpixel => &mut self.subpixel_textures,
            };

            if let Some(tile) = textures
                .iter_mut()
                .rev()
                .find_map(|texture| texture.allocate(size))
            {
                return Some(tile);
            }
        }

        let texture = self.push_texture(size, texture_kind)?;
        texture.allocate(size)
    }

    fn push_texture(
        &mut self,
        min_size: Size<DevicePixels>,
        kind: AtlasTextureKind,
    ) -> Option<&mut DirectXAtlasTexture> {
        const DEFAULT_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(1024),
            height: DevicePixels(1024),
        };
        // Max texture size for DirectX. See:
        // https://learn.microsoft.com/en-us/windows/win32/direct3d11/overviews-direct3d-11-resources-limits
        const MAX_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(16384),
            height: DevicePixels(16384),
        };
        let size = min_size.min(&MAX_ATLAS_SIZE).max(&DEFAULT_ATLAS_SIZE);
        let pixel_format;
        let bind_flag;
        let bytes_per_pixel;
        match kind {
            AtlasTextureKind::Monochrome => {
                pixel_format = DXGI_FORMAT_R8_UNORM;
                bind_flag = D3D11_BIND_SHADER_RESOURCE;
                bytes_per_pixel = 1;
            }
            AtlasTextureKind::Polychrome => {
                pixel_format = DXGI_FORMAT_B8G8R8A8_UNORM;
                bind_flag = D3D11_BIND_SHADER_RESOURCE;
                bytes_per_pixel = 4;
            }
            AtlasTextureKind::Subpixel => {
                pixel_format = DXGI_FORMAT_R8G8B8A8_UNORM;
                bind_flag = D3D11_BIND_SHADER_RESOURCE;
                bytes_per_pixel = 4;
            }
        }
        let texture_desc = D3D11_TEXTURE2D_DESC {
            Width: size.width.0 as u32,
            Height: size.height.0 as u32,
            MipLevels: 1,
            ArraySize: 1,
            Format: pixel_format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bind_flag.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            // This only returns None if the device is lost, which we will recreate later.
            // So it's ok to return None here.
            self.device
                .CreateTexture2D(&texture_desc, None, Some(&mut texture))
                .ok()?;
        }
        let texture = texture.unwrap();

        let texture_list = match kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
            AtlasTextureKind::Subpixel => &mut self.subpixel_textures,
        };
        let index = texture_list.free_list.pop();
        let view = unsafe {
            let mut view = None;
            self.device
                .CreateShaderResourceView(&texture, None, Some(&mut view))
                .ok()?;
            [view]
        };
        let atlas_texture = DirectXAtlasTexture {
            id: AtlasTextureId {
                index: index.unwrap_or(texture_list.textures.len()) as u32,
                kind,
            },
            bytes_per_pixel,
            allocator: etagere::BucketedAtlasAllocator::new(device_size_to_etagere(size)),
            texture,
            view,
            live_atlas_keys: 0,
            external: None,
        };
        if let Some(ix) = index {
            texture_list.textures[ix] = Some(atlas_texture);
            texture_list.textures.get_mut(ix).unwrap().as_mut()
        } else {
            texture_list.textures.push(Some(atlas_texture));
            texture_list.textures.last_mut().unwrap().as_mut()
        }
    }

    fn texture(&self, id: AtlasTextureId) -> Option<&DirectXAtlasTexture> {
        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &self.monochrome_textures,
            AtlasTextureKind::Polychrome => &self.polychrome_textures,
            AtlasTextureKind::Subpixel => &self.subpixel_textures,
        };
        textures.textures.get(id.index as usize)?.as_ref()
    }
}

impl DirectXAtlasTexture {
    fn allocate(&mut self, size: Size<DevicePixels>) -> Option<AtlasTile> {
        let allocation = self.allocator.allocate(device_size_to_etagere(size))?;
        let tile = AtlasTile {
            texture_id: self.id,
            tile_id: allocation.id.into(),
            bounds: Bounds {
                origin: etagere_point_to_device(allocation.rectangle.min),
                size,
            },
            padding: 0,
        };
        self.live_atlas_keys += 1;
        Some(tile)
    }

    fn upload(
        &self,
        device_context: &ID3D11DeviceContext,
        bounds: Bounds<DevicePixels>,
        bytes: &[u8],
    ) {
        // `UpdateSubresource` reads `row_pitch * height` bytes from `bytes` based on the
        // `D3D11_BOX` below. If the caller hands us a slice shorter than that, the driver would
        // over-read past the end of the source buffer (potentially by multiple megabytes), so bail
        // out instead. This is a first-insert path rather than a per-frame one, so the check is
        // effectively free.
        let row_bytes = bounds.size.width.to_bytes(self.bytes_per_pixel as u8) as usize;
        let expected = row_bytes * bounds.size.height.0.max(0) as usize;
        if bytes.len() < expected {
            log::error!(
                "DirectXAtlasTexture::upload: source slice is {} bytes but the {}x{} region \
                 requires {} bytes; skipping upload to avoid a driver over-read",
                bytes.len(),
                bounds.size.width.0,
                bounds.size.height.0,
                expected,
            );
            return;
        }
        unsafe {
            device_context.UpdateSubresource(
                &self.texture,
                0,
                Some(&D3D11_BOX {
                    left: bounds.left().0 as u32,
                    top: bounds.top().0 as u32,
                    front: 0,
                    right: bounds.right().0 as u32,
                    bottom: bounds.bottom().0 as u32,
                    back: 1,
                }),
                bytes.as_ptr() as _,
                bounds.size.width.to_bytes(self.bytes_per_pixel as u8),
                0,
            );
        }
    }

    fn decrement_ref_count(&mut self) {
        self.live_atlas_keys -= 1;
    }

    fn is_unreferenced(&mut self) -> bool {
        self.live_atlas_keys == 0
    }
}

fn device_size_to_etagere(size: Size<DevicePixels>) -> etagere::Size {
    etagere::Size::new(size.width.into(), size.height.into())
}

fn etagere_point_to_device(value: etagere::Point) -> Point<DevicePixels> {
    Point {
        x: DevicePixels::from(value.x),
        y: DevicePixels::from(value.y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{ImageId, RenderImageParams};
    use std::borrow::Cow;
    use windows::Win32::{
        Foundation::HMODULE,
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_WARP,
            Direct3D11::{D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice},
        },
    };

    fn create_atlas() -> Option<DirectXAtlas> {
        let mut device: Option<ID3D11Device> = None;
        let mut device_context: Option<ID3D11DeviceContext> = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut device_context),
            )
        }
        .ok()?;
        Some(DirectXAtlas::new(&device?, &device_context?))
    }

    fn make_image_key(image_id: usize) -> AtlasKey {
        AtlasKey::Image(RenderImageParams {
            image_id: ImageId(image_id),
            frame_index: 0,
        })
    }

    fn insert_tile(atlas: &DirectXAtlas, key: AtlasKey, size: Size<DevicePixels>) -> AtlasTile {
        atlas
            .get_or_insert_with(key, &mut || {
                let byte_count = (size.width.0 as usize) * (size.height.0 as usize) * 4;
                Ok(Some((size, Cow::Owned(vec![0u8; byte_count]))))
            })
            .expect("allocation should succeed")
            .expect("callback returns Some")
    }

    #[test]
    fn test_remove_deallocates_tile_space_for_reuse() {
        let Some(atlas) = create_atlas() else {
            return;
        };

        let small = Size {
            width: DevicePixels(64),
            height: DevicePixels(64),
        };
        let big = Size {
            width: DevicePixels(700),
            height: DevicePixels(700),
        };

        let keeper_key = make_image_key(1);
        let big_key_a = make_image_key(2);
        let big_key_b = make_image_key(3);

        let keeper_tile = insert_tile(&atlas, keeper_key, small);
        let tile_a = insert_tile(&atlas, big_key_a.clone(), big);
        assert_eq!(keeper_tile.texture_id, tile_a.texture_id);

        atlas.remove(&big_key_a);

        let tile_b = insert_tile(&atlas, big_key_b, big);
        assert_eq!(tile_b.texture_id, keeper_tile.texture_id);
    }
    #[test]
    #[ignore = "requires native Windows shared D3D11 fences"]
    fn external_eviction_retains_import_until_consumer_completion() {
        use crate::shared_texture::{SharedFrameOwner, SharedHandle, SharedTexture};
        use std::sync::{Arc, atomic::AtomicU64};
        use windows::{
            Win32::Graphics::{Direct3D11::*, Dxgi::*},
            core::{Interface, PCWSTR},
        };
        #[derive(Debug)]
        struct ProducerOwner;
        impl SharedFrameOwner for ProducerOwner {}
        struct ReleaseGate(ID3D11DeviceContext4, ID3D11Fence);
        impl Drop for ReleaseGate {
            fn drop(&mut self) {
                unsafe {
                    let _ = self.0.Signal(&self.1, 1);
                    self.0.Flush();
                }
            }
        }
        let atlas = create_atlas().expect("WARP device required");
        let other = create_atlas().expect("second WARP device required");
        let (device, context) = {
            let lock = atlas.0.lock();
            (
                lock.backend.device.clone(),
                lock.backend
                    .device_context
                    .cast::<ID3D11DeviceContext4>()
                    .unwrap(),
            )
        };
        let (other_device, other_context) = {
            let lock = other.0.lock();
            (
                lock.backend.device.clone(),
                lock.backend
                    .device_context
                    .cast::<ID3D11DeviceContext4>()
                    .unwrap(),
            )
        };
        unsafe {
            let device5: ID3D11Device5 = device.cast().unwrap();
            let mut ready: Option<ID3D11Fence> = None;
            let mut consumed: Option<ID3D11Fence> = None;
            let mut gate: Option<ID3D11Fence> = None;
            device5
                .CreateFence(1, D3D11_FENCE_FLAG_SHARED, &mut ready)
                .unwrap();
            device5
                .CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut consumed)
                .unwrap();
            device5
                .CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut gate)
                .unwrap();
            let (ready, consumed, gate) = (ready.unwrap(), consumed.unwrap(), gate.unwrap());
            let gate_handle = SharedHandle::from_owned(
                gate.CreateSharedHandle(None, 0x10000000, PCWSTR::null())
                    .unwrap(),
            );
            let mut other_gate = None;
            other_device
                .cast::<ID3D11Device5>()
                .unwrap()
                .OpenSharedFence::<ID3D11Fence>(gate_handle.raw(), &mut other_gate)
                .unwrap();
            let release = ReleaseGate(other_context, other_gate.unwrap());
            let mut texture = None;
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: 64,
                        Height: 64,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                        MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE
                            | D3D11_RESOURCE_MISC_SHARED)
                            .0 as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut texture),
                )
                .unwrap();
            let texture = texture.unwrap();
            let resource: IDXGIResource1 = texture.cast().unwrap();
            let dxgi: IDXGIDevice = device.cast().unwrap();
            let luid = dxgi.GetAdapter().unwrap().GetDesc().unwrap().AdapterLuid;
            let producer = Arc::new(ProducerOwner);
            let retained = Arc::downgrade(&producer);
            let frame = Arc::new(SharedTexture {
                producer_owner: producer,
                texture_handle: Arc::new(SharedHandle::from_owned(
                    resource
                        .CreateSharedHandle(
                            None,
                            DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0,
                            PCWSTR::null(),
                        )
                        .unwrap(),
                )),
                ready_fence_handle: Arc::new(SharedHandle::from_owned(
                    ready
                        .CreateSharedHandle(None, 0x10000000, PCWSTR::null())
                        .unwrap(),
                )),
                consumed_fence_handle: Arc::new(SharedHandle::from_owned(
                    consumed
                        .CreateSharedHandle(None, 0x10000000, PCWSTR::null())
                        .unwrap(),
                )),
                adapter_luid: ((luid.HighPart as u32 as u64) << 32) | luid.LowPart as u64,
                width: 64,
                height: 64,
                ready_value: 1,
                last_read: Arc::new(AtomicU64::new(0)),
            });
            let key = make_image_key(991);
            let tile = atlas
                .get_or_insert_external(key.clone(), frame.clone())
                .unwrap();
            let binding = atlas.texture_binding(tile.texture_id).unwrap();
            context.Wait(&gate, 1).unwrap();
            binding.external.as_ref().unwrap().before_draw().unwrap();
            // The queued completion is blocked, just as a queued draw would be.
            binding.external.as_ref().unwrap().after_draw().unwrap();
            drop(binding);
            drop(frame);
            atlas.remove(&key);
            assert_eq!(atlas.0.lock().backend.retired_external.len(), 1);
            assert!(
                retained.upgrade().is_some(),
                "atlas eviction must retain producer lease"
            );
            assert!(
                atlas.texture_binding(tile.texture_id).is_none(),
                "retired texture must not be drawable"
            );
            drop(release);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                let completed = consumed.GetCompletedValue();
                assert_ne!(completed, u64::MAX, "consumer device lost");
                if completed >= 1 {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "consumer completion timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            atlas.0.lock().backend.collect_retired();
            assert!(atlas.0.lock().backend.retired_external.is_empty());
            assert!(
                retained.upgrade().is_none(),
                "completed eviction must release producer lease"
            );
        }
    }
}
