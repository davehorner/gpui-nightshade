//! Native Windows GPU interop is confined to this module and GPUI's importer.
mod vulkan;
use gpui_nightshade::{presentation::FrameId, wgpu};
use gpui_windows::shared_texture::{SharedFrameOwner, SharedHandle, SharedTexture};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicU64, Ordering},
};
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};

/// Same immutable-frame contract for the two native Windows producers.
pub(crate) enum PresentationRing {
    Dx12(SharedRing),
    Vulkan(vulkan::VulkanRing),
}
impl PresentationRing {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        match device.adapter_info().backend {
            wgpu::Backend::Vulkan => Ok(Self::Vulkan(vulkan::VulkanRing::new(
                device, width, height,
            )?)),
            wgpu::Backend::Dx12 => Ok(Self::Dx12(SharedRing::new(device, width, height)?)),
            _ => Err("Windows GPU sharing requires Vulkan or DX12".into()),
        }
    }
    pub fn check_device(&self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Self::Dx12(r) => r.check_device(),
            Self::Vulkan(r) => r.check_device(),
        }
    }
    pub fn producer_idle(&self) -> bool {
        match self {
            Self::Dx12(r) => r.producer_idle(),
            Self::Vulkan(r) => r.producer_idle(),
        }
    }
    pub fn can_submit(&self) -> bool {
        match self {
            Self::Dx12(r) => r.can_submit(),
            Self::Vulkan(r) => r.can_submit(),
        }
    }
    pub fn queue_frame(&mut self, frame: Arc<SharedTexture>, id: FrameId) {
        match self {
            Self::Dx12(r) => r.queue_frame(frame, id),
            Self::Vulkan(r) => r.queue_frame(frame, id),
        }
    }
    pub fn pending_count(&self) -> usize {
        match self {
            Self::Dx12(r) => r.pending_count(),
            Self::Vulkan(r) => r.pending_count(),
        }
    }
    pub fn retire(&mut self) {
        match self {
            Self::Dx12(r) => r.retire(),
            Self::Vulkan(r) => r.retire(),
        }
    }
    pub fn retired_idle(&self) -> bool {
        match self {
            Self::Dx12(r) => r.retired_idle(),
            Self::Vulkan(r) => r.retired_idle(),
        }
    }
    pub fn take_ready(&mut self) -> Option<(Arc<SharedTexture>, FrameId)> {
        match self {
            Self::Dx12(r) => r.take_ready(),
            Self::Vulkan(r) => r.take_ready(),
        }
    }
    pub fn copy_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &wgpu::Texture,
    ) -> Result<Option<Arc<SharedTexture>>, Box<dyn std::error::Error>> {
        match self {
            Self::Dx12(r) => r.copy_frame(device, queue, source),
            Self::Vulkan(r) => r.copy_frame(device, queue, source),
        }
    }
}

struct Slot {
    texture: wgpu::Texture,
    ready_fence: ID3D12Fence,
    consumed_fence: ID3D12Fence,
    texture_handle: Arc<SharedHandle>,
    ready_handle: Arc<SharedHandle>,
    consumed_handle: Arc<SharedHandle>,
    leased: Weak<SharedTexture>,
    last_read: Arc<AtomicU64>,
    value: u64,
}
#[derive(Debug)]
struct FrameResources {
    _texture: wgpu::Texture,
    _ready: ID3D12Fence,
    _consumed: ID3D12Fence,
}
impl SharedFrameOwner for FrameResources {}
pub struct SharedRing {
    slots: Vec<Slot>,
    queue: ID3D12CommandQueue,
    device: ID3D12Device,
    luid: u64,
    width: u32,
    height: u32,
    pending: Vec<(Arc<SharedTexture>, FrameId)>,
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
            let ready_fence: ID3D12Fence = unsafe { raw.CreateFence(0, D3D12_FENCE_FLAG_SHARED)? };
            let consumed_fence: ID3D12Fence =
                unsafe { raw.CreateFence(0, D3D12_FENCE_FLAG_SHARED)? };
            // NT handles are owned by SharedHandle and shared by frames only through Arc.
            let texture_handle = Arc::new(unsafe {
                SharedHandle::from_owned(raw.CreateSharedHandle(&resource, None, 0x10000000, None)?)
            });
            let ready_handle = Arc::new(unsafe {
                SharedHandle::from_owned(raw.CreateSharedHandle(
                    &ready_fence,
                    None,
                    0x10000000,
                    None,
                )?)
            });
            let consumed_handle = Arc::new(unsafe {
                SharedHandle::from_owned(raw.CreateSharedHandle(
                    &consumed_fence,
                    None,
                    0x10000000,
                    None,
                )?)
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
                ready_fence,
                consumed_fence,
                texture_handle,
                ready_handle,
                consumed_handle,
                leased: Weak::new(),
                last_read: Arc::new(AtomicU64::new(0)),
                value: 0,
            });
        }
        Ok(Self {
            slots,
            queue,
            device: raw.clone(),
            luid,
            width,
            height,
            pending: Vec::new(),
        })
    }

    pub fn check_device(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some((index, slot)) = self.slots.iter().enumerate().find(|(_, slot)| unsafe {
            slot.ready_fence.GetCompletedValue() == u64::MAX
                || slot.consumed_fence.GetCompletedValue() == u64::MAX
        }) {
            return Err(format!(
                "Shared fence abandoned: slot {index}, producer {:?}, consumer {:?}, ready {}, read {}, leases {}; D3D12 status: {:?}",
                unsafe { slot.ready_fence.GetCompletedValue() }, unsafe { slot.consumed_fence.GetCompletedValue() }, slot.value, slot.last_read.load(Ordering::Acquire), slot.leased.strong_count(), unsafe { self.device.GetDeviceRemovedReason() }
            )
            .into());
        }
        Ok(())
    }
    pub fn producer_idle(&self) -> bool {
        self.slots.iter().all(|slot| unsafe {
            completed_at_least(slot.ready_fence.GetCompletedValue(), slot.value)
        })
    }
    pub fn can_submit(&self) -> bool {
        self.slots.iter().any(Slot::available)
    }
    pub fn queue_frame(&mut self, frame: Arc<SharedTexture>, id: FrameId) {
        self.pending.push((frame, id));
        debug_assert!(self.pending.len() <= 3);
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub fn retire(&mut self) {
        self.pending.clear();
    }
    pub fn retired_idle(&self) -> bool {
        self.slots.iter().all(Slot::available)
    }
    /// Publish only completed copies. GPUI must not park its presentation queue
    /// behind a producer frame that is still rendering/compiling on another device.
    pub fn take_ready(&mut self) -> Option<(Arc<SharedTexture>, FrameId)> {
        let index = self.pending.iter().rposition(|(frame, _)| {
            self.slots.iter().any(|slot| {
                slot.texture_handle.raw() == frame.texture_handle.raw()
                    && unsafe {
                        let completed = slot.ready_fence.GetCompletedValue();
                        completed != u64::MAX && completed >= frame.ready_value
                    }
            })
        })?;
        self.pending.drain(..=index).next_back()
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
        let Some(slot) = self.slots.iter_mut().find(|slot| slot.available()) else {
            return Ok(None);
        };
        slot.value = slot
            .value
            .checked_add(1)
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
            self.queue.Signal(&slot.ready_fence, slot.value)?;
        }
        let frame = Arc::new(SharedTexture {
            producer_owner: Arc::new(FrameResources {
                _texture: slot.texture.clone(),
                _ready: slot.ready_fence.clone(),
                _consumed: slot.consumed_fence.clone(),
            }),
            texture_handle: slot.texture_handle.clone(),
            ready_fence_handle: slot.ready_handle.clone(),
            consumed_fence_handle: slot.consumed_handle.clone(),
            adapter_luid: self.luid,
            width: self.width,
            height: self.height,
            ready_value: slot.value,
            last_read: slot.last_read.clone(),
        });
        slot.leased = Arc::downgrade(&frame);
        Ok(Some(frame))
    }
}

impl Slot {
    fn available(&self) -> bool {
        // Leases stop future imports; the consumer timeline covers queued reads.
        self.leased.strong_count() == 0
            && unsafe {
                completed_at_least(self.ready_fence.GetCompletedValue(), self.value)
                    && completed_at_least(
                        self.consumed_fence.GetCompletedValue(),
                        self.last_read.load(Ordering::Acquire),
                    )
            }
    }
}

fn completed_at_least(completed: u64, required: u64) -> bool {
    completed != u64::MAX && completed >= required
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_nightshade::host::SceneHost;
    use std::time::{Duration, Instant};
    use windows::{
        Win32::{
            Foundation::{HMODULE, LUID},
            Graphics::{Direct3D::*, Direct3D11::*, Dxgi::*},
        },
        core::Interface,
    };

    fn wait(fence: &ID3D12Fence, value: u64) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let completed = unsafe { fence.GetCompletedValue() };
            assert_ne!(completed, u64::MAX, "device lost");
            if completed >= value {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "fence {completed} did not reach {value}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    #[ignore = "requires D3D12/D3D11 sharing on a native Windows GPU"]
    fn reuse_waits_for_last_consumer_read_and_timelines_do_not_rewind() {
        let host = pollster::block_on(SceneHost::cube_with_backends(64, 64, wgpu::Backends::DX12))
            .unwrap();
        let mut ring = SharedRing::new(&host.renderer.device, 64, 64).unwrap();
        let factory: IDXGIFactory4 =
            unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe {
            factory.EnumAdapterByLuid(LUID {
                LowPart: ring.luid as u32,
                HighPart: (ring.luid >> 32) as i32,
            })
        }
        .unwrap();
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .unwrap();
        let device: ID3D11Device5 = device.unwrap().cast().unwrap();
        let context: ID3D11DeviceContext4 = context.unwrap().cast().unwrap();
        let gate: ID3D12Fence =
            unsafe { ring.device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        unsafe { ring.queue.Wait(&gate, 1) }.unwrap();
        let frame = ring
            .copy_frame(
                &host.renderer.device,
                &host.renderer.queue,
                host.renderer.external_output.as_ref().unwrap(),
            )
            .unwrap()
            .unwrap();
        let id = FrameId {
            generation: 7,
            sequence: 42,
        };
        ring.queue_frame(frame.clone(), id);
        let withheld = ring.take_ready().is_none();
        // Always release the queue before asserting, so a test failure cannot strand it.
        unsafe { gate.Signal(1) }.unwrap();
        assert!(
            withheld,
            "an unfinished copy must not enter the GPUI compositor"
        );
        wait(&ring.slots[0].ready_fence, frame.ready_value);
        let published = ring.take_ready().unwrap();
        assert!(Arc::ptr_eq(&frame, &published.0));
        assert_eq!(published.1, id);
        drop(published);
        let mut ready = None;
        let mut consumed = None;
        let (consumer_texture, consumer_owner) = gpui_windows::shared_texture::import(
            &device.cast::<ID3D11Device>().unwrap(),
            &context.cast::<ID3D11DeviceContext>().unwrap(),
            frame.clone(),
        )
        .unwrap();
        unsafe {
            device
                .OpenSharedFence::<ID3D11Fence>(frame.ready_fence_handle.raw(), &mut ready)
                .unwrap();
            device
                .OpenSharedFence::<ID3D11Fence>(frame.consumed_fence_handle.raw(), &mut consumed)
                .unwrap();
            context
                .Wait(ready.as_ref().unwrap(), frame.ready_value)
                .unwrap();
            let texture = consumer_texture;
            let mut staging = None;
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
                        Usage: D3D11_USAGE_STAGING,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        ..Default::default()
                    },
                    None,
                    Some(&mut staging),
                )
                .unwrap();
            let staging = staging.unwrap();
            context.CopyResource(&staging, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .unwrap();
            let mut colors = std::collections::HashSet::new();
            for row in 0..64 {
                let bytes = std::slice::from_raw_parts(
                    mapped
                        .pData
                        .cast::<u8>()
                        .add(row * mapped.RowPitch as usize),
                    64 * 4,
                );
                for pixel in bytes.as_chunks::<4>().0 {
                    colors.insert([pixel[0], pixel[1], pixel[2]]);
                }
            }
            context.Unmap(&staging, 0);
            assert!(
                colors.len() > 8,
                "D3D11 must see the GPU-rendered cube pixels"
            );
            context.Signal(consumed.as_ref().unwrap(), 1).unwrap();
            context.Flush();
        }
        frame.last_read.store(1, Ordering::Release);
        wait(&ring.slots[0].consumed_fence, 1);
        drop(consumer_owner);
        // Reserve a repeated cached draw before releasing its CPU lease. The first
        // completed draw is insufficient to release this slot for another copy.
        frame.last_read.store(2, Ordering::Release);
        drop(frame);
        assert!(!ring.slots[0].available());
        assert!(
            !ring.retired_idle(),
            "ring retirement must include outstanding consumer work"
        );
        unsafe {
            context.Signal(consumed.as_ref().unwrap(), 2).unwrap();
            context.Flush();
        }
        wait(&ring.slots[0].consumed_fence, 2);
        assert!(ring.slots[0].available());
        assert!(ring.retired_idle());
        let next = ring
            .copy_frame(
                &host.renderer.device,
                &host.renderer.queue,
                host.renderer.external_output.as_ref().unwrap(),
            )
            .unwrap()
            .unwrap();
        wait(&ring.slots[0].ready_fence, next.ready_value);
        unsafe {
            context.Signal(consumed.as_ref().unwrap(), 3).unwrap();
            context.Flush();
        }
        wait(&ring.slots[0].consumed_fence, 3);
        assert_eq!(
            unsafe { ring.slots[0].ready_fence.GetCompletedValue() },
            next.ready_value
        );
        drop(ring);
        assert_ne!(
            unsafe { ready.as_ref().unwrap().GetCompletedValue() },
            u64::MAX,
            "a retained frame must keep producer fences alive after resize/ring retirement"
        );
        drop(next);
    }
}
