// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use tsukumoe_core::tmod::{self, PlaybackData};

use crate::atlas::{self, AtlasOptions, AtlasReport};
use crate::{Error, Project, read_model};

/// The runtime format (`.tmod`) of a project: the compiled model, its motions,
/// expressions and pose, and its textures (`textures/*.png`, in path order),
/// packed into atlas pages with the default options.
pub fn runtime_bytes(project: &Project) -> Result<Vec<u8>, Error> {
    runtime_bytes_with(project, Some(&AtlasOptions::default())).map(|(bytes, _)| bytes)
}

/// [`runtime_bytes`] with the atlas options (`None` embeds each texture as it
/// is), and what packing did.
pub fn runtime_bytes_with(
    project: &Project,
    atlas: Option<&AtlasOptions>,
) -> Result<(Vec<u8>, AtlasReport), Error> {
    let model = read_model(project)?;
    let invalid = |e: &dyn std::fmt::Debug| Error::Invalid(format!("{e:?}"));
    let mut compiled = model.compile().map_err(|e| invalid(&e))?;
    let playback = PlaybackData::from_model(&model).map_err(|e| invalid(&e))?;
    let textures: Vec<&[u8]> = project
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
        .map(|(_, bytes)| bytes.as_slice())
        .collect();
    let (files, report) = match atlas {
        Some(options) => atlas::build(&mut compiled, &textures, options)?,
        None => atlas::unpacked(&textures),
    };
    let refs: Vec<&[u8]> = files.iter().map(Vec::as_slice).collect();
    let bytes = tmod::write_with(&compiled, &playback, &refs).map_err(|e| invalid(&e))?;
    Ok((bytes, report))
}
