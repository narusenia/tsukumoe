// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A small in-house dock (egui_dock does not support egui 0.37): splits with
//! draggable dividers, tab groups, drag-to-move, left and right side strips,
//! and a layout that round-trips through JSON.

mod model;
mod view;

pub use model::{
    Axis, DEFAULT_WIDTH, Edge, Layout, MAX_RATIO, MAX_WIDTH, MIN_RATIO, MIN_WIDTH, Node, Side,
    Strip, Target,
};
pub use view::{PanelHost, drop_edge, show};
