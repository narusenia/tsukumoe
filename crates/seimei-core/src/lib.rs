// Copyright 2026 Seimei Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! GPU-independent rig model, evaluation pipeline and edit commands.
//!
//! This crate must not depend on `wgpu` or `gpui*`, and must build for
//! `wasm32-unknown-unknown` (see `docs/specifications/architecture.md`).

pub mod arena;
pub mod command;
pub mod compiled;
pub mod driver;
pub mod elements;
pub mod history;
pub mod keyform;
pub mod model;
pub mod physics;
pub mod playback;
pub mod rig;
pub mod table;
pub mod validate;
