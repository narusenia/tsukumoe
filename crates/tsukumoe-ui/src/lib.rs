// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Tsukumoe's UI layer on egui (ADR-0027): design tokens, a theme that can be
//! switched and loaded from JSON, and a few components that read it. The
//! `gallery` example shows every component in every state.

mod components;
mod font;
mod theme;

pub use components::{Variant, button, hint, label, panel, slider, switch};
pub use font::install_cjk_font;
pub use theme::{Colors, Metrics, Mode, Theme, init, set_mode, set_theme};
