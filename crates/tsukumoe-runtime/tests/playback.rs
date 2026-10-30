// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The runtime against known values and invariances. Models are built with
//! `tsukumoe-core`, written as `.tmod` and played back through the runtime.

use tsukumoe_core::elements::{Parameter, Part};
use tsukumoe_core::elements::{ParameterId, PartId};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::playback::{
    Curve, CurveTarget, Expression, ExpressionBlend, ExpressionParameter, Motion, Pose, PoseEntry,
    Segment,
};
use tsukumoe_core::rig::ArtMesh;
use tsukumoe_core::table::Table;
use tsukumoe_core::tmod::{self, AlignedBuf, PlaybackData};
use tsukumoe_runtime::{
    AutoConfig, BlinkConfig, BreathConfig, Error, MAX_LAYERS, Priority, Runtime,
};

struct Ids {
    p: ParameterId,
    q: ParameterId,
    a: PartId,
    b: PartId,
}

#[derive(Default)]
struct Content {
    motions: Vec<(&'static str, Motion)>,
    expressions: Vec<(&'static str, Expression)>,
    pose: Option<Pose>,
}

/// P (0..1, 0), Q (-1..1, 0), EyeL and EyeR (0..1, 1), Breath (0..1, 0);
/// parts A and B, each with one triangle mesh (`MA`, `MB`).
fn model(content: impl FnOnce(&Ids) -> Content) -> Model {
    let mut parameters = Table::new();
    let mut add = |name, min, max, default| {
        parameters
            .insert(name, Parameter::new(min, max, default))
            .unwrap()
    };
    let p = add("P", 0.0, 1.0, 0.0);
    let q = add("Q", -1.0, 1.0, 0.0);
    add("EyeL", 0.0, 1.0, 1.0);
    add("EyeR", 0.0, 1.0, 1.0);
    add("Breath", 0.0, 1.0, 0.0);
    let mut parts = Table::new();
    let a = parts.insert("A", Part::default()).unwrap();
    let b = parts.insert("B", Part::default()).unwrap();
    let tri = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    let mut meshes = Table::new();
    for (name, part) in [("MA", a), ("MB", b)] {
        meshes
            .insert(
                name,
                ArtMesh::new(part, tri.clone(), tri.clone(), vec![0, 1, 2]),
            )
            .unwrap();
    }
    let ids = Ids { p, q, a, b };
    let content = content(&ids);
    let mut motions = Table::new();
    for (name, m) in content.motions {
        motions.insert(name, m).unwrap();
    }
    let mut expressions = Table::new();
    for (name, e) in content.expressions {
        expressions.insert(name, e).unwrap();
    }
    Model::from_parts(ModelParts {
        parameters,
        parts,
        meshes,
        motions,
        expressions,
        pose: content.pose,
        ..ModelParts::default()
    })
    .unwrap()
}

fn bytes(m: &Model) -> Vec<u8> {
    let c = m.compile().unwrap();
    tmod::write_with(&c, &PlaybackData::from_model(m).unwrap(), &[]).unwrap()
}

fn runtime(m: &Model) -> Runtime<'static> {
    Runtime::from_bytes(&bytes(m)).unwrap()
}

/// A motion holding `value` on `target`, `looped` over `duration`.
fn hold(target: CurveTarget, value: f32, duration: f32, fade: f32) -> Motion {
    Motion {
        duration,
        fps: 30.0,
        looped: true,
        fade_in: fade,
        fade_out: fade,
        curves: vec![Curve {
            target,
            start: [0.0, value],
            segments: vec![Segment::Linear([duration, value])],
            fade_in: None,
            fade_out: None,
        }],
    }
}

fn linear(target: CurveTarget, points: &[[f32; 2]], looped: bool) -> Motion {
    Motion {
        duration: points.last().unwrap()[0],
        fps: 30.0,
        looped,
        fade_in: 0.0,
        fade_out: 0.0,
        curves: vec![Curve {
            target,
            start: points[0],
            segments: points[1..].iter().map(|&p| Segment::Linear(p)).collect(),
            fade_in: None,
            fade_out: None,
        }],
    }
}

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
}

fn value(rt: &Runtime, name: &str) -> f32 {
    rt.parameter_values()[rt.parameter_index(name).unwrap()]
}

fn opacity(rt: &Runtime, mesh: &str) -> f32 {
    let list = rt.draw_list();
    let i = rt.model().mesh_index(mesh).unwrap();
    list.drawable(i).opacity
}

/// Advances `seconds` in steps of `dt`.
fn run(rt: &mut Runtime, seconds: f32, dt: f32) {
    let steps = (seconds / dt).round() as usize;
    for _ in 0..steps {
        rt.update(dt);
    }
}

#[test]
fn a_motion_plays_by_time_and_ends() {
    let m = model(|id| Content {
        motions: vec![(
            "up",
            linear(
                CurveTarget::Parameter(id.p),
                &[[0.0, 0.0], [1.0, 1.0]],
                false,
            ),
        )],
        ..Content::default()
    });
    let mut rt = runtime(&m);
    assert_eq!(rt.play_motion("up", 0, Priority::Normal), Ok(true));
    rt.update(0.0);
    near(value(&rt, "P"), 0.0);
    rt.update(0.25);
    near(value(&rt, "P"), 0.25);
    run(&mut rt, 0.75, 0.25);
    near(value(&rt, "P"), 1.0);
    // The motion has ended: the parameter returns to its default.
    rt.update(0.1);
    near(value(&rt, "P"), 0.0);
    assert!(!rt.is_playing(0));
}

#[test]
fn a_loop_is_continuous_at_the_seam() {
    let m = model(|id| Content {
        motions: vec![(
            "wave",
            linear(
                CurveTarget::Parameter(id.p),
                &[[0.0, 0.2], [1.0, 0.8], [2.0, 0.2]],
                true,
            ),
        )],
        ..Content::default()
    });
    let mut rt = runtime(&m);
    rt.play_motion("wave", 0, Priority::Normal).unwrap();
    rt.update(1.999);
    let before = value(&rt, "P");
    rt.update(0.002);
    let after = value(&rt, "P");
    assert!((before - 0.2).abs() < 1e-3 && (after - 0.2).abs() < 1e-3);
    assert!((before - after).abs() < 2e-3);
    // And the loop goes on: the middle of the second round.
    rt.update(0.999);
    near(value(&rt, "P"), 0.8);
    assert!(rt.is_playing(0));
}

#[test]
fn a_crossfade_ends_with_the_new_motion_at_full_weight() {
    let m = model(|id| Content {
        motions: vec![
            ("a", hold(CurveTarget::Parameter(id.q), 1.0, 4.0, 1.0)),
            ("b", hold(CurveTarget::Parameter(id.q), -1.0, 4.0, 1.0)),
        ],
        ..Content::default()
    });
    let mut rt = runtime(&m);
    rt.play_motion("a", 0, Priority::Normal).unwrap();
    run(&mut rt, 2.0, 0.5);
    near(value(&rt, "Q"), 1.0);
    rt.play_motion("b", 0, Priority::Normal).unwrap();
    // Halfway: a at weight 1/2 over the default 0 gives 1/2, then b at 1/2 over that.
    rt.update(0.5);
    near(value(&rt, "Q"), -0.25);
    rt.update(0.5);
    near(value(&rt, "Q"), -1.0);
    // a is gone: b alone from here on.
    rt.update(0.5);
    near(value(&rt, "Q"), -1.0);
    assert!(rt.is_playing(0));
}

#[test]
fn a_motion_fades_in_from_the_default_and_out_when_stopped() {
    let m = model(|id| Content {
        motions: vec![("a", hold(CurveTarget::Parameter(id.q), 1.0, 4.0, 1.0))],
        ..Content::default()
    });
    let mut rt = runtime(&m);
    rt.play_motion("a", 0, Priority::Normal).unwrap();
    rt.update(0.5);
    near(value(&rt, "Q"), 0.5);
    rt.update(1.0);
    near(value(&rt, "Q"), 1.0);
    rt.stop(0);
    assert!(!rt.is_playing(0));
    rt.update(0.25);
    near(value(&rt, "Q"), 0.75);
    rt.update(0.75);
    near(value(&rt, "Q"), 0.0);
    rt.update(0.5);
    near(value(&rt, "Q"), 0.0);
}

#[test]
fn a_curve_may_fade_on_its_own() {
    let m = model(|id| {
        let mut slow = hold(CurveTarget::Parameter(id.q), 1.0, 4.0, 0.0);
        slow.curves[0].fade_in = Some(2.0);
        slow.curves[0].fade_out = Some(0.0);
        Content {
            motions: vec![("slow", slow)],
            ..Content::default()
        }
    });
    let mut rt = runtime(&m);
    rt.play_motion("slow", 0, Priority::Normal).unwrap();
    rt.update(1.0);
    near(value(&rt, "Q"), 0.5);
}

#[test]
fn priorities_block_lower_motions_and_force_cuts_the_layer() {
    let m = model(|id| {
        let q = |v| hold(CurveTarget::Parameter(id.q), v, 4.0, 0.0);
        Content {
            motions: vec![
                ("idle", q(0.5)),
                ("normal", q(-0.5)),
                ("force", hold(CurveTarget::Parameter(id.q), 0.75, 4.0, 1.0)),
                ("slow", hold(CurveTarget::Parameter(id.q), -1.0, 4.0, 1.0)),
            ],
            ..Content::default()
        }
    });
    let mut rt = runtime(&m);
    // An idle motion yields to a normal one that is already playing.
    assert_eq!(rt.play_motion("normal", 0, Priority::Normal), Ok(true));
    assert_eq!(rt.play_motion("idle", 1, Priority::Idle), Ok(false));
    rt.update(0.1);
    near(value(&rt, "Q"), -0.5);
    // After it is stopped the idle one may start; while both play, normal wins.
    rt.stop(0);
    assert_eq!(rt.play_motion("idle", 1, Priority::Idle), Ok(true));
    assert_eq!(rt.play_motion("normal", 2, Priority::Normal), Ok(true));
    rt.update(0.1);
    near(value(&rt, "Q"), -0.5);
    // Force starts whatever plays elsewhere, and a normal one is blocked by it.
    assert_eq!(rt.play_motion("force", 3, Priority::Force), Ok(true));
    assert_eq!(rt.play_motion("normal", 4, Priority::Normal), Ok(false));
    // Equal priorities do not block each other.
    assert_eq!(rt.play_motion("slow", 5, Priority::Force), Ok(true));
    // Layers of one priority blend in layer order: layer 5 has the last word.
    run(&mut rt, 2.0, 0.5);
    near(value(&rt, "Q"), -1.0);
}

#[test]
fn a_forced_motion_cuts_its_layer_where_a_normal_one_crossfades() {
    let make = |priority| {
        let m = model(|id| Content {
            motions: vec![
                ("a", hold(CurveTarget::Parameter(id.q), 1.0, 4.0, 1.0)),
                ("b", hold(CurveTarget::Parameter(id.q), -1.0, 4.0, 1.0)),
            ],
            ..Content::default()
        });
        let mut rt = runtime(&m);
        rt.play_motion("a", 0, Priority::Normal).unwrap();
        run(&mut rt, 2.0, 0.5);
        rt.play_motion("b", 0, priority).unwrap();
        rt.update(0.5);
        value(&rt, "Q")
    };
    near(make(Priority::Normal), -0.25);
    // Cut: only b, at half weight over the default.
    near(make(Priority::Force), -0.5);
}

#[test]
fn a_higher_priority_motion_on_the_same_layer_is_not_replaced_by_a_lower_one() {
    let m = model(|id| Content {
        motions: vec![
            ("n", hold(CurveTarget::Parameter(id.q), 0.5, 4.0, 0.0)),
            ("i", hold(CurveTarget::Parameter(id.q), -0.5, 4.0, 0.0)),
        ],
        ..Content::default()
    });
    let mut rt = runtime(&m);
    rt.play_motion("n", 0, Priority::Normal).unwrap();
    assert_eq!(rt.play_motion("i", 0, Priority::Idle), Ok(false));
    rt.update(0.1);
    near(value(&rt, "Q"), 0.5);
}

#[test]
fn part_opacity_curves_drive_the_draw_list() {
    let m = model(|id| Content {
        motions: vec![(
            "hide",
            linear(
                CurveTarget::PartOpacity(id.a),
                &[[0.0, 1.0], [1.0, 0.0]],
                false,
            ),
        )],
        ..Content::default()
    });
    let mut rt = runtime(&m);
    rt.update(0.0);
    near(opacity(&rt, "MA"), 1.0);
    rt.play_motion("hide", 0, Priority::Normal).unwrap();
    rt.update(0.5);
    near(opacity(&rt, "MA"), 0.5);
    near(opacity(&rt, "MB"), 1.0);
    // Ended: the part is back to its model opacity.
    run(&mut rt, 1.0, 0.5);
    rt.update(0.5);
    near(opacity(&rt, "MA"), 1.0);
}

#[test]
fn a_motion_is_the_same_at_the_same_time_whatever_the_step() {
    let m = model(|id| {
        let mut wave = linear(
            CurveTarget::Parameter(id.p),
            &[[0.0, 0.0], [0.7, 1.0], [1.3, 0.3], [2.0, 0.9]],
            true,
        );
        wave.fade_in = 0.4;
        wave.curves.push(Curve {
            target: CurveTarget::Parameter(id.q),
            start: [0.0, 0.0],
            segments: vec![Segment::Bezier([[0.5, 1.0], [1.5, -1.0], [2.0, 0.0]])],
            fade_in: None,
            fade_out: None,
        });
        Content {
            motions: vec![
                ("wave", wave),
                ("b", hold(CurveTarget::Parameter(id.q), -1.0, 3.0, 0.8)),
            ],
            ..Content::default()
        }
    });
    let play = |dt: f32| {
        let mut rt = runtime(&m);
        rt.play_motion("wave", 0, Priority::Normal).unwrap();
        run(&mut rt, 1.2, dt);
        rt.play_motion("b", 0, Priority::Normal).unwrap();
        run(&mut rt, 0.6, dt);
        (value(&rt, "P"), value(&rt, "Q"))
    };
    let reference = play(0.6);
    for dt in [0.05, 0.2, 0.3, 1.0 / 60.0 * 3.0] {
        let got = play(dt);
        assert!(
            (got.0 - reference.0).abs() < 1e-4 && (got.1 - reference.1).abs() < 1e-4,
            "dt {dt}: {got:?} vs {reference:?}"
        );
    }
}

#[test]
fn expressions_add_multiply_and_overwrite_with_fades() {
    let param = |parameter, blend, value| ExpressionParameter {
        parameter,
        blend,
        value,
    };
    let m = model(|id| Content {
        expressions: vec![
            (
                "add",
                Expression {
                    fade_in: 1.0,
                    fade_out: 1.0,
                    parameters: vec![param(id.q, ExpressionBlend::Add, 0.5)],
                },
            ),
            (
                "mul",
                Expression {
                    fade_in: 0.0,
                    fade_out: 0.0,
                    parameters: vec![
                        param(id.p, ExpressionBlend::Multiply, 3.0),
                        param(id.q, ExpressionBlend::Multiply, 0.5),
                    ],
                },
            ),
            (
                "set",
                Expression {
                    fade_in: 0.0,
                    fade_out: 0.0,
                    parameters: vec![param(id.q, ExpressionBlend::Overwrite, 0.2)],
                },
            ),
        ],
        motions: vec![("q", hold(CurveTarget::Parameter(id.q), 0.4, 9.0, 0.0))],
        ..Content::default()
    });
    let mut rt = runtime(&m);
    rt.play_motion("q", 0, Priority::Normal).unwrap();
    rt.play_expression("add").unwrap();
    // Halfway through the fade: 0.4 + 0.5 * 0.5.
    rt.update(0.5);
    near(value(&rt, "Q"), 0.65);
    rt.update(0.5);
    near(value(&rt, "Q"), 0.9);
    // Multiply on top, in the order started: (0.4 + 0.5) * 0.5.
    rt.play_expression("mul").unwrap();
    rt.update(0.0);
    near(value(&rt, "Q"), 0.45);
    // Overwrite last: the value is 0.2 whatever came before.
    rt.play_expression("set").unwrap();
    rt.update(0.0);
    near(value(&rt, "Q"), 0.2);
    // Fading out: the weight falls and then the expression is dropped.
    rt.stop_expression("set").unwrap();
    rt.stop_expression("add").unwrap();
    rt.stop_expression("mul").unwrap();
    rt.update(0.5);
    // "set" and "mul" have no fade out and drop at once; "add" is at weight 1/2.
    near(value(&rt, "Q"), 0.65);
}

#[test]
fn expressions_stack_in_the_order_they_start() {
    let e = |blend, v, id: &Ids| Expression {
        fade_in: 0.0,
        fade_out: 0.0,
        parameters: vec![ExpressionParameter {
            parameter: id.q,
            blend,
            value: v,
        }],
    };
    let m = model(|id| Content {
        expressions: vec![
            ("add", e(ExpressionBlend::Add, 0.5, id)),
            ("set", e(ExpressionBlend::Overwrite, 0.2, id)),
        ],
        ..Content::default()
    });
    let mut rt = runtime(&m);
    rt.play_expression("add").unwrap();
    rt.play_expression("set").unwrap();
    rt.update(0.0);
    near(value(&rt, "Q"), 0.2);
    // Playing "add" again moves it last.
    rt.play_expression("add").unwrap();
    rt.update(0.0);
    near(value(&rt, "Q"), 0.7);
}

#[test]
fn the_pose_shows_one_part_of_a_group_and_fades_the_other() {
    let m = model(|id| Content {
        motions: vec![(
            "show_b",
            hold(CurveTarget::PartOpacity(id.b), 1.0, 9.0, 0.0),
        )],
        pose: Some(Pose {
            fade_in: 0.5,
            groups: vec![vec![
                PoseEntry {
                    part: id.a,
                    linked: vec![],
                },
                PoseEntry {
                    part: id.b,
                    linked: vec![],
                },
            ]],
        }),
        ..Content::default()
    });
    let at = |dt: f32, total: f32| {
        let mut rt = runtime(&m);
        rt.update(0.0);
        // The first part of the group is shown from the start.
        near(opacity(&rt, "MA"), 1.0);
        near(opacity(&rt, "MB"), 0.0);
        rt.play_motion("show_b", 0, Priority::Normal).unwrap();
        run(&mut rt, total, dt);
        (opacity(&rt, "MA"), opacity(&rt, "MB"))
    };
    let (a, b) = at(0.25, 0.25);
    near(a, 0.5);
    near(b, 0.5);
    for dt in [0.05, 0.125, 0.25] {
        let (a, b) = at(dt, 0.25);
        near(a, 0.5);
        near(b, 0.5);
    }
    let (a, b) = at(0.5, 0.5);
    near(a, 0.0);
    near(b, 1.0);
    let (a, b) = at(0.5, 2.0);
    near(a, 0.0);
    near(b, 1.0);
}

#[test]
fn linked_parts_follow_the_pose_entry() {
    let m = model(|id| Content {
        pose: Some(Pose {
            fade_in: 0.0,
            groups: vec![vec![
                PoseEntry {
                    part: id.a,
                    linked: vec![],
                },
                PoseEntry {
                    part: id.b,
                    linked: vec![id.b],
                },
            ]],
        }),
        ..Content::default()
    });
    let mut rt = runtime(&m);
    rt.update(0.0);
    near(opacity(&rt, "MA"), 1.0);
    near(opacity(&rt, "MB"), 0.0);
}

#[test]
fn blinking_is_deterministic_closes_both_eyes_and_follows_the_seed() {
    let m = model(|_| Content::default());
    let config = |seed| {
        let mut b = BlinkConfig::new("EyeL", "EyeR");
        b.interval = 1.0;
        b.seed = seed;
        AutoConfig {
            blink: Some(b),
            breath: None,
        }
    };
    let trace = |seed: u64, dt: f32| {
        let mut rt = runtime(&m);
        rt.set_auto(&config(seed)).unwrap();
        let mut out = Vec::new();
        for _ in 0..(30.0 / dt) as usize {
            rt.update(dt);
            assert_eq!(value(&rt, "EyeL"), value(&rt, "EyeR"));
            out.push(value(&rt, "EyeL"));
        }
        out
    };
    let a = trace(7, 1.0 / 60.0);
    assert_eq!(a, trace(7, 1.0 / 60.0));
    assert_ne!(a, trace(8, 1.0 / 60.0));
    assert!(a.contains(&0.0));
    assert!(a.iter().all(|&v| (0.0..=1.0).contains(&v)));
    let blinks = a.windows(2).filter(|w| w[0] == 1.0 && w[1] < 1.0).count();
    assert!((10..=45).contains(&blinks), "{blinks} blinks in 30 s");
    // The same times with a step twice as long: every second sample matches.
    let coarse = trace(7, 1.0 / 30.0);
    for (i, v) in coarse.iter().enumerate() {
        assert!((v - a[2 * i + 1]).abs() < 1e-4, "sample {i}");
    }
}

#[test]
fn breathing_follows_the_sine_and_nothing_runs_unless_configured() {
    let m = model(|_| Content::default());
    let mut rt = runtime(&m);
    run(&mut rt, 1.0, 0.25);
    // Not configured: nothing is written, the defaults stand.
    assert_eq!(rt.parameter_values(), [0.0, 0.0, 1.0, 1.0, 0.0]);
    rt.set_auto(&AutoConfig {
        blink: None,
        breath: Some(BreathConfig {
            parameter: "Breath".into(),
            cycle: 4.0,
            amplitude: 0.5,
            offset: 0.5,
        }),
    })
    .unwrap();
    rt.update(0.0);
    near(value(&rt, "Breath"), 0.5);
    rt.update(1.0);
    near(value(&rt, "Breath"), 1.0);
    rt.update(2.0);
    near(value(&rt, "Breath"), 0.0);
    // Back off: no more writes.
    rt.set_auto(&AutoConfig::default()).unwrap();
    rt.update(1.0);
    near(value(&rt, "Breath"), 0.0);
    assert_eq!(rt.parameter_values(), [0.0, 0.0, 1.0, 1.0, 0.0]);
}

#[test]
fn a_bad_auto_config_is_an_error_and_changes_nothing() {
    let m = model(|_| Content::default());
    let mut rt = runtime(&m);
    let breath = |name: &str, cycle| AutoConfig {
        blink: None,
        breath: Some(BreathConfig {
            parameter: name.into(),
            cycle,
            amplitude: 1.0,
            offset: 0.0,
        }),
    };
    rt.set_auto(&breath("Breath", 2.0)).unwrap();
    assert_eq!(
        rt.set_auto(&breath("Nope", 2.0)),
        Err(Error::UnknownParameter("Nope".into()))
    );
    assert!(matches!(
        rt.set_auto(&breath("Breath", 0.0)),
        Err(Error::BadAutoValue(_))
    ));
    let mut blink = BlinkConfig::new("EyeL", "Gone");
    assert!(matches!(
        rt.set_auto(&AutoConfig {
            blink: Some(blink.clone()),
            breath: None
        }),
        Err(Error::UnknownParameter(_))
    ));
    blink.right_eye = "EyeR".into();
    blink.interval = 0.0;
    assert!(matches!(
        rt.set_auto(&AutoConfig {
            blink: Some(blink),
            breath: None
        }),
        Err(Error::BadAutoValue(_))
    ));
    // The breath from before is still running.
    rt.update(0.5);
    assert!(value(&rt, "Breath") > 0.0);
}

#[test]
fn a_set_parameter_has_the_last_word_until_cleared() {
    let m = model(|id| Content {
        motions: vec![("q", hold(CurveTarget::Parameter(id.q), 0.4, 9.0, 0.0))],
        ..Content::default()
    });
    let mut rt = runtime(&m);
    rt.play_motion("q", 0, Priority::Normal).unwrap();
    let q = rt.parameter_index("Q").unwrap();
    assert!(rt.set_parameter(q, -0.9));
    run(&mut rt, 1.0, 0.25);
    near(value(&rt, "Q"), -0.9);
    assert!(rt.clear_parameter(q));
    rt.update(0.1);
    near(value(&rt, "Q"), 0.4);
    assert!(!rt.set_parameter(99, 1.0));
    assert!(!rt.clear_parameter(99));
}

#[test]
fn errors_name_what_was_wrong() {
    let m = model(|_| Content::default());
    let mut rt = runtime(&m);
    assert_eq!(
        rt.play_motion("nope", 0, Priority::Normal),
        Err(Error::UnknownMotion("nope".into()))
    );
    assert_eq!(
        rt.play_motion("nope", MAX_LAYERS, Priority::Normal),
        Err(Error::BadLayer(MAX_LAYERS))
    );
    assert_eq!(
        rt.play_expression("nope"),
        Err(Error::UnknownExpression("nope".into()))
    );
    assert!(rt.stop_expression("nope").is_err());
    // Odd steps are harmless.
    for dt in [f32::NAN, -1.0, f32::INFINITY, 1e30, 0.0] {
        rt.update(dt);
    }
    // Garbage is an error, not a panic.
    assert!(matches!(
        Runtime::from_bytes(b"not a tmod"),
        Err(Error::Load(_))
    ));
}

/// A scripted session to compare loaders with.
fn session(rt: &mut Runtime) -> Vec<u32> {
    let mut out = Vec::new();
    rt.set_auto(&AutoConfig {
        blink: Some(BlinkConfig::new("EyeL", "EyeR")),
        breath: Some(BreathConfig {
            parameter: "Breath".into(),
            cycle: 3.0,
            amplitude: 0.4,
            offset: 0.5,
        }),
    })
    .unwrap();
    rt.play_motion("wave", 0, Priority::Normal).unwrap();
    rt.play_expression("add").unwrap();
    for step in 0..120 {
        if step == 40 {
            rt.play_motion("b", 0, Priority::Normal).unwrap();
        }
        if step == 80 {
            rt.stop_expression("add").unwrap();
        }
        rt.update(1.0 / 30.0);
        out.extend(rt.parameter_values().iter().map(|v| v.to_bits()));
        out.extend(rt.draw_list().positions().iter().map(|v| v.to_bits()));
        let list = rt.draw_list();
        out.extend((0..list.len()).map(|i| list.drawable(i).opacity.to_bits()));
    }
    out
}

#[test]
fn every_way_of_loading_gives_the_same_playback() {
    let m = model(|id| Content {
        motions: vec![
            (
                "wave",
                linear(
                    CurveTarget::Parameter(id.p),
                    &[[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]],
                    true,
                ),
            ),
            ("b", hold(CurveTarget::PartOpacity(id.b), 0.5, 3.0, 0.5)),
        ],
        expressions: vec![(
            "add",
            Expression {
                fade_in: 0.5,
                fade_out: 0.5,
                parameters: vec![ExpressionParameter {
                    parameter: id.q,
                    blend: ExpressionBlend::Add,
                    value: 0.3,
                }],
            },
        )],
        pose: Some(Pose {
            fade_in: 0.3,
            groups: vec![vec![
                PoseEntry {
                    part: id.a,
                    linked: vec![],
                },
                PoseEntry {
                    part: id.b,
                    linked: vec![],
                },
            ]],
        }),
    });
    // Built from the compiled data (owned).
    let mut owned =
        Runtime::new(m.compile().unwrap(), PlaybackData::from_model(&m).unwrap()).unwrap();
    let expected = session(&mut owned);
    // Copied from unaligned bytes.
    assert_eq!(
        session(&mut Runtime::from_bytes(&bytes(&m)).unwrap()),
        expected
    );
    // Read in place from an aligned buffer.
    let buf = AlignedBuf::from_bytes(&bytes(&m));
    assert_eq!(session(&mut Runtime::load(&buf).unwrap()), expected);
    // A misaligned slice is refused, not copied.
    let mut shifted = vec![0u8; buf.len() + 17];
    let k = (1..=16)
        .find(|k| !(shifted.as_ptr() as usize + k).is_multiple_of(16))
        .unwrap();
    shifted[k..k + buf.len()].copy_from_slice(&buf);
    assert!(matches!(
        Runtime::load(&shifted[k..k + buf.len()]),
        Err(Error::Load(tmod::Error::Misaligned))
    ));
}

#[test]
fn a_file_without_motions_plays_too() {
    let m = model(|_| Content::default());
    let mut rt = runtime(&m);
    rt.update(0.1);
    assert_eq!(rt.playback().motions.len(), 0);
    assert_eq!(
        rt.play_motion("any", 0, Priority::Normal),
        Err(Error::UnknownMotion("any".into()))
    );
    near(opacity(&rt, "MA"), 1.0);
}
