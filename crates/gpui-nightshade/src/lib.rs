pub mod host;
pub mod input;
pub mod presentation;
pub mod views;
pub use nightshade_wgpu::wgpu;

/// Vulkan by default; an explicit WGPU_BACKEND setting remains an override.
pub fn default_backends() -> wgpu::Backends {
    wgpu::Backends::from_env().unwrap_or(wgpu::Backends::VULKAN)
}
