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

/// GPU transports retain a texture and an explicit synchronization contract.
/// No native handles escape through this interface.
pub trait GpuFrame: Send + Sync {
    fn id(&self) -> FrameId;
    fn dimensions(&self) -> (u32, u32);
    fn color_space(&self) -> ColorSpace;
}

pub enum PresentationFrame {
    Cpu(CpuFrame),
    Gpu(std::sync::Arc<dyn GpuFrame>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackendPreference {
    #[default]
    Auto,
    Readback,
    WindowsShared,
}
impl std::str::FromStr for BackendPreference {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "readback" => Ok(Self::Readback),
            "windows-shared" => Ok(Self::WindowsShared),
            _ => Err(format!("unknown presentation backend: {value}")),
        }
    }
}
