// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! GPU-independent rig model, evaluation pipeline and edit commands.
//!
//! This crate must not depend on `wgpu` or `egui*`, and must build for
//! `wasm32-unknown-unknown` (see `docs/specifications/architecture.md`).

pub mod affine;
pub mod arena;
pub mod command;
pub mod compiled;
pub mod draw;
pub mod driver;
pub mod elements;
pub mod eval;
pub mod expr;
pub mod history;
pub mod keyform;
pub mod layout;
pub mod model;
pub mod physics;
pub mod playback;
pub mod rig;
pub mod simulate;
pub mod table;
pub mod validate;
