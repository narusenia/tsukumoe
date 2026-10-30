// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Every component in every state, with a switch for the theme mode.

use tsukumoe_ui::{
    Mode, Theme, Variant, button, hint, label, panel, set_mode, slider, slider_state, switch,
};
use wgpui::{
    App, AppContext, Application, Context, Div, Entity, IntoElement, ParentElement, Render, Styled,
    TitlebarOptions, Window, WindowOptions, div,
};
use wgpui_base::slider::SliderState;

struct Gallery {
    mode: Mode,
    switch_on: bool,
    value: Entity<SliderState>,
    disabled_value: Entity<SliderState>,
}

impl Gallery {
    fn new(cx: &mut Context<Self>) -> Self {
        let value = slider_state(0.0, 1.0, 0.01, 0.3, cx);
        let disabled_value = slider_state(0.0, 1.0, 0.01, 0.6, cx);
        // Redraw the readout while the slider moves.
        cx.observe(&value, |_, _, cx| cx.notify()).detach();
        Self {
            mode: Mode::Dark,
            switch_on: true,
            value,
            disabled_value,
        }
    }
}

fn section(title: &str, cx: &App) -> Div {
    panel(cx).child(hint(title.to_string(), cx))
}

fn row() -> Div {
    div().flex().items_center().gap_2()
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::get(cx);
        let this = cx.entity();
        let mode_button = |mode: Mode, name: &'static str| {
            let this = this.clone();
            let variant = if self.mode == mode {
                Variant::Primary
            } else {
                Variant::Secondary
            };
            button(name, name, variant, cx).on_click(move |_, window, cx| {
                this.update(cx, |g, _| g.mode = mode);
                set_mode(mode, cx);
                window.refresh();
            })
        };
        let switch_this = this.clone();
        let readout = format!("{:.2}", self.value.read(cx).value().start());

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(t.metrics.pad)
            .p(t.metrics.pad)
            .bg(t.colors.background)
            .child(
                section("Theme", cx).child(
                    row()
                        .child(mode_button(Mode::Light, "Light"))
                        .child(mode_button(Mode::Dark, "Dark"))
                        .child(mode_button(Mode::System, "System")),
                ),
            )
            .child(
                section("Button", cx).child(
                    row()
                        .child(button("primary", "Primary", Variant::Primary, cx))
                        .child(button("secondary", "Secondary", Variant::Secondary, cx))
                        .child(
                            button("disabled", "Disabled", Variant::Secondary, cx).disabled(true),
                        ),
                ),
            )
            .child(
                section("Switch", cx).child(
                    row()
                        .child(
                            switch("on", self.switch_on, false, "Visible", cx).on_change(
                                move |next, _, window, cx| {
                                    switch_this.update(cx, |g, _| g.switch_on = next);
                                    window.refresh();
                                },
                            ),
                        )
                        .child(label(if self.switch_on { "On" } else { "Off" }, cx))
                        .child(switch("disabled", true, true, "Disabled", cx)),
                ),
            )
            .child(
                section("Slider", cx)
                    .child(
                        row()
                            .child(div().flex_1().child(slider(&self.value, false, cx)))
                            .child(div().w_10().child(label(readout, cx))),
                    )
                    .child(slider(&self.disabled_value, true, cx)),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        tsukumoe_ui::init(cx);
        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Tsukumoe UI gallery".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(Gallery::new),
        )
        .expect("open window");
    });
}
