//! Bounded native thumbnail requests and GPUI image ownership for the visible page.
use gpui_nightshade_components::gpui_kit::{App, RenderImage, Window};
use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
const MAX_REQUESTS: usize = 4;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIMENSION: u32 = 4096;
type Completed = (String, Option<Arc<RenderImage>>);
pub struct Thumbnails {
    images: HashMap<String, Option<Arc<RenderImage>>>,
    pending: HashSet<String>,
    sender: mpsc::Sender<Completed>,
    receiver: mpsc::Receiver<Completed>,
    changed: Arc<AtomicBool>,
}
impl Default for Thumbnails {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            images: HashMap::new(),
            pending: HashSet::new(),
            sender,
            receiver,
            changed: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Thumbnails {
    pub fn has_update(&self) -> bool {
        self.changed.load(Ordering::Acquire)
    }
    pub fn is_finished(&self, key: &str) -> bool {
        self.images.contains_key(key)
    }
    pub fn image(&self, key: &str) -> Option<Arc<RenderImage>> {
        self.images.get(key).and_then(Clone::clone)
    }
    pub fn update(&mut self, keys: impl Iterator<Item = String>, window: &mut Window) {
        let keys: Vec<_> = keys
            .filter(|key| key.starts_with("https://") || key.starts_with("http://"))
            .collect();
        let wanted: HashSet<_> = keys.iter().cloned().collect();
        self.changed.store(false, Ordering::Release);
        for (key, result) in self.receiver.try_iter() {
            self.pending.remove(&key);
            if wanted.contains(&key) {
                self.images.insert(key, result);
            }
        }
        self.images.retain(|key, image| {
            if wanted.contains(key) {
                return true;
            }
            if let Some(image) = image.take() {
                let _ = window.drop_image(image);
            }
            false
        });
        for key in keys {
            if self.pending.len() >= MAX_REQUESTS {
                break;
            }
            if self.images.contains_key(&key) || self.pending.contains(&key) {
                continue;
            }
            self.pending.insert(key.clone());
            let sender = self.sender.clone();
            let changed = self.changed.clone();
            let request = ehttp::Request::get(&key);
            ehttp::fetch(request, move |response| {
                let image = response
                    .ok()
                    .filter(|response| response.ok)
                    .and_then(|response| decode_thumbnail(&response.bytes))
                    .map(|image| Arc::new(RenderImage::new(vec![image::Frame::new(image)])));
                if sender.send((key, image)).is_ok() {
                    changed.store(true, Ordering::Release);
                }
            });
        }
    }
    pub fn release(&mut self, cx: &mut App) {
        for (_, image) in self.images.drain() {
            if let Some(image) = image {
                cx.drop_image(image, None);
            }
        }
    }
}
fn decode_thumbnail(bytes: &[u8]) -> Option<image::RgbaImage> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let dimensions = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if dimensions.0 > MAX_DIMENSION || dimensions.1 > MAX_DIMENSION {
        return None;
    }
    let mut image = image::load_from_memory(bytes)
        .ok()?
        .thumbnail(128, 96)
        .to_rgba8();
    // GPUI RenderImage stores BGRA, matching its decoded image path.
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(image)
}
#[cfg(test)]
mod tests {
    use super::decode_thumbnail;
    use std::io::Cursor;
    #[test]
    fn thumbnails_bound_dimensions_and_use_gpui_channel_order() {
        let image = image::RgbaImage::from_pixel(256, 128, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let thumbnail = decode_thumbnail(bytes.get_ref()).unwrap();
        assert_eq!(thumbnail.dimensions(), (128, 64));
        assert_eq!(thumbnail.get_pixel(0, 0).0, [30, 20, 10, 255]);
        assert!(decode_thumbnail(b"not an image").is_none());
        assert!(decode_thumbnail(&vec![0; 8 * 1024 * 1024 + 1]).is_none());
    }
}
