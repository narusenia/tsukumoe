// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;
use crate::compiled::BlendTargetRaw;
use crate::driver::Driver;
use crate::elements::{Parameter, Point};
use crate::eval::Frame;
use crate::expr::OpRaw;
use crate::keyform::{Axis, Binding, KeyformGrid};
use crate::model::Model;
use crate::playback::{
    Curve, CurveTarget, Expression, ExpressionBlend, ExpressionParameter, Motion, Pose, PoseEntry,
    Segment,
};
use crate::rig::{
    ArtMesh, BlendShape, BlendTarget, Bone, BoneTransform, DeformerRef, Glue, GlueNode,
    MeshKeyform, RotationDeformer, RotationKeyform, VertexInfluences, WarpDeformer, WarpKeyform,
};

const UNIT_GRID: [Point; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

/// Evaluates both models under the same inputs and requires identical bits.
pub(crate) fn assert_roundtrip(c: &CompiledModel<'_>) {
    let bytes = write(c, &[]).unwrap();
    let buf = AlignedBuf::from_bytes(&bytes);
    let t = Tmod::parse(&buf).unwrap();
    let (mut a, mut b) = (Frame::new(c), Frame::new(&t.model));
    for step in 0..8usize {
        for (i, p) in c.parameters.iter().enumerate() {
            let unit = ((step * 7 + i * 3) % 11) as f32 / 10.0;
            let v = match step {
                6 => f32::NAN,
                7 => p.max + 1.0,
                _ => p.min + (p.max - p.min) * unit,
            };
            a.set_parameter(i, v);
            b.set_parameter(i, v);
        }
        let dt = 1.0 / 60.0 * (step + 1) as f32;
        a.advance(c, dt);
        b.advance(&t.model, dt);
        same_frame(c, &a, &t.model, &b);
    }
}

fn same_frame(ca: &CompiledModel<'_>, a: &Frame, cb: &CompiledModel<'_>, b: &Frame) {
    let bits = |s: &[f32]| s.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(a.positions()), bits(b.positions()));
    let (la, lb) = (a.draw_list(ca), b.draw_list(cb));
    assert_eq!(la.order(), lb.order());
    for m in 0..la.len() {
        let (x, y) = (la.drawable(m), lb.drawable(m));
        assert_eq!(
            bits(&[x.opacity, x.draw_order]),
            bits(&[y.opacity, y.draw_order])
        );
        assert_eq!(bits(&x.multiply_color), bits(&y.multiply_color));
        assert_eq!(bits(&x.screen_color), bits(&y.screen_color));
        assert_eq!(
            (x.name, x.texture, x.blend_mode),
            (y.name, y.texture, y.blend_mode)
        );
        assert_eq!((x.masks, x.indices), (y.masks, y.indices));
        assert_eq!(bits(x.positions), bits(y.positions));
        assert_eq!(bits(x.uvs), bits(y.uvs));
    }
}

/// Parameters P, Q (bindings), a rotation, a warp, meshes with a mask, a
/// glue, a blend shape, a bone with influences, a driver and a pendulum.
fn rich_model() -> Model {
    let mut m = crate::simulate::sample_model(0.95);
    let part = m
        .parts
        .insert("Part", crate::elements::Part::default())
        .unwrap();
    let mut p = |name: &str| {
        m.parameters
            .insert(name, Parameter::new(-1.0, 1.0, 0.0))
            .unwrap()
    };
    let (p0, q) = (p("P"), p("Q"));
    let driven = m
        .parameters
        .insert("Driven", Parameter::new(0.0, 1.0, 0.0))
        .unwrap();
    m.drivers
        .insert(
            "D",
            Driver {
                output: driven,
                expression: "clamp(P * 0.5 + 0.5, 0, 1)".into(),
            },
        )
        .unwrap();
    let b = m.bindings.insert(
        Binding::new(vec![
            Axis {
                parameter: p0,
                keys: vec![-1.0, 1.0],
            },
            Axis {
                parameter: q,
                keys: vec![-1.0, 0.0, 1.0],
            },
        ])
        .unwrap(),
    );
    let b1 = m.bindings.insert(
        Binding::new(vec![Axis {
            parameter: driven,
            keys: vec![0.0, 1.0],
        }])
        .unwrap(),
    );
    let mut rot = RotationDeformer {
        part,
        parent: None,
        base: RotationKeyform::default(),
        keyforms: None,
    };
    rot.keyforms = Some(KeyformGrid::new(
        b,
        (0..6)
            .map(|i| RotationKeyform {
                angle_degrees: i as f32 * 7.0,
                reflect_x: i == 3,
                ..RotationKeyform::default()
            })
            .collect(),
    ));
    let rot = m.rotations.insert("R", rot).unwrap();
    let warp_kf = |dx: f32| WarpKeyform {
        points: UNIT_GRID.iter().map(|p| [p[0] + dx, p[1]]).collect(),
        opacity: 1.0 - dx,
    };
    let w = m
        .warps
        .insert(
            "W",
            WarpDeformer {
                part,
                parent: Some(DeformerRef::Rotation(rot)),
                rows: 1,
                columns: 1,
                bezier: false,
                base_points: UNIT_GRID.to_vec(),
                keyforms: Some(KeyformGrid::new(b1, vec![warp_kf(0.0), warp_kf(0.3)])),
            },
        )
        .unwrap();
    let tri = [[0.2, 0.2], [0.8, 0.2], [0.5, 0.9]];
    let mesh_kf = |x: f32| MeshKeyform {
        positions: tri.iter().map(|p| [p[0] + x, p[1]]).collect(),
        opacity: 1.0 - x,
        draw_order: 500.0 + x,
        multiply_color: [1.0, 0.5, 0.25],
        screen_color: [0.0; 3],
    };
    let mut a = ArtMesh::new(part, tri.to_vec(), tri.to_vec(), vec![0, 1, 2]);
    a.parent = Some(DeformerRef::Warp(w));
    a.keyforms = Some(KeyformGrid::new(b1, vec![mesh_kf(0.0), mesh_kf(0.2)]));
    let a = m.meshes.insert("A", a).unwrap();
    let mut other = ArtMesh::new(part, tri.to_vec(), tri.to_vec(), vec![0, 1, 2]);
    other.parent = Some(DeformerRef::Rotation(rot));
    other.masks = vec![a];
    other.mask_inverted = true;
    other.texture = 1;
    let other = m.meshes.insert("B", other).unwrap();
    let root = m
        .bones
        .insert(
            "Root",
            Bone {
                parent: None,
                base: BoneTransform::default(),
                keyforms: Some(KeyformGrid::new(
                    b1,
                    vec![
                        BoneTransform::default(),
                        BoneTransform {
                            rotation_degrees: 30.0,
                            ..BoneTransform::default()
                        },
                    ],
                )),
            },
        )
        .unwrap();
    let mut skinned = ArtMesh::new(part, tri.to_vec(), tri.to_vec(), vec![0, 1, 2]);
    skinned.influences = (0..3)
        .map(|_| VertexInfluences {
            bones: [Some(root), None, None, None],
            weights: [0.75, 0.0, 0.0, 0.0],
        })
        .collect();
    m.meshes.insert("Skinned", skinned).unwrap();
    m.glues
        .insert(
            "G",
            Glue {
                mesh_a: a,
                mesh_b: other,
                nodes: vec![GlueNode {
                    index_a: 0,
                    index_b: 0,
                    weight: 0.5,
                }],
                intensity: 1.0,
                keyforms: Some(KeyformGrid::new(b1, vec![0.2, 1.0])),
            },
        )
        .unwrap();
    m.blend_shapes
        .insert(
            "S",
            BlendShape {
                target: BlendTarget::Mesh(a),
                parameter: q,
                keys: vec![0.0, 1.0],
                weights: vec![0.0, 1.0],
                deltas: vec![[0.1, 0.0]; 3],
            },
        )
        .unwrap();
    m.blend_shapes
        .insert(
            "WS",
            BlendShape {
                target: BlendTarget::Warp(w),
                parameter: p0,
                keys: vec![-1.0, 1.0],
                weights: vec![1.0, 0.0],
                deltas: vec![[0.0, 0.1]; 4],
            },
        )
        .unwrap();
    assert!(m.validate().is_empty());
    m
}

fn rich_bytes() -> Vec<u8> {
    let c = rich_model().compile().unwrap();
    let png = [&PNG_SIGNATURE[..], b"data-a"].concat();
    let png2 = [&PNG_SIGNATURE[..], b"b"].concat();
    write(&c, &[&png, &png2]).unwrap()
}

/// The rich model with two motions, two expressions and a pose.
pub(crate) fn playback_model() -> Model {
    let mut m = rich_model();
    let part = m.parts.find("Part").unwrap();
    let other = m
        .parts
        .insert("Other", crate::elements::Part::default())
        .unwrap();
    let (p, q) = (
        m.parameters.find("P").unwrap(),
        m.parameters.find("Q").unwrap(),
    );
    let curve = |target, segments: Vec<Segment>| Curve {
        target,
        start: [0.0, 0.0],
        segments,
        fade_in: None,
        fade_out: None,
    };
    let mut sway = curve(
        CurveTarget::Parameter(p),
        vec![
            Segment::Linear([0.5, 1.0]),
            Segment::Bezier([[0.6, 1.0], [0.8, 0.0], [1.0, 0.0]]),
            Segment::Stepped([1.5, 0.5]),
            Segment::InverseStepped([2.0, -0.5]),
        ],
    );
    sway.fade_in = Some(0.25);
    m.motions
        .insert(
            "Sway",
            Motion {
                duration: 2.0,
                fps: 30.0,
                looped: true,
                fade_in: 0.5,
                fade_out: 0.5,
                curves: vec![sway, curve(CurveTarget::PartOpacity(other), vec![])],
            },
        )
        .unwrap();
    m.motions
        .insert(
            "Still",
            Motion {
                duration: 0.0,
                fps: 30.0,
                looped: false,
                fade_in: 0.0,
                fade_out: 0.0,
                curves: vec![],
            },
        )
        .unwrap();
    let param = |parameter, blend, value| ExpressionParameter {
        parameter,
        blend,
        value,
    };
    m.expressions
        .insert(
            "Smile",
            Expression {
                fade_in: 0.2,
                fade_out: 0.3,
                parameters: vec![
                    param(q, ExpressionBlend::Add, 0.5),
                    param(p, ExpressionBlend::Multiply, 0.8),
                    param(q, ExpressionBlend::Overwrite, -1.0),
                ],
            },
        )
        .unwrap();
    m.expressions
        .insert(
            "Empty",
            Expression {
                fade_in: 0.0,
                fade_out: 0.0,
                parameters: vec![],
            },
        )
        .unwrap();
    m.pose = Some(Pose {
        fade_in: 0.5,
        groups: vec![vec![
            PoseEntry {
                part,
                linked: vec![other],
            },
            PoseEntry {
                part: other,
                linked: vec![],
            },
        ]],
    });
    assert!(m.validate().is_empty());
    m
}

fn playback_bytes() -> Vec<u8> {
    let m = playback_model();
    let c = m.compile().unwrap();
    let p = PlaybackData::from_model(&m).unwrap();
    write_with(&c, &p, &[]).unwrap()
}

/// Parses and, when that passes, evaluates and reads the draw list: a loaded
/// file must never panic anywhere.
fn exercise(bytes: &[u8]) -> Result<(), Error> {
    let buf = AlignedBuf::from_bytes(bytes);
    let t = Tmod::parse(&buf)?;
    let c = &t.model;
    let mut f = Frame::new(c);
    for step in 0..3 {
        for (i, p) in c.parameters.iter().enumerate() {
            f.set_parameter(i, [p.min, p.max, f32::NAN][step]);
        }
        f.advance(c, 0.05);
        let list = f.draw_list(c);
        for d in list.drawables() {
            let _ = (d.name, d.masks, d.indices);
        }
    }
    for i in 0..t.texture_count() {
        t.texture(i).unwrap();
    }
    let p = &t.playback;
    for (i, m) in p.motions.iter().enumerate() {
        let _ = p.motion_name(i);
        for c in p.motion_curves(m) {
            let _ = p.curve_segments(c).len();
        }
    }
    for (i, e) in p.expressions.iter().enumerate() {
        let _ = (p.expression_name(i), p.expression_params_of(e));
    }
    for pose in p.pose.iter() {
        for g in p.pose_groups_of(pose) {
            for e in p.group_entries(g) {
                let _ = (c.part_opacity[e.part as usize], p.entry_links(e));
            }
        }
    }
    Ok(())
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Whether the file was accepted.
fn no_panic(bytes: &[u8], what: impl Fn() -> String) -> bool {
    match catch_unwind(AssertUnwindSafe(|| exercise(bytes).is_ok())) {
        Ok(accepted) => accepted,
        Err(_) => panic!("panicked on {}", what()),
    }
}

#[test]
fn round_trip_evaluates_bit_for_bit() {
    assert_roundtrip(&rich_model().compile().unwrap());
    assert_roundtrip(&CompiledModel::default());
}

#[test]
fn the_same_model_gives_the_same_bytes() {
    let a = rich_bytes();
    assert_eq!(a, rich_bytes());
    assert_eq!(a.len() % ALIGN, 0);
}

#[test]
fn sections_are_aligned_and_the_header_is_as_documented() {
    let bytes = rich_bytes();
    assert_eq!(&bytes[..4], b"TMOD");
    assert_eq!(le_u32(&bytes, 16), 44);
    assert_eq!(le_u64(&bytes, 24), bytes.len() as u64);
    for i in 0..44 {
        let at = HEADER_LEN + i * ENTRY_LEN;
        assert_eq!(le_u64(&bytes, at + 8) % 16, 0);
    }
}

#[test]
fn the_pod_layouts_match_the_specification() {
    use crate::compiled::*;
    use std::mem::size_of;
    let sizes = [
        (size_of::<StrRef>(), 8),
        (size_of::<DriverDef>(), 12),
        (size_of::<OpRaw>(), 8),
        (size_of::<ParentRaw>(), 8),
        (size_of::<ParamDef>(), 16),
        (size_of::<AxisDef>(), 12),
        (size_of::<BindingDef>(), 20),
        (size_of::<KeyformRef>(), 8),
        (size_of::<BoneDef>(), 56),
        (size_of::<BoneTransform>(), 20),
        (size_of::<PhysicsInputDef>(), 16),
        (size_of::<PhysicsOutputDef>(), 24),
        (size_of::<PhysicsParticle>(), 32),
        (size_of::<PhysicsDef>(), 60),
        (size_of::<Influence>(), 32),
        (size_of::<MeshDef>(), 108),
        (size_of::<WarpDef>(), 48),
        (size_of::<RotationKf>(), 28),
        (size_of::<RotationDef>(), 44),
        (size_of::<GlueDef>(), 28),
        (size_of::<GlueNode>(), 12),
        (size_of::<BlendTargetRaw>(), 8),
        (size_of::<BlendDef>(), 24),
    ];
    for (i, (actual, expected)) in sizes.iter().enumerate() {
        assert_eq!(actual, expected, "entry {i}");
    }
}

#[test]
fn a_loaded_model_points_into_the_buffer() {
    let bytes = rich_bytes();
    let buf = AlignedBuf::from_bytes(&bytes);
    let t = Tmod::parse(&buf).unwrap();
    let (lo, hi) = (buf.as_ptr() as usize, buf.as_ptr() as usize + buf.len());
    #[allow(clippy::ptr_arg)] // the Cow variant is the point
    fn inside<T: Pod>(c: &Cow<'_, [T]>, lo: usize, hi: usize) -> bool {
        let p = c.as_ptr() as usize;
        matches!(c, Cow::Borrowed(_)) && p >= lo && p + std::mem::size_of_val(&**c) <= hi
    }
    let m = &t.model;
    macro_rules! all_inside {
        ($($f:ident),*) => {$(
            assert!(inside(&m.$f, lo, hi), stringify!($f));
        )*};
    }
    all_inside!(
        strings,
        parameter_names,
        parameters,
        bindings,
        axes,
        axis_keys,
        drivers,
        ops,
        part_parents,
        part_opacity,
        mesh_names,
        meshes,
        mesh_base_positions,
        mesh_uvs,
        mesh_indices,
        mesh_masks,
        mesh_kf_positions,
        mesh_kf_attrs,
        warps,
        warp_base_points,
        warp_kf_points,
        warp_kf_opacity,
        rotations,
        rotation_kf,
        deformer_order,
        bone_names,
        bones,
        bone_kf,
        influences,
        physics,
        physics_inputs,
        physics_outputs,
        physics_particles,
        glues,
        glue_nodes,
        glue_kf_intensity,
        blends,
        blend_keys,
        blend_weights,
        blend_deltas,
        blend_refs
    );
    // Textures too, and an evaluation allocates nothing new for the model.
    let png = t.texture(0).unwrap();
    assert!((png.as_ptr() as usize) >= lo && (png.as_ptr() as usize) < hi);
    assert_eq!(t.texture_count(), 2);
    assert_eq!(t.texture(1).unwrap(), [&PNG_SIGNATURE[..], b"b"].concat());
    assert!(t.texture(2).is_none());
}

#[test]
fn a_misaligned_buffer_is_an_error_not_a_copy() {
    let buf = AlignedBuf::from_bytes(&rich_bytes());
    let mut shifted = vec![0u8; buf.len() + ALIGN + 1];
    let k = (1..=ALIGN)
        .find(|k| !(shifted.as_ptr() as usize + k).is_multiple_of(ALIGN))
        .unwrap();
    shifted[k..k + buf.len()].copy_from_slice(&buf);
    assert_eq!(
        Tmod::parse(&shifted[k..k + buf.len()]).unwrap_err(),
        Error::Misaligned
    );
}

#[test]
fn a_file_can_be_read_into_an_aligned_buffer() {
    let dir = std::env::temp_dir().join(format!("tsukumoe-tmod-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("m.tmod");
    let bytes = rich_bytes();
    std::fs::write(&path, &bytes).unwrap();
    let buf = AlignedBuf::read_file(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(&*buf, &bytes[..]);
    Tmod::parse(&buf).unwrap();
    assert!(AlignedBuf::read_file(dir.join("missing")).is_err());
}

// REQ-FMT-005: the four patterns of flags and versions.

fn patch(bytes: &[u8], at: usize, value: &[u8]) -> Vec<u8> {
    let mut b = bytes.to_vec();
    b[at..at + value.len()].copy_from_slice(value);
    b
}

fn parse(bytes: &[u8]) -> Result<(), Error> {
    Tmod::parse(&AlignedBuf::from_bytes(bytes)).map(|_| ())
}

/// The file with one more section.
fn with_section(bytes: &[u8], id: u32, flags: u32) -> Vec<u8> {
    let buf = AlignedBuf::from_bytes(bytes);
    let mut sections = Vec::new();
    for i in 0..le_u32(&buf, 16) as usize {
        let at = HEADER_LEN + i * ENTRY_LEN;
        let (off, len) = (
            le_u64(&buf, at + 8) as usize,
            le_u64(&buf, at + 16) as usize,
        );
        sections.push(Section {
            id: le_u32(&buf, at),
            flags: le_u32(&buf, at + 4),
            count: le_u32(&buf, at + 24),
            data: Cow::Owned(buf[off..off + len].to_vec()),
        });
    }
    sections.push(Section {
        id,
        flags,
        count: 3,
        data: Cow::Owned(vec![9; 24]),
    });
    assemble(&sections)
}

#[test]
fn a_newer_major_version_is_rejected() {
    let b = patch(&rich_bytes(), 4, &2u16.to_le_bytes());
    assert_eq!(
        parse(&b),
        Err(Error::UnsupportedMajor {
            found: 2,
            supported: 1
        })
    );
    // A newer minor version only adds optional content.
    parse(&patch(&rich_bytes(), 6, &9u16.to_le_bytes())).unwrap();
}

#[test]
fn an_unknown_required_flag_is_rejected() {
    let b = patch(&rich_bytes(), 8, &0x10u32.to_le_bytes());
    assert_eq!(parse(&b), Err(Error::UnknownRequiredFlags(0x10)));
}

#[test]
fn an_unknown_optional_flag_is_ignored() {
    parse(&patch(&rich_bytes(), 12, &0xFFFF_FFFFu32.to_le_bytes())).unwrap();
}

#[test]
fn unknown_sections_are_skipped_or_rejected_by_their_mark() {
    let b = rich_bytes();
    parse(&with_section(&b, 0x7000, 0)).unwrap();
    assert_eq!(
        parse(&with_section(&b, 0x7000, SECTION_REQUIRED)),
        Err(Error::UnknownRequiredSection(0x7000))
    );
    assert_eq!(
        parse(&with_section(&b, id::META, 0)),
        Err(Error::DuplicateSection(id::META))
    );
}

#[test]
fn a_missing_section_is_an_error() {
    let b = rich_bytes();
    let buf = AlignedBuf::from_bytes(&b);
    // Turn the META entry into an unknown optional section.
    let at = HEADER_LEN;
    assert_eq!(le_u32(&buf, at), id::META);
    let b = patch(&b, at, &0x7001u32.to_le_bytes());
    let b = patch(&b, at + 4, &0u32.to_le_bytes());
    assert_eq!(parse(&b), Err(Error::MissingSection(id::META)));
}

#[test]
fn an_invalid_model_is_not_written() {
    let mut c = rich_model().compile().unwrap();
    c.meshes.to_mut()[0].part = 99;
    assert!(matches!(write(&c, &[]), Err(Error::Layout(_))));
    let c = rich_model().compile().unwrap();
    assert!(matches!(write(&c, &[b"not a png"]), Err(Error::Texture(_))));
}

#[test]
fn every_truncation_is_an_error() {
    for b in [rich_bytes(), playback_bytes()] {
        for n in 0..b.len() {
            assert!(parse(&b[..n]).is_err(), "prefix of {n} bytes");
        }
    }
}

#[test]
fn single_byte_changes_never_panic() {
    for b in [rich_bytes(), playback_bytes()] {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        // The header and the table, exhaustively with a few values.
        let head = HEADER_LEN + ENTRY_LEN * le_u32(&b, 16) as usize;
        for at in 0..head {
            for v in [0x00, 0x01, 0x7F, 0x80, 0xFF, b[at] ^ 0x01] {
                no_panic(&patch(&b, at, &[v]), || format!("byte {at} = {v:#x}"));
            }
        }
        // Everywhere else, at random. Some changes land on data that stays valid,
        // so the evaluation path runs too.
        let mut accepted = 0;
        for _ in 0..20_000 {
            let at = rng.next() as usize % b.len();
            let v = rng.next() as u8;
            accepted += usize::from(no_panic(&patch(&b, at, &[v]), || {
                format!("byte {at} = {v:#x}")
            }));
        }
        assert!(accepted > 1_000 && accepted < 20_000, "{accepted} accepted");
    }
}

#[test]
fn boundary_words_never_panic() {
    for b in [rich_bytes(), playback_bytes()] {
        let special = [
            0u32,
            1,
            2,
            3,
            15,
            16,
            17,
            0x7FFF_FFFF,
            0x8000_0000,
            0xFFFF_FFFE,
            0xFFFF_FFFF,
            f32::NAN.to_bits(),
            f32::INFINITY.to_bits(),
            (-1.0f32).to_bits(),
            1e30f32.to_bits(),
            b.len() as u32,
        ];
        for at in (0..b.len() - 3).step_by(4) {
            for v in special {
                no_panic(&patch(&b, at, &v.to_le_bytes()), || {
                    format!("word {at} = {v:#x}")
                });
            }
        }
        // 64-bit fields of the table: offsets and lengths.
        for i in 0..le_u32(&b, 16) as usize {
            let at = HEADER_LEN + i * ENTRY_LEN;
            for field in [8, 16] {
                for v in [
                    0u64,
                    1,
                    16,
                    u64::MAX,
                    u64::MAX - 15,
                    b.len() as u64,
                    1 << 32,
                ] {
                    no_panic(&patch(&b, at + field, &v.to_le_bytes()), || {
                        format!("entry {i} field {field} = {v:#x}")
                    });
                }
            }
        }
        for v in [0u64, 1, u64::MAX, b.len() as u64 + 16] {
            no_panic(&patch(&b, 24, &v.to_le_bytes()), || format!("length = {v}"));
        }
    }
}

#[test]
fn random_garbage_and_shuffled_pieces_never_panic() {
    for b in [rich_bytes(), playback_bytes()] {
        let mut rng = Rng(42);
        for _ in 0..2_000 {
            // Copy a random run of the file over another place.
            let (len, from, to) = (
                1 + rng.next() as usize % 64,
                rng.next() as usize % b.len(),
                rng.next() as usize % b.len(),
            );
            let len = len.min(b.len() - from).min(b.len() - to);
            let run = b[from..from + len].to_vec();
            no_panic(&patch(&b, to, &run), || {
                format!("copy {len} bytes {from} -> {to}")
            });
        }
        for n in [0, 1, 31, 32, 33, 100, 4096] {
            let junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
            no_panic(&junk, || format!("{n} random bytes"));
            no_panic(&[&MAGIC[..], &junk].concat(), || {
                format!("TMOD + {n} random bytes")
            });
        }
    }
}

#[test]
fn a_model_with_a_hand_edited_index_is_rejected() {
    // A table reference past its array is caught by `check`, not by a panic.
    let mut c = rich_model().compile().unwrap();
    c.blends.to_mut()[0].target = BlendTargetRaw { kind: 0, index: 77 };
    assert!(c.check().is_err());
    let mut c = rich_model().compile().unwrap();
    c.deformer_order.to_mut().reverse();
    assert!(c.check().is_err());
}

#[test]
fn a_file_without_playback_data_has_none_and_one_with_it_round_trips() {
    let plain = rich_bytes();
    let buf = AlignedBuf::from_bytes(&plain);
    let t = Tmod::parse(&buf).unwrap();
    assert!(t.playback.is_empty());
    assert!(t.sections.iter().all(|s| s.id < id::PLAYBACK_STRINGS));

    let m = playback_model();
    let bytes = playback_bytes();
    assert_eq!(bytes, playback_bytes());
    let buf = AlignedBuf::from_bytes(&bytes);
    let t = Tmod::parse(&buf).unwrap();
    assert_eq!(t.playback, PlaybackData::from_model(&m).unwrap());
    assert_eq!(t.playback.motion_index("Sway"), Some(0));
    assert_eq!(t.playback.motion_name(1), "Still");
    assert_eq!(t.playback.expression_index("Smile"), Some(0));
    assert_eq!(t.playback.pose.len(), 1);
    // The playback sections are optional and the evaluation data is unchanged.
    for s in t.sections.iter().filter(|s| s.id >= id::PLAYBACK_STRINGS) {
        assert!(!s.required, "{:#x}", s.id);
    }
    assert_eq!(t.sections.len(), 52);
    let c = m.compile().unwrap();
    assert!(matches!(t.playback.motions, Cow::Borrowed(_)));
    let (lo, hi) = (buf.as_ptr() as usize, buf.as_ptr() as usize + buf.len());
    let p = t.playback.curves.as_ptr() as usize;
    assert!(p >= lo && p < hi);
    assert_roundtrip(&c);
}

#[test]
fn the_playback_layouts_match_the_specification() {
    use super::playback::*;
    use std::mem::size_of;
    assert_eq!(size_of::<MotionDef>(), 36);
    assert_eq!(size_of::<CurveDef>(), 36);
    assert_eq!(size_of::<SegmentDef>(), 28);
    assert_eq!(size_of::<ExpressionDef>(), 24);
    assert_eq!(size_of::<ExpressionParamDef>(), 12);
    assert_eq!(size_of::<PoseDef>(), 12);
    assert_eq!(size_of::<PoseGroupDef>(), 8);
    assert_eq!(size_of::<PoseEntryDef>(), 12);
}

#[test]
fn a_loader_that_predates_the_playback_sections_still_reads_the_file() {
    // Simulated by renaming them to IDs nobody knows: they are skipped.
    let b = playback_bytes();
    let n = le_u32(&b, 16) as usize;
    let mut b = b;
    for i in 0..n {
        let at = HEADER_LEN + i * ENTRY_LEN;
        let sec = le_u32(&b, at);
        if sec >= id::PLAYBACK_STRINGS {
            b = patch(&b, at, &(sec + 0x1000).to_le_bytes());
        }
    }
    let buf = AlignedBuf::from_bytes(&b);
    let t = Tmod::parse(&buf).unwrap();
    assert!(t.playback.is_empty());
}

#[test]
fn partial_or_inconsistent_playback_data_is_rejected() {
    let b = playback_bytes();
    let n = le_u32(&b, 16) as usize;
    let entry = |sec: u32| {
        (0..n)
            .map(|i| HEADER_LEN + i * ENTRY_LEN)
            .find(|&at| le_u32(&b, at) == sec)
            .unwrap()
    };
    // One of the ten missing.
    let missing = patch(&b, entry(id::POSE_LINKS), &0x7777u32.to_le_bytes());
    assert_eq!(parse(&missing), Err(Error::MissingSection(id::POSE_LINKS)));
    // A curve that targets a parameter past the end.
    let buf = AlignedBuf::from_bytes(&b);
    let at = le_u64(&buf, entry(id::MOTION_CURVES) + 8) as usize;
    let bad = patch(&b, at + 4, &999u32.to_le_bytes());
    assert!(matches!(parse(&bad), Err(Error::Layout(_))));
    // A segment whose time runs backwards.
    let at = le_u64(&buf, entry(id::MOTION_SEGMENTS) + 8) as usize;
    let bad = patch(&b, at + 4, &(-1.0f32).to_le_bytes());
    assert!(matches!(parse(&bad), Err(Error::Layout(_))));
    // An expression blend that is not known.
    let at = le_u64(&buf, entry(id::EXPRESSION_PARAMS) + 8) as usize;
    let bad = patch(&b, at + 4, &9u32.to_le_bytes());
    assert!(matches!(parse(&bad), Err(Error::Layout(_))));
}

#[test]
fn bad_playback_data_is_not_written() {
    let m = playback_model();
    let c = m.compile().unwrap();
    let mut p = PlaybackData::from_model(&m).unwrap();
    p.segments.to_mut()[0].end_value = f32::NAN;
    assert!(matches!(write_with(&c, &p, &[]), Err(Error::Layout(_))));
    let mut p = PlaybackData::from_model(&m).unwrap();
    p.expression_params.to_mut()[0].parameter = 77;
    assert!(matches!(write_with(&c, &p, &[]), Err(Error::Layout(_))));
    // The editing model refuses a curve that runs backwards.
    let mut m = playback_model();
    let id = m.motions.find("Sway").unwrap();
    m.motions.get_mut(id).unwrap().curves[0].segments[0] = Segment::Linear([-1.0, 0.0]);
    assert!(PlaybackData::from_model(&m).is_err());
}
