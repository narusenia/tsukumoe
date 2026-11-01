// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The command palette: search every edit command, fill its arguments in a
//! form generated from the schema, and run it through the dispatcher.

use egui::Ui;
use tsukumoe_core::command::Command;
use tsukumoe_ui::{Variant, button, checkbox, hint, label, selectable_row, text_input, tr};

use crate::commands::{CommandInfo, FieldKind, build, search};

pub struct Palette {
    list: Vec<CommandInfo>,
    pub query: String,
    selected: Option<usize>,
    inputs: Vec<String>,
    pub error: Option<String>,
    /// Focus the search field on the next frame.
    pub focus: bool,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            list: crate::commands::all(),
            query: String::new(),
            selected: None,
            inputs: Vec::new(),
            error: None,
            focus: false,
        }
    }
}

impl Palette {
    pub fn commands(&self) -> &[CommandInfo] {
        &self.list
    }

    pub fn select(&mut self, name: &str) {
        self.selected = self.list.iter().position(|c| c.name == name);
        self.inputs = self
            .selected
            .map(|i| vec![String::new(); self.list[i].fields.len()])
            .unwrap_or_default();
        self.error = None;
    }

    pub fn selected(&self) -> Option<&CommandInfo> {
        self.selected.map(|i| &self.list[i])
    }

    /// Builds the command from the form; the error is also kept for display.
    pub fn build(&mut self) -> Option<Command> {
        let info = self.selected()?.clone();
        match build(&info, &self.inputs) {
            Ok(c) => {
                self.error = None;
                Some(c)
            }
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }

    /// Draws the palette; returns a command when the user runs one.
    pub fn show(&mut self, ui: &mut Ui) -> Option<Command> {
        let ctx = ui.ctx().clone();
        let name = tr(&ctx, "palette-search");
        let search_box = text_input(ui, &mut self.query, &name, &name);
        if std::mem::take(&mut self.focus) {
            search_box.request_focus();
        }
        let matches: Vec<String> = search(&self.list, &self.query)
            .iter()
            .map(|c| c.name.clone())
            .collect();
        // Enter picks the first match.
        if search_box.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(first) = matches.first()
        {
            self.select(first);
        }
        egui::ScrollArea::vertical()
            .id_salt("palette-list")
            .max_height(ui.available_height() * 0.5)
            .show(ui, |ui| {
                for m in &matches {
                    let on = self.selected().is_some_and(|c| &c.name == m);
                    if selectable_row(ui, on, None, m).clicked() {
                        self.select(m);
                    }
                }
            });
        let mut run = false;
        if let Some(i) = self.selected {
            ui.separator();
            let info = self.list[i].clone();
            hint(ui, info.description.clone());
            for (field, text) in info.fields.iter().zip(self.inputs.iter_mut()) {
                ui.horizontal(|ui| {
                    label(
                        ui,
                        format!("{}{}", field.name, if field.required { " *" } else { "" }),
                    );
                    if field.kind == FieldKind::Bool {
                        let mut on = text == "true";
                        if checkbox(ui, &mut on, &field.name).changed() {
                            *text = on.to_string();
                        }
                    } else {
                        text_input(ui, text, &field.name, "");
                    }
                });
            }
            if button(ui, &tr(&ctx, "palette-run"), Variant::Primary).clicked() {
                run = true;
            }
        }
        if let Some(e) = &self.error {
            hint(ui, e.clone());
        }
        if run { self.build() } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_sizes_the_form_and_a_bad_form_keeps_an_error() {
        let mut p = Palette::default();
        assert!(p.commands().len() >= 10);
        p.select("add_part");
        assert_eq!(p.selected().unwrap().name, "add_part");
        assert!(p.build().is_none() && p.error.is_some());
        p.inputs[0] = "Arm".into();
        let c = p.build().unwrap();
        assert_eq!(
            c,
            Command::AddPart {
                name: "Arm".into(),
                parent: None
            }
        );
        assert!(p.error.is_none());
        p.select("nope");
        assert!(p.selected().is_none());
    }
}
