// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The Tsukumoe editor. Panels and state live here (and are tested without a
//! window); `main.rs` only starts the window.

pub mod app;
pub mod dispatcher;
pub mod editor;
pub mod panels;
pub mod project;
pub mod settings;
pub mod workspace;
