// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use wgpui::{App, IntoElement, ParentElement, SharedString, Styled, div};

use crate::theme::Theme;

pub fn label(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    let t = Theme::get(cx);
    div()
        .text_size(t.metrics.text)
        .line_height(t.metrics.line)
        .text_color(t.colors.text)
        .child(text.into())
}

/// A secondary line of text.
pub fn hint(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    let t = Theme::get(cx);
    div()
        .text_size(t.metrics.text)
        .line_height(t.metrics.line)
        .text_color(t.colors.text_muted)
        .child(text.into())
}
