// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The viewport while no project is open: the ways to start, and the recent
//! projects. It returns the actions the editor should run.

use std::path::PathBuf;

use egui::Ui;
use tsukumoe_ui::{Icon, Variant, hint, selectable_row, tr};

const BUTTON_WIDTH: f32 = 240.0;
const RECENT_WIDTH: f32 = 420.0;

/// The end of a long folder path, with the cut marked.
fn shorten(text: &str, keep: usize) -> String {
    let count = text.chars().count();
    if count <= keep {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(count - keep).collect();
    format!("…{tail}")
}

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
            // The same width, so the three read as one group.
            if tsukumoe_ui::button_wide(ui, &tr(&ctx, key), variant, BUTTON_WIDTH).clicked() {
                actions.push(action.to_owned());
            }
        }
        ui.add_space(16.0);
        hint(ui, tr(&ctx, "menu-recent"));
        if recent.is_empty() {
            hint(ui, tr(&ctx, "menu-no-recent"));
        }
        let width = RECENT_WIDTH.min(ui.available_width());
        let muted = tsukumoe_ui::Theme::get(&ctx).colors.text_muted;
        for path in recent.iter().take(RECENT_SHOWN) {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let folder = path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            ui.allocate_ui(egui::vec2(width, ui.spacing().interact_size.y), |ui| {
                let row = selectable_row(ui, false, Some(Icon::Folder), &name);
                ui.painter().text(
                    egui::pos2(row.rect.right() - 8.0, row.rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    shorten(&folder, 32),
                    egui::FontId::proportional(11.0),
                    muted,
                );
                if row.on_hover_text(path.display().to_string()).clicked() {
                    actions.push(format!("file.open-recent:{}", path.display()));
                }
            });
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
        h.get_by_label("New project…").click();
        h.run();
        h.get_by_label("b.tsukumoe").click();
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
