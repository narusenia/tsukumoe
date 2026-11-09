// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Texture atlases at export (M5-03a). The project keeps one PNG per image (a
//! PSD import makes one per layer, and a re-import depends on that); the
//! runtime file packs them into a few pages instead, so a model with hundreds
//! of layers draws from a handful of textures. The editing model is not
//! touched: only the compiled model's texture numbers and UVs change.
//!
//! An image stays alone when a mesh's UVs reach outside it (a texture that
//! tiles or mirrors would show its neighbours in a page), or when it is bigger
//! than a page. Images no mesh uses are left out. If packing would not give
//! fewer textures, the file is written as it was.

use std::collections::BTreeMap;

use tsukumoe_core::atlas::{Placement, pack};
use tsukumoe_core::compiled::CompiledModel;

use crate::Error;

/// UVs may stray this far outside 0 to 1 (rounding) and still count as inside.
const UV_SLACK: f32 = 1e-3;

/// How the pages are made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasOptions {
    /// The most a page is wide or tall, in pixels.
    pub max_size: u32,
    /// Pixels kept around each image, filled from its edge so that smooth
    /// sampling does not pick up the neighbour.
    pub padding: u32,
}

impl Default for AtlasOptions {
    fn default() -> Self {
        Self {
            max_size: 4096,
            padding: 2,
        }
    }
}

/// What packing did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AtlasReport {
    /// Textures the project had.
    pub before: usize,
    /// Textures the runtime file has.
    pub after: usize,
    /// The packed pages' sizes (the images left alone are not here).
    pub pages: Vec<(u32, u32)>,
}

/// The textures as they are, for a file that is not packed.
pub(crate) fn unpacked(textures: &[&[u8]]) -> (Vec<Vec<u8>>, AtlasReport) {
    (
        textures.iter().map(|t| t.to_vec()).collect(),
        AtlasReport {
            before: textures.len(),
            after: textures.len(),
            pages: Vec::new(),
        },
    )
}

struct Decoded {
    width: u32,
    height: u32,
    /// Straight-alpha RGBA8.
    rgba: Vec<u8>,
}

fn decode(index: usize, bytes: &[u8]) -> Result<Decoded, Error> {
    let bad = |e: &dyn std::fmt::Display| Error::Invalid(format!("texture {index}: {e}"));
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| bad(&e))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| bad(&"too large"))?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| bad(&e))?;
    let data = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(bad(&"indexed color was not expanded")),
    };
    Ok(Decoded {
        width: info.width,
        height: info.height,
        rgba,
    })
}

fn encode(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(rgba))
        .map_err(|e| Error::Invalid(format!("atlas page: {e}")))?;
    Ok(out)
}

/// Draws `image` at `(x, y)` on a page `page_width` wide, with `padding`
/// pixels around it filled from the nearest edge pixel.
fn blit(page: &mut [u8], page_width: u32, image: &Decoded, x: u32, y: u32, padding: u32) {
    let (w, h, p) = (image.width as i64, image.height as i64, i64::from(padding));
    for dy in -p..h + p {
        let sy = dy.clamp(0, h - 1) as usize;
        for dx in -p..w + p {
            let sx = dx.clamp(0, w - 1) as usize;
            let from = (sy * image.width as usize + sx) * 4;
            let to = ((i64::from(y) + dy) as usize * page_width as usize
                + (i64::from(x) + dx) as usize)
                * 4;
            page[to..to + 4].copy_from_slice(&image.rgba[from..from + 4]);
        }
    }
}

/// Where each old texture ended up.
enum Home {
    /// A rectangle of a page of the new list.
    Packed {
        page: usize,
        at: Placement,
        size: (u32, u32),
    },
    /// Its own place in the new list.
    Alone(usize),
}

/// Packs the textures `compiled` uses, and moves its meshes' texture numbers
/// and UVs to match. Returns the PNG files to embed, in the order the meshes
/// now number them.
pub fn build(
    compiled: &mut CompiledModel<'static>,
    textures: &[&[u8]],
    options: &AtlasOptions,
) -> Result<(Vec<Vec<u8>>, AtlasReport), Error> {
    // Which textures meshes use, and whether every mesh's UVs stay inside.
    let mut used = vec![false; textures.len()];
    let mut inside = vec![true; textures.len()];
    for def in compiled.meshes.iter() {
        let t = def.texture as usize;
        if t >= textures.len() {
            continue;
        }
        used[t] = true;
        let start = def.vertex_start as usize * 2;
        let uvs = &compiled.mesh_uvs[start..start + def.vertex_count as usize * 2];
        if uvs
            .iter()
            .any(|&c| !(-UV_SLACK..=1.0 + UV_SLACK).contains(&c))
        {
            inside[t] = false;
        }
    }
    let decoded: BTreeMap<usize, Decoded> = (0..textures.len())
        .filter(|&t| used[t] && inside[t])
        .map(|t| decode(t, textures[t]).map(|d| (t, d)))
        .collect::<Result<_, _>>()?;
    let candidates: Vec<usize> = decoded.keys().copied().collect();
    let sizes: Vec<(u32, u32)> = candidates
        .iter()
        .map(|t| (decoded[t].width, decoded[t].height))
        .collect();
    let packing = pack(&sizes, options.max_size, options.padding);
    let placed = packing.slots.iter().flatten().count();
    // Gain nothing and the file is as it was.
    if placed == 0 || packing.pages.len() >= placed {
        return Ok(unpacked(textures));
    }

    let mut files: Vec<Vec<u8>> = Vec::new();
    let mut homes: BTreeMap<usize, Home> = BTreeMap::new();
    for (n, &(pw, ph)) in packing.pages.iter().enumerate() {
        let mut page = vec![0u8; pw as usize * ph as usize * 4];
        for (i, slot) in packing.slots.iter().enumerate() {
            let Some(at) = slot.filter(|s| s.page == n) else {
                continue;
            };
            let t = candidates[i];
            blit(&mut page, pw, &decoded[&t], at.x, at.y, options.padding);
            homes.insert(
                t,
                Home::Packed {
                    page: n,
                    at,
                    size: (decoded[&t].width, decoded[&t].height),
                },
            );
        }
        files.push(encode(pw, ph, &page)?);
    }
    for t in 0..textures.len() {
        if used[t] && !homes.contains_key(&t) {
            homes.insert(t, Home::Alone(files.len()));
            files.push(textures[t].to_vec());
        }
    }

    let sizes_of_pages = packing.pages.clone();
    for def in compiled.meshes.to_mut().iter_mut() {
        let Some(home) = homes.get(&(def.texture as usize)) else {
            continue;
        };
        match home {
            Home::Alone(n) => def.texture = *n as u32,
            Home::Packed { page, at, size } => {
                let (pw, ph) = sizes_of_pages[*page];
                let start = def.vertex_start as usize * 2;
                let end = start + def.vertex_count as usize * 2;
                for uv in compiled.mesh_uvs.to_mut()[start..end]
                    .as_chunks_mut::<2>()
                    .0
                {
                    uv[0] = (at.x as f32 + uv[0] * size.0 as f32) / pw as f32;
                    uv[1] = (at.y as f32 + uv[1] * size.1 as f32) / ph as f32;
                }
                def.texture = *page as u32;
            }
        }
    }
    let report = AtlasReport {
        before: textures.len(),
        after: files.len(),
        pages: packing.pages,
    };
    Ok((files, report))
}
