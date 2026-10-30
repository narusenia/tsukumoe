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
