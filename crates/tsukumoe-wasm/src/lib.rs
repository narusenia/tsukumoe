// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! WebAssembly build of the runtime and the wgpu renderer, for the browser.
//! This is the stage 1 surface (load, parameters, update, draw, pixel
//! readback); the packaged API comes with the npm package.

#![cfg(target_arch = "wasm32")]

use std::cell::Cell;
use std::rc::Rc;

use tsukumoe_core::tmod::{AlignedBuf, Tmod};
use tsukumoe_gpu::{OUTPUT_FORMAT, Renderer, Texture};
use tsukumoe_runtime::Runtime;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

fn err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

/// Resolves on the next task, so the browser can run GPU callbacks.
async fn yield_task() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback(&resolve);
        }
    });
    let _ = JsFuture::from(promise).await;
}

/// A model on a canvas.
#[wasm_bindgen]
pub struct Player {
    runtime: Runtime<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    view: [f32; 4],
    backend: String,
    /// Milliseconds: instance, adapter, device, renderer, textures.
    timings: Vec<f64>,
}

#[wasm_bindgen]
impl Player {
    /// Loads `tmod` and prepares `canvas`. WebGPU is used when the browser has
    /// it; `force_webgl` selects WebGL2 instead.
    pub async fn create(
        canvas: web_sys::HtmlCanvasElement,
        tmod: &[u8],
        force_webgl: bool,
    ) -> Result<Player, JsValue> {
        let t0 = now();
        let buf = AlignedBuf::from_bytes(tmod);
        let parsed = Tmod::parse(&buf).map_err(err)?;
        let textures = (0..parsed.texture_count())
            .map(|i| {
                let png = parsed.texture(i).ok_or("texture outside the data")?;
                Texture::from_png(png).map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let runtime = Runtime::from_bytes(tmod).map_err(err)?;
        let t_load = now();

        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = if force_webgl {
            wgpu::Backends::GL
        } else {
            wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
        };
        let instance = wgpu::util::new_instance_with_webgpu_detection(desc).await;
        let t_instance = now();
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(err)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(err)?;
        let t_adapter = now();
        let backend = format!("{:?}", adapter.get_info().backend);
        // WebGL2 has lower limits than the desktop defaults.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(err)?;
        let t_device = now();

        let caps = surface.get_capabilities(&adapter);
        if !caps.formats.contains(&OUTPUT_FORMAT) {
            return Err(err(format!(
                "surface formats {:?} lack Rgba8Unorm",
                caps.formats
            )));
        }
        let alpha_mode = if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else {
            caps.alpha_modes[0]
        };
        surface.configure(
            &device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: OUTPUT_FORMAT,
                width: canvas.width().max(1),
                height: canvas.height().max(1),
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode,
                color_space: Default::default(),
                view_formats: vec![],
            },
        );

        let mut renderer = Renderer::new(&device);
        let t_renderer = now();
        renderer
            .set_textures(&device, &queue, &textures)
            .map_err(err)?;
        let t_textures = now();

        let mut player = Player {
            runtime,
            device,
            queue,
            surface,
            renderer,
            view: [0.0, 0.0, 1.0, 1.0],
            backend,
            timings: vec![
                t_load - t0,
                t_instance - t_load,
                t_adapter - t_instance,
                t_device - t_adapter,
                t_renderer - t_device,
                t_textures - t_renderer,
            ],
        };
        player.runtime.update(0.0);
        player.view = player.bounds_array();
        Ok(player)
    }

    /// `wgpu` backend in use, e.g. `BrowserWebGpu` or `Gl`.
    pub fn backend(&self) -> String {
        self.backend.clone()
    }

    /// Milliseconds spent in: parse and PNG decode, instance (with WebGPU
    /// detection), adapter, device, renderer (pipelines), texture upload.
    pub fn timings(&self) -> Vec<f64> {
        self.timings.clone()
    }

    /// Returns false when the model has no such parameter.
    pub fn set_parameter(&mut self, name: &str, value: f32) -> bool {
        self.runtime
            .parameter_index(name)
            .is_some_and(|i| self.runtime.set_parameter(i, value))
    }

    pub fn update(&mut self, dt: f32) {
        self.runtime.update(dt);
    }

    /// The bounding box `[x0, y0, x1, y1]` of the current draw list.
    pub fn bounds(&self) -> Vec<f32> {
        self.bounds_array().to_vec()
    }

    fn bounds_array(&self) -> [f32; 4] {
        let list = self.runtime.draw_list();
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for p in list.positions().as_chunks::<2>().0 {
            b = [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ];
        }
        b
    }

    /// The drawing-space rectangle shown, `[x0, y0, x1, y1]`.
    pub fn set_view(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.view = [x0, y0, x1, y1];
    }

    /// Draws the last update into the canvas.
    pub fn render(&mut self) -> Result<(), JsValue> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            other => return Err(err(format!("surface texture: {other:?}"))),
        };
        let view = frame.texture.create_view(&Default::default());
        let size = (frame.texture.width(), frame.texture.height());
        self.renderer
            .render_to_view(
                &self.device,
                &self.queue,
                &self.runtime.draw_list(),
                self.view,
                &view,
                size,
            )
            .map_err(err)?;
        self.queue.present(frame);
        Ok(())
    }

    /// Draws the last update into an off-screen `width` x `height` target and
    /// returns its premultiplied RGBA8 pixels, rows top to bottom (the same
    /// layout as `tsukumoe_gpu::Image`). `width * 4` must be a multiple of 256.
    pub async fn read_pixels(&mut self, width: u32, height: u32) -> Result<Vec<u8>, JsValue> {
        let row = width * 4;
        if row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT != 0 {
            return Err(err("width * 4 must be a multiple of 256"));
        }
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
            .render(
                &self.device,
                &self.queue,
                &self.runtime.draw_list(),
                self.view,
                &target,
            )
            .map_err(err)?;
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
        let done = Rc::new(Cell::new(None));
        let flag = done.clone();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| flag.set(Some(r.is_ok())));
        // WebGL2 runs wgpu-core, whose callbacks fire on `poll`.
        while done.get().is_none() {
            let _ = self.device.poll(wgpu::PollType::Poll);
            yield_task().await;
        }
        if done.get() != Some(true) {
            return Err(err("mapping the readback buffer failed"));
        }
        let mapped = buffer.get_mapped_range(..).map_err(err)?;
        Ok(mapped.to_vec())
    }
}
