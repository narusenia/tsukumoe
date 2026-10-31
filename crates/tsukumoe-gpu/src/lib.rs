// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The wgpu reference renderer: a draw list in, an image out. The rules it
//! follows are in `docs/specifications/rendering.md`.

mod headless;
mod renderer;

pub use headless::{Diff, Headless, Image};
pub use renderer::{DrawOptions, OUTPUT_FORMAT, Renderer, Texture};

#[derive(Debug)]
pub enum Error {
    /// No adapter, or the adapter refused to give a device.
    Gpu(String),
    /// A drawable uses a texture that was not registered.
    MissingTexture(u32),
    /// Texture pixel data does not match its size.
    BadTexture { width: u32, height: u32, len: usize },
    /// Not a PNG, or one this reader cannot decode.
    BadPng(String),
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
            Error::BadPng(m) => write!(f, "png: {m}"),
            Error::BadTarget => write!(f, "target must be Rgba8Unorm with RENDER_ATTACHMENT"),
        }
    }
}

impl std::error::Error for Error {}

/// Decodes a PNG of any color type to straight-alpha RGBA8.
pub(crate) fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), Error> {
    let bad = |e: png::DecodingError| Error::BadPng(e.to_string());
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(bad)?;
    let mut buf = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| Error::BadPng("too large".into()))?
    ];
    let info = reader.next_frame(&mut buf).map_err(bad)?;
    let data = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => {
            return Err(Error::BadPng("indexed color was not expanded".into()));
        }
    };
    Ok((info.width, info.height, rgba))
}
