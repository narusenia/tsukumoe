// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use tsukumoe_core::draw::{DrawList, Drawable};
use tsukumoe_core::rig::BlendMode;
use wgpu::util::DeviceExt;

use crate::Error;

/// The only target format: results are compared as integers across machines.
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// RGBA8 pixels with straight alpha, rows top to bottom.
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Texture {
    pub fn from_png(bytes: &[u8]) -> Result<Self, Error> {
        let (width, height, rgba) = crate::decode_png(bytes)?;
        Ok(Self {
            width,
            height,
            rgba,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    view: [f32; 4],
    multiply: [f32; 4],
    screen: [f32; 4],
    opacity: f32,
    mask_mode: u32,
    pad: [u32; 2],
}

/// The spacing of per-drawable uniform slots: the largest offset alignment
/// any backend asks for.
const UNIFORM_STRIDE: usize = 256;

struct Pipelines {
    /// `[blend mode][double sided]`
    main: [[wgpu::RenderPipeline; 2]; 3],
    /// `[double sided]`
    mask: [wgpu::RenderPipeline; 2],
}

struct Mask {
    size: (u32, u32),
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

/// How [`Renderer::render_to_view_with`] differs from [`Renderer::render_to_view`].
#[derive(Clone, Copy)]
pub struct DrawOptions {
    /// Clear the target first; `false` draws over what is there.
    pub clear: bool,
}

impl Default for DrawOptions {
    fn default() -> Self {
        Self { clear: true }
    }
}

/// Draws a draw list into any `Rgba8Unorm` texture of a device the caller owns.
pub struct Renderer {
    pipelines: Pipelines,
    params_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    mask_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: Vec<wgpu::BindGroup>,
    mask: Option<Mask>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(size_of::<Params>() as u64),
            },
            count: None,
        };
        let texture_entry = |binding, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = |entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries,
            })
        };
        let params_layout = layout(&[uniform(0)]);
        let texture_layout = layout(&[
            texture_entry(0, true),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ]);
        let mask_layout = layout(&[texture_entry(0, false)]);

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let pipeline_layout = |layouts: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: layouts,
                immediate_size: 0,
            })
        };
        let main_layout = pipeline_layout(&[
            Some(&params_layout),
            Some(&texture_layout),
            Some(&mask_layout),
        ]);
        let mask_pipeline_layout = pipeline_layout(&[Some(&params_layout), Some(&texture_layout)]);

        let vertex_buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: 8,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 8,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![1 => Float32x2],
            }),
        ];
        let make = |layout: &wgpu::PipelineLayout,
                    fragment: &str,
                    format,
                    blend: wgpu::BlendState,
                    double_sided: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &vertex_buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: (!double_sided).then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        use wgpu::BlendFactor as F;
        let component = |src, dst| wgpu::BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: wgpu::BlendOperation::Add,
        };
        let blend = |color, alpha| wgpu::BlendState { color, alpha };
        let blends = [
            // Normal
            blend(
                component(F::One, F::OneMinusSrcAlpha),
                component(F::One, F::OneMinusSrcAlpha),
            ),
            // Additive
            blend(component(F::One, F::One), component(F::Zero, F::One)),
            // Multiply
            blend(
                component(F::Dst, F::OneMinusSrcAlpha),
                component(F::Zero, F::One),
            ),
        ];
        let max = wgpu::BlendComponent {
            src_factor: F::One,
            dst_factor: F::One,
            operation: wgpu::BlendOperation::Max,
        };
        let pipelines = Pipelines {
            main: blends
                .map(|b| [false, true].map(|d| make(&main_layout, "fs", OUTPUT_FORMAT, b, d))),
            mask: [false, true].map(|d| {
                make(
                    &mask_pipeline_layout,
                    "fs_mask",
                    MASK_FORMAT,
                    blend(max, max),
                    d,
                )
            }),
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipelines,
            params_layout,
            texture_layout,
            mask_layout,
            sampler,
            textures: Vec::new(),
            mask: None,
        }
    }

    /// Replaces the registered textures; a drawable's `texture` indexes this
    /// slice. Colors are premultiplied here (see `rendering.md` 4).
    pub fn set_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        textures: &[Texture],
    ) -> Result<(), Error> {
        let mut bound = Vec::with_capacity(textures.len());
        for t in textures {
            let len = t.width as usize * t.height as usize * 4;
            if t.rgba.len() != len || t.width == 0 || t.height == 0 {
                return Err(Error::BadTexture {
                    width: t.width,
                    height: t.height,
                    len: t.rgba.len(),
                });
            }
            let mut data = t.rgba.clone();
            for px in data.as_chunks_mut::<4>().0 {
                let a = u32::from(px[3]);
                for c in &mut px[..3] {
                    *c = ((u32::from(*c) * a + 127) / 255) as u8;
                }
            }
            let texture = device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: t.width,
                        height: t.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &data,
            );
            let view = texture.create_view(&Default::default());
            bound.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            }));
        }
        self.textures = bound;
        Ok(())
    }

    /// Clears `target` and draws `list` into it. `view` is
    /// `(x_min, y_min, x_max, y_max)` of drawing space.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        list: &DrawList,
        view: [f32; 4],
        target: &wgpu::Texture,
    ) -> Result<(), Error> {
        if target.format() != OUTPUT_FORMAT
            || !target
                .usage()
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        {
            return Err(Error::BadTarget);
        }
        let target_view = target.create_view(&Default::default());
        self.render_to_view(
            device,
            queue,
            list,
            view,
            &target_view,
            (target.width(), target.height()),
        )
    }

    /// Like [`Renderer::render`], into a view the caller owns, such as the back
    /// buffer of a UI surface. A view does not expose its texture's format or
    /// usage, so the caller must pass an `Rgba8Unorm` texture with
    /// `RENDER_ATTACHMENT`, `size` wide and high.
    pub fn render_to_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        list: &DrawList,
        view: [f32; 4],
        target_view: &wgpu::TextureView,
        size: (u32, u32),
    ) -> Result<(), Error> {
        self.render_to_view_with(
            device,
            queue,
            list,
            view,
            target_view,
            size,
            DrawOptions::default(),
        )
    }

    /// [`Renderer::render_to_view`] that can keep the target's contents.
    #[allow(clippy::too_many_arguments)]
    pub fn render_to_view_with(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        list: &DrawList,
        view: [f32; 4],
        target_view: &wgpu::TextureView,
        size: (u32, u32),
        options: DrawOptions,
    ) -> Result<(), Error> {
        // Check every texture first so a failure leaves the target alone.
        for d in list.drawables() {
            if self.textures.get(d.texture as usize).is_none() {
                return Err(Error::MissingTexture(d.texture));
            }
        }
        self.ensure_mask(device, size);

        let vertex = |data: &[f32]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::VERTEX,
            })
        };
        let positions = vertex(list.positions());
        let uvs = vertex(list.uvs());
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(list.indices()),
            usage: wgpu::BufferUsages::INDEX,
        });
        let view_params = [
            view[0],
            view[1],
            1.0 / (view[2] - view[0]),
            1.0 / (view[3] - view[1]),
        ];
        // Every drawable's parameters go into one buffer, a slot each at a
        // dynamic offset, rather than a buffer and a bind group each. Slot 0
        // is for the mask passes.
        let slot = |n: usize| (n * UNIFORM_STRIDE) as u32;
        let order = list.order();
        let mut uniforms = vec![0u8; UNIFORM_STRIDE * (order.len() + 1)];
        let mut put = |n: usize, p: Params| {
            uniforms[n * UNIFORM_STRIDE..][..size_of::<Params>()]
                .copy_from_slice(bytemuck::bytes_of(&p));
        };
        put(
            0,
            Params {
                view: view_params,
                multiply: [1.0; 4],
                screen: [0.0; 4],
                opacity: 1.0,
                mask_mode: 0,
                pad: [0; 2],
            },
        );
        for (i, &mesh) in order.iter().enumerate() {
            let d = list.drawable(mesh as usize);
            if d.opacity <= 0.0 || d.indices.is_empty() {
                continue;
            }
            put(
                i + 1,
                Params {
                    view: view_params,
                    multiply: [
                        d.multiply_color[0],
                        d.multiply_color[1],
                        d.multiply_color[2],
                        1.0,
                    ],
                    screen: [d.screen_color[0], d.screen_color[1], d.screen_color[2], 0.0],
                    opacity: d.opacity,
                    mask_mode: match (!d.masks.is_empty(), d.mask_inverted) {
                        (false, _) => 0,
                        (true, false) => 1,
                        (true, true) => 2,
                    },
                    pad: [0; 2],
                },
            );
        }
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &uniforms,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let params = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.params_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(size_of::<Params>() as u64),
                }),
            }],
        });
        let mask = self.mask.as_ref().expect("mask ensured above");

        let mut encoder = device.create_command_encoder(&Default::default());
        let begin_pass =
            |encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, clear: bool| {
                encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if clear {
                                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                                } else {
                                    wgpu::LoadOp::Load
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    })
                    .forget_lifetime()
            };
        let draw = |pass: &mut wgpu::RenderPass<'_>,
                    pipeline: &wgpu::RenderPipeline,
                    offset: u32,
                    groups: &[&wgpu::BindGroup],
                    d: &Drawable| {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, Some(&params), &[offset]);
            for (i, g) in groups.iter().enumerate() {
                pass.set_bind_group(i as u32 + 1, Some(*g), &[]);
            }
            // The vertex offset goes into the buffer binding rather than into
            // `draw_indexed`'s base vertex, which WebGL2 does not have.
            let offset = d.vertex_start as u64 * 8;
            pass.set_vertex_buffer(0, positions.slice(offset..));
            pass.set_vertex_buffer(1, uvs.slice(offset..));
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            let first = d.index_start as u32;
            pass.draw_indexed(first..first + d.indices.len() as u32, 0, 0..1);
        };
        let drawn = |mesh: u32| {
            let d = list.drawable(mesh as usize);
            d.opacity > 0.0 && !d.indices.is_empty()
        };
        // Nothing drawn: an empty pass still clears the target.
        if !list.order().iter().any(|&m| drawn(m)) {
            drop(begin_pass(&mut encoder, target_view, options.clear));
        }

        // Drawables that need no mask share one pass; a masked one ends it,
        // because its mask is drawn by passes of its own.
        let mut clear = options.clear;
        let mut main: Option<wgpu::RenderPass<'static>> = None;
        for (position, &mesh) in order.iter().enumerate() {
            let d = list.drawable(mesh as usize);
            if !drawn(mesh) {
                continue;
            }
            let texture = &self.textures[d.texture as usize];
            let masked = !d.masks.is_empty();
            if masked {
                main = None;
                let mut first = true;
                for &m in d.masks {
                    let md = list.drawable(m as usize);
                    let Some(mask_texture) = self.textures.get(md.texture as usize) else {
                        return Err(Error::MissingTexture(md.texture));
                    };
                    if md.indices.is_empty() {
                        continue;
                    }
                    let mut pass = begin_pass(&mut encoder, &mask.view, first);
                    draw(
                        &mut pass,
                        &self.pipelines.mask[usize::from(md.double_sided)],
                        slot(0),
                        &[mask_texture],
                        &md,
                    );
                    first = false;
                }
                if first {
                    // No mask mesh has triangles: the mask value is 0 everywhere.
                    drop(begin_pass(&mut encoder, &mask.view, true));
                }
            }
            let mode = match d.blend_mode {
                BlendMode::Normal => 0,
                BlendMode::Additive => 1,
                BlendMode::Multiply => 2,
            };
            let pass = main.get_or_insert_with(|| {
                begin_pass(&mut encoder, target_view, std::mem::take(&mut clear))
            });
            draw(
                pass,
                &self.pipelines.main[mode][usize::from(d.double_sided)],
                slot(position + 1),
                &[texture, &mask.bind],
                &d,
            );
        }
        drop(main);
        queue.submit([encoder.finish()]);
        Ok(())
    }

    fn ensure_mask(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.mask.as_ref().is_some_and(|m| m.size == size) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mask"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: MASK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.mask_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        self.mask = Some(Mask { size, view, bind });
    }
}
