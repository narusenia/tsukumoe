// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use wgpui::{App, AppContext, Entity, IntoElement, ParentElement, Styled, div, px, relative};
use wgpui_base::slider::{SliderState, SliderValue};
use wgpui_base::{Slider, SliderIndicator, SliderThumb, SliderTrack};

use crate::theme::Theme;

const THUMB: f32 = 12.0;

/// Slider state over `min..=max`; subscribe to its `SliderEvent` for changes.
pub fn slider_state(
    min: f32,
    max: f32,
    step: f32,
    value: f32,
    cx: &mut App,
) -> Entity<SliderState> {
    cx.new(|_| {
        SliderState::new()
            .min(min)
            .max(max)
            .step(step)
            .default_value(value)
    })
}

/// A horizontal single-value slider.
pub fn slider(state: &Entity<SliderState>, disabled: bool, cx: &App) -> impl IntoElement {
    let colors = Theme::get(cx).colors;
    let (min, max, value) = {
        let s = state.read(cx);
        let value = match s.value() {
            SliderValue::Single(v) | SliderValue::Range(_, v) => v,
        };
        (s.min_value(), s.max_value(), value)
    };
    let fraction = if max > min {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let fill = if disabled {
        colors.disabled
    } else {
        colors.accent
    };
    Slider::new(state)
        .disabled(disabled)
        .flex()
        .items_center()
        .w_full()
        .h(px(16.0))
        .child(
            SliderTrack::new(state)
                .disabled(disabled)
                .flex()
                .items_center()
                .w_full()
                .h(px(16.0))
                .child(
                    SliderIndicator::new(state)
                        .relative()
                        .w_full()
                        .h(px(3.0))
                        .rounded_full()
                        .bg(colors.control_hover)
                        .child(
                            div()
                                .absolute()
                                .h_full()
                                .left_0()
                                .w(relative(fraction))
                                .rounded_full()
                                .bg(fill),
                        )
                        .child(
                            SliderThumb::new(state)
                                .disabled(disabled)
                                .absolute()
                                .top(px(-(THUMB - 3.0) / 2.0))
                                .left(relative(fraction))
                                .ml(px(-THUMB / 2.0))
                                .size(px(THUMB))
                                .rounded_full()
                                .bg(colors.text),
                        ),
                ),
        )
}
