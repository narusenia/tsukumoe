// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Runs the `tsukumoe` binary on a generated project: a red quad covering
//! `[0, 1]^2` that the parameter `P` moves to `[1, 2] x [0, 1]`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use tsukumoe_core::elements::{Parameter, Part, Point};
use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
use tsukumoe_core::model::ModelParts;
use tsukumoe_core::rig::{ArtMesh, MeshKeyform};
use tsukumoe_format::{Manifest, Project, write_model};
use tsukumoe_gpu::Image;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tsukumoe"))
        .args(args)
        .output()
        .unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

fn path(p: &std::path::Path) -> &str {
    p.to_str().unwrap()
}

fn quad(x: f32) -> Vec<Point> {
    vec![[x, 0.0], [x + 1.0, 0.0], [x + 1.0, 1.0], [x, 1.0]]
}

/// Writes the project into `<name>/project` and returns that directory.
fn project(name: &str) -> PathBuf {
    project_with(name, false)
}

/// With `playback`, the project also has two motions, an expression and a pose.
fn project_with(name: &str, playback: bool) -> PathBuf {
    let mut p = ModelParts::default();
    let part = p.parts.insert("Part", Part::default()).unwrap();
    let param = p
        .parameters
        .insert("P", Parameter::new(0.0, 1.0, 0.0))
        .unwrap();
    let binding = p.bindings.insert(
        Binding::new(vec![Axis {
            parameter: param,
            keys: vec![0.0, 1.0],
        }])
        .unwrap(),
    );
    let mut mesh = ArtMesh::new(part, quad(0.0), vec![[0.5, 0.5]; 4], vec![0, 1, 2, 0, 2, 3]);
    let keyform = |x| MeshKeyform {
        positions: quad(x),
        opacity: 1.0,
        draw_order: 500.0,
        multiply_color: [1.0; 3],
        screen_color: [0.0; 3],
    };
    mesh.keyforms = Some(KeyformGrid::new(binding, vec![keyform(0.0), keyform(1.0)]));
    p.meshes.insert("Quad", mesh).unwrap();
    if playback {
        use tsukumoe_core::playback::{
            Curve, CurveTarget, Expression, ExpressionBlend, ExpressionParameter, Motion, Pose,
            PoseEntry, Segment,
        };
        let other = p.parts.insert("Other", Part::default()).unwrap();
        let motion = |target, duration| Motion {
            duration,
            fps: 30.0,
            looped: false,
            fade_in: 0.1,
            fade_out: 0.1,
            curves: vec![Curve {
                target,
                start: [0.0, 0.0],
                segments: vec![Segment::Linear([duration, 1.0])],
                fade_in: None,
                fade_out: None,
            }],
        };
        p.motions
            .insert("Slide", motion(CurveTarget::Parameter(param), 1.0))
            .unwrap();
        p.motions
            .insert("Fade", motion(CurveTarget::PartOpacity(other), 2.0))
            .unwrap();
        p.expressions
            .insert(
                "Push",
                Expression {
                    fade_in: 0.2,
                    fade_out: 0.2,
                    parameters: vec![ExpressionParameter {
                        parameter: param,
                        blend: ExpressionBlend::Add,
                        value: 0.5,
                    }],
                },
            )
            .unwrap();
        p.pose = Some(Pose {
            fade_in: 0.5,
            groups: vec![vec![
                PoseEntry {
                    part,
                    linked: vec![],
                },
                PoseEntry {
                    part: other,
                    linked: vec![],
                },
            ]],
        });
    }
    let model = tsukumoe_core::model::Model::from_parts(p).unwrap();

    let mut project = Project::new(Manifest::new(Some("test".into())));
    write_model(&model, &mut project).unwrap();
    let red = Image {
        width: 1,
        height: 1,
        rgba: vec![255, 0, 0, 255],
    };
    project.files.insert("textures/a.png".into(), red.to_png());
    let dir = scratch(name).join("project");
    project.write_dir(&dir).unwrap();
    dir
}

#[test]
fn convert_round_trips_between_directory_and_zip() {
    let dir = project("convert");
    let zip = dir.with_file_name("project.zip");
    let back = dir.with_file_name("back");
    assert_eq!(code(&run(&["convert", path(&dir), path(&zip)])), 0);
    assert_eq!(code(&run(&["convert", path(&zip), path(&back)])), 0);
    assert_eq!(
        Project::read_dir(&back).unwrap(),
        Project::read_dir(&dir).unwrap()
    );
}

#[test]
fn validate_reports_json_and_the_exit_code() {
    let dir = project("validate");
    let ok = run(&["validate", path(&dir)]);
    assert_eq!(code(&ok), 0);
    let report: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["meshes"], 1);

    fs::write(dir.join("model.json"), b"{ broken").unwrap();
    let bad = run(&["validate", path(&dir)]);
    assert_eq!(code(&bad), 1);
    let report: serde_json::Value = serde_json::from_slice(&bad.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert!(report["errors"][0].as_str().unwrap().contains("model.json"));
}

#[test]
fn render_follows_parameters_and_diff_compares_the_images() {
    let dir = project("render");
    let out = dir.with_file_name("out");
    fs::create_dir_all(&out).unwrap();
    let (a, b, d) = (out.join("a.png"), out.join("b.png"), out.join("d.png"));
    let render = |png: &std::path::Path, value: &str| {
        let o = run(&[
            "render",
            path(&dir),
            "-o",
            path(png),
            "--param",
            value,
            "--view",
            "0,0,1,1",
            "--size",
            "4x4",
        ]);
        if code(&o) == 1 && String::from_utf8_lossy(&o.stderr).contains("no adapter") {
            return false;
        }
        assert_eq!(code(&o), 0, "{}", String::from_utf8_lossy(&o.stderr));
        true
    };
    if !render(&a, "P=0") {
        assert!(
            std::env::var_os("TSUKUMOE_REQUIRE_GPU").is_none(),
            "no adapter"
        );
        eprintln!("skipped: no adapter");
        return;
    }
    assert!(render(&b, "P=1"));

    let red = Image::from_png(&fs::read(&a).unwrap()).unwrap();
    assert!(red.rgba.chunks(4).all(|p| p == [255, 0, 0, 255]));
    let empty = Image::from_png(&fs::read(&b).unwrap()).unwrap();
    assert!(empty.rgba.iter().all(|&c| c == 0));

    let same = run(&["diff", path(&a), path(&a)]);
    assert_eq!(code(&same), 0);
    let different = run(&["diff", path(&a), path(&b), "-o", path(&d)]);
    assert_eq!(code(&different), 1);
    let report: serde_json::Value = serde_json::from_slice(&different.stdout).unwrap();
    assert_eq!(report["same"], false);
    assert_eq!(report["max"], 255);
    assert_eq!(report["differing"], 16);
    assert!(d.is_file());
    // a tolerance of 255 accepts any difference
    assert_eq!(
        code(&run(&["diff", path(&a), path(&b), "--tolerance", "255"])),
        0
    );
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    let dir = project("errors");
    assert_eq!(code(&run(&[])), 2);
    assert_eq!(code(&run(&["nope"])), 2);
    assert_eq!(code(&run(&["render", path(&dir)])), 2);
    assert_eq!(
        code(&run(&[
            "render",
            path(&dir),
            "-o",
            "x.png",
            "--size",
            "axb"
        ])),
        2
    );
    assert_eq!(
        code(&run(&["render", path(&dir), "-o", "x.png", "--bogus", "1"])),
        2
    );
    assert_eq!(
        code(&run(&[
            "render",
            path(&dir),
            "-o",
            "x.png",
            "--param",
            "Nope=1"
        ])),
        1
    );
    assert_eq!(code(&run(&["validate", "/does/not/exist"])), 1);
    assert_eq!(code(&run(&["diff", "/does/not/exist", "/nor/this"])), 1);
}

#[test]
fn generate_builds_the_reference_model_at_the_required_scale() {
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let dir = scratch("generate").join("reference");
    let out = run(&["generate", table, path(&dir)]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let size: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    // REQ-PERF-001: ArtMesh 150, 30k vertices, 100 parameters, 80 deformers, 20 physics.
    assert_eq!(size["meshes"], 150);
    assert!(size["vertices"].as_u64().unwrap() >= 30_000);
    assert_eq!(size["parameters"], 100);
    assert_eq!(size["deformers"], 80);
    assert_eq!(size["physics"], 20);
    assert_eq!(code(&run(&["validate", path(&dir)])), 0);

    // Generation is deterministic.
    let again = dir.with_file_name("again");
    assert_eq!(code(&run(&["generate", table, path(&again)])), 0);
    assert_eq!(
        Project::read_dir(&dir).unwrap(),
        Project::read_dir(&again).unwrap()
    );
}

#[test]
fn generate_rejects_a_bad_table() {
    let dir = scratch("generate-bad");
    let table = dir.join("parts.json");
    fs::write(&table, br#"{"name":"x","grid":15,"groups":[{"name":"A","meshes":99,"warps":7,"parameters":10,"chains":2}]}"#).unwrap();
    assert_eq!(
        code(&run(&["generate", path(&table), path(&dir.join("out"))])),
        1
    );
    fs::write(&table, b"not json").unwrap();
    assert_eq!(
        code(&run(&["generate", path(&table), path(&dir.join("out"))])),
        1
    );
    assert_eq!(code(&run(&["generate", path(&table)])), 2);
}

#[test]
fn convert_writes_a_tmod_that_inspect_summarizes() {
    let dir = project("tmod");
    let out = dir.with_file_name("quad.tmod");
    assert_eq!(code(&run(&["convert", path(&dir), path(&out)])), 0);
    let report = run(&["inspect", path(&out)]);
    assert_eq!(
        code(&report),
        0,
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(v["version"], "1.0");
    assert_eq!(v["meshes"], 1);
    assert_eq!(v["vertices"], 4);
    assert_eq!(v["parameters"], 1);
    assert_eq!(v["textures"], 1);
    assert_eq!(v["bytes"], fs::metadata(&out).unwrap().len());
    assert_eq!(v["sections"].as_array().unwrap().len(), 44);
    assert_eq!(v["motions"], 0);
    assert_eq!(v["expressions"], 0);
    assert_eq!(v["pose_groups"], 0);

    // Same project, same bytes.
    let again = dir.with_file_name("again.tmod");
    assert_eq!(code(&run(&["convert", path(&dir), path(&again)])), 0);
    assert_eq!(fs::read(&out).unwrap(), fs::read(&again).unwrap());

    assert_eq!(code(&run(&["inspect", "/does/not/exist.tmod"])), 1);
    let junk = dir.with_file_name("junk.tmod");
    fs::write(
        &junk,
        b"not a tmod file at all, but long enough to have a header",
    )
    .unwrap();
    assert_eq!(code(&run(&["inspect", path(&junk)])), 1);
    assert_eq!(code(&run(&["inspect"])), 2);
}

#[test]
fn convert_puts_motions_expressions_and_the_pose_into_the_tmod() {
    use tsukumoe_runtime::{Priority, Runtime};

    let dir = project_with("tmod-playback", true);
    let out = dir.with_file_name("playback.tmod");
    assert_eq!(code(&run(&["convert", path(&dir), path(&out)])), 0);
    let report = run(&["inspect", path(&out)]);
    assert_eq!(code(&report), 0);
    let v: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(v["motions"], 2);
    assert_eq!(v["curves"], 2);
    assert_eq!(v["expressions"], 1);
    assert_eq!(v["pose_groups"], 1);
    assert_eq!(v["sections"].as_array().unwrap().len(), 44 + 10);

    // The runtime plays what `convert` wrote.
    let mut rt = Runtime::from_bytes(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(rt.play_motion("Slide", 0, Priority::Normal), Ok(true));
    rt.update(0.5);
    assert!((rt.parameter_values()[0] - 0.5).abs() < 1e-5);
}

/// `docs/specifications/samples/quad.tmod` is the file `convert` makes from
/// the quad project above; `quad.expected.json` is what a loader must read
/// and evaluate (REQ-FMT-006). Regenerate with `TSUKUMOE_UPDATE_SAMPLES=1`.
#[test]
fn the_specification_sample_is_current() {
    use tsukumoe_core::eval::Frame;
    use tsukumoe_core::tmod::{AlignedBuf, Tmod};

    let dir = project("sample");
    let out = dir.with_file_name("quad.tmod");
    assert_eq!(code(&run(&["convert", path(&dir), path(&out)])), 0);
    let bytes = fs::read(&out).unwrap();

    let buf = AlignedBuf::from_bytes(&bytes);
    let t = Tmod::parse(&buf).unwrap();
    let m = &t.model;
    let mut frame = Frame::new(m);
    let cases: Vec<_> = [0.0f32, 0.5, 1.0]
        .iter()
        .map(|&p| {
            frame.set_parameter(0, p);
            frame.evaluate(m);
            serde_json::json!({ "P": p, "positions": frame.positions() })
        })
        .collect();
    let expected = serde_json::json!({
        "file": "quad.tmod",
        "version": "1.0",
        "parameters": [{ "name": m.parameter_name(0), "min": m.parameters[0].min,
            "max": m.parameters[0].max, "default": m.parameters[0].default }],
        "meshes": [{ "name": m.mesh_name(0), "vertices": m.meshes[0].vertex_count,
            "indices": &m.mesh_indices[..], "uvs": &m.mesh_uvs[..] }],
        "textures": t.texture_count(),
        "evaluations": cases,
    });
    let mut json = serde_json::to_string_pretty(&expected).unwrap();
    json.push('\n');

    let samples =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/specifications/samples");
    if std::env::var_os("TSUKUMOE_UPDATE_SAMPLES").is_some() {
        fs::create_dir_all(&samples).unwrap();
        fs::write(samples.join("quad.tmod"), &bytes).unwrap();
        fs::write(samples.join("quad.expected.json"), &json).unwrap();
    }
    assert_eq!(
        fs::read(samples.join("quad.tmod")).expect("run with TSUKUMOE_UPDATE_SAMPLES=1"),
        bytes,
        "quad.tmod is stale; run with TSUKUMOE_UPDATE_SAMPLES=1"
    );
    assert_eq!(
        fs::read_to_string(samples.join("quad.expected.json")).unwrap(),
        json,
        "quad.expected.json is stale; run with TSUKUMOE_UPDATE_SAMPLES=1"
    );
}
