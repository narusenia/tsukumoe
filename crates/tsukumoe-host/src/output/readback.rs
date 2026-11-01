// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! GPU to CPU copy of a finished frame, for sinks that take pixels.
//! shortcut: every call waits for the GPU to finish, so it stalls the frame;
//! fine for one sink at stream resolutions, replace with a shared texture if
//! it shows up in use.

/// Reads `Rgba8Unorm` frames into one reused, tightly packed buffer.
#[derive(Default)]
pub struct Readback {
    buffer: Option<(wgpu::Buffer, (u32, u32))>,
    rgba: Vec<u8>,
}

impl Readback {
    /// The frame's pixels, `width * height * 4` bytes, rows without padding.
    /// `frame` needs `COPY_SRC` usage.
    pub fn read(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::Texture,
    ) -> Result<&[u8], String> {
        let (width, height) = (frame.width(), frame.height());
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        if self.buffer.as_ref().is_none_or(|b| b.1 != (width, height)) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size: u64::from(row) * u64::from(height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.buffer = Some((buffer, (width, height)));
        }
        let buffer = &self.buffer.as_ref().expect("made above").0;
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            frame.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        buffer.map_async(wgpu::MapMode::Read, .., |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| format!("readback: {e}"))?;
        let mapped = buffer
            .get_mapped_range(..)
            .map_err(|e| format!("readback: {e}"))?;
        self.rgba.clear();
        for y in 0..height as usize {
            let start = y * row as usize;
            self.rgba
                .extend_from_slice(&mapped[start..start + width as usize * 4]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(&self.rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_come_back_packed_even_when_the_width_is_not_aligned() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Some(adapter) = [true, false].into_iter().find_map(|fallback| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                force_fallback_adapter: fallback,
                ..Default::default()
            }))
            .ok()
        }) else {
            return;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            return;
        };
        // 5 px = 20 bytes per row, padded to 256 in the copy.
        let (w, h) = (5u32, 3u32);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let pixels: Vec<u8> = (0..w * h * 4).map(|i| i as u8).collect();
        queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let mut readback = Readback::default();
        // Twice: the second call reuses the buffer.
        for _ in 0..2 {
            assert_eq!(readback.read(&device, &queue, &texture).unwrap(), pixels);
        }
    }
}
