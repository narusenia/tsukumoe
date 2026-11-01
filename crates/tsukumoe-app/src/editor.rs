// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The editor's state and its window layout: menus, workspace tabs, the dock
//! with the standard panels, dialogs and the status line. It knows nothing of
//! eframe's window, so a test can drive it with a harness.

use std::path::{Path, PathBuf};

use egui::{Context, Ui};
use tsukumoe_ui::dock::{self, PanelHost};
use tsukumoe_ui::{Arg, Toasts, Tone, Variant, button, hint, text_input, tr, tr_args};

use crate::project::{Document, Recent};
use crate::settings;
use crate::workspace::{Layouts, Preset, Workspace};

const LAYOUTS_FILE: &str = "layouts.json";
const STATE_FILE: &str = "state.json";

pub enum Dialog {
    Open(String),
    SaveAs(String),
    SaveLayout(String),
    ConfirmPreset(Preset),
}

pub struct Editor {
    pub doc: Option<Document>,
    pub workspace: Workspace,
    pub layouts: Layouts,
    pub recent: Recent,
    /// Where the editor's settings are saved; `None` without a home folder.
    pub settings_dir: Option<PathBuf>,
    pub dialog: Option<Dialog>,
    pub toasts: Toasts,
    layouts_dirty: bool,
}

impl Editor {
    /// Loads the saved layouts and state from `settings_dir`; anything missing
    /// or damaged gives the defaults.
    pub fn new(settings_dir: Option<PathBuf>) -> Self {
        let read = |name| {
            settings_dir
                .as_deref()
                .and_then(|d| settings::read(d, name))
        };
        let layouts = read(LAYOUTS_FILE).map_or_else(Layouts::default, |t| Layouts::from_json(&t));
        let state: serde_json::Value = read(STATE_FILE)
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            doc: None,
            workspace: serde_json::from_value(state["workspace"].clone()).unwrap_or_default(),
            layouts,
            recent: Recent::from_json(&state["recent"]),
            settings_dir,
            dialog: None,
            toasts: Toasts::default(),
            layouts_dirty: false,
        }
    }

    fn persist_state(&self) -> Result<(), String> {
        let Some(dir) = &self.settings_dir else {
            return Ok(());
        };
        let state = serde_json::json!({
            "workspace": self.workspace,
            "recent": self.recent.to_json(),
        });
        settings::write(
            dir,
            STATE_FILE,
            &serde_json::to_string_pretty(&state).unwrap_or_default(),
        )
    }

    fn persist_layouts(&mut self) -> Result<(), String> {
        self.layouts_dirty = false;
        match &self.settings_dir {
            Some(dir) => settings::write(dir, LAYOUTS_FILE, &self.layouts.to_json()),
            None => Ok(()),
        }
    }

    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        match Document::open(path) {
            Ok(doc) => {
                self.doc = Some(doc);
                self.recent.push(path);
                self.persist_state()
            }
            Err(e) => {
                self.recent.remove(path);
                let _ = self.persist_state();
                Err(e)
            }
        }
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.doc.as_mut().map_or(Ok(()), Document::save)
    }

    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        let Some(doc) = self.doc.as_mut() else {
            return Ok(());
        };
        doc.save_to(path)?;
        self.recent.push(path);
        self.persist_state()
    }

    pub fn switch_workspace(&mut self, workspace: Workspace) {
        if self.workspace != workspace {
            self.workspace = workspace;
            let _ = self.persist_state();
        }
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        self.layouts.apply_preset(self.workspace, preset);
        self.layouts_dirty = true;
    }

    fn report(&mut self, ctx: &Context, result: Result<(), String>, ok: Option<String>) {
        match result {
            Ok(()) => {
                if let Some(text) = ok {
                    self.toasts.push(ctx, Tone::Success, text);
                }
            }
            Err(e) => self.toasts.push(ctx, Tone::Error, e),
        }
    }

    /// One frame of the whole editor window.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("tsukumoe-menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("tsukumoe-status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            let mut layout = self.layouts.current(self.workspace).clone();
            let before = layout.clone();
            dock::show(ui, &mut layout, &mut Panels { editor: self });
            if layout != before {
                *self.layouts.current_mut(self.workspace) = layout;
                self.layouts_dirty = true;
            }
        });
        self.dialogs(&ctx);
        // Writing while a divider is dragged would spam the disk.
        if self.layouts_dirty && !ctx.input(|i| i.pointer.any_down()) {
            let result = self.persist_layouts();
            self.report(&ctx, result, None);
        }
        self.toasts.show(&ctx);
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let t = |key: &str| tr(&ctx, key);
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t("menu-file"), |ui| {
                if tsukumoe_ui::menu_item(ui, &t("menu-open"), None).clicked() {
                    self.dialog = Some(Dialog::Open(String::new()));
                    ui.close();
                }
                let has_doc = self.doc.is_some();
                ui.add_enabled_ui(has_doc, |ui| {
                    if tsukumoe_ui::menu_item(ui, &t("menu-save"), None).clicked() {
                        let r = self.save();
                        self.report(&ctx, r, Some(t("app-saved")));
                        ui.close();
                    }
                    if tsukumoe_ui::menu_item(ui, &t("menu-save-as"), None).clicked() {
                        self.dialog = Some(Dialog::SaveAs(String::new()));
                        ui.close();
                    }
                });
                ui.menu_button(t("menu-recent"), |ui| {
                    if self.recent.paths().is_empty() {
                        ui.add_enabled(false, egui::Label::new(t("menu-no-recent")));
                    }
                    for path in self.recent.paths().to_vec() {
                        let name = path.display().to_string();
                        if tsukumoe_ui::menu_item(ui, &name, None).clicked() {
                            let r = self.open(&path);
                            let ok =
                                tr_args(&ctx, "app-opened", &[("name", Arg::from(name.as_str()))]);
                            self.report(&ctx, r, Some(ok));
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button(t("menu-layout"), |ui| {
                for preset in Preset::ALL {
                    if tsukumoe_ui::menu_item(
                        ui,
                        &t(&format!("layout-preset-{}", preset.key())),
                        None,
                    )
                    .clicked()
                    {
                        self.dialog = Some(Dialog::ConfirmPreset(preset));
                        ui.close();
                    }
                }
                if tsukumoe_ui::menu_item(ui, &t("layout-save-named"), None).clicked() {
                    self.dialog = Some(Dialog::SaveLayout(String::new()));
                    ui.close();
                }
                ui.menu_button(t("layout-load-named"), |ui| {
                    let names: Vec<String> = self
                        .layouts
                        .saved
                        .iter()
                        .filter(|s| s.workspace == self.workspace)
                        .map(|s| s.name.clone())
                        .collect();
                    if names.is_empty() {
                        ui.add_enabled(false, egui::Label::new(t("layout-none-saved")));
                    }
                    for name in names {
                        if tsukumoe_ui::menu_item(ui, &name, None).clicked() {
                            if self.layouts.load_named(self.workspace, &name) {
                                self.layouts_dirty = true;
                            }
                            ui.close();
                        }
                    }
                });
            });
            ui.separator();
            let mut selected = Workspace::ALL
                .iter()
                .position(|w| *w == self.workspace)
                .unwrap_or(0);
            let names: Vec<String> = Workspace::ALL
                .iter()
                .map(|w| t(&format!("workspace-{}", w.key())))
                .collect();
            let names_ref: Vec<&str> = names.iter().map(String::as_str).collect();
            if tsukumoe_ui::tabs(ui, &mut selected, &names_ref).changed() {
                self.switch_workspace(Workspace::ALL[selected]);
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| match &self.doc {
            Some(doc) => {
                hint(ui, doc.name());
                if doc.dirty() {
                    tsukumoe_ui::badge(ui, &tr(&ctx, "app-unsaved"), Tone::Warning);
                }
                if let Some(e) = doc.dispatcher.error() {
                    tsukumoe_ui::badge(ui, e, Tone::Error);
                }
            }
            None => {
                hint(ui, tr(&ctx, "app-no-project"));
            }
        });
    }

    fn dialogs(&mut self, ctx: &Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let t = |key: &str| tr(ctx, key);
        let mut open = true;
        let mut done = false;
        let mut cancel = false;
        let (title, id) = match &dialog {
            Dialog::Open(_) => (t("dialog-open-title"), "dialog-open"),
            Dialog::SaveAs(_) => (t("dialog-save-as-title"), "dialog-save-as"),
            Dialog::SaveLayout(_) => (t("layout-save-title"), "dialog-save-layout"),
            Dialog::ConfirmPreset(_) => (t("layout-confirm-title"), "dialog-confirm-preset"),
        };
        tsukumoe_ui::dialog(ctx, id, &title, &mut open, |ui| {
            match &mut dialog {
                Dialog::Open(text) | Dialog::SaveAs(text) => {
                    let name = t("dialog-path");
                    text_input(ui, text, &name, &name).request_focus();
                }
                Dialog::SaveLayout(text) => {
                    let name = t("layout-name");
                    text_input(ui, text, &name, &name).request_focus();
                }
                Dialog::ConfirmPreset(_) => {
                    hint(ui, t("layout-confirm-body"));
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                done = button(ui, &t("dialog-ok"), Variant::Primary).clicked();
                if button(ui, &t("dialog-cancel"), Variant::Secondary).clicked() {
                    cancel = true;
                }
            });
        });
        open &= !cancel;
        if done {
            match &dialog {
                Dialog::Open(text) if !text.trim().is_empty() => {
                    let path = PathBuf::from(text.trim());
                    let r = self.open(&path);
                    let ok = tr_args(
                        ctx,
                        "app-opened",
                        &[("name", Arg::from(path.display().to_string().as_str()))],
                    );
                    self.report(ctx, r, Some(ok));
                }
                Dialog::SaveAs(text) if !text.trim().is_empty() => {
                    let r = self.save_as(Path::new(text.trim()));
                    self.report(ctx, r, Some(t("app-saved")));
                }
                Dialog::SaveLayout(text) => {
                    if self.layouts.save_named(self.workspace, text) {
                        self.layouts_dirty = true;
                    }
                }
                Dialog::ConfirmPreset(preset) => self.apply_preset(*preset),
                _ => {}
            }
            open = false;
        }
        if open {
            self.dialog = Some(dialog);
        }
    }
}

/// The dock's view of the editor: panel names and their contents.
struct Panels<'a> {
    editor: &'a mut Editor,
}

impl PanelHost for Panels<'_> {
    fn title(&self, ctx: &Context, id: &str) -> String {
        tr(ctx, &format!("panel-{id}"))
    }

    fn ui(&mut self, ui: &mut Ui, _id: &str) {
        let _ = &self.editor;
        hint(ui, tr(ui.ctx(), "panel-empty"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tsukumoe-app-editor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn harness(editor: Editor) -> Harness<'static, Editor> {
        Harness::builder()
            .with_size(egui::vec2(1200.0, 700.0))
            .build_ui_state(
                |ui, editor: &mut Editor| {
                    tsukumoe_ui::init(ui.ctx());
                    editor.ui(ui);
                },
                editor,
            )
    }

    #[test]
    fn switching_workspaces_keeps_the_document_and_separate_layouts() {
        let dir = scratch("ws");
        let mut editor = Editor::new(Some(dir.clone()));
        editor
            .open(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tsukumoe-format/tests/fixtures/minimal"),
            )
            .unwrap();
        editor
            .doc
            .as_mut()
            .unwrap()
            .dispatcher
            .submit(tsukumoe_core::command::Command::AddPart {
                name: "P".into(),
                parent: None,
            })
            .unwrap();
        let mut h = harness(editor);
        h.run();
        h.get_by_label("Animation").click();
        h.run();
        let e = h.state();
        assert_eq!(e.workspace, Workspace::Animation);
        let d = e.doc.as_ref().unwrap();
        assert!(
            d.dirty() && d.dispatcher.can_undo(),
            "model and history survive"
        );
        assert_ne!(
            e.layouts.current(Workspace::Modeling),
            e.layouts.current(Workspace::Animation)
        );
        // the choice is remembered
        assert_eq!(
            Editor::new(Some(dir.clone())).workspace,
            Workspace::Animation
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_preset_change_is_saved_and_restored() {
        let dir = scratch("preset");
        let mut editor = Editor::new(Some(dir.clone()));
        editor.apply_preset(Preset::Cubism);
        let mut h = harness(editor);
        h.run();
        let again = Editor::new(Some(dir.clone()));
        assert_eq!(
            again.layouts.current(Workspace::Modeling),
            &crate::workspace::builtin_layout(Preset::Cubism, Workspace::Modeling)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn opening_a_missing_project_reports_and_keeps_going() {
        let mut editor = Editor::new(None);
        assert!(editor.open(Path::new("/no/such")).is_err());
        assert!(editor.doc.is_none() && editor.recent.paths().is_empty());
    }
}
