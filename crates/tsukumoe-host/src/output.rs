// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Publishing finished frames to other programs. Knows nothing about the UI
//! toolkit: a [`FrameSink`] takes a texture, and [`Output`] decides which
//! texture, at which size, and when. Syphon and Spout2 are sinks; others (a virtual
//! camera) plug into the same trait.

#[cfg(any(all(feature = "spout", windows), test))]
mod readback;
#[cfg(all(feature = "spout", windows))]
mod spout;
#[cfg(all(feature = "spout", windows))]
pub use spout::SpoutSink;
#[cfg(all(feature = "syphon", target_os = "macos"))]
mod syphon;
#[cfg(all(feature = "syphon", target_os = "macos"))]
pub use syphon::SyphonSink;

/// Whether this build can publish through Syphon.
pub const SYPHON_AVAILABLE: bool = cfg!(all(feature = "syphon", target_os = "macos"));

/// Whether this build can publish through Spout2.
pub const SPOUT_AVAILABLE: bool = cfg!(all(feature = "spout", windows));

/// What a sink is doing, for the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkStatus {
    /// Published, but nobody receives it.
    NoClients,
    Publishing,
}

/// Takes finished frames.
pub trait FrameSink {
    /// The name receivers see.
    fn name(&self) -> &str;

    fn status(&self) -> SinkStatus;

    /// Publishes one frame: an `Rgba8Unorm` texture with premultiplied alpha
    /// and `TEXTURE_BINDING` and `COPY_SRC` usage. The sink may not keep it past the call.
    fn publish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::Texture,
    ) -> Result<(), String>;
}

/// Parses `<w>x<h>`: two sizes from 1 to [`MAX_SIDE`].
pub fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let bad = || format!("'{text}' is not a size like 1920x1080 (each side 1-{MAX_SIDE})");
    let (w, h) = text.split_once('x').ok_or_else(bad)?;
    let side = |s: &str| {
        s.parse::<u32>()
            .ok()
            .filter(|n| (1..=MAX_SIDE).contains(n))
            .ok_or_else(bad)
    };
    Ok((side(w)?, side(h)?))
}

pub const MAX_SIDE: u32 = 16384;

/// The attached sink, if any, and the size it publishes at.
pub struct Output {
    sink: Option<Box<dyn FrameSink>>,
    /// Publish at this size, drawn separately from the window.
    fixed: Option<(u32, u32)>,
    own: Option<(wgpu::Texture, wgpu::TextureView)>,
}

impl Output {
    pub fn new(fixed: Option<(u32, u32)>) -> Self {
        Self {
            sink: None,
            fixed,
            own: None,
        }
    }

    pub fn attach(&mut self, sink: Box<dyn FrameSink>) {
        self.sink = Some(sink);
    }

    pub fn detach(&mut self) {
        self.sink = None;
        self.own = None;
    }

    pub fn enabled(&self) -> bool {
        self.sink.is_some()
    }

    pub fn sink(&self) -> Option<&dyn FrameSink> {
        self.sink.as_deref()
    }

    /// Hands the frame to the sink; does nothing when none is attached. With a
    /// fixed size, `draw` first renders into a texture view of that size and
    /// the sink gets that texture; otherwise it gets `window`.
    pub fn publish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        window: &wgpu::Texture,
        draw: impl FnOnce(&wgpu::TextureView, (u32, u32)) -> Result<(), String>,
    ) -> Result<(), String> {
        let Some(sink) = self.sink.as_mut() else {
            return Ok(());
        };
        let Some(size) = self.fixed else {
            return sink.publish(device, queue, window);
        };
        let (texture, view) = self.own.get_or_insert_with(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("output"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: tsukumoe_gpu::OUTPUT_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            (texture, view)
        });
        draw(view, size)?;
        sink.publish(device, queue, texture)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// A device from any adapter, or `None` where the machine has none.
    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = [true, false].into_iter().find_map(|fallback| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                force_fallback_adapter: fallback,
                ..Default::default()
            }))
            .ok()
        })?;
        pollster::block_on(adapter.request_device(&Default::default())).ok()
    }

    fn texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: tsukumoe_gpu::OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    /// Records the size of every frame it is given.
    struct Fake(Rc<RefCell<Vec<(u32, u32)>>>);

    impl FrameSink for Fake {
        fn name(&self) -> &str {
            "fake"
        }
        fn status(&self) -> SinkStatus {
            SinkStatus::Publishing
        }
        fn publish(
            &mut self,
            _: &wgpu::Device,
            _: &wgpu::Queue,
            frame: &wgpu::Texture,
        ) -> Result<(), String> {
            self.0.borrow_mut().push((frame.width(), frame.height()));
            Ok(())
        }
    }

    #[test]
    fn each_frame_reaches_the_sink_once_and_nothing_is_drawn_for_the_window_size() {
        let Some((device, queue)) = gpu() else { return };
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut out = Output::new(None);
        out.attach(Box::new(Fake(log.clone())));
        let window = texture(&device, 64, 32);
        for _ in 0..3 {
            out.publish(&device, &queue, &window, |_, _| {
                panic!("the window's own frame is published")
            })
            .unwrap();
        }
        assert_eq!(*log.borrow(), [(64, 32); 3]);
    }

    #[test]
    fn a_detached_output_calls_nothing() {
        let Some((device, queue)) = gpu() else { return };
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut out = Output::new(Some((16, 16)));
        let window = texture(&device, 64, 32);
        let called = RefCell::new(false);
        out.publish(&device, &queue, &window, |_, _| {
            *called.borrow_mut() = true;
            Ok(())
        })
        .unwrap();
        assert!(!*called.borrow() && log.borrow().is_empty());
        out.attach(Box::new(Fake(log.clone())));
        out.detach();
        assert!(!out.enabled());
        out.publish(&device, &queue, &window, |_, _| Ok(()))
            .unwrap();
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn a_fixed_size_is_drawn_separately_and_published_at_that_size() {
        let Some((device, queue)) = gpu() else { return };
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut out = Output::new(Some((48, 24)));
        out.attach(Box::new(Fake(log.clone())));
        let window = texture(&device, 64, 32);
        let drawn = RefCell::new(Vec::new());
        for _ in 0..2 {
            out.publish(&device, &queue, &window, |_, size| {
                drawn.borrow_mut().push(size);
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(*drawn.borrow(), [(48, 24); 2]);
        assert_eq!(*log.borrow(), [(48, 24); 2]);
    }

    #[test]
    fn a_failed_draw_publishes_nothing() {
        let Some((device, queue)) = gpu() else { return };
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut out = Output::new(Some((8, 8)));
        out.attach(Box::new(Fake(log.clone())));
        let window = texture(&device, 4, 4);
        let result = out.publish(&device, &queue, &window, |_, _| Err("boom".into()));
        assert_eq!(result, Err("boom".into()));
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn sizes_parse_as_width_x_height() {
        assert_eq!(parse_size("1920x1080"), Ok((1920, 1080)));
        for bad in [
            "", "1920", "0x10", "10x0", "axb", "10x", "x10", "16385x10", "-1x5", "1x2x3",
        ] {
            assert!(parse_size(bad).is_err(), "{bad}");
        }
    }
}
