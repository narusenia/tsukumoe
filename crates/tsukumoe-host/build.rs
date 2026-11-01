// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

fn main() {
    // syphon-core links Syphon.framework by an @rpath install name but only adds
    // an rpath for its own tests.
    if std::env::var_os("CARGO_FEATURE_SYPHON").is_some()
        && std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos")
    {
        // The universal (x86_64 + arm64) framework syphon-core reassembles in its
        // build directory comes first: the official release is x86_64 only and
        // fails to load on Apple Silicon. shortcut: this bakes an absolute path
        // under target/ into the binary, fine for development, not for shipping.
        if let Some(dir) = std::env::var_os("DEP_SYPHON_FRAMEWORK_DIR") {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.to_string_lossy());
        }
        // Then the framework a user installed from the official release.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/Library/Frameworks");
    }
    // Spout2 (compiled in by spout-rs) imports TaskDialogIndirect, which only
    // exists in comctl32 v6. rustc links with /MANIFEST:NO, which drops the
    // manifest dependency Spout2 asks for by pragma, so the exe would not start.
    if std::env::var_os("CARGO_FEATURE_SPOUT").is_some()
        && std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc")
    {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
