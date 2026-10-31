// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! WebAssembly build of the runtime and the wgpu renderer, for the browser.
//! [`Model`] is the playback side (no graphics, so it runs in Node too);
//! [`Screen`] draws a model into a canvas. The npm package
//! `packages/tsukumoe-web` wraps both; `docs/specifications/web-api.md` is
//! the contract.

#![cfg(target_arch = "wasm32")]

mod model;

use std::cell::Cell;
use std::rc::Rc;

use tsukumoe_gpu::{OUTPUT_FORMAT, Renderer, Texture};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// A JS `Error` with the message of `e`.
pub(crate) fn err(e: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&e.to_string()).into()
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

/// Which graphics API to use.
#[derive(Clone, Copy, PartialEq)]
enum Choice {
    Auto,
    WebGpu,
    WebGl2,
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    backend: String,
    /// Milliseconds: instance, adapter, device.
    timings: [f64; 3],
}

/// WebGPU first: its adapter is requested before the canvas is touched, so a
/// browser that has `navigator.gpu` but no adapter leaves the canvas free for
/// WebGL2.
async fn open_webgpu(canvas: &web_sys::HtmlCanvasElement) -> Result<Gpu, JsValue> {
    let t0 = now();
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::BROWSER_WEBGPU;
    let instance = wgpu::Instance::new(desc);
    let t_instance = now();
    let adapter = instance
        .request_adapter(&Default::default())
        .await
        .map_err(err)?;
    let t_adapter = now();
    let (device, queue) = adapter
        .request_device(&Default::default())
        .await
        .map_err(err)?;
    let t_device = now();
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(err)?;
    let caps = surface.get_capabilities(&adapter);
    if !caps.formats.contains(&OUTPUT_FORMAT) {
        return Err(err(format!(
            "surface formats {:?} lack Rgba8Unorm",
            caps.formats
        )));
    }
    Ok(Gpu {
        config: surface_config(canvas, wgpu::CompositeAlphaMode::PreMultiplied),
        device,
        queue,
        surface,
        backend: format!("{:?}", adapter.get_info().backend),
        timings: [
            t_instance - t0,
            t_adapter - t_instance,
            t_device - t_adapter,
        ],
    })
}

async fn open_webgl2(canvas: &web_sys::HtmlCanvasElement) -> Result<Gpu, JsValue> {
    let t0 = now();
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::GL;
    let instance = wgpu::Instance::new(desc);
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
    let alpha = if caps
        .alpha_modes
        .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
    {
        wgpu::CompositeAlphaMode::PreMultiplied
    } else {
        caps.alpha_modes[0]
    };
    Ok(Gpu {
        config: surface_config(canvas, alpha),
        device,
        queue,
        surface,
        backend: format!("{:?}", adapter.get_info().backend),
        timings: [
            t_instance - t0,
            t_adapter - t_instance,
            t_device - t_adapter,
        ],
    })
}

fn surface_config(
    canvas: &web_sys::HtmlCanvasElement,
    alpha_mode: wgpu::CompositeAlphaMode,
) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: OUTPUT_FORMAT,
        width: canvas.width().max(1),
        height: canvas.height().max(1),
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode,
        color_space: Default::default(),
        view_formats: vec![],
    }
}

use model::Model;

/// A canvas with a graphics device, drawing a [`Model`].
#[wasm_bindgen]
pub struct Screen {
    canvas: web_sys::HtmlCanvasElement,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    view: [f32; 4],
    backend: String,
    /// Milliseconds: PNG decode, instance, adapter, device, renderer
    /// (pipelines), texture upload.
    timings: Vec<f64>,
}

#[wasm_bindgen]
impl Screen {
    /// Prepares `canvas` for drawing `model`. `backend` is `"auto"` (WebGPU,
    /// else WebGL2), `"webgpu"` or `"webgl2"`. WebGPU is tried before the
    /// canvas is touched, so on failure with `"auto"` WebGL2 gets the same
    /// canvas; a failure is reported with a message.
    pub async fn create(
        canvas: web_sys::HtmlCanvasElement,
        model: &Model,
        backend: &str,
    ) -> Result<Screen, JsValue> {
        let choice = match backend {
            "auto" => Choice::Auto,
            "webgpu" => Choice::WebGpu,
            "webgl2" => Choice::WebGl2,
            other => return Err(err(format!("unknown backend {other:?}"))),
        };
        let t0 = now();
        let textures = (0..model.texture_count())
            .map(|i| Texture::from_png(&model.texture_png(i).unwrap_or_default()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(err)?;
        let t_decode = now() - t0;

        let gpu = match choice {
            Choice::WebGl2 => open_webgl2(&canvas).await?,
            Choice::WebGpu => open_webgpu(&canvas).await?,
            Choice::Auto => match open_webgpu(&canvas).await {
                Ok(g) => g,
                Err(_) => open_webgl2(&canvas).await?,
            },
        };
        let Gpu {
            device,
            queue,
            surface,
            config,
            backend,
            timings: [t_instance, t_adapter, t_device],
        } = gpu;
        surface.configure(&device, &config);

        let t = now();
        let mut renderer = Renderer::new(&device);
        let t_renderer = now() - t;
        let t = now();
        renderer
            .set_textures(&device, &queue, &textures)
            .map_err(err)?;
        let t_textures = now() - t;

        let mut screen = Screen {
            canvas,
            device,
            queue,
            surface,
            config,
            renderer,
            view: [0.0, 0.0, 1.0, 1.0],
            backend,
            timings: vec![
                t_decode, t_instance, t_adapter, t_device, t_renderer, t_textures,
            ],
        };
        screen.fit_view(model);
        Ok(screen)
    }

    /// `wgpu` backend in use: `BrowserWebGpu` or `Gl`.
    pub fn backend(&self) -> String {
        self.backend.clone()
    }

    /// Milliseconds spent in: PNG decode, instance, adapter, device, renderer
    /// (pipelines), texture upload.
    pub fn timings(&self) -> Vec<f64> {
        self.timings.clone()
    }

    /// The drawing-space rectangle shown, `[x0, y0, x1, y1]`.
    pub fn set_view(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.view = [x0, y0, x1, y1];
    }

    /// Fits the view to the model, centred, with the canvas's aspect ratio.
    pub fn fit_view(&mut self, model: &Model) {
        let b = model.bounds_array();
        let aspect = self.config.width as f32 / self.config.height as f32;
        let (w, h) = (b[2] - b[0], b[3] - b[1]);
        let (w, h) = if w / h.max(f32::MIN_POSITIVE) > aspect {
            (w, w / aspect)
        } else {
            (h * aspect, h)
        };
        let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        self.view = [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0];
    }

    /// Sets the size of the canvas (in pixels) and of the surface.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.canvas.set_width(width.max(1));
        self.canvas.set_height(height.max(1));
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    /// Draws the last update into the canvas.
    pub fn render(&mut self, model: &Model) -> Result<(), JsValue> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            // Nothing to draw into right now; the next frame will do.
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            other => return Err(err(format!("surface texture: {other:?}"))),
        };
        let view = frame.texture.create_view(&Default::default());
        let size = (frame.texture.width(), frame.texture.height());
        self.renderer
            .render_to_view(
                &self.device,
                &self.queue,
                &model.runtime.draw_list(),
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
    pub async fn read_pixels(
        &mut self,
        model: &Model,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, JsValue> {
        let row = width * 4;
        if !row.is_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) {
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
                &model.runtime.draw_list(),
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
