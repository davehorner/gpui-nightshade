//! Frames carry ownership, physical dimensions and resize generations independently of transport.
pub mod readback;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameId {
    pub generation: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    Gamma22,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMode {
    Opaque,
    Straight,
    Premultiplied,
}

#[derive(Debug)]
pub struct CpuFrame {
    pub id: FrameId,
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8; transfer function and alpha are explicit metadata.
    pub rgba: Vec<u8>,
    pub color_space: ColorSpace,
    pub alpha: AlphaMode,
    pub latency: std::time::Duration,
}

/// Metadata for one immutable published frame, independent of its pixel storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInfo {
    pub id: FrameId,
    pub width: u32,
    pub height: u32,
    pub color_space: ColorSpace,
    pub alpha: AlphaMode,
}
impl CpuFrame {
    pub fn info(&self) -> FrameInfo {
        FrameInfo {
            id: self.id,
            width: self.width,
            height: self.height,
            color_space: self.color_space,
            alpha: self.alpha,
        }
    }
}

/// The allocated transport, rather than the user's requested preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentationBackend {
    Readback,
    WindowsShared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationCapabilities {
    /// Pixel bytes stay on the GPU during normal presentation.
    pub gpu_resident: bool,
    /// The renderer and GPUI must use the same physical adapter.
    pub requires_matching_adapter: bool,
}
impl PresentationBackend {
    pub fn capabilities(self) -> PresentationCapabilities {
        let shared = self == Self::WindowsShared;
        PresentationCapabilities {
            gpu_resident: shared,
            requires_matching_adapter: shared,
        }
    }
}

/// Owned status snapshot for an embedding application. No native handles escape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentationStatus {
    pub requested: BackendPreference,
    /// None during initialization. This is the last initialized transport;
    /// an error means that it must not be treated as usable.
    pub active: Option<PresentationBackend>,
    pub graphics_backend: Option<crate::wgpu::Backend>,
    pub adapter: Option<String>,
    pub visible: bool,
    pub fallback_reason: Option<String>,
    /// Failures are reported, not silently presented as recovery.
    pub error: Option<String>,
}
impl PresentationStatus {
    pub fn initializing(requested: BackendPreference) -> Self {
        Self {
            requested,
            active: None,
            graphics_backend: None,
            adapter: None,
            visible: true,
            fallback_reason: None,
            error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackendPreference {
    /// Use the default Vulkan renderer with readback presentation.
    #[default]
    Auto,
    Readback,
    WindowsShared,
    /// Opt in to DX12 sharing with automatic readback fallback.
    WindowsSharedAuto,
}
impl std::str::FromStr for BackendPreference {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "readback" => Ok(Self::Readback),
            "windows-shared" => Ok(Self::WindowsShared),
            "windows-shared-auto" => Ok(Self::WindowsSharedAuto),
            _ => Err(format!("unknown presentation backend: {value}")),
        }
    }
}
