// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Every component in every state, in the light and dark themes and both
//! densities. Run with `cargo run -p tsukumoe-ui --example gallery`.

use eframe::egui;
use tsukumoe_ui::Theme;
use tsukumoe_ui::showcase::Showcase;

struct Gallery(Showcase);

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
            .show(ui, |ui| self.0.show_all(ui));
        self.0.overlays(&ctx);
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Tsukumoe UI gallery",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Tsukumoe UI gallery")
                .with_inner_size([620.0, 900.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            tsukumoe_ui::init(&cc.egui_ctx);
            Ok(Box::new(Gallery(Showcase::new())))
        }),
    )
}
