// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use wgpui::{App, ElementId, ParentElement, SharedString, Styled, px};
use wgpui_base::{Switch, SwitchThumb, SwitchTrack};

use crate::theme::Theme;

const WIDTH: f32 = 28.0;
const HEIGHT: f32 = 16.0;
const THUMB: f32 = 12.0;

/// A controlled switch: pass the current value and update it in `.on_change(..)`.
/// `disabled` is a parameter because the track and thumb are colored from it.
pub fn switch(
    id: impl Into<ElementId>,
    checked: bool,
    disabled: bool,
    name: impl Into<SharedString>,
    cx: &App,
) -> Switch {
    let colors = Theme::get(cx).colors;
    let id = id.into();
    let inset = (HEIGHT - THUMB) / 2.0;
    let thumb_x = if checked {
        WIDTH - THUMB - inset * 2.0
    } else {
        0.0
    };
    Switch::new(id.clone())
        .checked(checked)
        .disabled(disabled)
        .accessibility_label(name)
        .child(
            SwitchTrack::new((id, "track"))
                .checked(checked)
                .disabled(disabled)
                .w(px(WIDTH))
                .h(px(HEIGHT))
                .rounded_full()
                .flex()
                .items_center()
                .p(px(inset))
                .bg(match (disabled, checked) {
                    (true, _) => colors.control,
                    (_, true) => colors.accent,
                    _ => colors.control_hover,
                })
                .child(
                    SwitchThumb::new(checked)
                        .disabled(disabled)
                        .size(px(THUMB))
                        .rounded_full()
                        .left(px(thumb_x))
                        .bg(if disabled {
                            colors.disabled
                        } else {
                            colors.text
                        }),
                ),
        )
}
