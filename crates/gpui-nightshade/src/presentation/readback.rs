use super::{AlphaMode, ColorSpace, CpuFrame, FrameId};
use crate::wgpu;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Instant,
};

struct Slot {
    buffer: wgpu::Buffer,
    status: Arc<AtomicU8>, // 0 idle, 1 mapping, 2 ready, 3 failed
    frame: Option<(FrameId, Instant)>,
}

/// Non-sRGB sources must contain display-encoded gamma 2.2 bytes, as in the default Nightshade postprocess output.
/// Fixed-size ring. Resize replaces this object; outstanding callbacks only own atomics.
pub struct ReadbackRing {
    slots: Vec<Slot>,
    width: u32,
    height: u32,
    stride: u32,
    generation: u64,
    format: wgpu::TextureFormat,
    alpha: AlphaMode,
    last_delivered: Option<u64>,
    pub dropped: u64,
}

impl ReadbackRing {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        generation: u64,
        format: wgpu::TextureFormat,
        alpha: AlphaMode,
        capacity: usize,
    ) -> Result<Self, String> {
        if !(2..=4).contains(&capacity) {
            return Err("ring capacity must be 2..=4".into());
        }
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err("readback requires RGBA8/BGRA8".into());
        }
        let stride = aligned_stride(width).ok_or("invalid width")?;
        let bytes = u64::from(stride) * u64::from(height);
        if height == 0
            || bytes > device.limits().max_buffer_size
            || bytes * capacity as u64 > 256 * 1024 * 1024
        {
            return Err("readback exceeds 256 MiB budget or device limits".into());
        }
        let slots = (0..capacity)
            .map(|_| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("viewport readback"),
                    size: bytes,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                status: Arc::new(AtomicU8::new(0)),
                frame: None,
            })
            .collect();
        Ok(Self {
            slots,
            width,
            height,
            stride,
            generation,
            format,
            alpha,
            last_delivered: None,
            dropped: 0,
        })
    }

    /// Call on the render worker, after Nightshade submits the frame. Never waits.
    pub fn enqueue(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        sequence: u64,
    ) -> Result<bool, String> {
        if texture.width() != self.width
            || texture.height() != self.height
            || texture.format() != self.format
            || texture.sample_count() != 1
            || !texture.usage().contains(wgpu::TextureUsages::COPY_SRC)
        {
            return Err("readback source differs from ring configuration".into());
        }
        let Some(slot) = self.slots.iter_mut().find(|s| s.frame.is_none()) else {
            self.dropped += 1;
            return Ok(false);
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &slot.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.stride),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        slot.frame = Some((
            FrameId {
                generation: self.generation,
                sequence,
            },
            Instant::now(),
        ));
        slot.status.store(1, Ordering::Release);
        let status = slot.status.clone();
        slot.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                status.store(if result.is_ok() { 2 } else { 3 }, Ordering::Release);
            });
        Ok(true)
    }

    /// The caller drives device.poll(Poll) on its worker, never on GPUI's UI thread.
    pub fn take_latest(&mut self) -> Option<CpuFrame> {
        let newest = self
            .slots
            .iter()
            .filter(|s| s.status.load(Ordering::Acquire) == 2)
            .filter_map(|s| s.frame.as_ref().map(|(id, _)| id.sequence))
            .filter(|seq| self.last_delivered.is_none_or(|last| *seq > last))
            .max();
        let mut result = None;
        for slot in &mut self.slots {
            let status = slot.status.load(Ordering::Acquire);
            if status < 2 {
                continue;
            }
            let (id, submitted) = slot.frame.take().expect("mapped slot has a frame");
            if status == 2 && Some(id.sequence) == newest {
                let bytes = slot.buffer.slice(..).get_mapped_range();
                let mut rgba = Vec::with_capacity(self.width as usize * self.height as usize * 4);
                for row in bytes.chunks_exact(self.stride as usize) {
                    rgba.extend_from_slice(&row[..self.width as usize * 4]);
                }
                if matches!(
                    self.format,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
                ) {
                    for pixel in rgba.as_chunks_mut::<4>().0 {
                        pixel.swap(0, 2);
                    }
                }
                if self.alpha == AlphaMode::Opaque {
                    for pixel in rgba.as_chunks_mut::<4>().0 {
                        pixel[3] = 255;
                    }
                }
                drop(bytes);
                result = Some(CpuFrame {
                    id,
                    width: self.width,
                    height: self.height,
                    rgba,
                    color_space: if self.format.is_srgb() {
                        ColorSpace::Srgb
                    } else {
                        ColorSpace::Gamma22
                    },
                    alpha: self.alpha,
                    latency: submitted.elapsed(),
                });
                self.last_delivered = Some(id.sequence);
            } else {
                self.dropped += 1;
            }
            slot.buffer.unmap();
            slot.status.store(0, Ordering::Release);
        }
        result
    }
    pub fn in_flight(&self) -> usize {
        self.slots.iter().filter(|s| s.frame.is_some()).count()
    }
    pub fn allocated_bytes(&self) -> u64 {
        u64::from(self.stride) * u64::from(self.height) * self.slots.len() as u64
    }
}

pub fn aligned_stride(width: u32) -> Option<u32> {
    if width == 0 {
        return None;
    }
    width
        .checked_mul(4)?
        .checked_add(255)
        .map(|bytes| bytes & !255)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn row_alignment_and_overflow() {
        assert_eq!(aligned_stride(1), Some(256));
        assert_eq!(aligned_stride(64), Some(256));
        assert_eq!(aligned_stride(65), Some(512));
        assert_eq!(aligned_stride(0), None);
        assert_eq!(aligned_stride(u32::MAX), None);
    }
}
