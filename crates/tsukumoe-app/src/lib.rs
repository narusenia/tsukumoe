// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The Tsukumoe editor. Panels and state live here (and are tested without a
//! window); `main.rs` only starts the window.

pub mod actions;
pub mod app;
pub mod commands;
pub mod dispatcher;
pub mod drag;
pub mod editor;
pub mod menu;
#[cfg(target_os = "macos")]
mod native;
pub mod panels;
pub mod pie;
pub mod project;
pub mod scene;
pub mod settings;
pub mod view;
pub mod workspace;
