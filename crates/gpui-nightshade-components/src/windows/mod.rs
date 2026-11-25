//! Unsafe D3D interop is confined to this module and GPUI's shared_texture importer.
use gpui_nightshade::wgpu;
use gpui_windows::shared_texture::{SharedHandle, SharedTexture};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};

struct Slot {
    texture: wgpu::Texture,
    fence: ID3D12Fence,
    texture_handle: Arc<SharedHandle>,
    fence_handle: Arc<SharedHandle>,
    leased: Weak<SharedTexture>,
    sampled: Arc<AtomicBool>,
    value: u64,
}
pub struct SharedRing {
    slots: Vec<Slot>,
    queue: ID3D12CommandQueue,
    luid: u64,
    width: u32,
    height: u32,
}
impl SharedRing {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if u64::from(width) * u64::from(height) * 4 * 3 > 256 * 1024 * 1024 {
            return Err("shared texture ring exceeds 256 MiB".into());
        }
        // SAFETY: guard borrows the wgpu device throughout native allocation. Resources are
        // imported back into this exact device, retaining their COM ownership in wgpu.
        let hal = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or("shared textures require DX12")?;
        let raw = hal.raw_device();
        let queue = hal.raw_queue().clone();
        let luid = unsafe { raw.GetAdapterLuid() };
        let luid = ((luid.HighPart as u32 as u64) << 32) | luid.LowPart as u64;
        let mut slots = Vec::new();
        for _ in 0..3 {
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Alignment: 0,
                Width: width as u64,
                Height: height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                Flags: D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS
                    | D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            };
            let mut resource = None;
            let resource: ID3D12Resource = unsafe {
                raw.CreateCommittedResource(
                    &D3D12_HEAP_PROPERTIES {
                        Type: D3D12_HEAP_TYPE_DEFAULT,
                        CreationNodeMask: 1,
                        VisibleNodeMask: 1,
                        ..Default::default()
                    },
                    D3D12_HEAP_FLAG_SHARED,
                    &desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut resource,
                )?;
                resource.ok_or("CreateCommittedResource returned no resource")?
            };
            let fence: ID3D12Fence = unsafe { raw.CreateFence(0, D3D12_FENCE_FLAG_SHARED)? };
            // NT handles are owned by SharedHandle and shared by frames only through Arc.
            let texture_handle = Arc::new(unsafe {
                SharedHandle::from_owned(raw.CreateSharedHandle(&resource, None, 0x10000000, None)?)
            });
            let fence_handle = Arc::new(unsafe {
                SharedHandle::from_owned(raw.CreateSharedHandle(&fence, None, 0x10000000, None)?)
            });
            let extent = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let hal_texture = unsafe {
                wgpu::hal::dx12::Device::texture_from_raw(
                    resource,
                    wgpu::TextureFormat::Bgra8Unorm,
                    wgpu::TextureDimension::D2,
                    extent,
                    1,
                    1,
                )
            };
            let texture = unsafe {
                device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
                    hal_texture,
                    &wgpu::TextureDescriptor {
                        label: Some("D3D12/D3D11 shared presentation"),
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Bgra8Unorm,
                        usage: wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    },
                )
            };
            slots.push(Slot {
                texture,
                fence,
                texture_handle,
                fence_handle,
                leased: Weak::new(),
                sampled: Arc::new(AtomicBool::new(false)),
                value: 0,
            });
        }
        Ok(Self {
            slots,
            queue,
            luid,
            width,
            height,
        })
    }

    pub fn check_device(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self
            .slots
            .iter()
            .any(|slot| unsafe { slot.fence.GetCompletedValue() == u64::MAX })
        {
            return Err("D3D12 device was removed; shared presentation stopped".into());
        }
        Ok(())
    }
    pub fn producer_idle(&self) -> bool {
        self.slots
            .iter()
            .all(|slot| unsafe { slot.fence.GetCompletedValue() >= slot.value.saturating_sub(1) })
    }
    pub fn can_submit(&self) -> bool {
        self.slots.iter().any(|slot| {
            let required = if slot.sampled.load(Ordering::Acquire) {
                slot.value
            } else {
                slot.value.saturating_sub(1)
            };
            slot.leased.strong_count() == 0 && unsafe { slot.fence.GetCompletedValue() >= required }
        })
    }
    pub fn copy_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::Texture,
    ) -> Result<Option<Arc<SharedTexture>>, Box<dyn std::error::Error>> {
        if source.format() != wgpu::TextureFormat::Bgra8Unorm
            || source.width() != self.width
            || source.height() != self.height
        {
            return Err("shared copy source is incompatible".into());
        }
        let Some(slot) = self.slots.iter_mut().find(|slot| {
            if slot.leased.strong_count() != 0 {
                return false;
            }
            let required = if slot.sampled.load(Ordering::Acquire) {
                slot.value
            } else {
                slot.value.saturating_sub(1)
            };
            // No CPU wait: unavailable slots provide backpressure.
            unsafe { slot.fence.GetCompletedValue() >= required }
        }) else {
            return Ok(None);
        };
        slot.sampled = Arc::new(AtomicBool::new(false));
        slot.value = slot
            .value
            .checked_add(2)
            .ok_or("shared fence timeline exhausted")?;
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            slot.texture.as_image_copy(),
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        // wgpu's DX12 backend maps PRESENT to COMMON. This explicit barrier makes
        // the texture legal to acquire from D3D11 and keeps wgpu's tracker coherent.
        encoder.transition_resources(
            std::iter::empty(),
            std::iter::once(wgpu::TextureTransition {
                texture: &slot.texture,
                selector: None,
                state: wgpu::TextureUses::PRESENT,
            }),
        );
        queue.submit([encoder.finish()]);
        unsafe {
            self.queue.Signal(&slot.fence, slot.value - 1)?;
        }
        let frame = Arc::new(SharedTexture {
            texture_handle: slot.texture_handle.clone(),
            fence_handle: slot.fence_handle.clone(),
            adapter_luid: self.luid,
            width: self.width,
            height: self.height,
            ready_value: slot.value - 1,
            done_value: slot.value,
            sampled: slot.sampled.clone(),
        });
        slot.leased = Arc::downgrade(&frame);
        Ok(Some(frame))
    }
}
