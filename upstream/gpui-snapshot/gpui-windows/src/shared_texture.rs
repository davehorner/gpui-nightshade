//! D3D12/D3D11 shared immutable frames, imported by GPUI's regular sprite compositor.
use anyhow::Context;
use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
    pub texture_handle: Arc<SharedHandle>,
    pub fence_handle: Arc<SharedHandle>,
    pub adapter_luid: u64,
    pub width: u32,
    pub height: u32,
    pub ready_value: u64,
    pub done_value: u64,
    pub sampled: Arc<AtomicBool>,
}
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
pub(crate) struct ImportedTexture {
    pub frame: Arc<dyn gpui::ExternalTexture>,
    pub fence: ID3D11Fence,
    pub context: ID3D11DeviceContext4,
}
impl ImportedTexture {
    pub fn before_draw(&self) -> anyhow::Result<()> {
        let frame = self.frame.as_any().downcast_ref::<SharedTexture>().unwrap();
        frame.sampled.store(true, Ordering::Release);
        unsafe {
            self.context.Wait(&self.fence, frame.ready_value)?;
        }
        Ok(())
    }
    pub fn after_draw(&self) -> anyhow::Result<()> {
        let frame = self.frame.as_any().downcast_ref::<SharedTexture>().unwrap();
        unsafe {
            self.context.Signal(&self.fence, frame.done_value)?;
            self.context.Flush();
        }
        Ok(())
    }
}
pub(crate) fn import(
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
    let texture: ID3D11Texture2D = unsafe {
        device1
            .OpenSharedResource1(frame.texture_handle.raw())
            .context("OpenSharedResource1 texture")?
    };
    let mut fence = None;
    unsafe {
        device5
            .OpenSharedFence::<ID3D11Fence>(frame.fence_handle.raw(), &mut fence)
            .context("OpenSharedFence")?;
    }
    let fence = fence.ok_or_else(|| anyhow::anyhow!("shared fence import returned no fence"))?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
    }
    anyhow::ensure!(
        desc.Width == frame.width
            && desc.Height == frame.height
            && desc.SampleDesc.Count == 1
            && desc.Format == windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        "incompatible shared texture descriptor"
    );
    Ok((
        texture,
        ImportedTexture {
            frame: owner,
            fence,
            context: context.cast()?,
        },
    ))
}
