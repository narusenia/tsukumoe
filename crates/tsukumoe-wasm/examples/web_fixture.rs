// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Writes a small `.tmod` for the browser check and the Node tests of
//! `packages/tsukumoe-web`: masked meshes (normal and inverted, a mask with a
//! soft alpha ramp), five parameters, a motion and an expression.
//! `web_fixture <out.tmod>`.

use tsukumoe_core::elements::{Parameter, Part, Point};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::playback::{
    Curve, CurveTarget, Expression, ExpressionBlend, ExpressionParameter, Motion, Segment,
};
use tsukumoe_core::rig::ArtMesh;
use tsukumoe_core::tmod::{self, PlaybackData};
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
        .expect("usage: web_fixture <out.tmod>");
    let mut m = ModelParts::default();
    let mut add = |name: &str, min, max, default| {
        m.parameters
            .insert(name, Parameter::new(min, max, default))
            .unwrap()
    };
    let p = add("P", 0.0, 1.0, 0.0);
    let q = add("Q", -1.0, 1.0, 0.0);
    add("EyeL", 0.0, 1.0, 1.0);
    add("EyeR", 0.0, 1.0, 1.0);
    add("Breath", 0.0, 1.0, 0.0);
    // P rises from 0 to 1 in one second; the motion lasts two and holds.
    m.motions
        .insert(
            "rise",
            Motion {
                duration: 2.0,
                fps: 30.0,
                looped: false,
                fade_in: 0.0,
                fade_out: 0.0,
                curves: vec![Curve {
                    target: CurveTarget::Parameter(p),
                    start: [0.0, 0.0],
                    segments: vec![Segment::Linear([1.0, 1.0]), Segment::Stepped([2.0, 1.0])],
                    fade_in: None,
                    fade_out: None,
                }],
            },
        )
        .unwrap();
    m.expressions
        .insert(
            "half_q",
            Expression {
                fade_in: 0.0,
                fade_out: 0.0,
                parameters: vec![ExpressionParameter {
                    parameter: q,
                    blend: ExpressionBlend::Overwrite,
                    value: 0.5,
                }],
            },
        )
        .unwrap();
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

    let model = Model::from_parts(m).unwrap();
    let compiled = model.compile().unwrap();
    let playback = PlaybackData::from_model(&model).unwrap();
    let refs: Vec<&[u8]> = pngs.iter().map(Vec::as_slice).collect();
    std::fs::write(out, tmod::write_with(&compiled, &playback, &refs).unwrap()).unwrap();
}
