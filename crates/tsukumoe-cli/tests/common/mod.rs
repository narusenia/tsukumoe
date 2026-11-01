// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Helpers shared by the integration tests that edit projects.

use std::fs;
use std::path::PathBuf;

use tsukumoe_core::model::Model;
use tsukumoe_format::{Manifest, Project, write_model};

pub fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// An empty project at `<name>/project` (a directory, or an archive with `zip`).
pub fn empty_project(name: &str, zip: bool) -> PathBuf {
    let dir = scratch(name);
    let mut project = Project::new(Manifest::new(None));
    write_model(&Model::new(), &mut project).unwrap();
    if zip {
        let path = dir.join("project.zip");
        fs::write(&path, project.write_zip().unwrap()).unwrap();
        path
    } else {
        let path = dir.join("project");
        project.write_dir(&path).unwrap();
        path
    }
}
