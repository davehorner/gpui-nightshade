pub use gpui_kit;
pub mod viewport;
mod visibility;
mod worker;
pub use viewport::NightshadeViewport;
#[cfg(target_os = "windows")]
mod windows;

/// Constructs a complete scene on its render worker. The callback may be retried by auto fallback.
pub type SceneFactory = std::sync::Arc<
    dyn Fn(
            gpui_nightshade::wgpu::Backends,
            u32,
            u32,
        )
            -> Result<Box<dyn gpui_nightshade::host::ViewportHost>, Box<dyn std::error::Error>>
        + Send
        + Sync,
>;

// Embedding applications can inspect presentation without importing native APIs.
pub use gpui_nightshade::presentation::{
    AlphaMode, BackendPreference, ColorSpace, FrameId, FrameInfo, PresentationBackend,
    PresentationCapabilities, PresentationStatus,
};

pub use gpui_nightshade::views::CameraLayout;
