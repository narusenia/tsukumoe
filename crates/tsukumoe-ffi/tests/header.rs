// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `include/tsukumoe.h` must match what cbindgen generates from the crate.
//! Regenerate with `TSUKUMOE_UPDATE_HEADER=1 cargo test -p tsukumoe-ffi --test header`.

use std::fs;
use std::path::Path;

#[test]
fn the_header_matches_the_exported_api() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = cbindgen::Config::from_file(dir.join("cbindgen.toml")).unwrap();
    let mut generated = Vec::new();
    cbindgen::Builder::new()
        .with_crate(dir)
        .with_config(config)
        .generate()
        .expect("cbindgen failed")
        .write(&mut generated);
    let path = dir.join("include/tsukumoe.h");
    if std::env::var_os("TSUKUMOE_UPDATE_HEADER").is_some() {
        fs::write(&path, &generated).unwrap();
    }
    let committed = fs::read(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "include/tsukumoe.h is out of date; run with TSUKUMOE_UPDATE_HEADER=1 to regenerate"
    );
}
