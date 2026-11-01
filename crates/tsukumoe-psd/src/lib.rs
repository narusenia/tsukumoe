// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! PSD reading and import planning. The reader treats its input as
//! untrusted; see `docs/implementation/M5-editor/psd-import.md`.

mod image;
mod import;
mod map;
mod plan;
mod reader;
mod scan;
#[cfg(any(test, feature = "test-writer"))]
pub mod writer;

pub use image::{Image, hash_reader};
pub use import::Prepared;
pub use map::{GroupRec, LayerRec, MAP_PATH, PsdMap, Source, UNITS_PER_PIXEL};
pub use plan::{
    Action, Choices, Existing, Item, Plan, ROOT_PART, TextureJob, plan, remap_uvs, render_textures,
    texture_paths,
};
pub use reader::{
    Channel, Entry, EntryKind, Limits, Psd, PsdError, Rect, Warning, WarningKind,
    layer_buffer_allocations,
};
pub use scan::{Note, Scan, ScannedGroup, ScannedLayer, blend_mode};

#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod tests;
