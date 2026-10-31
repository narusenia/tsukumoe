// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The draw-list JSON of the cross-environment parity test and the comparison
//! of two captures. The format is in `docs/specifications/draw-list.md`.

use serde::{Deserialize, Serialize};
use tsukumoe_core::draw::DrawList;

/// How many of the worst pixels and numbers a report lists.
const WORST: usize = 5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drawable {
    pub mesh: u32,
    pub texture: u32,
    pub blend_mode: u32,
    pub double_sided: bool,
    pub mask_inverted: bool,
    pub masks: Vec<u32>,
    pub opacity: f32,
    pub draw_order: f32,
    pub multiply_color: [f32; 3],
    pub screen_color: [f32; 3],
    pub positions: Vec<f32>,
    pub uvs: Vec<f32>,
}

/// Drawables back to front.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawListFile {
    pub drawables: Vec<Drawable>,
}

impl DrawListFile {
    pub fn from_draw_list(list: &DrawList) -> Self {
        let drawables = list
            .drawables()
            .map(|d| Drawable {
                mesh: d.mesh as u32,
                texture: d.texture,
                blend_mode: d.blend_mode.to_u32(),
                double_sided: d.double_sided,
                mask_inverted: d.mask_inverted,
                masks: d.masks.to_vec(),
                opacity: d.opacity,
                draw_order: d.draw_order,
                multiply_color: d.multiply_color,
                screen_color: d.screen_color,
                positions: d.positions.to_vec(),
                uvs: d.uvs.to_vec(),
            })
            .collect();
        Self { drawables }
    }
}

#[derive(Debug, Serialize)]
pub struct WorstPixel {
    pub pixel: usize,
    pub x: Option<usize>,
    pub y: Option<usize>,
    pub a: [u8; 4],
    pub b: [u8; 4],
}

#[derive(Debug, Serialize)]
pub struct ImageReport {
    pub pixels: usize,
    pub max: u8,
    pub over: usize,
    pub worst: Vec<WorstPixel>,
    pub ok: bool,
}

/// Compares premultiplied RGBA8 buffers. `width` only turns pixel indices
/// into coordinates.
pub fn compare_images(
    a: &[u8],
    b: &[u8],
    tolerance: u8,
    width: Option<usize>,
) -> Result<ImageReport, String> {
    if a.len() != b.len() || !a.len().is_multiple_of(4) {
        return Err(format!(
            "image sizes differ: {} and {} bytes",
            a.len(),
            b.len()
        ));
    }
    let mut diffs: Vec<(u8, usize)> = a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .enumerate()
        .map(|(i, (p, q))| {
            (
                p.iter().zip(q).map(|(x, y)| x.abs_diff(*y)).max().unwrap(),
                i,
            )
        })
        .collect();
    let over = diffs.iter().filter(|d| d.0 > tolerance).count();
    // Largest first; ties by lowest pixel index.
    diffs.sort_by_key(|&(d, i)| (std::cmp::Reverse(d), i));
    let px = |buf: &[u8], i: usize| <[u8; 4]>::try_from(&buf[i * 4..i * 4 + 4]).unwrap();
    let worst = diffs
        .iter()
        .take(WORST)
        .filter(|d| d.0 > 0)
        .map(|&(_, i)| WorstPixel {
            pixel: i,
            x: width.map(|w| i % w),
            y: width.map(|w| i / w),
            a: px(a, i),
            b: px(b, i),
        })
        .collect();
    Ok(ImageReport {
        pixels: a.len() / 4,
        max: diffs.first().map_or(0, |d| d.0),
        over,
        worst,
        ok: over == 0,
    })
}

#[derive(Debug, Serialize)]
pub struct WorstNumber {
    pub at: String,
    pub a: f32,
    pub b: f32,
}

#[derive(Debug, Serialize)]
pub struct DrawListReport {
    pub drawables: usize,
    pub max: f32,
    pub worst: Vec<WorstNumber>,
    /// Order, mesh, texture, blend mode, flags, masks and array lengths.
    pub mismatches: Vec<String>,
    pub ok: bool,
}

/// Compares two draw lists: everything that is not a float must be equal,
/// floats may differ by `tolerance` (absolute).
pub fn compare_draw_lists(a: &DrawListFile, b: &DrawListFile, tolerance: f32) -> DrawListReport {
    let mut mismatches = Vec::new();
    let mut numbers: Vec<(f32, WorstNumber)> = Vec::new();
    let mut number = |at: String, x: f32, y: f32| {
        // NaN differences count as the largest.
        let d = if x == y { 0.0 } else { (x - y).abs() };
        numbers.push((
            if d.is_nan() { f32::INFINITY } else { d },
            WorstNumber { at, a: x, b: y },
        ));
    };
    if a.drawables.len() != b.drawables.len() {
        mismatches.push(format!(
            "drawable count: {} and {}",
            a.drawables.len(),
            b.drawables.len()
        ));
    }
    for (i, (p, q)) in a.drawables.iter().zip(&b.drawables).enumerate() {
        let exact = [
            ("mesh", p.mesh == q.mesh),
            ("texture", p.texture == q.texture),
            ("blend_mode", p.blend_mode == q.blend_mode),
            ("double_sided", p.double_sided == q.double_sided),
            ("mask_inverted", p.mask_inverted == q.mask_inverted),
            ("masks", p.masks == q.masks),
        ];
        for (name, same) in exact {
            if !same {
                mismatches.push(format!("drawables[{i}].{name}"));
            }
        }
        number(format!("drawables[{i}].opacity"), p.opacity, q.opacity);
        number(
            format!("drawables[{i}].draw_order"),
            p.draw_order,
            q.draw_order,
        );
        let arrays: [(&str, &[f32], &[f32]); 4] = [
            ("multiply_color", &p.multiply_color, &q.multiply_color),
            ("screen_color", &p.screen_color, &q.screen_color),
            ("positions", &p.positions, &q.positions),
            ("uvs", &p.uvs, &q.uvs),
        ];
        for (name, x, y) in arrays {
            if x.len() != y.len() {
                mismatches.push(format!(
                    "drawables[{i}].{name} length: {} and {}",
                    x.len(),
                    y.len()
                ));
                continue;
            }
            for (k, (&u, &v)) in x.iter().zip(y).enumerate() {
                number(format!("drawables[{i}].{name}[{k}]"), u, v);
            }
        }
    }
    let max = numbers.iter().map(|n| n.0).fold(0.0, f32::max);
    numbers.sort_by(|x, y| y.0.total_cmp(&x.0));
    let worst = numbers
        .into_iter()
        .take(WORST)
        .filter(|n| n.0 > 0.0)
        .map(|n| n.1)
        .collect();
    DrawListReport {
        drawables: a.drawables.len().min(b.drawables.len()),
        max,
        worst,
        ok: mismatches.is_empty() && max <= tolerance,
        mismatches,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawable(mesh: u32) -> Drawable {
        Drawable {
            mesh,
            texture: 0,
            blend_mode: 0,
            double_sided: false,
            mask_inverted: false,
            masks: vec![],
            opacity: 1.0,
            draw_order: 500.0,
            multiply_color: [1.0; 3],
            screen_color: [0.0; 3],
            positions: vec![0.0, 0.0, 1.0, 0.0],
            uvs: vec![0.0, 1.0, 1.0, 1.0],
        }
    }

    fn list(drawables: Vec<Drawable>) -> DrawListFile {
        DrawListFile { drawables }
    }

    #[test]
    fn images_within_and_beyond_the_tolerance() {
        let a = vec![10u8; 16];
        let mut b = a.clone();
        b[4] = 12; // pixel 1, off by 2
        assert!(compare_images(&a, &b, 2, Some(2)).unwrap().ok);
        b[9] = 14; // pixel 2, off by 4
        let r = compare_images(&a, &b, 2, Some(2)).unwrap();
        assert!(!r.ok);
        assert_eq!((r.max, r.over), (4, 1));
        assert_eq!(
            (r.worst[0].pixel, r.worst[0].x, r.worst[0].y),
            (2, Some(0), Some(1))
        );
        assert!(compare_images(&a, &b[..12], 2, None).is_err());
    }

    #[test]
    fn draw_lists_numbers_and_exact_fields() {
        let a = list(vec![drawable(0), drawable(1)]);
        assert!(compare_draw_lists(&a, &a, 1e-4).ok);

        let mut b = a.clone();
        b.drawables[1].positions[1] += 5e-5;
        assert!(compare_draw_lists(&a, &b, 1e-4).ok);
        b.drawables[1].positions[1] += 1e-3;
        let r = compare_draw_lists(&a, &b, 1e-4);
        assert!(!r.ok);
        assert_eq!(r.worst[0].at, "drawables[1].positions[1]");

        // A different order, mask or flag fails however small the numbers differ.
        let swapped = list(vec![drawable(1), drawable(0)]);
        assert!(
            compare_draw_lists(&a, &swapped, 1e-4)
                .mismatches
                .contains(&"drawables[0].mesh".into())
        );
        let mut c = a.clone();
        c.drawables[0].mask_inverted = true;
        assert!(!compare_draw_lists(&a, &c, 1e-4).ok);
    }

    #[test]
    fn unity_shaped_json_parses() {
        let text = r#"{"drawables":[{"mesh":0,"texture":1,"blend_mode":2,"double_sided":true,
            "mask_inverted":false,"masks":[3],"opacity":0.5,"draw_order":1,
            "multiply_color":[1,1,1],"screen_color":[0,0,0],"positions":[0,1],"uvs":[1,0]}]}"#;
        let l: DrawListFile = serde_json::from_str(text).unwrap();
        assert_eq!(l.drawables[0].masks, [3]);
        assert_eq!(
            serde_json::from_str::<DrawListFile>(&serde_json::to_string(&l).unwrap()).unwrap(),
            l
        );
    }
}
