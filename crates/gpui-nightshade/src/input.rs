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
