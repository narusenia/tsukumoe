// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Tsukumoe's UI layer: design tokens and styled components built on the
//! unstyled behavior of `wgpui-base` (ADR-0025, ADR-0026). Every look comes
//! from the [`Theme`]; the `gallery` example shows each component's states.

mod button;
mod label;
mod panel;
mod slider;
mod switch;
mod theme;

pub use button::{Variant, button};
pub use label::{hint, label};
pub use panel::panel;
pub use slider::{slider, slider_state};
pub use switch::switch;
pub use theme::{Colors, Metrics, Mode, Theme, init, set_mode, set_theme};
pub use wgpui_base::slider::{SliderEvent, SliderState, SliderValue};
