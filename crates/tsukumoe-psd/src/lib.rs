// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! PSD reading and import planning. The reader treats its input as
//! untrusted; see `docs/implementation/M5-editor/psd-import.md`.

mod image;
mod reader;
#[cfg(any(test, feature = "test-writer"))]
pub mod writer;

pub use image::{Image, hash_reader};
pub use reader::{Channel, Entry, EntryKind, Limits, Psd, PsdError, Rect, Warning, WarningKind};

#[cfg(test)]
mod tests;
