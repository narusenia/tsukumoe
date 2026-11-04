// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use tsukumoe_core::tmod::{self, PlaybackData};

use crate::{Error, Project, read_model};

/// The runtime format (`.tmod`) of a project: the compiled model, its motions,
/// expressions and pose, and its textures (`textures/*.png`, in path order).
pub fn runtime_bytes(project: &Project) -> Result<Vec<u8>, Error> {
    let model = read_model(project)?;
    let invalid = |e: &dyn std::fmt::Debug| Error::Invalid(format!("{e:?}"));
    let compiled = model.compile().map_err(|e| invalid(&e))?;
    let playback = PlaybackData::from_model(&model).map_err(|e| invalid(&e))?;
    let textures: Vec<&[u8]> = project
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
        .map(|(_, bytes)| bytes.as_slice())
        .collect();
    tmod::write_with(&compiled, &playback, &textures).map_err(|e| invalid(&e))
}
