//! Bounded presentation targets; phone pixel density must not imply desktop GPU load.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WebRenderOptions {
    pub max_width: u32,
    pub max_height: u32,
    pub frame_ms: u32,
    pub staging_frames: usize,
}
impl WebRenderOptions {
    pub fn for_browser(user_agent: &str, search: &str) -> Self {
        let safe = search
            .trim_start_matches('?')
            .split('&')
            .any(|p| p == "safe=1");
        if safe {
            Self {
                max_width: 640,
                max_height: 480,
                frame_ms: 66,
                staging_frames: 2,
            }
        } else if ["Android", "iPhone", "iPad", "Mobile"]
            .iter()
            .any(|name| user_agent.contains(name))
        {
            Self {
                max_width: 960,
                max_height: 640,
                frame_ms: 40,
                staging_frames: 2,
            }
        } else {
            Self {
                max_width: 1920,
                max_height: 1080,
                frame_ms: 16,
                staging_frames: 3,
            }
        }
    }
    pub fn render_size(self, width: u32, height: u32) -> (u32, u32) {
        let (w, h) = (width.max(1) as f64, height.max(1) as f64);
        let scale = (self.max_width.max(1) as f64 / w)
            .min(self.max_height.max(1) as f64 / h)
            .min(1.);
        (
            (w * scale).floor().max(1.) as u32,
            (h * scale).floor().max(1.) as u32,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn android_high_dpi_targets_keep_aspect_and_memory_bounds() {
        let options =
            WebRenderOptions::for_browser("Mozilla/5.0 (Linux; Android 15) Chrome/145 Mobile", "");
        assert_eq!(options.render_size(1440, 3200), (288, 640));
        assert_eq!(options.render_size(3200, 1440), (960, 432));
        assert_eq!(options.staging_frames, 2);
        assert_eq!(options.frame_ms, 40);
        assert_eq!(options.render_size(0, 0), (1, 1));
    }
    #[test]
    fn safe_mode_is_explicit_and_desktop_remains_available() {
        let safe = WebRenderOptions::for_browser("desktop", "?foo=1&safe=1");
        assert_eq!(safe.render_size(3840, 2160), (640, 360));
        assert_eq!(safe.frame_ms, 66);
        assert_eq!(
            WebRenderOptions::for_browser("desktop", "?safe=10").render_size(3840, 2160),
            (1920, 1080)
        );
    }
}
