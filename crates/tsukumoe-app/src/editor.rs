// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The editor's state and its window layout: menus, workspace tabs, the dock
//! with the standard panels, dialogs and the status line. It knows nothing of
//! eframe's window, so a test can drive it with a harness.

use std::path::{Path, PathBuf};

use crate::actions::{self, ACTIONS, Tool};
use crate::drag::{AxisTarget, DragAssignment, Modifier};
use crate::menu;
use crate::panels::palette::Palette;
use crate::panels::{history, inspector, parameters, tree, viewport};
use crate::pie::{Entry, PieConfig};
use eframe::egui_wgpu::RenderState;
use egui::{Context, Ui};
use tsukumoe_ui::dock::{self, PanelHost};
use tsukumoe_ui::{
    Arg, Icon, Keymap, Toasts, Tone, Variant, button, checkbox, hint, label, text_input, tr,
    tr_args,
};

use crate::panels::viewport::Viewport;
use crate::project::{Document, Recent};
use crate::psd_import::{PsdImport, RowKind, Stage};
use crate::settings;
use crate::view::ViewState;
use crate::workspace::{Layouts, Preset, Workspace};

const LAYOUTS_FILE: &str = "layouts.json";
const STATE_FILE: &str = "state.json";
const KEYMAP_FILE: &str = "keymap.json";
const DRAG_FILE: &str = "drag.json";
const PIE_FILE: &str = "pie.json";

pub enum Dialog {
    Open(String),
    SaveAs(String),
    SaveLayout(String),
    ConfirmPreset(Preset),
    /// The shortcut list: the text of each action's field, and the last error.
    Keymap(Vec<String>, Option<String>),
    /// The drag-to-parameter assignment being edited.
    Drag,
    /// The pie menu slots being edited.
    Pie,
    /// File > Import PSD.
    PsdImport(Box<PsdImport>),
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
    pub view: ViewState,
    pub palette: Palette,
    pub keymap: Keymap,
    pub drag: DragAssignment,
    pub pie_config: PieConfig,
    /// Whether the system draws the menus (macOS), leaving only the tabs here.
    pub native_menu: bool,
    /// The open pie menu, while its key is held.
    pie: Option<tsukumoe_ui::PieMenu>,
    /// Whether the pointer was over the viewport last frame (a keymap context).
    viewport_hovered: bool,
    viewport: Option<Viewport>,
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
        let keymap = read(KEYMAP_FILE)
            .and_then(|t| Keymap::from_json(&t).ok())
            .unwrap_or_else(actions::cubism_compatible);
        let drag =
            read(DRAG_FILE).map_or_else(DragAssignment::default, |t| DragAssignment::from_json(&t));
        let pie_config =
            read(PIE_FILE).map_or_else(PieConfig::default, |t| PieConfig::from_json(&t));
        Self {
            doc: None,
            workspace: serde_json::from_value(state["workspace"].clone()).unwrap_or_default(),
            layouts,
            recent: Recent::from_json(&state["recent"]),
            settings_dir,
            dialog: None,
            toasts: Toasts::default(),
            view: ViewState::default(),
            palette: Palette::default(),
            keymap,
            drag,
            pie_config,
            native_menu: false,
            pie: None,
            viewport_hovered: false,
            viewport: None,
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
                self.view = ViewState::default();
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

    /// The single entry for UI edits: sends `command` to the dispatcher and
    /// shows a failure as an error toast (the model stays as it was).
    pub fn submit(&mut self, ctx: &Context, command: tsukumoe_core::command::Command) -> bool {
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        match doc.dispatcher.submit(command) {
            Ok(()) => true,
            Err(e) => {
                self.toasts.push(ctx, Tone::Error, e);
                false
            }
        }
    }

    fn save_keymap(&mut self, ctx: &Context) {
        if let Some(dir) = &self.settings_dir
            && let Err(e) = settings::write(dir, KEYMAP_FILE, &self.keymap.to_json(ctx))
        {
            self.toasts.push(ctx, Tone::Error, e);
        }
    }

    /// Runs an action by name (a key press, a menu item, a pie menu item) and
    /// says whether it knows the name. Unknown names, e.g. from a hand-edited
    /// keymap, do nothing. `file.open-recent:<path>` and `layout.load:<name>`
    /// carry their argument after the colon.
    pub fn run_action(&mut self, ctx: &Context, action: &str) -> bool {
        match action {
            "file.open" => self.dialog = Some(Dialog::Open(String::new())),
            "file.save-as" => {
                if self.doc.is_some() {
                    self.dialog = Some(Dialog::SaveAs(String::new()));
                }
            }
            "file.save" => {
                let r = self.save();
                self.report(ctx, r, Some(tr(ctx, "app-saved")));
            }
            "file.import-psd" => {
                if self.doc.is_some() {
                    self.dialog = Some(Dialog::PsdImport(Box::default()));
                }
            }
            "edit.undo" => drop(self.undo()),
            "edit.redo" => drop(self.redo()),
            "view.palette" => self.open_palette(),
            "view.pie-settings" => self.dialog = Some(Dialog::Pie),
            "view.drag-settings" => self.dialog = Some(Dialog::Drag),
            "view.keymap-settings" => {
                let texts = ACTIONS
                    .iter()
                    .map(|a| {
                        self.keymap
                            .keys_text(a, actions::context_of(a))
                            .unwrap_or_default()
                    })
                    .collect();
                self.dialog = Some(Dialog::Keymap(texts, None));
            }
            "layout.save" => self.dialog = Some(Dialog::SaveLayout(String::new())),
            // Holding the key repeats the press; reopening would move the menu
            // to the pointer every time.
            "view.pie-menu" => {
                if self.pie.is_none() {
                    let at = ctx.pointer_latest_pos().unwrap_or_default();
                    self.pie = Some(tsukumoe_ui::PieMenu::open(at, self.pie_config.items(ctx)));
                }
            }
            "viewport.zoom-in" | "viewport.zoom-out" => {
                let f = if action.ends_with("in") { 1.25 } else { 0.8 };
                self.view.camera.zoom_at(f, [0.5, 0.5]);
                self.view.touch();
            }
            "viewport.reset-camera" => {
                self.view.camera.reset();
                self.view.touch();
            }
            other => {
                if let Some(tool) = Tool::from_action(other) {
                    self.view.tool = tool;
                } else if let Some(path) = other.strip_prefix("file.open-recent:") {
                    let path = PathBuf::from(path);
                    let r = self.open(&path);
                    let name = path.display().to_string();
                    let ok = tr_args(ctx, "app-opened", &[("name", Arg::from(name.as_str()))]);
                    self.report(ctx, r, Some(ok));
                } else if let Some(name) = other.strip_prefix("layout.load:") {
                    if self.layouts.load_named(self.workspace, name) {
                        self.layouts_dirty = true;
                    }
                } else if let Some(preset) = other
                    .strip_prefix("layout.preset.")
                    .and_then(|k| Preset::ALL.into_iter().find(|p| p.key() == k))
                {
                    self.dialog = Some(Dialog::ConfirmPreset(preset));
                } else {
                    return false;
                }
            }
        }
        true
    }

    /// Draws the open pie menu; the key's release picks the item under the
    /// pointer (or cancels at the center).
    fn pie_frame(&mut self, ctx: &Context) {
        let Some(pie) = self.pie.as_mut() else {
            return;
        };
        let released = match self.keymap.shortcut("view.pie-menu") {
            Some(s) => ctx.input(|i| i.key_released(s.logical_key)),
            None => true,
        };
        match pie.show(ctx, released) {
            tsukumoe_ui::PieEvent::Open => {}
            tsukumoe_ui::PieEvent::Chosen(action) => {
                self.pie = None;
                self.run_action(ctx, &action);
            }
            tsukumoe_ui::PieEvent::Cancelled => self.pie = None,
        }
    }

    /// Shows the command palette panel and puts the cursor in its search field.
    pub fn open_palette(&mut self) {
        self.layouts.current_mut(self.workspace).reveal("palette");
        self.layouts_dirty = true;
        self.palette.focus = true;
    }

    pub fn undo(&mut self) -> bool {
        self.doc.as_mut().is_some_and(|d| d.dispatcher.undo())
    }

    pub fn redo(&mut self) -> bool {
        self.doc.as_mut().is_some_and(|d| d.dispatcher.redo())
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
    /// `rs` is the wgpu state for the viewport; without it the viewport says so.
    pub fn ui(&mut self, ui: &mut Ui, rs: Option<&RenderState>) {
        let ctx = ui.ctx().clone();
        let active: &[&str] = if self.viewport_hovered {
            &["viewport"]
        } else {
            &[]
        };
        self.viewport_hovered = false;
        for action in self.keymap.triggered(&ctx, active) {
            self.run_action(&ctx, &action);
        }
        self.pie_frame(&ctx);
        egui::Panel::top("tsukumoe-menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("tsukumoe-status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            let mut layout = self.layouts.current(self.workspace).clone();
            let before = layout.clone();
            dock::show(ui, &mut layout, &mut Panels { editor: self, rs });
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

    /// The menu as shown now, from [`menu::MENU`] and the editor's state.
    pub fn menu_entries(&self, ctx: &Context) -> Vec<menu::Entry> {
        let (can_undo, can_redo) = self.doc.as_ref().map_or((false, false), |d| {
            (d.dispatcher.can_undo(), d.dispatcher.can_redo())
        });
        let state = menu::State {
            has_doc: self.doc.is_some(),
            can_undo,
            can_redo,
            recent: self
                .recent
                .paths()
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            saved_layouts: self
                .layouts
                .saved
                .iter()
                .filter(|s| s.workspace == self.workspace)
                .map(|s| s.name.clone())
                .collect(),
        };
        menu::resolve(menu::MENU, &state, &|key| tr(ctx, key), &|action| {
            self.keymap.shortcut_text(ctx, action)
        })
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let t = |key: &str| tr(&ctx, key);
        let entries = self.menu_entries(&ctx);
        let mut chosen = None;
        egui::MenuBar::new().ui(ui, |ui| {
            for entry in entries.iter().filter(|_| !self.native_menu) {
                if let menu::Entry::Sub { label, entries } = entry {
                    ui.menu_button(label, |ui| show_entries(ui, entries, &mut chosen));
                }
            }
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
        if let Some(action) = chosen {
            self.run_action(&ctx, &action);
        }
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
        if matches!(dialog, Dialog::Pie) {
            let mut open = true;
            if self.pie_dialog(ctx, &mut open)
                && let Some(dir) = &self.settings_dir
                && let Err(e) = settings::write(dir, PIE_FILE, &self.pie_config.to_json())
            {
                self.toasts.push(ctx, Tone::Error, e);
            }
            if open {
                self.dialog = Some(dialog);
            }
            return;
        }
        if matches!(dialog, Dialog::Drag) {
            let mut open = true;
            if self.drag_dialog(ctx, &mut open)
                && let Some(dir) = &self.settings_dir
                && let Err(e) = settings::write(dir, DRAG_FILE, &self.drag.to_json())
            {
                self.toasts.push(ctx, Tone::Error, e);
            }
            if open {
                self.dialog = Some(dialog);
            }
            return;
        }
        if let Dialog::PsdImport(state) = &mut dialog {
            let mut open = true;
            self.psd_dialog(ctx, state, &mut open);
            if open {
                self.dialog = Some(dialog);
            }
            return;
        }
        if let Dialog::Keymap(texts, error) = &mut dialog {
            let mut open = true;
            let changed = self.keymap_dialog(ctx, texts, error, &mut open);
            if changed {
                self.save_keymap(ctx);
            }
            if open {
                self.dialog = Some(dialog);
            }
            return;
        }
        let mut open = true;
        let mut done = false;
        let mut cancel = false;
        let (title, id) = match &dialog {
            Dialog::Open(_) => (t("dialog-open-title"), "dialog-open"),
            Dialog::SaveAs(_) => (t("dialog-save-as-title"), "dialog-save-as"),
            Dialog::SaveLayout(_) => (t("layout-save-title"), "dialog-save-layout"),
            Dialog::ConfirmPreset(_) => (t("layout-confirm-title"), "dialog-confirm-preset"),
            Dialog::Keymap(..) | Dialog::Drag | Dialog::Pie | Dialog::PsdImport(_) => {
                unreachable!("handled above")
            }
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
                Dialog::Keymap(..) | Dialog::Drag | Dialog::Pie | Dialog::PsdImport(_) => {}
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

impl Editor {
    /// File > Import PSD: path, then the diff with the choices, then one
    /// command. The state is `PsdImport`; this only draws and forwards.
    fn psd_dialog(&mut self, ctx: &Context, state: &mut PsdImport, open: &mut bool) {
        let t = |key: &str| tr(ctx, key);
        let Some(doc) = self.doc.as_mut() else {
            *open = false;
            return;
        };
        if state.poll(doc) || state.stage == Stage::Loading {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let candidates = state.match_candidates();
        let rows = state.rows();
        let mut load = false;
        let mut confirm = false;
        let mut cancel = false;
        let mut remove: Option<(String, bool)> = None;
        let mut matched: Option<(String, Option<String>)> = None;
        let none = t("psd-none");
        tsukumoe_ui::dialog(ctx, "dialog-psd-import", &t("psd-title"), open, |ui| {
            ui.set_min_width(520.0);
            match state.stage {
                Stage::Path => {
                    let name = t("psd-path");
                    let response = text_input(ui, &mut state.path, &name, &name);
                    response.request_focus();
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        load = true;
                    }
                }
                Stage::Loading => {
                    hint(ui, t("psd-loading"));
                }
                Stage::Review => {
                    hint(ui, state.path.clone());
                    if rows.is_empty() {
                        hint(ui, t("psd-nothing"));
                    }
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for row in &rows {
                                ui.horizontal(|ui| {
                                    let (key, tone) = match row.kind {
                                        RowKind::AddGroup => ("psd-row-add-group", Tone::Success),
                                        RowKind::AddLayer => ("psd-row-add-layer", Tone::Success),
                                        RowKind::ImageChanged => ("psd-row-image", Tone::Warning),
                                        RowKind::Updated => ("psd-row-updated", Tone::Neutral),
                                        RowKind::Missing { .. } => ("psd-row-missing", Tone::Error),
                                    };
                                    tsukumoe_ui::badge(ui, &t(key), tone);
                                    label(ui, row.key.clone());
                                    if !row.detail.is_empty() {
                                        hint(ui, row.detail.clone());
                                    }
                                    if let RowKind::Missing { remove: current } = row.kind {
                                        let mut value = current;
                                        if checkbox(ui, &mut value, &t("psd-remove")).changed() {
                                            remove = Some((row.key.clone(), value));
                                        }
                                    }
                                    let can_match = !candidates.is_empty()
                                        && (row.kind == RowKind::AddLayer
                                            || row.matched_with.is_some());
                                    if can_match && state.is_new_layer(&row.key) {
                                        let mut value = row.matched_with.clone();
                                        let mut options: Vec<(Option<String>, String)> =
                                            vec![(None, none.clone())];
                                        options.extend(
                                            candidates.iter().map(|c| (Some(c.clone()), c.clone())),
                                        );
                                        let refs: Vec<(Option<String>, &str)> = options
                                            .iter()
                                            .map(|(v, n)| (v.clone(), n.as_str()))
                                            .collect();
                                        if tsukumoe_ui::select(
                                            ui,
                                            ("psd-match", &row.key),
                                            &t("psd-match"),
                                            &mut value,
                                            &refs,
                                        )
                                        .changed()
                                        {
                                            matched = Some((row.key.clone(), value));
                                        }
                                    }
                                });
                            }
                        });
                    for note in state.notes() {
                        hint(ui, format!("{}: {}", note.layer, note.message));
                    }
                }
            }
            if let Some(e) = &state.error {
                tsukumoe_ui::badge(ui, e, Tone::Error);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                match state.stage {
                    Stage::Path => load |= button(ui, &t("psd-read"), Variant::Primary).clicked(),
                    Stage::Loading => {}
                    Stage::Review => {
                        ui.add_enabled_ui(state.can_confirm(), |ui| {
                            confirm = button(ui, &t("psd-import"), Variant::Primary).clicked();
                        });
                    }
                }
                cancel = button(ui, &t("dialog-cancel"), Variant::Secondary).clicked();
            });
        });
        if let Some((key, value)) = remove {
            state.set_remove(&key, value, doc);
        }
        if let Some((new, old)) = matched {
            state.set_match(&new, old.as_deref(), doc);
        }
        if load {
            state.start();
        }
        if confirm {
            match state.confirm(doc) {
                Ok(()) => {
                    let layers = state.plan().map_or(0, |p| p.items.len());
                    self.toasts.push(
                        ctx,
                        Tone::Success,
                        tr_args(ctx, "psd-imported", &[("count", Arg::from(layers))]),
                    );
                    // The viewport reads its textures when it is made.
                    self.viewport = None;
                    *open = false;
                }
                Err(e) => state.error = Some(e),
            }
        }
        if cancel {
            *open = false;
        }
    }
}

impl Editor {
    /// Which action each pie slot runs; returns whether anything changed.
    fn pie_dialog(&mut self, ctx: &Context, open: &mut bool) -> bool {
        let t = |key: &str| tr(ctx, key);
        let before = self.pie_config.clone();
        let names: Vec<(String, String)> = ACTIONS
            .iter()
            .map(|a| ((*a).to_owned(), t(&actions::label_key(a))))
            .collect();
        let options: Vec<(String, &str)> =
            names.iter().map(|(a, n)| (a.clone(), n.as_str())).collect();
        let config = &mut self.pie_config;
        let mut reset = false;
        tsukumoe_ui::dialog(ctx, "dialog-pie", &t("pie-title"), open, |ui| {
            hint(ui, t("pie-hint"));
            fn slots(
                ui: &mut Ui,
                entries: &mut [Entry],
                options: &[(String, &str)],
                path: &str,
                t: &dyn Fn(&str) -> String,
            ) {
                for (i, entry) in entries.iter_mut().enumerate() {
                    let id = format!("{path}/{i}");
                    match entry {
                        Entry::Action { action } => {
                            tsukumoe_ui::select(ui, id, &format!("{}", i + 1), action, options);
                        }
                        Entry::Group { label, items } => {
                            label_text(ui, &t(label));
                            ui.indent(id.clone(), |ui| slots(ui, items, options, &id, t));
                        }
                    }
                }
            }
            fn label_text(ui: &mut Ui, text: &str) {
                label(ui, text);
            }
            slots(ui, &mut config.items, &options, "pie", &t);
            reset = button(ui, &t("pie-reset"), Variant::Secondary).clicked();
        });
        if reset {
            *config = PieConfig::default();
        }
        *config != before
    }

    /// The drag-to-parameter settings; returns whether they changed.
    fn drag_dialog(&mut self, ctx: &Context, open: &mut bool) -> bool {
        let t = |key: &str| tr(ctx, key);
        let names: Vec<String> = self.doc.as_ref().map_or_else(Vec::new, |d| {
            d.dispatcher
                .model()
                .parameters()
                .iter()
                .map(|(_, n, _)| n.to_owned())
                .collect()
        });
        let before = self.drag.clone();
        let drag = &mut self.drag;
        tsukumoe_ui::dialog(ctx, "dialog-drag", &t("drag-title"), open, |ui| {
            hint(ui, t("drag-hint"));
            let modifiers = [
                (Modifier::Alt, "Alt".to_owned()),
                (Modifier::Ctrl, "Ctrl".to_owned()),
                (Modifier::Cmd, "Cmd".to_owned()),
            ];
            let options: Vec<(Modifier, &str)> =
                modifiers.iter().map(|(m, n)| (*m, n.as_str())).collect();
            tsukumoe_ui::select(
                ui,
                "drag-modifier",
                &t("drag-modifier"),
                &mut drag.modifier,
                &options,
            );
            for (key, axis) in [("drag-x", &mut drag.x), ("drag-y", &mut drag.y)] {
                ui.separator();
                label(ui, t(key));
                let mut choice: Option<String> = axis.as_ref().map(|a| a.parameter.clone());
                let mut options: Vec<(Option<String>, &str)> = vec![(None, "-")];
                options.extend(names.iter().map(|n| (Some(n.clone()), n.as_str())));
                let none = t("drag-none");
                options[0].1 = &none;
                if tsukumoe_ui::select(ui, key, &t("drag-parameter"), &mut choice, &options)
                    .changed()
                {
                    *axis = choice.map(|parameter| match axis.take() {
                        Some(a) => AxisTarget { parameter, ..a },
                        None => AxisTarget {
                            parameter,
                            points_per_range: 400.0,
                            invert: false,
                        },
                    });
                }
                if let Some(a) = axis {
                    ui.horizontal(|ui| {
                        label(ui, t("drag-sensitivity"));
                        tsukumoe_ui::number(
                            ui,
                            &mut a.points_per_range,
                            20.0..=4000.0,
                            5.0,
                            &t("drag-sensitivity"),
                        );
                        checkbox(ui, &mut a.invert, &t("drag-invert"));
                    });
                }
            }
        });
        *drag != before
    }

    /// The shortcut list. Each field applies when it loses focus; returns
    /// whether the keymap changed.
    fn keymap_dialog(
        &mut self,
        ctx: &Context,
        texts: &mut [String],
        error: &mut Option<String>,
        open: &mut bool,
    ) -> bool {
        let t = |key: &str| tr(ctx, key);
        let mut changed = false;
        let mut reset = false;
        tsukumoe_ui::dialog(ctx, "dialog-keymap", &t("keymap-title"), open, |ui| {
            hint(ui, t("keymap-hint"));
            for (action, text) in ACTIONS.iter().zip(texts.iter_mut()) {
                ui.horizontal(|ui| {
                    ui.set_min_width(420.0);
                    ui.label(t(&actions::label_key(action)));
                    let r = text_input(ui, text, action, "");
                    if r.lost_focus() {
                        let context = actions::context_of(action);
                        let text = text.trim();
                        if text.is_empty() {
                            changed |= self.keymap.unbind(action, context);
                            *error = None;
                        } else {
                            match self.keymap.bind(action, context, text) {
                                Ok(()) => {
                                    changed = true;
                                    *error = None;
                                }
                                Err(_) => *error = Some(t("keymap-bad")),
                            }
                        }
                    }
                });
            }
            for (a, b) in self.keymap.conflicts() {
                let name = |id: &str| t(&actions::label_key(id));
                hint(
                    ui,
                    tr_args(
                        ctx,
                        "keymap-conflict",
                        &[
                            ("a", Arg::from(name(&a).as_str())),
                            ("b", Arg::from(name(&b).as_str())),
                        ],
                    ),
                );
            }
            if let Some(e) = error.as_ref() {
                hint(ui, e.clone());
            }
            reset = button(ui, &t("keymap-reset"), Variant::Secondary).clicked();
        });
        if reset {
            self.keymap = actions::cubism_compatible();
            for (action, text) in ACTIONS.iter().zip(texts.iter_mut()) {
                *text = self
                    .keymap
                    .keys_text(action, actions::context_of(action))
                    .unwrap_or_default();
            }
            changed = true;
        }
        changed
    }
}

/// The dock's view of the editor: panel names and their contents.
/// Draws `entries` as egui menu items; the action of a clicked one goes to `chosen`.
fn show_entries(ui: &mut Ui, entries: &[menu::Entry], chosen: &mut Option<String>) {
    for entry in entries {
        match entry {
            menu::Entry::Separator => {
                ui.separator();
            }
            menu::Entry::Sub { label, entries } => {
                ui.menu_button(label, |ui| show_entries(ui, entries, chosen));
            }
            menu::Entry::Item {
                action,
                label,
                enabled,
                shortcut,
            } => {
                ui.add_enabled_ui(*enabled, |ui| {
                    if tsukumoe_ui::menu_item(ui, label, shortcut.as_deref()).clicked() {
                        *chosen = Some(action.clone());
                        ui.close();
                    }
                });
            }
        }
    }
}

struct Panels<'a> {
    editor: &'a mut Editor,
    rs: Option<&'a RenderState>,
}

impl PanelHost for Panels<'_> {
    fn title(&self, ctx: &Context, id: &str) -> String {
        tr(ctx, &format!("panel-{id}"))
    }

    fn icon(&self, id: &str) -> Option<Icon> {
        Some(match id {
            "viewport" => Icon::Image,
            "tree" => Icon::ListTree,
            "parameters" => Icon::Sliders,
            "inspector" => Icon::Info,
            "history" => Icon::History,
            "palette" => Icon::Search,
            "ai_diff" => Icon::Sparkles,
            _ => return None,
        })
    }

    fn ui(&mut self, ui: &mut Ui, id: &str) {
        let ctx = ui.ctx().clone();
        let editor = &mut *self.editor;
        let Some(doc) = editor.doc.as_mut() else {
            match id {
                "viewport" => viewport::placeholder(ui, false),
                _ => {
                    hint(ui, tr(&ctx, "history-empty"));
                }
            }
            return;
        };
        match id {
            "history" => history::show(ui, &mut doc.dispatcher),
            "tree" => tree::show(ui, doc.dispatcher.model(), &mut editor.view),
            "parameters" => parameters::show(ui, doc.dispatcher.model(), &mut editor.view),
            "inspector" => {
                let command =
                    inspector::show(ui, doc.dispatcher.model(), editor.view.selection.as_ref());
                if let Some(command) = command {
                    editor.submit(&ctx, command);
                }
            }
            "palette" => {
                if let Some(command) = editor.palette.show(ui) {
                    editor.submit(&ctx, command);
                }
            }
            "viewport" => match self.rs {
                Some(rs) => {
                    if editor.viewport.as_ref().map(Viewport::doc_id) != Some(doc.id) {
                        match Viewport::new(rs, doc) {
                            Ok(v) => editor.viewport = Some(v),
                            Err(e) => {
                                editor.viewport = None;
                                hint(ui, e);
                            }
                        }
                    }
                    if let Some(vp) = editor.viewport.as_mut() {
                        editor.viewport_hovered = vp
                            .show(ui, rs, doc, &mut editor.view, &editor.drag)
                            .hovered();
                    }
                }
                None => viewport::placeholder(ui, true),
            },
            _ => {
                hint(ui, tr(&ctx, "panel-empty"));
            }
        }
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
                    editor.ui(ui, None);
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
    fn edits_go_through_one_door_and_failures_surface() {
        use tsukumoe_core::command::Command;
        let mut editor = Editor::new(None);
        let ctx = Context::default();
        assert!(!editor.submit(
            &ctx,
            Command::AddPart {
                name: "x".into(),
                parent: None
            }
        ));
        editor
            .open(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tsukumoe-format/tests/fixtures/minimal"),
            )
            .unwrap();
        let add = || Command::AddPart {
            name: "P".into(),
            parent: None,
        };
        assert!(editor.submit(&ctx, add()));
        let parts = |e: &Editor| e.doc.as_ref().unwrap().dispatcher.model().parts().len();
        let n = parts(&editor);
        assert!(!editor.submit(&ctx, add()), "duplicate name fails");
        assert_eq!(parts(&editor), n);
        assert!(!editor.toasts.is_empty());
        assert!(editor.doc.as_ref().unwrap().dispatcher.error().is_some());
        assert!(editor.undo() && parts(&editor) == n - 1);
        assert!(editor.redo() && parts(&editor) == n);
    }

    fn open_minimal() -> Editor {
        let mut editor = Editor::new(None);
        editor
            .open(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tsukumoe-format/tests/fixtures/minimal"),
            )
            .unwrap();
        editor
    }

    #[test]
    fn importing_a_psd_through_the_dialog_changes_the_document_in_one_step() {
        use tsukumoe_psd::writer::{LayerSpec, Node, PsdSpec, write};
        let dir = scratch("psd");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.psd");
        let layer = Node::Layer(LayerSpec::solid("bg", 0, 0, 8, 8, [1, 2, 3, 255]));
        std::fs::write(&file, write(&PsdSpec::new(8, 8, vec![layer]))).unwrap();

        let ctx = Context::default();
        let mut none = Editor::new(None);
        assert!(none.run_action(&ctx, "file.import-psd"));
        assert!(none.dialog.is_none(), "needs a project");

        let mut editor = open_minimal();
        assert!(editor.run_action(&ctx, "file.import-psd"));
        let Some(Dialog::PsdImport(mut state)) = editor.dialog.take() else {
            panic!("the dialog did not open");
        };
        state.path = file.display().to_string();
        state.load_blocking(editor.doc.as_ref().unwrap());
        assert!(state.can_confirm());
        editor.dialog = Some(Dialog::PsdImport(state));
        // The dialog draws in the review stage.
        let mut harness = harness(editor);
        harness.run();
        let mut editor = harness.into_state();
        let Some(Dialog::PsdImport(mut state)) = editor.dialog.take() else {
            panic!("the dialog closed");
        };
        let doc = editor.doc.as_mut().unwrap();
        let before = doc.dispatcher.model().meshes().len();
        state.confirm(doc).unwrap();
        assert_eq!(doc.dispatcher.model().meshes().len(), before + 1);
        assert!(doc.dispatcher.undo());
        assert_eq!(doc.dispatcher.model().meshes().len(), before);
    }

    #[test]
    fn menu_actions_open_their_dialogs_and_unknown_names_are_refused() {
        let ctx = Context::default();
        let mut editor = Editor::new(None);
        assert!(editor.run_action(&ctx, "file.save-as"));
        assert!(editor.dialog.is_none(), "nothing to save without a project");
        let mut editor = open_minimal();
        assert!(editor.run_action(&ctx, "file.save-as"));
        assert!(matches!(editor.dialog, Some(Dialog::SaveAs(_))));
        assert!(editor.run_action(&ctx, "layout.preset.cubism"));
        assert!(matches!(
            editor.dialog,
            Some(Dialog::ConfirmPreset(Preset::Cubism))
        ));
        assert!(editor.run_action(&ctx, "view.keymap-settings"));
        assert!(matches!(editor.dialog, Some(Dialog::Keymap(..))));
        assert!(editor.run_action(&ctx, "layout.save"));
        assert!(matches!(editor.dialog, Some(Dialog::SaveLayout(_))));
        assert!(!editor.run_action(&ctx, "layout.preset.nope"));
        assert!(!editor.run_action(&ctx, "nonsense.action"));
    }

    #[test]
    fn a_shortcut_runs_its_action_and_tools_switch() {
        use tsukumoe_core::command::Command;
        let mut editor = open_minimal();
        let ctx = Context::default();
        assert!(editor.submit(
            &ctx,
            Command::AddPart {
                name: "P".into(),
                parent: None
            }
        ));
        let mut h = harness(editor);
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert!(
            !h.state().doc.as_ref().unwrap().dispatcher.can_undo(),
            "Mod+Z undid it"
        );
        h.key_press(egui::Key::M);
        h.run();
        assert_eq!(h.state().view.tool, Tool::Mesh);
        h.state_mut().run_action(&ctx, "nonsense.action");
    }

    fn frame(ctx: &Context, events: Vec<egui::Event>, mut f: impl FnMut(&Context)) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| f(ui.ctx()));
        out.textures_delta.clear();
    }

    #[test]
    fn the_pie_menu_opens_on_its_key_and_releasing_toward_an_item_runs_it() {
        let ctx = Context::default();
        let mut editor = Editor::new(None);
        editor.pie_config = PieConfig::from_json(
            r#"{"items":[{"action":"tool.mesh"},{"action":"tool.deformer"},{"action":"tool.select"},{"action":"edit.redo"}]}"#,
        );
        let at = egui::pos2(400.0, 400.0);
        frame(&ctx, vec![egui::Event::PointerMoved(at)], |ctx| {
            editor.run_action(ctx, "view.pie-menu");
            editor.pie_frame(ctx);
        });
        assert!(editor.pie.is_some(), "open while the key is held");
        // release straight up: the first item
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Q,
            physical_key: Some(egui::Key::Q),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(at + egui::vec2(0.0, -60.0)),
                key(false),
            ],
            |ctx| {
                editor.pie_frame(ctx);
            },
        );
        assert!(editor.pie.is_none());
        assert_eq!(editor.view.tool, Tool::Mesh);
        // releasing at the center cancels without running anything
        editor.view.tool = Tool::Select;
        frame(&ctx, vec![egui::Event::PointerMoved(at)], |ctx| {
            editor.run_action(ctx, "view.pie-menu");
            editor.pie_frame(ctx);
        });
        frame(&ctx, vec![key(false)], |ctx| editor.pie_frame(ctx));
        assert!(editor.pie.is_none());
        assert_eq!(editor.view.tool, Tool::Select);
    }

    #[test]
    fn a_repeated_key_press_does_not_move_the_open_pie_menu() {
        let ctx = Context::default();
        let mut editor = Editor::new(None);
        editor.pie_config = PieConfig::from_json(
            r#"{"items":[{"action":"tool.mesh"},{"action":"tool.deformer"},{"action":"tool.select"},{"action":"edit.redo"}]}"#,
        );
        let at = egui::pos2(400.0, 400.0);
        frame(&ctx, vec![egui::Event::PointerMoved(at)], |ctx| {
            editor.run_action(ctx, "view.pie-menu");
            editor.pie_frame(ctx);
        });
        // the key repeats while the pointer is elsewhere
        let away = at + egui::vec2(300.0, 0.0);
        frame(&ctx, vec![egui::Event::PointerMoved(away)], |ctx| {
            editor.run_action(ctx, "view.pie-menu");
            editor.pie_frame(ctx);
        });
        // straight up from where it first opened still picks the first item
        let release = egui::Event::Key {
            key: egui::Key::Q,
            physical_key: Some(egui::Key::Q),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(at + egui::vec2(0.0, -60.0)),
                release,
            ],
            |ctx| editor.pie_frame(ctx),
        );
        assert_eq!(editor.view.tool, Tool::Mesh);
    }

    #[test]
    fn user_key_assignments_are_saved_and_restored() {
        let dir = scratch("keymap");
        let ctx = Context::default();
        let mut editor = Editor::new(Some(dir.clone()));
        editor.keymap.bind("edit.undo", None, "Mod+Y").unwrap();
        editor.save_keymap(&ctx);
        let again = Editor::new(Some(dir.clone()));
        assert_eq!(
            again.keymap.keys_text("edit.undo", None).as_deref(),
            Some("Mod+Y")
        );
        // a damaged file falls back to the preset
        settings::write(&dir, KEYMAP_FILE, "{").unwrap();
        let broken = Editor::new(Some(dir.clone()));
        assert_eq!(
            broken.keymap.keys_text("edit.undo", None).as_deref(),
            Some("Mod+Z")
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
