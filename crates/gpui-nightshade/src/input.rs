/// Keep the point on the camera's focus plane at the same screen location during zoom.
pub fn zoom_anchor_shift(
    old_radius: f32,
    new_radius: f32,
    yaw: f32,
    pitch: f32,
    y_fov: f32,
    anchor: [f32; 3],
) -> [f32; 3] {
    let span = (old_radius - new_radius) * (y_fov * 0.5).tan();
    let x = (2. * anchor[0] - 1.) * anchor[2] * span;
    let y = (1. - 2. * anchor[1]) * span;
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    [cy * x - sy * sp * y, cp * y, -sy * x - cy * sp * y]
}

/// Convert a pointer from window logical pixels into viewport-local physical pixels.
pub fn local_pointer(
    pointer: [f32; 2],
    origin: [f32; 2],
    size: [f32; 2],
    scale: f32,
    captured: bool,
) -> Option<[f32; 2]> {
    if !scale.is_finite()
        || scale <= 0.0
        || size.iter().any(|n| !n.is_finite() || *n <= 0.0)
        || pointer.iter().chain(origin.iter()).any(|n| !n.is_finite())
    {
        return None;
    }
    let local = [pointer[0] - origin[0], pointer[1] - origin[1]];
    if !captured && (local[0] < 0.0 || local[1] < 0.0 || local[0] >= size[0] || local[1] >= size[1])
    {
        return None;
    }
    Some([local[0] * scale, local[1] * scale])
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Move,
    Press,
    Release,
    Leave,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerInput {
    pub phase: PointerPhase,
    pub position: [f32; 2],
    pub control: bool,
    pub shift: bool,
}
/// Ordered edges with latest-move coalescing. Overflow releases the gesture.
#[derive(Default)]
pub struct PointerQueue(std::collections::VecDeque<PointerInput>);
impl PointerQueue {
    pub const CAPACITY: usize = 128;
    pub fn push(&mut self, event: PointerInput) {
        if event.phase == PointerPhase::Move
            && self
                .0
                .back()
                .is_some_and(|last| last.phase == PointerPhase::Move)
        {
            self.0.pop_back();
        }
        if self.0.len() >= Self::CAPACITY {
            self.cancel();
            return;
        }
        self.0.push_back(event);
    }
    pub fn cancel(&mut self) {
        self.0.clear();
        self.0.push_back(PointerInput {
            phase: PointerPhase::Cancel,
            position: [0.; 2],
            control: false,
            shift: false,
        });
    }
    pub fn pop(&mut self) -> Option<PointerInput> {
        self.0.pop_front()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
#[cfg(test)]
mod pointer_tests {
    use super::{PointerInput, PointerPhase as Phase, PointerQueue};
    fn event(phase: Phase, x: f32) -> PointerInput {
        PointerInput {
            phase,
            position: [x, 0.],
            control: false,
            shift: false,
        }
    }
    #[test]
    fn coalescing_keeps_press_and_release_positions_and_overflow_releases() {
        let mut queue = PointerQueue::default();
        for e in [
            event(Phase::Press, 1.),
            event(Phase::Move, 2.),
            event(Phase::Move, 3.),
            event(Phase::Release, 4.),
        ] {
            queue.push(e);
        }
        assert_eq!(queue.pop(), Some(event(Phase::Press, 1.)));
        assert_eq!(queue.pop(), Some(event(Phase::Move, 3.)));
        assert_eq!(queue.pop(), Some(event(Phase::Release, 4.)));
        for _ in 0..=PointerQueue::CAPACITY {
            queue.push(event(Phase::Press, 1.));
        }
        assert_eq!(queue.pop().unwrap().phase, Phase::Cancel);
        assert!(queue.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinch_anchor_stays_at_its_projected_position() {
        for (yaw, pitch) in [(0., 0.), (1.2, 0.8), (-2., -0.7)] {
            for anchor in [[0.5, 0.5, 1.], [0.8, 0.2, 0.7], [0.1, 0.9, 2.]] {
                let old = 12.;
                let new = 3.;
                let fov = std::f32::consts::FRAC_PI_4;
                let point = zoom_anchor_shift(old, 0., yaw, pitch, fov, anchor);
                let shift = zoom_anchor_shift(old, new, yaw, pitch, fov, anchor);
                let (sy, cy) = yaw.sin_cos();
                let (sp, cp) = pitch.sin_cos();
                let right = [cy, 0., -sy];
                let up = [-sy * sp, cp, -cy * sp];
                let dot = |v: [f32; 3]| (0..3).map(|i| (point[i] - shift[i]) * v[i]).sum::<f32>();
                let span = new * (fov * 0.5).tan();
                assert!((0.5 + dot(right) / (2. * span * anchor[2]) - anchor[0]).abs() < 1e-5);
                assert!((0.5 - dot(up) / (2. * span) - anchor[1]).abs() < 1e-5);
                let reverse = zoom_anchor_shift(new, old, yaw, pitch, fov, anchor);
                for i in 0..3 {
                    assert!((shift[i] + reverse[i]).abs() < 1e-5);
                }
            }
        }
    }
    #[test]
    fn dpi_and_capture() {
        assert_eq!(
            local_pointer([12., 23.], [10., 20.], [100., 100.], 2., false),
            Some([4., 6.])
        );
        assert_eq!(
            local_pointer([9., 23.], [10., 20.], [100., 100.], 2., false),
            None
        );
        assert_eq!(
            local_pointer([9., 23.], [10., 20.], [100., 100.], 2., true),
            Some([-2., 6.])
        );
    }
}
