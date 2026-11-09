// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The standard panels. Each is a state type (or pure functions) plus a `show`
//! that draws it, so the logic is testable without a window.

pub mod bone_overlay;
pub mod bones;
pub mod curve_editor;
pub mod deformer_overlay;
pub mod expressions;
pub mod history;
pub mod inspector;
pub mod keys;
pub mod mesh_overlay;
pub mod model_settings;
pub mod overlay;
pub mod palette;
pub mod parameters;
pub mod physics;
pub mod physics_diagram;
pub mod proportional_overlay;
pub mod selection_overlay;
pub mod splash;
pub mod timeline;
pub mod titlebar;
pub mod toolbar;
pub mod tree;
pub mod view_overlay;
pub mod viewport;
