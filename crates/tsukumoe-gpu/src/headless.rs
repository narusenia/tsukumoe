// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use tsukumoe_core::draw::DrawList;

use crate::{Error, OUTPUT_FORMAT, Renderer, Texture};

/// A premultiplied RGBA8 image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    /// PNG bytes. PNG is straight alpha, so the colors are un-premultiplied.
    pub fn to_png(&self) -> Vec<u8> {
        let mut straight = self.rgba.clone();
        for px in straight.as_chunks_mut::<4>().0 {
            let a = u32::from(px[3]);
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * 255 + a / 2).checked_div(a).unwrap_or(0)).min(255) as u8;
            }
        }
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .and_then(|mut w| w.write_image_data(&straight))
            .expect("writing PNG to memory cannot fail");
        out
    }
}

/// A renderer that owns its device and reads images back. A software adapter
/// is preferred, so the output does not depend on the machine's GPU.
pub struct Headless {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
}

impl Headless {
    pub fn new() -> Result<Self, Error> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = [true, false]
            .into_iter()
            .find_map(|fallback| {
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    force_fallback_adapter: fallback,
                    ..Default::default()
                }))
                .ok()
            })
            .ok_or_else(|| Error::Gpu("no adapter".into()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
            .map_err(|e| Error::Gpu(e.to_string()))?;
        let renderer = Renderer::new(&device);
        Ok(Self {
            device,
            queue,
            renderer,
        })
    }

    pub fn set_textures(&mut self, textures: &[Texture]) -> Result<(), Error> {
        self.renderer
            .set_textures(&self.device, &self.queue, textures)
    }

    /// Draws `list` into a `width` x `height` image showing `view`
    /// `(x_min, y_min, x_max, y_max)` of drawing space.
    pub fn render(
        &mut self,
        list: &DrawList,
        view: [f32; 4],
        width: u32,
        height: u32,
    ) -> Result<Image, Error> {
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.renderer
            .render(&self.device, &self.queue, list, view, &target)?;

        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
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
        self.queue.submit([encoder.finish()]);
        buffer.map_async(wgpu::MapMode::Read, .., |r| r.expect("map readback buffer"));
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| Error::Gpu(e.to_string()))?;
        let mapped = buffer.get_mapped_range(..);
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height as usize {
            let start = y * row as usize;
            rgba.extend_from_slice(&mapped[start..start + width as usize * 4]);
        }
        Ok(Image {
            width,
            height,
            rgba,
        })
    }
}
