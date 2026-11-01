// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::process::ExitCode;

fn main() -> ExitCode {
    let project = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    match tsukumoe_app::app::run(project) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
