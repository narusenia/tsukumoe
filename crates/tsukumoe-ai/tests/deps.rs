// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The crate sits below the UI boundary and beside the core: no core, runtime,
//! egui, wgpu, HTTP client or inference runtime, and `sha2` is the only
//! external dependency besides serde.

use std::process::Command;

fn tree(extra: &[&str]) -> String {
    let out = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--offline",
            "-e",
            "normal,build,dev",
            "--prefix",
            "none",
        ])
        .args(extra)
        .arg("--manifest-path")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .expect("run cargo tree");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn names(tree: &str) -> Vec<&str> {
    tree.lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect()
}

#[test]
fn forbidden_crates_are_not_in_the_dependency_tree() {
    let tree = tree(&[]);
    assert!(tree.starts_with("tsukumoe-ai "), "{tree}");
    for name in names(&tree) {
        let banned = matches!(
            name,
            "tsukumoe-core"
                | "tsukumoe-runtime"
                | "wgpu"
                | "eframe"
                | "reqwest"
                | "ureq"
                | "hyper"
                | "curl"
                | "isahc"
                | "attohttpc"
                | "ort"
                | "tract-onnx"
                | "candle-core"
                | "onnxruntime"
        ) || name.starts_with("egui");
        assert!(!banned, "forbidden dependency: {name}");
    }
}

#[test]
fn sha2_is_the_only_external_direct_dependency() {
    let tree = tree(&["--depth", "1"]);
    let mut direct = names(&tree);
    direct.remove(0); // the crate itself
    for name in direct {
        let ok = name == "sha2" || name.starts_with("serde");
        assert!(ok, "unexpected direct dependency: {name}");
    }
}
