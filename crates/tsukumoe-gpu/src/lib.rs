// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The wgpu reference renderer: a draw list in, an image out. The rules it
//! follows are in `docs/specifications/rendering.md`.

mod headless;
mod renderer;

pub use headless::{Headless, Image};
pub use renderer::{OUTPUT_FORMAT, Renderer, Texture};

#[derive(Debug)]
pub enum Error {
    /// No adapter, or the adapter refused to give a device.
    Gpu(String),
    /// A drawable uses a texture that was not registered.
    MissingTexture(u32),
    /// Texture pixel data does not match its size.
    BadTexture { width: u32, height: u32, len: usize },
    /// The target texture is not `Rgba8Unorm` with `RENDER_ATTACHMENT`.
    BadTarget,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Gpu(m) => write!(f, "gpu: {m}"),
            Error::MissingTexture(i) => write!(f, "texture {i} is not registered"),
            Error::BadTexture { width, height, len } => {
                write!(
                    f,
                    "a {width}x{height} texture needs {} bytes, got {len}",
                    *width as usize * *height as usize * 4
                )
            }
            Error::BadTarget => write!(f, "target must be Rgba8Unorm with RENDER_ATTACHMENT"),
        }
    }
}

impl std::error::Error for Error {}
