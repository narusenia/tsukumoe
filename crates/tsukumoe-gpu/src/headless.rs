// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use tsukumoe_core::draw::DrawList;

use crate::{DrawOptions, Error, OUTPUT_FORMAT, Renderer, Texture};

/// A premultiplied RGBA8 image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// How two images of the same size differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diff {
    /// Largest difference of any channel of any pixel.
    pub max: u8,
    /// Pixels with a channel differing by more than the tolerance.
    pub differing: usize,
    pub pixels: usize,
}

impl Image {
    /// Decodes a PNG; the colors become premultiplied.
    pub fn from_png(bytes: &[u8]) -> Result<Self, Error> {
        let (width, height, mut rgba) = crate::decode_png(bytes)?;
        for px in rgba.as_chunks_mut::<4>().0 {
            let a = u32::from(px[3]);
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * a + 127) / 255) as u8;
            }
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    /// Compares with `other`, `None` when the sizes differ. The second image
    /// is `|self - other|` per channel, alpha forced opaque.
    pub fn diff(&self, other: &Image, tolerance: u8) -> Option<(Diff, Image)> {
        if (self.width, self.height) != (other.width, other.height) {
            return None;
        }
        let mut out = Vec::with_capacity(self.rgba.len());
        let mut diff = Diff {
            max: 0,
            differing: 0,
            pixels: self.rgba.len() / 4,
        };
        for (a, b) in self.rgba.chunks(4).zip(other.rgba.chunks(4)) {
            let d: [u8; 4] = std::array::from_fn(|i| a[i].abs_diff(b[i]));
            let worst = d.into_iter().max().unwrap_or(0);
            diff.max = diff.max.max(worst);
            diff.differing += usize::from(worst > tolerance);
            out.extend_from_slice(&[d[0], d[1], d[2], 255]);
        }
        Some((
            diff,
            Image {
                width: self.width,
                height: self.height,
                rgba: out,
            },
        ))
    }

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

struct FrameTarget {
    size: (u32, u32),
    view: wgpu::TextureView,
}

/// A renderer that owns its device and reads images back. A software adapter
/// is preferred, so the output does not depend on the machine's GPU.
pub struct Headless {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    adapter: String,
    frame: Option<FrameTarget>,
}

impl Headless {
    pub fn new() -> Result<Self, Error> {
        Self::open(&[true, false])
    }

    /// Like [`Headless::new`] but only a hardware adapter, for measuring
    /// speed.
    pub fn hardware() -> Result<Self, Error> {
        Self::open(&[false])
    }

    fn open(fallbacks: &[bool]) -> Result<Self, Error> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = fallbacks
            .iter()
            .find_map(|&fallback| {
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    force_fallback_adapter: fallback,
                    ..Default::default()
                }))
                .ok()
            })
            .ok_or_else(|| Error::Gpu("no adapter".into()))?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
            .map_err(|e| Error::Gpu(e.to_string()))?;
        let renderer = Renderer::new(&device);
        Ok(Self {
            device,
            queue,
            renderer,
            adapter: format!("{} ({:?})", info.name, info.backend),
            frame: None,
        })
    }

    /// The adapter's name and backend.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Draws `list` over a persistent `width` x `height` target (cleared
    /// first when `clear`) and submits it, without waiting for the GPU or
    /// reading anything back. For measuring speed: [`Headless::wait`] ends a
    /// frame.
    pub fn draw(
        &mut self,
        list: &DrawList<'_>,
        view: [f32; 4],
        clear: bool,
        width: u32,
        height: u32,
    ) -> Result<(), Error> {
        if self
            .frame
            .as_ref()
            .is_none_or(|f| f.size != (width, height))
        {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("frame"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: OUTPUT_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.frame = Some(FrameTarget {
                size: (width, height),
                view,
            });
        }
        let frame = self.frame.as_ref().expect("frame target made above");
        self.renderer.render_to_view_with(
            &self.device,
            &self.queue,
            list,
            view,
            &frame.view,
            (width, height),
            DrawOptions {
                clear,
                ..Default::default()
            },
        )
    }

    /// Waits for the GPU to finish what was submitted. Returns how long the
    /// wait took: after one [`Headless::draw`], an upper bound of its GPU
    /// time (it includes the submission latency). Metal's pass timestamps
    /// cannot be used (the end of a pass reads earlier than its start), so
    /// the speed of the GPU is measured this way.
    pub fn wait(&mut self) -> Result<std::time::Duration, Error> {
        let start = std::time::Instant::now();
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| Error::Gpu(e.to_string()))?;
        Ok(start.elapsed())
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
        self.render_with(list, view, width, height, None)
    }

    /// [`Headless::render`] of the one mesh `mesh` (a mesh index), shown even
    /// when it is hidden.
    pub fn render_only(
        &mut self,
        list: &DrawList,
        view: [f32; 4],
        mesh: u32,
        width: u32,
        height: u32,
    ) -> Result<Image, Error> {
        self.render_with(list, view, width, height, Some(mesh))
    }

    fn render_with(
        &mut self,
        list: &DrawList,
        view: [f32; 4],
        width: u32,
        height: u32,
        only: Option<u32>,
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
        self.renderer.render_to_view_with(
            &self.device,
            &self.queue,
            list,
            view,
            &target.create_view(&Default::default()),
            (width, height),
            DrawOptions {
                only,
                ..Default::default()
            },
        )?;

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
        let mapped = buffer
            .get_mapped_range(..)
            .map_err(|e| Error::Gpu(e.to_string()))?;
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
