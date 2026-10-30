// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use tsukumoe_core::driver::Driver;
use tsukumoe_core::elements::{Parameter, Part};
use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::physics::{
    ChainJoint, Channel, Particle, PendulumGroup, PhysicsChain, PhysicsInput, PhysicsOutput, Range,
};
use tsukumoe_core::playback::{
    Curve, CurveTarget, Expression, ExpressionBlend, ExpressionParameter, Motion, Pose, PoseEntry,
    Segment,
};
use tsukumoe_core::rig::{
    ArtMesh, BlendMode, BlendShape, BlendTarget, Bone, BoneTransform, DeformerRef, Glue, GlueNode,
    MeshKeyform, RotationDeformer, RotationKeyform, VertexInfluences, WarpDeformer, WarpKeyform,
};
use tsukumoe_format::{Error, Manifest, Project, read_model, write_model};

const GRID: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
const TRI: [[f32; 2]; 3] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];

/// One of every kind of element, referring to each other.
fn rich() -> Model {
    let mut p = ModelParts::default();
    let angle = p
        .parameters
        .insert(
            "ParamAngle",
            Parameter {
                display_name: "Angle".into(),
                min: -30.0,
                max: 30.0,
                default: 0.0,
                repeat: true,
                group: Some("Face".into()),
            },
        )
        .unwrap();
    let open = p
        .parameters
        .insert("ParamOpen", Parameter::new(0.0, 1.0, 0.0))
        .unwrap();
    let out = p
        .parameters
        .insert("ParamOut", Parameter::new(-1.0, 1.0, 0.0))
        .unwrap();
    let driven = p
        .parameters
        .insert("ParamDriven", Parameter::new(0.0, 1.0, 0.0))
        .unwrap();

    let root = p.parts.insert("Root", Part::default()).unwrap();
    let child = p
        .parts
        .insert(
            "Child 子",
            Part {
                parent: Some(root),
                opacity: 0.5,
                visible: false,
            },
        )
        .unwrap();

    let b3 = p.bindings.insert(
        Binding::new(vec![Axis {
            parameter: angle,
            keys: vec![-30.0, 0.0, 30.0],
        }])
        .unwrap(),
    );
    // A binding no element uses still survives.
    p.bindings.insert(
        Binding::new(vec![Axis {
            parameter: open,
            keys: vec![0.0, 1.0],
        }])
        .unwrap(),
    );

    let pose_bone = |x: f32| BoneTransform {
        translation: [x, 2.0],
        rotation_degrees: 10.0 * x,
        scale: [1.0, 1.5],
    };
    let spine = p
        .bones
        .insert(
            "Spine",
            Bone {
                parent: None,
                base: pose_bone(0.0),
                keyforms: Some(KeyformGrid::new(
                    b3,
                    vec![pose_bone(-1.0), pose_bone(0.0), pose_bone(1.0)],
                )),
            },
        )
        .unwrap();
    let arm = p
        .bones
        .insert(
            "Arm",
            Bone {
                parent: Some(spine),
                base: pose_bone(0.5),
                keyforms: None,
            },
        )
        .unwrap();

    let warp_kf = |dx: f32| WarpKeyform {
        points: GRID.iter().map(|q| [q[0] + dx, q[1]]).collect(),
        opacity: 1.0 - dx.abs() * 0.1,
    };
    let warp = p
        .warps
        .insert(
            "Warp",
            WarpDeformer {
                part: root,
                parent: None,
                rows: 1,
                columns: 1,
                bezier: true,
                base_points: GRID.to_vec(),
                keyforms: Some(KeyformGrid::new(
                    b3,
                    vec![warp_kf(-1.0), warp_kf(0.0), warp_kf(1.0)],
                )),
            },
        )
        .unwrap();
    let rot_kf = |a: f32| RotationKeyform {
        origin: [0.5, 0.5],
        angle_degrees: a,
        scale: 1.25,
        reflect_x: a > 0.0,
        reflect_y: false,
        opacity: 0.9,
    };
    let rotation = p
        .rotations
        .insert(
            "Rotation",
            RotationDeformer {
                part: child,
                parent: Some(DeformerRef::Warp(warp)),
                base: rot_kf(0.0),
                keyforms: Some(KeyformGrid::new(
                    b3,
                    vec![rot_kf(-5.0), rot_kf(0.0), rot_kf(5.0)],
                )),
            },
        )
        .unwrap();

    let mesh_kf = |dx: f32| MeshKeyform {
        positions: TRI.iter().map(|q| [q[0] + dx, q[1]]).collect(),
        opacity: 0.75,
        draw_order: 400.0 + dx,
        multiply_color: [0.9, 0.8, 0.7],
        screen_color: [0.1, 0.2, 0.3],
    };
    let mut face = ArtMesh::new(child, TRI.to_vec(), TRI.to_vec(), vec![0, 1, 2]);
    face.parent = Some(DeformerRef::Rotation(rotation));
    face.texture = 2;
    face.blend_mode = BlendMode::Additive;
    face.double_sided = true;
    face.influences = vec![
        VertexInfluences {
            bones: [Some(spine), Some(arm), None, None],
            weights: [0.25, 0.75, 0.0, 0.0],
        },
        VertexInfluences {
            bones: [Some(arm), None, Some(spine), None],
            weights: [1.0, 0.0, 0.0, 0.0],
        },
        VertexInfluences::default(),
    ];
    face.keyforms = Some(KeyformGrid::new(
        b3,
        vec![mesh_kf(-1.0), mesh_kf(0.0), mesh_kf(1.0)],
    ));
    let face = p.meshes.insert("Face", face).unwrap();
    let mut eye = ArtMesh::new(child, TRI.to_vec(), TRI.to_vec(), vec![0, 1, 2]);
    eye.masks = vec![face];
    eye.mask_inverted = true;
    eye.blend_mode = BlendMode::Multiply;
    let eye = p.meshes.insert("Eye", eye).unwrap();

    let glue_binding = p.bindings.insert(
        Binding::new(vec![Axis {
            parameter: open,
            keys: vec![0.0, 1.0],
        }])
        .unwrap(),
    );
    p.glues
        .insert(
            "Glue",
            Glue {
                mesh_a: face,
                mesh_b: eye,
                nodes: vec![GlueNode {
                    index_a: 0,
                    index_b: 1,
                    weight: 0.25,
                }],
                intensity: 0.8,
                keyforms: Some(KeyformGrid::new(glue_binding, vec![0.0, 1.0])),
            },
        )
        .unwrap();
    p.blend_shapes
        .insert(
            "Cheek",
            BlendShape {
                target: BlendTarget::Mesh(eye),
                parameter: open,
                keys: vec![0.0, 1.0],
                weights: vec![0.0, 1.0],
                deltas: vec![[0.1, 0.0]; 3],
            },
        )
        .unwrap();
    p.blend_shapes
        .insert(
            "Bulge",
            BlendShape {
                target: BlendTarget::Warp(warp),
                parameter: angle,
                keys: vec![-30.0, 30.0],
                weights: vec![0.0, 1.0],
                deltas: vec![[0.0, 0.1]; 4],
            },
        )
        .unwrap();
    p.drivers
        .insert(
            "Drive",
            Driver {
                output: driven,
                expression: "clamp(ParamOpen * 2, 0, 1)".into(),
            },
        )
        .unwrap();

    p.motions
        .insert(
            "idle/loop 1",
            Motion {
                duration: 3.0,
                fps: 30.0,
                looped: true,
                fade_in: 0.5,
                fade_out: 0.25,
                curves: vec![
                    Curve {
                        target: CurveTarget::Parameter(angle),
                        start: [0.0, 0.0],
                        segments: vec![
                            Segment::Linear([1.0, 10.0]),
                            Segment::Bezier([[1.2, 10.0], [1.8, 5.0], [2.0, 0.0]]),
                            Segment::Stepped([2.5, 3.0]),
                            Segment::InverseStepped([3.0, -3.0]),
                        ],
                        fade_in: Some(0.1),
                        fade_out: None,
                    },
                    Curve {
                        target: CurveTarget::PartOpacity(child),
                        start: [0.0, 1.0],
                        segments: vec![],
                        fade_in: None,
                        fade_out: Some(0.2),
                    },
                ],
            },
        )
        .unwrap();
    p.motions
        .insert(
            "second",
            Motion {
                duration: 1.0,
                fps: 60.0,
                looped: false,
                fade_in: 0.0,
                fade_out: 0.0,
                curves: vec![],
            },
        )
        .unwrap();
    p.expressions
        .insert(
            "smile",
            Expression {
                fade_in: 0.5,
                fade_out: 0.5,
                parameters: vec![
                    ExpressionParameter {
                        parameter: open,
                        blend: ExpressionBlend::Add,
                        value: 0.5,
                    },
                    ExpressionParameter {
                        parameter: out,
                        blend: ExpressionBlend::Multiply,
                        value: 0.9,
                    },
                    ExpressionParameter {
                        parameter: angle,
                        blend: ExpressionBlend::Overwrite,
                        value: 1.0,
                    },
                ],
            },
        )
        .unwrap();
    p.pose = Some(Pose {
        fade_in: 0.5,
        groups: vec![vec![
            PoseEntry {
                part: root,
                linked: vec![child],
            },
            PoseEntry {
                part: child,
                linked: vec![],
            },
        ]],
    });

    let io_in = |parameter, channel| PhysicsInput {
        parameter,
        channel,
        weight: 60.0,
        reflect: true,
    };
    let io_out = |parameter, channel, particle| PhysicsOutput {
        parameter,
        channel,
        particle,
        scale: 1.5,
        weight: 100.0,
        reflect: false,
    };
    p.physics.pendulums.push(PendulumGroup {
        name: "Hair".into(),
        inputs: vec![io_in(angle, Channel::X), io_in(open, Channel::Angle)],
        outputs: vec![io_out(out, Channel::Angle, 1)],
        particles: vec![
            Particle {
                position: [0.0, 0.0],
                mobility: 1.0,
                delay: 1.0,
                acceleration: 1.0,
                radius: 0.0,
            },
            Particle {
                position: [0.0, 10.0],
                mobility: 0.9,
                delay: 0.8,
                acceleration: 1.5,
                radius: 10.0,
            },
        ],
        position_range: Range {
            min: -10.0,
            max: 10.0,
            default: 0.0,
        },
        angle_range: Range {
            min: -30.0,
            max: 30.0,
            default: 0.0,
        },
    });
    p.physics.chains.push(PhysicsChain {
        name: "Ribbon".into(),
        joints: vec![
            ChainJoint {
                length: 1.0,
                stiffness: 0.5,
                damping: 0.1,
            },
            ChainJoint {
                length: 2.0,
                stiffness: 0.25,
                damping: 0.2,
            },
        ],
        gravity: [0.0, -9.8],
        wind: [1.0, 0.0],
        inputs: vec![io_in(angle, Channel::X)],
        outputs: vec![io_out(out, Channel::X, 1)],
    });

    Model::from_parts(p).expect("the sample model is consistent")
}

fn saved(model: &Model) -> Project {
    let mut project = Project::new(Manifest::new(Some("test".into())));
    write_model(model, &mut project).unwrap();
    project
}

#[test]
fn every_kind_of_element_survives_a_round_trip() {
    let model = rich();
    let project = saved(&model);
    let loaded = read_model(&project).unwrap();
    assert!(
        loaded == model,
        "the loaded model differs from the saved one"
    );
    // Saving again gives the same files, byte for byte.
    assert_eq!(saved(&loaded).files, project.files);
}

#[test]
fn the_archive_layout_gives_the_same_model() {
    let model = rich();
    let bytes = saved(&model).write_zip().unwrap();
    let loaded = read_model(&Project::read_zip(&bytes).unwrap()).unwrap();
    assert!(loaded == model);
}

#[test]
fn files_are_laid_out_as_documented() {
    let project = saved(&rich());
    let paths: Vec<_> = project.files.keys().map(String::as_str).collect();
    assert_eq!(
        paths,
        [
            "expressions/000000-smile.json",
            "model.json",
            "motions/000000-idle%2Floop%201.json",
            "motions/000001-second.json",
            "physics.json",
        ]
    );
    let text = String::from_utf8(project.files["model.json"].clone()).unwrap();
    assert!(text.starts_with("{\n  \"parameters\": [\n    {\n      \"name\": \"ParamAngle\""));
    assert!(text.ends_with("}\n"));
}

#[test]
fn rewriting_replaces_the_model_files_and_keeps_the_others() {
    let mut project = saved(&rich());
    project.files.insert("textures/a.png".into(), vec![1, 2, 3]);
    let mut model = Model::new();
    write_model(&model, &mut project).unwrap();
    assert!(project.files.contains_key("textures/a.png"));
    assert!(!project.files.keys().any(|k| k.starts_with("motions/")));
    assert!(read_model(&project).unwrap() == model);
    model = rich();
    write_model(&model, &mut project).unwrap();
    assert!(read_model(&project).unwrap() == model);
}

#[test]
fn omitted_fields_take_their_defaults() {
    let mut project = Project::new(Manifest::new(None));
    project.files.insert(
        "model.json".into(),
        br#"{
  "parameters": [{ "name": "P", "min": 0, "max": 1, "default": 0 }],
  "parts": [{ "name": "Part" }],
  "meshes": [{
    "name": "M", "part": "Part",
    "positions": [[0,0],[1,0],[0,1]], "uvs": [[0,0],[1,0],[0,1]], "indices": [0,1,2]
  }]
}"#
        .to_vec(),
    );
    let model = read_model(&project).unwrap();
    let (_, _, mesh) = model.meshes().iter().next().unwrap();
    assert_eq!(
        (mesh.opacity, mesh.draw_order, mesh.blend_mode),
        (1.0, 500.0, BlendMode::Normal)
    );
    assert_eq!(mesh.multiply_color, [1.0; 3]);
    let (_, _, part) = model.parts().iter().next().unwrap();
    assert!(part.visible && part.opacity == 1.0);
}

fn with_model_json(json: &str) -> Project {
    let mut project = Project::new(Manifest::new(None));
    project
        .files
        .insert("model.json".into(), json.as_bytes().to_vec());
    project
}

#[test]
fn bad_files_give_errors_that_say_what_is_wrong() {
    assert!(matches!(
        read_model(&Project::new(Manifest::new(None))),
        Err(Error::MissingFile(path)) if path == "model.json"
    ));
    assert!(matches!(
        read_model(&with_model_json("{ nope")),
        Err(Error::Json { file, .. }) if file == "model.json"
    ));
    assert!(matches!(
        read_model(&with_model_json(r#"{"parameters": [{"name": "P"}]}"#)),
        Err(Error::Json { .. })
    ));
    // A reference to something the file does not define.
    assert!(matches!(
        read_model(&with_model_json(r#"{"parts": [{"name": "A", "parent": "Nope"}]}"#)),
        Err(Error::Reference { field: "parent", name, .. }) if name == "Nope"
    ));
    // Names are unique.
    assert!(matches!(
        read_model(&with_model_json(
            r#"{"parts": [{"name": "A"}, {"name": "A"}]}"#
        )),
        Err(Error::Invalid(_))
    ));
    // A keyform grid names a binding that does not exist.
    let mesh = r#"{"parameters": [{"name":"P","min":0,"max":1,"default":0}],
        "parts": [{"name":"Part"}],
        "meshes": [{"name":"M","part":"Part","positions":[[0,0]],"uvs":[[0,0]],"indices":[],
          "keyforms":{"binding":3,"keyforms":[]}}]}"#;
    assert!(matches!(
        read_model(&with_model_json(mesh)),
        Err(Error::Invalid(_))
    ));
    // The content must make a consistent model: uvs for every vertex.
    let short_uvs = r#"{"parts": [{"name":"Part"}],
        "meshes": [{"name":"M","part":"Part","positions":[[0,0],[1,0]],"uvs":[[0,0]],"indices":[]}]}"#;
    assert!(matches!(
        read_model(&with_model_json(short_uvs)),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn damaged_model_files_never_panic() {
    let project = saved(&rich());
    for (path, bytes) in &project.files {
        for len in (0..bytes.len()).step_by(7) {
            let mut damaged = project.clone();
            damaged.files.insert(path.clone(), bytes[..len].to_vec());
            let _ = read_model(&damaged);
        }
        for i in (0..bytes.len()).step_by(11) {
            let mut damaged = project.clone();
            damaged.files.get_mut(path).unwrap()[i] ^= 0x55;
            let _ = read_model(&damaged);
        }
    }
}
