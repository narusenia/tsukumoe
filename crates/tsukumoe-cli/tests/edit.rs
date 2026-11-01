// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `tsukumoe edit`: commands applied from JSON are saved, and a failing
//! command writes nothing.

use std::fs;
use std::process::{Command, Output};

mod common;

use common::empty_project;
use tsukumoe_cli::store;
use tsukumoe_core::command::Command as Edit;
use tsukumoe_core::model::Model;
use tsukumoe_format::Project;

fn edit(project: &std::path::Path, json: &str) -> Output {
    let file = project.with_extension("command.json");
    fs::write(&file, json).unwrap();
    Command::new(env!("CARGO_BIN_EXE_tsukumoe"))
        .args(["edit", project.to_str().unwrap(), file.to_str().unwrap()])
        .output()
        .unwrap()
}

const SCENE: &str = r#"[
    {"type":"add_parameter","name":"P","min":0,"max":1,"default":0},
    {"type":"add_part","name":"Face"},
    {"type":"set_part_opacity","name":"Face","opacity":0.5}
]"#;

fn expected() -> Model {
    let mut model = Model::new();
    model
        .apply(
            serde_json::from_str::<Edit>(&format!(r#"{{"type":"batch","commands":{SCENE}}}"#))
                .unwrap(),
        )
        .unwrap();
    model
}

#[test]
fn edit_saves_a_directory_project_that_reads_back_the_same() {
    let project = empty_project("edit_dir", false);
    let out = edit(&project, SCENE);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (_, model) = store::open(project.to_str().unwrap()).unwrap();
    assert!(model == expected());
}

#[test]
fn edit_saves_an_archive_project() {
    let project = empty_project("edit_zip", true);
    assert!(edit(&project, SCENE).status.success());
    let (_, model) = store::open(project.to_str().unwrap()).unwrap();
    assert!(model == expected());
}

#[test]
fn a_failing_command_changes_no_file() {
    let project = empty_project("edit_fail", false);
    let before = Project::read_dir(&project).unwrap();
    // The second command fails (unknown part), so the first must not land.
    let out = edit(
        &project,
        r#"[{"type":"add_part","name":"Face"},{"type":"remove_part","name":"Nope"}]"#,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no part named 'Nope'"));
    assert!(Project::read_dir(&project).unwrap().files == before.files);
}

#[test]
fn malformed_commands_fail_without_writing() {
    let project = empty_project("edit_bad", false);
    let before = Project::read_dir(&project).unwrap();
    for json in [
        "not json",
        r#"{"type":"nope"}"#,
        r#"{"type":"add_part"}"#,
        "[1]",
    ] {
        assert_eq!(edit(&project, json).status.code(), Some(1), "{json}");
    }
    assert!(Project::read_dir(&project).unwrap().files == before.files);
}
