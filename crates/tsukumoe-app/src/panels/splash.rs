// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The viewport while no project is open: the ways to start, and the recent
//! projects. It returns the actions the editor should run.

use std::path::PathBuf;

use egui::Ui;
use tsukumoe_ui::{Variant, button, hint, tr};

/// How many recent projects are listed.
const RECENT_SHOWN: usize = 6;

pub fn show(ui: &mut Ui, recent: &[PathBuf]) -> Vec<String> {
    let ctx = ui.ctx().clone();
    let mut actions = Vec::new();
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 4.0).max(8.0));
        ui.heading("Tsukumoe");
        ui.add_space(12.0);
        for (key, action, variant) in [
            ("menu-new", "file.new", Variant::Primary),
            ("menu-open", "file.open", Variant::Secondary),
            ("menu-new-from-psd", "file.new-from-psd", Variant::Secondary),
        ] {
            if button(ui, &tr(&ctx, key), variant).clicked() {
                actions.push(action.to_owned());
            }
        }
        ui.add_space(16.0);
        hint(ui, tr(&ctx, "menu-recent"));
        if recent.is_empty() {
            hint(ui, tr(&ctx, "menu-no-recent"));
        }
        for path in recent.iter().take(RECENT_SHOWN) {
            let text = path.display().to_string();
            if ui.link(&text).clicked() {
                actions.push(format!("file.open-recent:{text}"));
            }
        }
    });
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn the_buttons_and_recent_projects_send_their_actions() {
        let mut h = Harness::new_ui_state(
            |ui, state: &mut Vec<String>| {
                tsukumoe_ui::init(ui.ctx());
                tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                state.extend(show(ui, &[PathBuf::from("/a/b.tsukumoe")]));
            },
            Vec::new(),
        );
        h.run();
        h.get_by_label("New project...").click();
        h.run();
        h.get_by_label("/a/b.tsukumoe").click();
        h.run();
        assert_eq!(
            h.state(),
            &[
                "file.new".to_owned(),
                "file.open-recent:/a/b.tsukumoe".to_owned()
            ]
        );
    }
}
