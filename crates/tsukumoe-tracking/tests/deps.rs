// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The crate sits below the UI boundary and beside the core: it must not pull
//! in the core, the runtime, egui or wgpu, directly or transitively.

use std::process::Command;

#[test]
fn forbidden_crates_are_not_in_the_dependency_tree() {
    let out = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--offline",
            "-e",
            "normal,build,dev",
            "--prefix",
            "none",
        ])
        .arg("--manifest-path")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .expect("run cargo tree");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let tree = String::from_utf8(out.stdout).unwrap();
    assert!(tree.starts_with("tsukumoe-tracking "), "{tree}");
    for line in tree.lines() {
        let name = line.split_whitespace().next().unwrap_or("");
        let banned = matches!(
            name,
            "tsukumoe-core" | "tsukumoe-runtime" | "wgpu" | "eframe"
        ) || name.starts_with("egui");
        assert!(!banned, "forbidden dependency: {name}");
    }
}

/// The only direct dependencies the crate may have are the I/O-free
/// serialization crates the workspace already uses.
#[test]
fn direct_dependencies_are_the_allowed_ones() {
    let out = Command::new(env!("CARGO"))
        .args(["tree", "--offline", "-e", "normal,build,dev"])
        .args(["--depth", "1", "--prefix", "none"])
        .arg("--manifest-path")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .expect("run cargo tree");
    assert!(out.status.success());
    let tree = String::from_utf8(out.stdout).unwrap();
    let mut direct: Vec<&str> = tree
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    direct.sort_unstable();
    direct.dedup();
    assert_eq!(direct, ["schemars", "serde", "serde_json"]);
}
