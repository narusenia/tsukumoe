// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Every component in every state, with a switch for the theme mode.

use eframe::egui;
use tsukumoe_ui::{Mode, Theme, Variant, button, hint, label, panel, set_mode, slider, switch};

struct Gallery {
    mode: Mode,
    switch_on: bool,
    value: f32,
    disabled_value: f32,
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    panel(ui, |ui| {
        ui.set_width(ui.available_width());
        hint(ui, title);
        add(ui);
    });
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let t = Theme::get(&ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(t.colors.background)
                    .inner_margin(t.metrics.pad),
            )
            .show(ui, |ui| {
                section(ui, "Theme", |ui| {
                    ui.horizontal(|ui| {
                        for (mode, name) in [
                            (Mode::Light, "Light"),
                            (Mode::Dark, "Dark"),
                            (Mode::System, "System"),
                        ] {
                            let variant = if self.mode == mode {
                                Variant::Primary
                            } else {
                                Variant::Secondary
                            };
                            if button(ui, name, variant).clicked() {
                                self.mode = mode;
                                set_mode(&ctx, mode);
                            }
                        }
                    });
                });
                section(ui, "Button", |ui| {
                    ui.horizontal(|ui| {
                        button(ui, "Primary", Variant::Primary);
                        button(ui, "Secondary", Variant::Secondary);
                        ui.add_enabled_ui(false, |ui| button(ui, "Disabled", Variant::Secondary));
                    });
                });
                section(ui, "Switch", |ui| {
                    ui.horizontal(|ui| {
                        switch(ui, &mut self.switch_on);
                        label(ui, if self.switch_on { "On" } else { "Off" });
                        ui.add_enabled_ui(false, |ui| switch(ui, &mut true));
                    });
                });
                section(ui, "Slider", |ui| {
                    ui.horizontal(|ui| {
                        slider(ui, &mut self.value, 0.0..=1.0);
                    });
                    label(ui, format!("{:.2}", self.value));
                    ui.add_enabled_ui(false, |ui| slider(ui, &mut self.disabled_value, 0.0..=1.0));
                });
                section(ui, "Text", |ui| {
                    label(ui, "ParamAngleX / 角度X / 日本語の表示");
                    hint(ui, "A secondary line");
                });
            });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Tsukumoe UI gallery",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Tsukumoe UI gallery")
                .with_inner_size([520.0, 520.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            tsukumoe_ui::init(&cc.egui_ctx);
            Ok(Box::new(Gallery {
                mode: Mode::Dark,
                switch_on: true,
                value: 0.3,
                disabled_value: 0.6,
            }))
        }),
    )
}
