// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use wgpui::{App, Div, Styled, div};

use crate::theme::Theme;

/// A bordered surface; add children with `.child(..)`.
pub fn panel(cx: &App) -> Div {
    let t = Theme::get(cx);
    div()
        .flex()
        .flex_col()
        .gap(t.metrics.gap)
        .p(t.metrics.pad)
        .bg(t.colors.surface)
        .border_1()
        .border_color(t.colors.border)
        .rounded(t.metrics.radius)
}
