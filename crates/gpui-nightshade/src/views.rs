/// Camera tiles sharing one scene and presentation image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraLayout {
    #[default]
    Single,
    Split,
    Quad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CameraLayout {
    /// Integer partition with no gaps, overlaps or zero-sized render targets.
    pub fn rects(self, width: u32, height: u32) -> Vec<CameraRect> {
        let (width, height) = (width.max(1), height.max(1));
        let columns = if self != Self::Single && width > 1 {
            2
        } else {
            1
        };
        let rows = if self == Self::Quad && height > 1 && columns == 2 {
            2
        } else {
            1
        };
        (0..rows)
            .flat_map(|row| {
                (0..columns).map(move |col| {
                    let x = width * col / columns;
                    let y = height * row / rows;
                    CameraRect {
                        x,
                        y,
                        width: width * (col + 1) / columns - x,
                        height: height * (row + 1) / rows - y,
                    }
                })
            })
            .collect()
    }

    pub fn hit(self, width: u32, height: u32, x: f32, y: f32) -> Option<usize> {
        self.rects(width, height).iter().position(|r| {
            x >= r.x as f32
                && y >= r.y as f32
                && x < (r.x + r.width) as f32
                && y < (r.y + r.height) as f32
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiles_cover_odd_and_tiny_images_once_and_hit_boundaries() {
        for layout in [
            CameraLayout::Single,
            CameraLayout::Split,
            CameraLayout::Quad,
        ] {
            for (w, h) in [(1, 1), (2, 1), (1, 7), (13, 9), (128, 73)] {
                let rects = layout.rects(w, h);
                for y in 0..h {
                    for x in 0..w {
                        let hits = rects
                            .iter()
                            .filter(|r| {
                                x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height
                            })
                            .count();
                        assert_eq!(hits, 1);
                        assert!(layout.hit(w, h, x as f32, y as f32).is_some());
                    }
                }
                assert_eq!(layout.hit(w, h, w as f32, 0.), None);
            }
        }
    }
}
