// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use wgpui::{App, ElementId, InteractiveElement, ParentElement, SharedString, Styled};
use wgpui_base::Button;

use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
}

/// A text button; chain `.on_click(..)` and `.disabled(..)` on the result.
pub fn button(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    variant: Variant,
    cx: &App,
) -> Button {
    let Theme {
        colors, metrics, ..
    } = Theme::get(cx);
    let (bg, fg, hover) = match variant {
        Variant::Primary => (colors.accent, colors.accent_text, colors.accent),
        Variant::Secondary => (colors.control, colors.text, colors.control_hover),
    };
    let text = text.into();
    Button::new(id)
        .accessibility_label(text.clone())
        .h(metrics.control)
        .px(metrics.pad)
        .flex()
        .items_center()
        .justify_center()
        .rounded(metrics.radius)
        .text_size(metrics.text)
        .bg(bg)
        .text_color(fg)
        .hover(|s| s.bg(hover).opacity(0.9))
        .styles(|styles| styles.disabled(|s| s.bg(colors.control).text_color(colors.disabled)))
        .child(text)
}
