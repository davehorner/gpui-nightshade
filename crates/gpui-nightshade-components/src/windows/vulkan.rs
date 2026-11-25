//! Vulkan copies into shared D3D allocations; no pixel staging or CPU mapping.
use super::completed_at_least;
use ash::vk;
use gpui_nightshade::{presentation::FrameId, wgpu};
use gpui_windows::shared_texture::{SharedFrameOwner, SharedHandle, SharedTexture};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use windows::Win32::{
    Foundation::LUID,
    Graphics::{
        Direct3D::D3D_FEATURE_LEVEL_11_0,
        Direct3D12::*,
        Dxgi::{Common::*, *},
    },
};

struct Allocation {
    // Keep the owning Vulkan device alive until native objects are destroyed.
    _device: wgpu::Device,
    raw: ash::Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
    _resource: ID3D12Resource,
}

impl std::fmt::Debug for Allocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VulkanSharedAllocation")
            .field("image", &self.image)
            .finish_non_exhaustive()
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        // The wgpu completion callback and immutable frame leases retain this
        // allocation until both the producer copy and every consumer use finish.
        unsafe {
            self.raw.destroy_image(self.image, None);
            self.raw.free_memory(self.memory, None);
        }
    }
}

#[derive(Debug)]
struct Owner {
    _allocation: Arc<Allocation>,
    // Raw copy commands are not tracked by wgpu; retain their source explicitly.
    _source: wgpu::Texture,
    _ready: ID3D12Fence,
    _consumed: ID3D12Fence,
}
impl SharedFrameOwner for Owner {}

struct Slot {
    allocation: Arc<Allocation>,
    ready: ID3D12Fence,
    consumed: ID3D12Fence,
    texture_handle: Arc<SharedHandle>,
    ready_handle: Arc<SharedHandle>,
    consumed_handle: Arc<SharedHandle>,
    last_read: Arc<AtomicU64>,
    leased: Weak<SharedTexture>,
    value: u64,
    published_value: u64,
    complete: Arc<AtomicBool>,
}
impl Slot {
    fn copy_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }
    fn available(&self) -> bool {
        self.leased.strong_count() == 0
            && self.copy_complete()
            && unsafe {
                completed_at_least(
                    self.consumed.GetCompletedValue(),
                    self.last_read.load(Ordering::Acquire),
                )
            }
    }
}

pub(crate) struct VulkanRing {
    slots: Vec<Slot>,
    device: ID3D12Device,
    family: u32,
    luid: u64,
    width: u32,
    height: u32,
    pending: Vec<(Arc<SharedTexture>, FrameId)>,
    failure: Option<String>,
}

impl Drop for VulkanRing {
    fn drop(&mut self) {
        // Worker-owned ring teardown drains pending callbacks before its owning
        // device disappears. Published frame owners only perform nonblocking frees.
        if !self.producer_idle()
            && let Some(slot) = self.slots.first()
        {
            let _ = slot
                .allocation
                ._device
                .poll(wgpu::PollType::wait_indefinitely());
        }
    }
}

impl VulkanRing {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|n| n.checked_mul(12));
        if width == 0 || height == 0 || bytes.is_none_or(|n| n > 256 * 1024 * 1024) {
            return Err("invalid Vulkan shared ring extent or memory budget".into());
        }
        // SAFETY: creation and submission are confined to the owning render worker.
        let hal = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }
            .ok_or("Vulkan sharing requires a Vulkan device")?;
        if !hal
            .enabled_device_extensions()
            .contains(&ash::khr::external_memory_win32::NAME)
        {
            return Err("VK_KHR_external_memory_win32 is unavailable".into());
        }
        let instance = hal.shared_instance().raw_instance();
        let physical = hal.raw_physical_device();
        let mut ids = vk::PhysicalDeviceIDProperties::default();
        let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut ids);
        unsafe { instance.get_physical_device_properties2(physical, &mut properties) };
        if ids.device_luid_valid == 0 {
            return Err("Vulkan adapter has no valid Windows LUID".into());
        }
        // GPUI's D3D11 device targets the adapter's primary node. A matching
        // group LUID alone is insufficient for another node in a linked adapter.
        if ids.device_node_mask != 1 {
            return Err(
                "Vulkan sharing requires the primary node of the matching Windows adapter".into(),
            );
        }
        let luid = u64::from_le_bytes(ids.device_luid);
        let mut external = vk::PhysicalDeviceExternalImageFormatInfo::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE);
        let info = vk::PhysicalDeviceImageFormatInfo2::default()
            .format(vk::Format::B8G8R8A8_UNORM)
            .ty(vk::ImageType::TYPE_2D)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::TRANSFER_DST)
            .push_next(&mut external);
        let mut external_properties = vk::ExternalImageFormatProperties::default();
        let mut image_properties =
            vk::ImageFormatProperties2::default().push_next(&mut external_properties);
        unsafe {
            instance.get_physical_device_image_format_properties2(
                physical,
                &info,
                &mut image_properties,
            )
        }?;
        let limits = image_properties.image_format_properties;
        if width > limits.max_extent.width
            || height > limits.max_extent.height
            || !limits.sample_counts.contains(vk::SampleCountFlags::TYPE_1)
        {
            return Err("Vulkan external image format does not support this output extent".into());
        }
        if !external_properties
            .external_memory_properties
            .external_memory_features
            .contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE)
        {
            return Err("Vulkan adapter cannot import shared D3D12 BGRA textures".into());
        }
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }?;
        let adapter: IDXGIAdapter = unsafe {
            factory.EnumAdapterByLuid(LUID {
                LowPart: luid as u32,
                HighPart: (luid >> 32) as i32,
            })
        }?;
        let mut dx_device = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut dx_device) }?;
        let dx_device: ID3D12Device =
            dx_device.ok_or("D3D12 fence/resource device creation returned no device")?;
        let raw = hal.raw_device();
        let external_memory = ash::khr::external_memory_win32::Device::new(instance, raw);
        let memory_properties = unsafe { instance.get_physical_device_memory_properties(physical) };
        let mut slots = Vec::new();
        for _ in 0..3 {
            let resource_desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
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
                ..Default::default()
            };
            let mut resource = None;
            unsafe {
                dx_device.CreateCommittedResource(
                    &D3D12_HEAP_PROPERTIES {
                        Type: D3D12_HEAP_TYPE_DEFAULT,
                        CreationNodeMask: 1,
                        VisibleNodeMask: 1,
                        ..Default::default()
                    },
                    D3D12_HEAP_FLAG_SHARED,
                    &resource_desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut resource,
                )
            }?;
            let resource: ID3D12Resource =
                resource.ok_or("shared resource allocation returned no resource")?;
            let ready: ID3D12Fence = unsafe { dx_device.CreateFence(0, D3D12_FENCE_FLAG_SHARED) }?;
            let consumed: ID3D12Fence =
                unsafe { dx_device.CreateFence(0, D3D12_FENCE_FLAG_SHARED) }?;
            let texture_handle = Arc::new(unsafe {
                SharedHandle::from_owned(
                    dx_device.CreateSharedHandle(&resource, None, 0x10000000, None)?,
                )
            });
            let ready_handle = Arc::new(unsafe {
                SharedHandle::from_owned(
                    dx_device.CreateSharedHandle(&ready, None, 0x10000000, None)?,
                )
            });
            let consumed_handle = Arc::new(unsafe {
                SharedHandle::from_owned(
                    dx_device.CreateSharedHandle(&consumed, None, 0x10000000, None)?,
                )
            });
            let mut external_info = vk::ExternalMemoryImageCreateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE);
            let image_info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::B8G8R8A8_UNORM)
                .extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::TRANSFER_DST)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .push_next(&mut external_info);
            let image = unsafe { raw.create_image(&image_info, None) }?;
            // Complete partial-allocation cleanup before returning any driver error.
            let imported = (|| -> Result<_, Box<dyn std::error::Error>> {
                let requirements = unsafe { raw.get_image_memory_requirements(image) };
                let mut handle_properties = vk::MemoryWin32HandlePropertiesKHR::default();
                unsafe {
                    external_memory.get_memory_win32_handle_properties(
                        vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE,
                        texture_handle.raw().0 as _,
                        &mut handle_properties,
                    )
                }?;
                let bits = requirements.memory_type_bits & handle_properties.memory_type_bits;
                let index = (0..memory_properties.memory_type_count)
                    .find(|i| {
                        bits & (1 << i) != 0
                            && memory_properties.memory_types[*i as usize]
                                .property_flags
                                .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                    })
                    .ok_or("no compatible Vulkan device-local memory type")?;
                let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
                let mut import = vk::ImportMemoryWin32HandleInfoKHR::default()
                    .handle_type(vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE)
                    .handle(texture_handle.raw().0 as _);
                let allocation = vk::MemoryAllocateInfo::default()
                    .allocation_size(requirements.size)
                    .memory_type_index(index)
                    .push_next(&mut dedicated)
                    .push_next(&mut import);
                Ok(unsafe { raw.allocate_memory(&allocation, None) }?)
            })();
            let memory = match imported {
                Ok(memory) => memory,
                Err(error) => {
                    unsafe { raw.destroy_image(image, None) };
                    return Err(error);
                }
            };
            if let Err(error) = unsafe { raw.bind_image_memory(image, memory, 0) } {
                unsafe {
                    raw.destroy_image(image, None);
                    raw.free_memory(memory, None)
                };
                return Err(error.into());
            }
            slots.push(Slot {
                allocation: Arc::new(Allocation {
                    _device: device.clone(),
                    raw: raw.clone(),
                    image,
                    memory,
                    _resource: resource,
                }),
                ready,
                consumed,
                texture_handle,
                ready_handle,
                consumed_handle,
                last_read: Arc::new(AtomicU64::new(0)),
                leased: Weak::new(),
                value: 0,
                published_value: 0,
                complete: Arc::new(AtomicBool::new(true)),
            });
        }
        Ok(Self {
            slots,
            device: dx_device,
            family: hal.queue_family_index(),
            luid,
            width,
            height,
            pending: Vec::new(),
            failure: None,
        })
    }

    pub fn check_device(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(error) = &self.failure {
            return Err(error.clone().into());
        }
        for slot in &self.slots {
            if unsafe {
                slot.ready.GetCompletedValue() == u64::MAX
                    || slot.consumed.GetCompletedValue() == u64::MAX
            } {
                return Err(format!("Vulkan shared fence device loss: {:?}", unsafe {
                    self.device.GetDeviceRemovedReason()
                })
                .into());
            }
        }
        Ok(())
    }
    pub fn producer_idle(&self) -> bool {
        self.slots.iter().all(|s| s.copy_complete())
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
    pub fn take_ready(&mut self) -> Option<(Arc<SharedTexture>, FrameId)> {
        for slot in &mut self.slots {
            if slot.value != slot.published_value && slot.copy_complete() {
                // CPU fence relay only after the actual Vulkan copy/release completes.
                // Pixel bytes never leave device memory; no UI-thread wait is used.
                match unsafe { slot.ready.Signal(slot.value) } {
                    Ok(()) => slot.published_value = slot.value,
                    Err(error) => {
                        self.failure = Some(format!(
                            "Vulkan shared producer fence signal failed: {error}"
                        ));
                        return None;
                    }
                }
            }
        }
        let index = self.pending.iter().rposition(|(f, _)| {
            self.slots.iter().any(|s| {
                s.texture_handle.raw() == f.texture_handle.raw()
                    && s.published_value >= f.ready_value
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
            return Err("Vulkan shared copy source is incompatible".into());
        }
        let Some(slot) = self.slots.iter_mut().find(|s| s.available()) else {
            return Ok(None);
        };
        let value = slot
            .value
            .checked_add(1)
            .ok_or("Vulkan shared timeline exhausted")?;
        let mut transition = device.create_command_encoder(&Default::default());
        transition.transition_resources(
            std::iter::empty(),
            std::iter::once(wgpu::TextureTransition {
                texture: source,
                selector: None,
                state: wgpu::TextureUses::COPY_SRC,
            }),
        );
        let transition = transition.finish();
        // wgpu 29 separates high-level and HAL encoding per command buffer.
        // Submit both buffers together to keep the tracked transition before the raw copy.
        let mut encoder = device.create_command_encoder(&Default::default());
        let source_hal = unsafe { source.as_hal::<wgpu::hal::api::Vulkan>() }
            .ok_or("copy source is not Vulkan")?;
        let a = &slot.allocation;
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        // SAFETY: wgpu owns the command buffer and its queue synchronization.
        // Record native barriers only inside its HAL callback; the source stays
        // COPY_SRC in wgpu's tracker and the imported target has external ownership.
        unsafe {
            encoder.as_hal_mut::<wgpu::hal::api::Vulkan, _, _>(
                |hal| -> Result<(), Box<dyn std::error::Error>> {
                    let command = hal.ok_or("copy encoder is not Vulkan")?.raw_handle();
                    // Entire allocation is overwritten. Discard the previous external contents
                    // while acquiring ownership; D3D11 reads are already fenced and unleased.
                    let acquire = vk::ImageMemoryBarrier::default()
                        .old_layout(vk::ImageLayout::UNDEFINED)
                        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .src_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                        .dst_queue_family_index(self.family)
                        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .image(a.image)
                        .subresource_range(range);
                    a.raw.cmd_pipeline_barrier(
                        command,
                        vk::PipelineStageFlags::TOP_OF_PIPE,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[acquire],
                    );
                    let layers = vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1);
                    let copy = vk::ImageCopy::default()
                        .src_subresource(layers)
                        .dst_subresource(layers)
                        .extent(vk::Extent3D {
                            width: self.width,
                            height: self.height,
                            depth: 1,
                        });
                    a.raw.cmd_copy_image(
                        command,
                        source_hal.raw_handle(),
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        a.image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[copy],
                    );
                    let release = vk::ImageMemoryBarrier::default()
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::GENERAL)
                        .src_queue_family_index(self.family)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .image(a.image)
                        .subresource_range(range);
                    a.raw.cmd_pipeline_barrier(
                        command,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[release],
                    );
                    Ok(())
                },
            )?;
        }
        slot.value = value;
        let owner = Arc::new(Owner {
            _allocation: slot.allocation.clone(),
            _source: source.clone(),
            _ready: slot.ready.clone(),
            _consumed: slot.consumed.clone(),
        });
        slot.complete.store(false, Ordering::Release);
        drop(source_hal);
        queue.submit([transition, encoder.finish()]);
        let complete = slot.complete.clone();
        let pending_owner = owner.clone();
        queue.on_submitted_work_done(move || {
            // Retain raw imported memory/source through actual GPU completion,
            // even if the worker or an unpublished frame is dropped meanwhile.
            drop(pending_owner);
            complete.store(true, Ordering::Release);
        });
        let frame = Arc::new(SharedTexture {
            producer_owner: owner,
            texture_handle: slot.texture_handle.clone(),
            ready_fence_handle: slot.ready_handle.clone(),
            consumed_fence_handle: slot.consumed_handle.clone(),
            adapter_luid: self.luid,
            width: self.width,
            height: self.height,
            ready_value: value,
            last_read: slot.last_read.clone(),
        });
        slot.leased = Arc::downgrade(&frame);
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_nightshade::{
        host::SceneHost,
        presentation::{AlphaMode, readback::ReadbackRing},
    };
    use std::time::{Duration, Instant};
    use windows::{
        Win32::{
            Foundation::HMODULE,
            Graphics::{Direct3D::*, Direct3D11::*},
        },
        core::Interface,
    };

    #[test]
    #[ignore = "requires Vulkan/D3D11 external memory on a native Windows GPU"]
    fn vulkan_shared_pixels_match_and_leases_gate_slot_reuse() {
        let mut host = pollster::block_on(SceneHost::cube_with_backends(
            64,
            64,
            wgpu::Backends::VULKAN,
        ))
        .unwrap();
        host.render().unwrap();
        let mut ring = VulkanRing::new(&host.renderer.device, 64, 64).unwrap();
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
        let mut frames: Vec<Arc<SharedTexture>> = Vec::new();
        let mut observed = Vec::new();
        for i in 0..6 {
            if i == 3 {
                assert!(
                    !ring.can_submit(),
                    "live frame leases must prevent overwrite"
                );
                frames[0].last_read.store(1, Ordering::Release);
                frames.remove(0);
                assert!(
                    !ring.can_submit(),
                    "consumer completion must also gate overwrite"
                );
                unsafe { ring.slots[0].consumed.Signal(1) }.unwrap();
                assert!(ring.can_submit());
                frames.clear();
            }
            host.yaw += 0.7;
            host.render().unwrap();
            let frame = ring
                .copy_frame(
                    &host.renderer.device,
                    &host.renderer.queue,
                    host.renderer.external_output.as_ref().unwrap(),
                )
                .unwrap()
                .unwrap();
            ring.queue_frame(
                frame.clone(),
                FrameId {
                    generation: 1,
                    sequence: i,
                },
            );
            let deadline = Instant::now() + Duration::from_secs(15);
            let (published, _) = loop {
                host.renderer.device.poll(wgpu::PollType::Poll).unwrap();
                ring.check_device().unwrap();
                if let Some(frame) = ring.take_ready() {
                    break frame;
                }
                assert!(Instant::now() < deadline, "Vulkan copy did not complete");
                std::thread::sleep(Duration::from_millis(1));
            };
            let (texture, owner) = gpui_windows::shared_texture::import(
                &device.cast().unwrap(),
                &context.cast().unwrap(),
                published.clone(),
            )
            .unwrap();
            let mut staging = None;
            unsafe {
                device.CreateTexture2D(
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
            }
            .unwrap();
            let staging = staging.unwrap();
            unsafe {
                context.CopyResource(&staging, &texture);
                context.Flush();
            }
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.unwrap();
            let mut actual = Vec::new();
            for y in 0..64 {
                let row = unsafe {
                    std::slice::from_raw_parts(
                        (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                        64 * 4,
                    )
                };
                for px in row.as_chunks::<4>().0 {
                    actual.extend_from_slice(&[px[2], px[1], px[0], 255]);
                }
            }
            unsafe { context.Unmap(&staging, 0) };
            let mut reference = ReadbackRing::new(
                &host.renderer.device,
                64,
                64,
                0,
                host.renderer.surface_format,
                AlphaMode::Opaque,
                2,
            )
            .unwrap();
            reference
                .enqueue(
                    &host.renderer.device,
                    &host.renderer.queue,
                    host.renderer.external_output.as_ref().unwrap(),
                    0,
                )
                .unwrap();
            host.renderer
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            let expected = reference.take_latest().unwrap();
            assert_eq!(actual, expected.rgba, "Vulkan and D3D11 pixels differ");
            observed.push(actual);
            drop(owner);
            drop(published);
            frames.push(frame);
        }
        assert_ne!(
            observed[0], observed[1],
            "camera animation did not change shared pixels"
        );
        assert!(
            !ring.can_submit(),
            "live frame leases must prevent overwrite"
        );
        assert_ne!(
            observed[0], observed[3],
            "reused shared allocation retained stale pixels"
        );
        ring.retire();
        drop(frames);
        assert!(ring.retired_idle());
    }
}
