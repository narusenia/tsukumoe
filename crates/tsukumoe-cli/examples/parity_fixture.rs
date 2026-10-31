// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Writes the `.tmod` of the parity fixture cases (`tests/parity/`): the masked
//! meshes of `web_fixture` (normal and inverted mask, a mask with a soft alpha
//! ramp), but with parameters that move things, so that parameter values, a
//! motion, an expression and physics all change the draw list. `P` moves and
//! fades mesh `a`, `Q` moves mesh `b`, `Sway` (a physics output driven by `P`)
//! moves mesh `c`; motion `rise` drives `P`, expression `half_q` sets `Q`.
//! `parity_fixture <out.tmod>`.

use tsukumoe_core::elements::{Parameter, Part, Point};
use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::physics::{ChainJoint, Channel, PhysicsChain, PhysicsInput, PhysicsOutput};
use tsukumoe_core::playback::{
    Curve, CurveTarget, Expression, ExpressionBlend, ExpressionParameter, Motion, Segment,
};
use tsukumoe_core::rig::{ArtMesh, MeshKeyform};
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

/// Keyforms along one parameter: the mesh's quad moved right by `shift` per key.
fn moving(
    m: &mut ModelParts,
    mesh: &mut ArtMesh,
    parameter: tsukumoe_core::elements::ParameterId,
    keys: &[f32],
    shift: impl Fn(f32) -> (f32, f32),
) {
    let binding = m.bindings.insert(
        Binding::new(vec![Axis {
            parameter,
            keys: keys.to_vec(),
        }])
        .unwrap(),
    );
    let forms = keys
        .iter()
        .map(|&k| {
            let (dx, opacity) = shift(k);
            MeshKeyform {
                positions: mesh.positions.iter().map(|p| [p[0] + dx, p[1]]).collect(),
                opacity,
                draw_order: mesh.draw_order,
                multiply_color: mesh.multiply_color,
                screen_color: mesh.screen_color,
            }
        })
        .collect();
    mesh.keyforms = Some(KeyformGrid::new(binding, forms));
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: parity_fixture <out.tmod>");
    let mut m = ModelParts::default();
    let mut add = |name: &str, min, max, default| {
        m.parameters
            .insert(name, Parameter::new(min, max, default))
            .unwrap()
    };
    let p = add("P", 0.0, 1.0, 0.0);
    let q = add("Q", -1.0, 1.0, 0.0);
    let sway = add("Sway", -1.0, 1.0, 0.0);
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
    // A chain that swings when P moves; its output is Sway.
    m.physics.chains.push(PhysicsChain {
        name: "SwayChain".into(),
        joints: vec![
            ChainJoint {
                length: 0.3,
                stiffness: 0.5,
                damping: 0.1
            };
            3
        ],
        gravity: [0.0, -9.8],
        wind: [0.0, 0.0],
        inputs: vec![PhysicsInput {
            parameter: p,
            channel: Channel::X,
            weight: 100.0,
            reflect: false,
        }],
        outputs: vec![PhysicsOutput {
            parameter: sway,
            channel: Channel::X,
            particle: 1,
            scale: 20.0,
            weight: 100.0,
            reflect: false,
        }],
    });
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
    moving(&mut m, &mut a, p, &[0.0, 1.0], |k| (0.2 * k, 1.0 - 0.5 * k));
    m.meshes.insert("a", a).unwrap();
    let mut b = quad(&mut m, [0.2, 0.2, 0.8, 0.8], 2);
    b.masks = vec![mask];
    b.mask_inverted = true;
    b.opacity = 0.8;
    moving(&mut m, &mut b, q, &[-1.0, 0.0, 1.0], |k| (0.2 * k, 0.8));
    m.meshes.insert("b", b).unwrap();
    let mut c = quad(&mut m, [0.0, 0.0, 0.3, 0.3], 0);
    moving(&mut m, &mut c, sway, &[-1.0, 0.0, 1.0], |k| (0.3 * k, 1.0));
    m.meshes.insert("c", c).unwrap();

    let model = Model::from_parts(m).unwrap();
    let compiled = model.compile().unwrap();
    let playback = PlaybackData::from_model(&model).unwrap();
    let refs: Vec<&[u8]> = pngs.iter().map(Vec::as_slice).collect();
    std::fs::write(out, tmod::write_with(&compiled, &playback, &refs).unwrap()).unwrap();
}
