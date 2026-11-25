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
