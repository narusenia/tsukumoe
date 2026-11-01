// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

fn main() {
    // syphon-core links Syphon.framework by an @rpath install name but only adds
    // an rpath for its own tests. The framework the user installs from the
    // official release lives in /Library/Frameworks, so look there at run time.
    if std::env::var_os("CARGO_FEATURE_SYPHON").is_some()
        && std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos")
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/Library/Frameworks");
    }
}
