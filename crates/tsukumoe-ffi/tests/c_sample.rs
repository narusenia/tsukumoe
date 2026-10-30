// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Builds `examples/c/main.c` with the C compiler `cc` finds (MSVC on
//! Windows), links it against the shared library and checks its output.
//!
//! The sample links the `cdylib`, not the `staticlib`: a static Rust library
//! needs the platform's system libraries on the link line (`rustc --print
//! native-static-libs`), which differ per OS and toolchain; the shared library
//! carries them. On Windows the link input is the import library
//! `tsukumoe_ffi.dll.lib` and the `.dll` is found through `PATH`.

use std::path::{Path, PathBuf};
use std::process::Command;

use tsukumoe_runtime::Runtime;

const QUAD: &[u8] = include_bytes!("../../../docs/specifications/samples/quad.tmod");

/// `target/<profile>/`, found from this test executable (`target/<profile>/deps/`).
fn profile_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().to_path_buf()
}

/// `cargo test` links tests against the rlib only; the shared library is
/// built here (the build lock is free while tests run).
fn build_shared_library(profile: &Path) {
    let mut cargo = Command::new(env!("CARGO"));
    cargo.args(["build", "-p", "tsukumoe-ffi", "--lib"]);
    if profile.file_name().unwrap() == "release" {
        cargo.arg("--release");
    }
    assert!(cargo.status().unwrap().success(), "cargo build failed");
}

fn compile(profile: &Path) -> (PathBuf, Command) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = profile.join("tsukumoe-c-sample");
    std::fs::create_dir_all(&work).unwrap();
    let target = env!("TSUKUMOE_TARGET");
    let tool = cc::Build::new()
        .target(target)
        .host(target)
        .opt_level(0)
        .debug(false)
        .cargo_metadata(false)
        .cargo_warnings(false)
        .get_compiler();
    let (source, include) = (manifest.join("examples/c/main.c"), manifest.join("include"));
    let exe = work.join(if cfg!(windows) {
        "sample.exe"
    } else {
        "sample"
    });
    let mut cmd = tool.to_command();
    if tool.is_like_msvc() {
        cmd.arg("/nologo")
            .arg(format!("/I{}", include.display()))
            .arg(format!("/Fo{}\\", work.display()))
            .arg(format!("/Fe{}", exe.display()))
            .arg(&source)
            .arg(profile.join("tsukumoe_ffi.dll.lib"));
    } else {
        cmd.args(["-std=c99", "-Wall", "-Wextra"])
            .arg("-I")
            .arg(&include)
            .arg(&source)
            .arg("-o")
            .arg(&exe)
            .arg(format!("-L{}", profile.display()))
            .arg("-ltsukumoe_ffi")
            .arg(format!("-Wl,-rpath,{}", profile.display()));
    }
    (exe, cmd)
}

#[test]
fn the_c_sample_builds_runs_and_prints_the_runtime_positions() {
    let profile = profile_dir();
    build_shared_library(&profile);
    let (exe, mut compile) = compile(&profile);
    let out = compile.output().unwrap();
    assert!(
        out.status.success(),
        "C compile failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let sample =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/specifications/samples/quad.tmod");
    let mut path =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    path.insert(0, profile.clone());
    let out = Command::new(&exe)
        .arg(&sample)
        .env("PATH", std::env::join_paths(path).unwrap())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "sample failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines[0], "abi 1.0");
    assert!(
        lines[4].starts_with("texture 0: ") && lines[4].ends_with(" bytes, PNG"),
        "{stdout}"
    );
    assert_eq!(lines.len(), 5, "{stdout}");

    let mut rt = Runtime::from_bytes(QUAD).unwrap();
    for (line, p) in lines[1..4].iter().zip([0.0, 0.5, 1.0]) {
        rt.set_parameter(0, p);
        rt.update(0.0);
        let want: Vec<u32> = rt
            .draw_list()
            .positions()
            .iter()
            .map(|f| f.to_bits())
            .collect();
        let (head, values) = line.split_once(':').unwrap();
        assert_eq!(head, format!("P {p}"));
        let got: Vec<u32> = values
            .split_whitespace()
            .map(|v| v.parse::<f32>().unwrap().to_bits())
            .collect();
        assert_eq!(got, want, "{line}");
    }
}
