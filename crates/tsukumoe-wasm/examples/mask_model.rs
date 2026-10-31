// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Writes a small `.tmod` with masked meshes (normal and inverted, a mask
//! with a soft alpha ramp) for the browser check: `mask_model <out.tmod>`.

use tsukumoe_core::elements::{Part, Point};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::rig::ArtMesh;
use tsukumoe_core::tmod;
use tsukumoe_gpu::Image;

const FRONT: [u32; 6] = [0, 1, 2, 0, 2, 3];

fn texture(f: impl Fn(f32, f32) -> [u8; 4]) -> Vec<u8> {
    const N: u32 = 64;
    let mut rgba = Vec::new();
    for y in 0..N {
        for x in 0..N {
            let px = f((x as f32 + 0.5) / N as f32, (y as f32 + 0.5) / N as f32);
            let a = u32::from(px[3]);
            // `Image` holds premultiplied colors.
            rgba.extend(
                px[..3]
                    .iter()
                    .map(|&c| ((u32::from(c) * a + 127) / 255) as u8),
            );
            rgba.push(px[3]);
        }
    }
    Image {
        width: N,
        height: N,
        rgba,
    }
    .to_png()
}

fn quad(m: &mut ModelParts, rect: [f32; 4], texture: u32) -> ArtMesh {
    let part = m
        .parts
        .insert(format!("part{}", m.parts.len()), Part::default())
        .unwrap();
    let [x0, y0, x1, y1] = rect;
    let positions: Vec<Point> = vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    // v = 0 is the top row of the texture, drawing space is y up.
    let uvs = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let mut mesh = ArtMesh::new(part, positions, uvs, FRONT.to_vec());
    mesh.texture = texture;
    mesh
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: mask_model <out.tmod>");
    let mut m = ModelParts::default();
    // Texture 0: colored gradient. 1: mask, alpha rises to the right and
    // falls toward the bottom, with a hole. 2: second color.
    let pngs = [
        texture(|u, v| [(u * 255.0) as u8, (v * 255.0) as u8, 160, 255]),
        texture(|u, v| {
            let hole = (u - 0.5).hypot(v - 0.5) < 0.18;
            let a = if hole { 0.0 } else { u * (1.0 - v) };
            [255, 255, 255, (a * 255.0) as u8]
        }),
        texture(|u, v| [255, (v * 255.0) as u8, (u * 255.0) as u8, 255]),
    ];
    let mut mask = quad(&mut m, [0.1, 0.1, 0.9, 0.9], 1);
    mask.opacity = 0.0;
    let mask = m.meshes.insert("mask", mask).unwrap();
    let mut a = quad(&mut m, [0.0, 0.0, 1.0, 1.0], 0);
    a.masks = vec![mask];
    m.meshes.insert("a", a).unwrap();
    let mut b = quad(&mut m, [0.2, 0.2, 0.8, 0.8], 2);
    b.masks = vec![mask];
    b.mask_inverted = true;
    b.opacity = 0.8;
    m.meshes.insert("b", b).unwrap();

    let compiled = Model::from_parts(m).unwrap().compile().unwrap();
    let refs: Vec<&[u8]> = pngs.iter().map(Vec::as_slice).collect();
    std::fs::write(out, tmod::write(&compiled, &refs).unwrap()).unwrap();
}
