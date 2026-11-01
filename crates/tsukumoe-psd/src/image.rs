// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A decoded layer: RGBA8 with straight (non-premultiplied) alpha.

use sha2::{Digest, Sha256};

use crate::reader::Rect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, row by row.
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            rgba: Vec::new(),
        }
    }

    /// The smallest rectangle (in pixels of this image) holding every pixel
    /// with alpha above 0, or `None` when the image is fully transparent.
    pub fn opaque_bounds(&self) -> Option<Rect> {
        let (w, h) = (self.width as usize, self.height as usize);
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h {
            let row = &self.rgba[y * w * 4..(y + 1) * w * 4];
            let first = row.as_chunks::<4>().0.iter().position(|p| p[3] > 0);
            let Some(first) = first else { continue };
            let last = row
                .as_chunks::<4>()
                .0
                .iter()
                .rposition(|p| p[3] > 0)
                .unwrap_or(first);
            x0 = x0.min(first);
            x1 = x1.max(last + 1);
            y0 = y0.min(y);
            y1 = y1.max(y + 1);
        }
        (x1 > x0 && y1 > y0).then_some(Rect {
            left: x0 as i32,
            top: y0 as i32,
            right: x1 as i32,
            bottom: y1 as i32,
        })
    }

    /// The part inside `rect`, which must lie within the image.
    pub fn crop(&self, rect: Rect) -> Image {
        let (w, cw, ch) = (
            self.width as usize,
            rect.width() as usize,
            rect.height() as usize,
        );
        let mut rgba = Vec::with_capacity(cw * ch * 4);
        for y in rect.top as usize..rect.bottom as usize {
            let start = (y * w + rect.left as usize) * 4;
            rgba.extend_from_slice(&self.rgba[start..start + cw * 4]);
        }
        Image {
            width: cw as u32,
            height: ch as u32,
            rgba,
        }
    }

    /// SHA-256 of the size and the pixels, as 64 hex digits.
    pub fn hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.width.to_le_bytes());
        h.update(self.height.to_le_bytes());
        h.update(&self.rgba);
        hex(&h.finalize())
    }

    /// PNG (RGBA, 8 bit, straight alpha).
    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&self.rgba).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
        Ok(out)
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// SHA-256 of a stream, as 64 hex digits.
pub fn hash_reader(r: &mut impl std::io::Read) -> std::io::Result<String> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(hex(&h.finalize()));
        }
        h.update(&buf[..n]);
    }
}
