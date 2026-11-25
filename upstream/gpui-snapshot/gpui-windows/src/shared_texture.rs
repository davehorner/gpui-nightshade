//! D3D12/D3D11 shared immutable frames, imported by GPUI's regular sprite compositor.
use anyhow::Context;
use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Graphics::{Direct3D11::*, Dxgi::IDXGIDevice},
    },
    core::Interface,
};

/// Owns an NT handle. Never exposes a borrowed handle beyond its owner lifetime.
#[derive(Debug)]
pub struct SharedHandle(HANDLE);
// SAFETY: NT shared-resource/fence handles support cross-thread operations. Arc owns
// the sole CloseHandle operation, so no borrower races a close.
unsafe impl Send for SharedHandle {}
unsafe impl Sync for SharedHandle {}
impl SharedHandle {
    /// Takes exclusive ownership of a valid NT handle.
    /// # Safety
    /// The handle must be owned, valid, and safe to close with CloseHandle.
    pub unsafe fn from_owned(handle: HANDLE) -> Self {
        Self(handle)
    }
    pub fn raw(&self) -> HANDLE {
        self.0
    }
}
impl Drop for SharedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[derive(Debug)]
pub struct SharedTexture {
    /// Retains producer-side resources/fences through cached and queued consumer draws.
    pub producer_owner: Arc<dyn SharedFrameOwner>,
    pub texture_handle: Arc<SharedHandle>,
    pub ready_fence_handle: Arc<SharedHandle>,
    pub consumed_fence_handle: Arc<SharedHandle>,
    pub adapter_luid: u64,
    pub width: u32,
    pub height: u32,
    pub ready_value: u64,
    /// Monotonic consumer-only timeline, incremented for every consumer draw of this slot.
    pub last_read: Arc<AtomicU64>,
}
pub trait SharedFrameOwner: Send + Sync + std::fmt::Debug {}
impl gpui::ExternalTexture for SharedTexture {
    fn size(&self) -> gpui::Size<gpui::DevicePixels> {
        gpui::size(
            gpui::DevicePixels(self.width as i32),
            gpui::DevicePixels(self.height as i32),
        )
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
/// Retain alongside the consumer texture until its queued use has retired.
pub struct ImportedTexture {
    frame: Arc<dyn gpui::ExternalTexture>,
    ready_fence: ID3D11Fence,
    consumed_fence: ID3D11Fence,
    context: ID3D11DeviceContext4,
}
impl ImportedTexture {
    pub(crate) fn before_draw(&self) -> anyhow::Result<()> {
        let frame = self.frame.as_any().downcast_ref::<SharedTexture>().unwrap();
        let completed = unsafe { self.ready_fence.GetCompletedValue() };
        anyhow::ensure!(
            completed != u64::MAX && completed >= frame.ready_value,
            "producer frame is not complete"
        );
        Ok(())
    }
    pub(crate) fn after_draw(&self) -> anyhow::Result<()> {
        let frame = self.frame.as_any().downcast_ref::<SharedTexture>().unwrap();
        let value = frame
            .last_read
            .try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| anyhow::anyhow!("consumer fence timeline exhausted"))?
            + 1;
        unsafe {
            self.context.Signal(&self.consumed_fence, value)?;
            self.context.Flush();
        }
        Ok(())
    }
}
/// Import a completed immutable producer frame as a D3D11 shader texture.
/// The returned guard retains producer resources and must outlive queued use.
pub fn import(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    owner: Arc<dyn gpui::ExternalTexture>,
) -> anyhow::Result<(ID3D11Texture2D, ImportedTexture)> {
    let frame = owner
        .as_any()
        .downcast_ref::<SharedTexture>()
        .ok_or_else(|| anyhow::anyhow!("unsupported external texture type"))?;
    let dxgi: IDXGIDevice = device.cast()?;
    let desc = unsafe { dxgi.GetAdapter()?.GetDesc()? };
    let luid = (desc.AdapterLuid.HighPart as u32 as u64) << 32 | desc.AdapterLuid.LowPart as u64;
    anyhow::ensure!(
        luid == frame.adapter_luid,
        "GPUI and Nightshade adapter LUIDs differ"
    );
    let device1: ID3D11Device1 = device.cast()?;
    let device5: ID3D11Device5 = device.cast()?;
    let shared_texture: ID3D11Texture2D = unsafe {
        device1
            .OpenSharedResource1(frame.texture_handle.raw())
            .context("OpenSharedResource1 texture")?
    };
    let mut ready_fence = None;
    let mut consumed_fence = None;
    unsafe {
        device5
            .OpenSharedFence::<ID3D11Fence>(frame.ready_fence_handle.raw(), &mut ready_fence)
            .context("OpenSharedFence producer")?;
        device5
            .OpenSharedFence::<ID3D11Fence>(frame.consumed_fence_handle.raw(), &mut consumed_fence)
            .context("OpenSharedFence consumer")?;
    }
    let ready_fence =
        ready_fence.ok_or_else(|| anyhow::anyhow!("producer fence import returned no fence"))?;
    let consumed_fence =
        consumed_fence.ok_or_else(|| anyhow::anyhow!("consumer fence import returned no fence"))?;
    let completed = unsafe { ready_fence.GetCompletedValue() };
    anyhow::ensure!(
        completed != u64::MAX && completed >= frame.ready_value,
        "shared producer frame is not complete (ready {}, completed {completed})",
        frame.ready_value
    );
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        shared_texture.GetDesc(&mut desc);
    }
    anyhow::ensure!(
        desc.Width == frame.width
            && desc.Height == frame.height
            && desc.SampleDesc.Count == 1
            && desc.Format == windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        "incompatible shared texture descriptor"
    );
    Ok((
        shared_texture,
        ImportedTexture {
            frame: owner,
            ready_fence,
            consumed_fence,
            context: context.cast()?,
        },
    ))
}

/// Bounded diagnostics for asynchronous device failures that surface at Present.
pub(crate) fn report_device_failure(device: &ID3D11Device) {
    eprintln!("GPUI D3D11 device status: {:?}", unsafe {
        device.GetDeviceRemovedReason()
    });
    if let Ok(info) = device.cast::<ID3D11InfoQueue>() {
        let count = unsafe { info.GetNumStoredMessages() };
        for index in count.saturating_sub(16)..count {
            let mut size = 0;
            if unsafe { info.GetMessage(index, None, &mut size) }.is_err()
                || size < std::mem::size_of::<D3D11_MESSAGE>()
                || size > 64 * 1024
            {
                continue;
            }
            // u64 storage supplies alignment required by D3D11_MESSAGE.
            let mut storage = vec![0u64; size.div_ceil(8)];
            let message = storage.as_mut_ptr().cast::<D3D11_MESSAGE>();
            if unsafe { info.GetMessage(index, Some(message), &mut size) }.is_ok() {
                let message = unsafe { &*message };
                if message.pDescription.is_null() || message.DescriptionByteLength > size {
                    continue;
                }
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        message.pDescription.cast::<u8>(),
                        message.DescriptionByteLength,
                    )
                };
                eprintln!(
                    "GPUI D3D11 {:?} {:?}: {}",
                    message.Severity,
                    message.ID,
                    String::from_utf8_lossy(bytes).trim_end_matches('\0')
                );
            }
        }
    }
}
