// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// The `cc` crate needs the target triple, which tests do not get otherwise
// (`tests/c_sample.rs` builds the C sample with it).
fn main() {
    let target = std::env::var("TARGET").unwrap();
    println!("cargo:rustc-env=TSUKUMOE_TARGET={target}");
}
