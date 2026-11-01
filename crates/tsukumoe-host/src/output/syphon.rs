// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The Syphon sink. The renderer draws `Rgba8Unorm` and Syphon wants
//! `Bgra8Unorm`, which a texture copy cannot convert, so each frame goes
//! through one full-screen pass that reads the frame and writes a BGRA texture
//! (the channels are swapped by the format, the values are untouched, so
//! premultiplied alpha stays premultiplied).

use syphon_wgpu::SyphonWgpuOutput;

use super::{FrameSink, SinkStatus};

const SHADER: &str = "
@group(0) @binding(0) var src: texture_2d<f32>;
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(src, vec2<i32>(pos.xy), 0);
}
";

const SYPHON_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

const INSTALL_HINT: &str = "Syphon.framework was not found. Install it from the official Syphon release (https://github.com/Syphon/Syphon-Framework/releases) and run again";

struct Gpu {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

pub struct SyphonSink {
    name: String,
    gpu: Option<Gpu>,
    /// The server and the BGRA texture it publishes, both of one size.
    server: Option<(SyphonWgpuOutput, wgpu::Texture, (u32, u32))>,
    clients: bool,
}

impl SyphonSink {
    /// Fails when Syphon.framework is not loadable. The server itself is
    /// created at the first frame, once the size and the device are known.
    pub fn new(name: &str) -> Result<Self, String> {
        if !syphon_wgpu::is_available() {
            return Err(INSTALL_HINT.into());
        }
        Ok(Self {
            name: name.to_owned(),
            gpu: None,
            server: None,
            clients: false,
        })
    }

    fn gpu(&mut self, device: &wgpu::Device) -> &Gpu {
        self.gpu.get_or_insert_with(|| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("syphon-blit"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("syphon-blit"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(SYPHON_FORMAT.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            Gpu { pipeline, layout }
        })
    }
}

impl FrameSink for SyphonSink {
    fn name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> SinkStatus {
        if self.clients {
            SinkStatus::Publishing
        } else {
            SinkStatus::NoClients
        }
    }

    fn publish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::Texture,
    ) -> Result<(), String> {
        let size = (frame.width(), frame.height());
        // shortcut: the server is rebuilt on every size change, so dragging the
        // window edge recreates it per frame; fine until that shows up in use.
        if self.server.as_ref().is_none_or(|s| s.2 != size) {
            let output = SyphonWgpuOutput::new(&self.name, device, queue, size.0, size.1)
                .map_err(|e| format!("Syphon: {e}"))?;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("syphon"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SYPHON_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            self.server = Some((output, texture, size));
        }
        self.clients = self.server.as_ref().is_some_and(|s| s.0.has_clients());
        // Nobody receives: skip the pass as well as the publish.
        if !self.clients {
            return Ok(());
        }
        let gpu = self.gpu(device);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &gpu.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &frame.create_view(&Default::default()),
                ),
            }],
        });
        let (output, texture, _) = self.server.as_mut().expect("the server was just made");
        let view = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("syphon-blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.gpu.as_ref().expect("built above").pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
        output.publish(texture, device, queue);
        Ok(())
    }
}
