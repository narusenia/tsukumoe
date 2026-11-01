// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The standard panels. Each is a state type (or pure functions) plus a `show`
//! that draws it, so the logic is testable without a window.

pub mod deformer_overlay;
pub mod history;
pub mod inspector;
pub mod mesh_overlay;
pub mod palette;
pub mod parameters;
pub mod tree;
pub mod viewport;
