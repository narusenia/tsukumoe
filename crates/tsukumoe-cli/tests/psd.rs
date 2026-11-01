// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `tsukumoe psd inspect | import`, with PSDs from the test writer.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

mod common;

use common::scratch;
use serde_json::Value;
use tsukumoe_psd::writer::{GroupSpec, LayerSpec, Node, PsdSpec, write};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tsukumoe"))
        .args(args)
        .output()
        .unwrap()
}

fn json(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn layer(name: &str, l: i32, t: i32, w: u32, h: u32, v: u8) -> Node {
    Node::Layer(LayerSpec::solid(name, l, t, w, h, [v, 90, 255 - v, 255]))
}

fn psd(dir: &Path, name: &str, eye: (i32, u32)) -> String {
    let mut hidden = LayerSpec::solid("hidden", 0, 0, 3, 3, [9, 9, 9, 255]);
    hidden.visible = false;
    hidden.blend = *b"over";
    let spec = PsdSpec::new(
        64,
        64,
        vec![
            Node::Group(GroupSpec::new(
                "Face",
                vec![layer("eye", eye.0, 4, eye.1, 4, 30), Node::Layer(hidden)],
            )),
            layer("bg", 0, 0, 64, 64, 1),
        ],
    );
    let path = dir.join(name);
    fs::write(&path, write(&spec)).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn inspect_lists_layers_and_warnings() {
    let dir = scratch("psd_inspect");
    let file = psd(&dir, "a.psd", (8, 6));
    let v = json(&run(&["psd", "inspect", &file]));
    assert_eq!(v["canvas"], serde_json::json!([64, 64]));
    assert_eq!(v["summary"]["layers"], 3);
    assert_eq!(v["summary"]["groups"], 1);
    assert_eq!(v["summary"]["hidden"], 1);
    let names: Vec<_> = v["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Face", "eye", "hidden", "bg"]);
    let warnings = v["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w["warning"].as_str().unwrap().contains("over"))
    );
}

#[test]
fn plan_writes_nothing_and_apply_creates_a_project_that_validates() {
    let dir = scratch("psd_import");
    let file = psd(&dir, "a.psd", (8, 6));
    let project = dir.join("proj");
    let project = project.to_str().unwrap();

    let plan = json(&run(&["psd", "import", project, &file]));
    assert_eq!(plan["applied"], false);
    assert_eq!(plan["summary"]["counts"]["add_layer"], 3);
    assert_eq!(plan["summary"]["counts"]["add_group"], 1);
    assert_eq!(plan["notes"].as_array().unwrap().len(), 1);
    assert!(!Path::new(project).exists(), "--plan must not write");

    let done = json(&run(&["psd", "import", project, &file, "--apply"]));
    assert_eq!(done["applied"], true);
    let textures: Vec<_> = fs::read_dir(Path::new(project).join("textures"))
        .unwrap()
        .collect();
    assert_eq!(textures.len(), 3);
    assert!(Path::new(project).join("sources/psd-map.json").is_file());
    assert!(
        !fs::read_dir(Path::new(project).join("textures"))
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp"))
    );
    let out = run(&["validate", project]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The same PSD again: an empty plan, and nothing is applied.
    let again = json(&run(&["psd", "import", project, &file, "--apply"]));
    assert_eq!(again["applied"], false);
    assert_eq!(again["summary"]["commands"], 0);
    assert_eq!(again["summary"]["counts"]["unchanged"], 4);

    // A changed eye image (new size and place) is a reimport of one layer.
    let next = psd(&dir, "b.psd", (20, 10));
    let plan = json(&run(&["psd", "import", project, &next, "--apply"]));
    assert_eq!(plan["applied"], true);
    assert_eq!(plan["summary"]["counts"]["image_changed"], 1);
    assert!(run(&["validate", project]).status.success());
    assert_eq!(
        fs::read_dir(Path::new(project).join("textures"))
            .unwrap()
            .count(),
        3
    );
}

#[test]
fn a_bad_psd_fails_and_writes_nothing() {
    let dir = scratch("psd_bad");
    let file = psd(&dir, "a.psd", (8, 6));
    let bytes = fs::read(&file).unwrap();
    let cut = dir.join("cut.psd");
    fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    let project = dir.join("proj");
    let out = run(&[
        "psd",
        "import",
        project.to_str().unwrap(),
        cut.to_str().unwrap(),
        "--apply",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!project.exists());
    let not_psd = dir.join("x.psd");
    fs::write(&not_psd, b"hello").unwrap();
    assert_eq!(
        run(&["psd", "inspect", not_psd.to_str().unwrap()])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn usage_errors_exit_two() {
    let dir = scratch("psd_usage");
    let file = psd(&dir, "a.psd", (8, 6));
    for args in [
        vec!["psd", "import", "p", file.as_str(), "--plan", "--apply"],
        vec![
            "psd",
            "import",
            "p",
            file.as_str(),
            "--keep-missing",
            "--remove-missing",
        ],
        vec!["psd", "import", "p", file.as_str(), "--map", "nope"],
        vec!["psd", "bogus"],
    ] {
        assert_eq!(run(&args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn vanished_layers_follow_the_missing_choice() {
    let dir = scratch("psd_missing");
    let file = psd(&dir, "a.psd", (8, 6));
    let project = dir.join("proj");
    let project = project.to_str().unwrap();
    json(&run(&["psd", "import", project, &file, "--apply"]));
    // The next PSD has no "bg".
    let spec = PsdSpec::new(
        64,
        64,
        vec![Node::Group(GroupSpec::new(
            "Face",
            vec![layer("eye", 8, 4, 6, 4, 30)],
        ))],
    );
    let next = dir.join("c.psd");
    fs::write(&next, write(&spec)).unwrap();
    let next = next.to_str().unwrap();
    let plan = json(&run(&["psd", "import", project, next]));
    assert_eq!(plan["summary"]["counts"]["missing"], 2);
    let done = json(&run(&[
        "psd",
        "import",
        project,
        next,
        "--apply",
        "--remove-missing",
    ]));
    assert_eq!(done["applied"], true);
    let model: Value =
        serde_json::from_slice(&fs::read(Path::new(project).join("model.json")).unwrap()).unwrap();
    let meshes: Vec<_> = model["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(meshes, ["Face/eye"]);
    assert!(run(&["validate", project]).status.success());
}
