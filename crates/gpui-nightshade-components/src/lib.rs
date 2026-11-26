pub use gpui_kit;
mod benchmark;
pub mod viewport;
mod visibility;
#[cfg(not(target_arch = "wasm32"))]
mod worker;
#[cfg(target_arch = "wasm32")]
#[path = "web_worker.rs"]
mod worker;
pub use viewport::NightshadeViewport;
#[cfg(target_os = "windows")]
mod windows;

/// Constructs a complete scene on its render worker. The callback may be retried by auto fallback.
#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(target_arch = "wasm32")]
pub type SceneFactory = std::sync::Arc<
    dyn Fn(
        gpui_nightshade::wgpu::Backends,
        u32,
        u32,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Box<dyn gpui_nightshade::host::ViewportHost>,
                        Box<dyn std::error::Error>,
                    >,
                >,
        >,
    >,
>;
