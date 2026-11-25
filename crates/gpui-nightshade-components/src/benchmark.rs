//! Opt-in presentation measurement. Normal rendering performs no event logging.
use gpui_nightshade::presentation::{FrameId, FrameInfo};
use std::{collections::VecDeque, sync::OnceLock, time::Instant};

pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("GPUI_NIGHTSHADE_BENCHMARK").is_some())
}
#[cfg(target_os = "windows")]
fn timestamp() -> u128 {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    static FREQUENCY: OnceLock<i64> = OnceLock::new();
    let frequency = *FREQUENCY.get_or_init(|| {
        let mut value = 0;
        unsafe { QueryPerformanceFrequency(&mut value) }.unwrap();
        value
    });
    let mut value = 0;
    unsafe { QueryPerformanceCounter(&mut value) }.unwrap();
    value as u128 * 1_000_000_000 / frequency as u128
}
#[cfg(not(target_os = "windows"))]
fn timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[derive(Clone)]
pub(crate) struct Stamp {
    started: Instant,
    submitted: Instant,
    cpu_ms: f64,
}
#[derive(Default)]
pub(crate) struct Recorder {
    pending: VecDeque<(FrameId, Stamp)>,
}
impl Recorder {
    pub fn begin(&self) -> Option<Instant> {
        enabled().then(Instant::now)
    }
    pub fn rendered(&self, start: Option<Instant>) -> Option<Stamp> {
        start.map(|started| Stamp {
            started,
            submitted: Instant::now(),
            cpu_ms: started.elapsed().as_secs_f64() * 1000.,
        })
    }
    pub fn submitted(&mut self, id: FrameId, stamp: Option<Stamp>) {
        if let Some(stamp) = stamp {
            self.pending.push_back((id, stamp));
            while self.pending.len() > 8 {
                self.pending.pop_front();
            }
        }
    }
    pub fn publish(&mut self, info: FrameInfo, mode: &str) -> Option<Stamp> {
        if !enabled() {
            return None;
        }
        let index = self.pending.iter().position(|(id, _)| *id == info.id)?;
        let (_, stamp) = self.pending.drain(..=index).next_back()?;
        eprintln!(
            "NSBENCH event=publish ns={} generation={} sequence={} width={} height={} mode={mode} render_cpu_ms={:.6} age_ms={:.6} post_submit_ms={:.6}",
            timestamp(),
            info.id.generation,
            info.id.sequence,
            info.width,
            info.height,
            stamp.cpu_ms,
            stamp.started.elapsed().as_secs_f64() * 1000.,
            stamp.submitted.elapsed().as_secs_f64() * 1000.
        );
        Some(stamp)
    }
}
pub(crate) fn adopt(info: FrameInfo, stamp: Option<&Stamp>) {
    if let Some(stamp) = stamp {
        eprintln!(
            "NSBENCH event=adopt ns={} generation={} sequence={} width={} height={} age_ms={:.6}",
            timestamp(),
            info.id.generation,
            info.id.sequence,
            info.width,
            info.height,
            stamp.started.elapsed().as_secs_f64() * 1000.
        );
    }
}
