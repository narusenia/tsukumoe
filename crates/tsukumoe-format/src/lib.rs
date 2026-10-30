// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The project file format: a zip archive or an unpacked directory holding
//! `manifest.json` and the files of a project. The rules are in
//! `docs/specifications/file-formats.md`.

mod container;
mod error;
mod manifest;

pub use container::{Limits, MANIFEST_PATH, Project, to_json};
pub use error::Error;
pub use manifest::{FORMAT_NAME, Feature, KNOWN_FEATURES, MAJOR, MINOR, Manifest, Version};
