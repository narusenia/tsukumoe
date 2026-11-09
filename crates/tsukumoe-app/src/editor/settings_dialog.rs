// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The settings dialog (M5-02f): one window with a tab for each group of
//! settings. The tabs draw what used to be separate dialogs (drag, pie menu,
//! deletion confirmations, shortcuts) and the look of the editor (language,
//! theme, density, tab style). A change is applied and saved at once.

use crate::options::{self, Background, Options};
use egui::ScrollArea;
use tsukumoe_ui::{Density, Language, Mode, Preferences, builtin_themes, form_row, section_header};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Controls,
    Pie,
    Shortcuts,
    View,
    Edit,
    Files,
}

impl SettingsTab {
    const ALL: [Self; 7] = [
        Self::General,
        Self::Controls,
        Self::Pie,
        Self::Shortcuts,
        Self::View,
        Self::Edit,
        Self::Files,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::General => "settings-tab-general",
            Self::Controls => "settings-tab-controls",
            Self::Pie => "settings-tab-pie",
            Self::Shortcuts => "settings-tab-shortcuts",
            Self::View => "settings-tab-view",
            Self::Edit => "settings-tab-edit",
            Self::Files => "settings-tab-files",
        }
    }
}

/// What the open dialog holds: the tab, and the shortcut fields being typed in.
pub struct SettingsState {
    pub tab: SettingsTab,
    texts: Vec<String>,
    error: Option<String>,
    /// The pie settings being edited: 0 the common slots, then one per tool.
    pie_mode: usize,
}

const WIDTH: f32 = 800.0;
const TAB_COLUMN: f32 = 140.0;
const BODY_MIN: f32 = 320.0;
const BODY_MAX: f32 = 900.0;

impl Editor {
    /// Opens the settings dialog on `tab`.
    pub(super) fn open_settings(&mut self, tab: SettingsTab) {
        let texts = ACTIONS
            .iter()
            .map(|a| {
                self.keymap
                    .keys_text(a, actions::context_of(a))
                    .unwrap_or_default()
            })
            .collect();
        self.dialog = Some(Dialog::Settings(Box::new(SettingsState {
            tab,
            texts,
            error: None,
            pie_mode: 0,
        })));
    }

    pub(super) fn settings_dialog(
        &mut self,
        ctx: &Context,
        state: &mut SettingsState,
        open: &mut bool,
    ) {
        let t = |key: &str| tr(ctx, key);
        // Tall enough for the longest tab, but never more than the window allows.
        let body_height = (ctx.content_rect().height() * 0.8).clamp(BODY_MIN, BODY_MAX);
        tsukumoe_ui::dialog_sized(
            ctx,
            "dialog-settings",
            &t("settings-title"),
            WIDTH,
            open,
            |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(TAB_COLUMN);
                        for tab in SettingsTab::ALL {
                            if tsukumoe_ui::selectable_row(
                                ui,
                                state.tab == tab,
                                None,
                                &t(tab.key()),
                            )
                            .clicked()
                            {
                                state.tab = tab;
                            }
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(WIDTH - TAB_COLUMN - 24.0);
                        ui.set_min_height(body_height);
                        ScrollArea::vertical()
                            .max_height(body_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| match state.tab {
                                SettingsTab::General => self.general_contents(ctx, ui),
                                SettingsTab::Controls => {
                                    section_header(ui, &t("drag-title"));
                                    self.drag_contents(ctx, ui);
                                    ui.add_space(12.0);
                                    section_header(ui, &t("confirm-settings-title"));
                                    self.confirm_contents(ctx, ui);
                                }
                                SettingsTab::Pie => self.pie_contents(ctx, ui, &mut state.pie_mode),
                                SettingsTab::Shortcuts => {
                                    self.keymap_contents(
                                        ctx,
                                        ui,
                                        &mut state.texts,
                                        &mut state.error,
                                    );
                                }
                                SettingsTab::View => self.view_contents(ctx, ui),
                                SettingsTab::Edit => self.edit_contents(ctx, ui),
                                SettingsTab::Files => self.files_contents(ctx, ui),
                            });
                    });
                });
            },
        );
    }

    /// Language, theme, density and the tab style. The first three are the
    /// preferences file shared with the host; they apply as soon as they change.
    fn general_contents(&mut self, ctx: &Context, ui: &mut Ui) {
        let t = |key: &str| tr(ctx, key);
        let themes = builtin_themes();
        let before = self.prefs.clone();
        let tab_style_before = self.tab_style;

        let named = self
            .prefs
            .theme
            .as_ref()
            .filter(|name| themes.iter().any(|t| &t.name == *name));
        let mut choice = match (named, self.prefs.mode) {
            (Some(name), _) => format!("theme:{name}"),
            (None, Mode::Dark) => "mode:dark".to_owned(),
            (None, Mode::Light) => "mode:light".to_owned(),
            (None, Mode::System) => "mode:system".to_owned(),
        };
        let modes = [
            ("mode:dark".to_owned(), t("theme-dark")),
            ("mode:light".to_owned(), t("theme-light")),
            ("mode:system".to_owned(), t("theme-system")),
        ];
        let mut options: Vec<(String, &str)> =
            modes.iter().map(|(k, n)| (k.clone(), n.as_str())).collect();
        options.extend(
            themes
                .iter()
                .map(|t| (format!("theme:{}", t.name), t.name.as_str())),
        );
        let theme_changed = labeled_select(
            ui,
            "settings-theme",
            &t("host-theme"),
            &mut choice,
            &options,
        )
        .changed();
        if theme_changed {
            match choice.split_once(':') {
                Some(("theme", name)) => self.prefs.theme = Some(name.to_owned()),
                Some((_, "light")) => (self.prefs.theme, self.prefs.mode) = (None, Mode::Light),
                Some((_, "system")) => (self.prefs.theme, self.prefs.mode) = (None, Mode::System),
                _ => (self.prefs.theme, self.prefs.mode) = (None, Mode::Dark),
            }
        }

        let densities = [
            (Density::Compact, t("density-compact")),
            (Density::Regular, t("density-regular")),
        ];
        let density_options: Vec<(Density, &str)> =
            densities.iter().map(|(d, n)| (*d, n.as_str())).collect();
        labeled_select(
            ui,
            "settings-density",
            &t("host-density"),
            &mut self.prefs.density,
            &density_options,
        );

        let auto = t("language-auto");
        let mut languages = vec![(None, auto.as_str())];
        languages.extend(Language::ALL.map(|l| (Some(l), l.native_name())));
        labeled_select(
            ui,
            "settings-language",
            &t("host-language"),
            &mut self.prefs.language,
            &languages,
        );

        let styles = [
            (TabStyle::Compact, t("tab-style-compact")),
            (TabStyle::Full, t("tab-style-full")),
        ];
        let style_options: Vec<(TabStyle, &str)> =
            styles.iter().map(|(s, n)| (*s, n.as_str())).collect();
        labeled_select(
            ui,
            "settings-tab-style",
            &t("menu-tab-style"),
            &mut self.tab_style,
            &style_options,
        );

        ui.add_space(8.0);
        if button(ui, &t("settings-reset"), Variant::Secondary).clicked() {
            self.prefs = Preferences::default();
            self.tab_style = TabStyle::default();
        }
        if self.prefs != before {
            self.prefs.apply(ctx, &themes);
            self.save_prefs(ctx);
        }
        if self.tab_style != tab_style_before {
            let _ = self.persist_state();
        }
    }

    fn save_prefs(&mut self, ctx: &Context) {
        if let Some(dir) = &self.config_dir
            && let Err(e) = settings::write(dir, "preferences.json", &self.prefs.to_json())
        {
            self.toasts.push(ctx, Tone::Error, e);
        }
    }

    fn confirm_contents(&mut self, ctx: &Context, ui: &mut Ui) {
        let t = |key: &str| tr(ctx, key);
        let before = self.confirm.clone();
        for kind in KINDS {
            let mut asks = self.confirm.asks(kind);
            if checkbox(ui, &mut asks, &t(kind.key())).changed() {
                self.confirm.set(kind, asks);
            }
        }
        ui.add_space(8.0);
        if button(ui, &t("confirm-reset"), Variant::Secondary).clicked() {
            self.confirm.reset();
        }
        if self.confirm != before {
            self.save_confirm(ctx);
        }
    }

    /// The pie menu: which slots each mode opens, laid out like the pie. Mode 0
    /// is the common set; a tool can have slots of its own.
    fn pie_contents(&mut self, ctx: &Context, ui: &mut Ui, mode: &mut usize) {
        use crate::actions::Tool;
        let t = |key: &str| tr(ctx, key);
        let before = self.pie_config.clone();
        let tools = Tool::ALL;
        let names: Vec<(String, String)> = ACTIONS
            .iter()
            .map(|a| ((*a).to_owned(), t(&actions::label_key(a))))
            .collect();
        let options: Vec<(String, &str)> =
            names.iter().map(|(a, n)| (a.clone(), n.as_str())).collect();
        // Which mode's slots are edited.
        let labels = [
            t("pie-mode-common"),
            t("pie-mode-select"),
            t("pie-mode-mesh"),
            t("pie-mode-deformer"),
            t("pie-mode-bone"),
        ];
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        tsukumoe_ui::tabs(ui, mode, &label_refs);
        *mode = (*mode).min(tools.len());
        let tool_key = (*mode > 0).then(|| tools[*mode - 1].key().to_owned());
        if let Some(key) = &tool_key {
            let mut own = self.pie_config.tools.contains_key(key);
            if tsukumoe_ui::checkbox(ui, &mut own, &t("pie-mode-own")).changed() {
                if own {
                    let copy = self.pie_config.items.clone();
                    self.pie_config.tools.insert(key.clone(), copy);
                } else {
                    self.pie_config.tools.remove(key);
                }
            }
            if !own {
                hint(ui, t("pie-mode-uses-common"));
            }
        }
        let editable = tool_key
            .as_ref()
            .is_none_or(|k| self.pie_config.tools.contains_key(k));
        if editable {
            let items = match &tool_key {
                Some(k) => self.pie_config.tools.get_mut(k).expect("checked above"),
                None => &mut self.pie_config.items,
            };
            Self::pie_slots(ui, items, &options, &t);
        }
        if button(ui, &t("pie-reset"), Variant::Secondary).clicked() {
            self.pie_config = PieConfig::default();
        }
        if self.pie_config != before
            && let Some(dir) = &self.settings_dir
            && let Err(e) = settings::write(dir, PIE_FILE, &self.pie_config.to_json())
        {
            self.toasts.push(ctx, Tone::Error, e);
        }
    }

    /// The slots of one set: a picture of the pie on the left, the numbered
    /// slots on the right. Each slot is an action or a group (a submenu with a
    /// name of its own and slots inside).
    fn pie_slots(
        ui: &mut Ui,
        items: &mut Vec<Entry>,
        options: &[(String, &str)],
        t: &dyn Fn(&str) -> String,
    ) {
        const MAX_SLOTS: usize = 12;
        const PIE_PICTURE: f32 = 300.0;
        /// The select's value that stands for "a group" (no action has a NUL in its name).
        const GROUP: &str = "\u{0}group";
        /// A group's name is a translation key for the built-in one, else typed text.
        fn group_name(label: &str, t: &dyn Fn(&str) -> String) -> String {
            t(label)
        }
        fn rows(
            ui: &mut Ui,
            entries: &mut [Entry],
            options: &[(String, &str)],
            path: &str,
            groups_allowed: bool,
            t: &dyn Fn(&str) -> String,
        ) {
            let mut choices: Vec<(String, &str)> = options.to_vec();
            let group_text = t("pie-slot-group");
            if groups_allowed {
                choices.push((GROUP.to_owned(), group_text.as_str()));
            }
            for (i, entry) in entries.iter_mut().enumerate() {
                let id = format!("{path}/{i}");
                let mut current = match &*entry {
                    Entry::Action { action } => action.clone(),
                    Entry::Group { .. } => GROUP.to_owned(),
                };
                let before = current.clone();
                ui.horizontal(|ui| {
                    // The same width for every slot, whatever it holds.
                    ui.spacing_mut().combo_width = 180.0;
                    ui.add_sized(
                        egui::vec2(14.0, ui.spacing().interact_size.y),
                        egui::Label::new(format!("{}", i + 1)),
                    );
                    tsukumoe_ui::select(
                        ui,
                        id.clone(),
                        &format!("{}", i + 1),
                        &mut current,
                        &choices,
                    );
                    if let Entry::Group { label, .. } = &mut *entry {
                        let mut text = group_name(label, t);
                        let name = t("pie-group-name");
                        if tsukumoe_ui::text_input(ui, &mut text, &name, &name).changed() {
                            *label = text;
                        }
                    }
                });
                if current != before {
                    *entry = if current == GROUP {
                        Entry::Group {
                            label: t("pie-group-new"),
                            items: vec![Entry::Action {
                                action: "view.palette".to_owned(),
                            }],
                        }
                    } else {
                        Entry::Action { action: current }
                    };
                }
                if let Entry::Group { items, .. } = entry {
                    ui.indent(id.clone(), |ui| {
                        rows(ui, items, options, &id, false, t);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled_ui(items.len() < MAX_SLOTS, |ui| {
                                    button(ui, &t("pie-slot-add"), Variant::Secondary).clicked()
                                })
                                .inner
                            {
                                items.push(Entry::Action {
                                    action: "view.palette".to_owned(),
                                });
                            }
                            if ui
                                .add_enabled_ui(items.len() > 1, |ui| {
                                    button(ui, &t("pie-slot-remove"), Variant::Secondary).clicked()
                                })
                                .inner
                            {
                                items.pop();
                            }
                        });
                    });
                }
            }
        }
        let n = items.len().max(1);
        ui.horizontal_top(|ui| {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(PIE_PICTURE, PIE_PICTURE), egui::Sense::hover());
            let theme = tsukumoe_ui::Theme::get(ui.ctx());
            let painter = ui.painter_at(rect);
            let radius = PIE_PICTURE / 2.0 - 16.0;
            painter.circle_filled(rect.center(), radius, theme.colors.surface);
            painter.circle_stroke(
                rect.center(),
                radius,
                egui::Stroke::new(1.0, theme.colors.border_strong),
            );
            painter.circle_filled(rect.center(), radius * 0.25, theme.colors.background);
            for (i, entry) in items.iter().enumerate() {
                let a = i as f32 * std::f32::consts::TAU / n as f32;
                let dir = egui::vec2(a.sin(), -a.cos());
                painter.line_segment(
                    [
                        rect.center() + dir * radius * 0.25,
                        rect.center() + dir * radius,
                    ],
                    egui::Stroke::new(1.0, theme.colors.border),
                );
                let name = match entry {
                    Entry::Action { action } => options
                        .iter()
                        .find(|(a, _)| a == action)
                        .map_or_else(String::new, |(_, n)| (*n).to_owned()),
                    Entry::Group { label, .. } => group_name(label, t),
                };
                // The slot's number near the rim, and its name, cut short, inside.
                painter.text(
                    rect.center() + dir * radius * 0.88,
                    egui::Align2::CENTER_CENTER,
                    format!("{}", i + 1),
                    egui::FontId::proportional(theme.metrics.text - 1.0),
                    theme.colors.text_muted,
                );
                painter.text(
                    rect.center() + dir * radius * 0.6,
                    egui::Align2::CENTER_CENTER,
                    shorten(&name, 7),
                    egui::FontId::proportional(theme.metrics.text - 1.0),
                    theme.colors.text,
                );
            }
            ui.vertical(|ui| {
                rows(ui, items, options, "pie", true, t);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled_ui(items.len() < MAX_SLOTS, |ui| {
                            button(ui, &t("pie-slot-add"), Variant::Secondary).clicked()
                        })
                        .inner
                    {
                        items.push(Entry::Action {
                            action: "view.palette".to_owned(),
                        });
                    }
                    if ui
                        .add_enabled_ui(items.len() > 1, |ui| {
                            button(ui, &t("pie-slot-remove"), Variant::Secondary).clicked()
                        })
                        .inner
                    {
                        items.pop();
                    }
                });
            });
        });
    }

    fn drag_contents(&mut self, ctx: &Context, ui: &mut Ui) {
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
        let modifiers = [
            (Modifier::Alt, "Alt".to_owned()),
            (Modifier::Ctrl, "Ctrl".to_owned()),
            (Modifier::Cmd, "Cmd".to_owned()),
        ];
        let options: Vec<(Modifier, &str)> =
            modifiers.iter().map(|(m, n)| (*m, n.as_str())).collect();
        labeled_select(
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
            if labeled_select(ui, key, &t("drag-parameter"), &mut choice, &options).changed() {
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
                form_row(ui, &t("drag-sensitivity"), |ui| {
                    tsukumoe_ui::number_or(
                        ui,
                        &mut a.points_per_range,
                        20.0..=4000.0,
                        5.0,
                        &t("drag-sensitivity"),
                        Some(400.0),
                    );
                    checkbox(ui, &mut a.invert, &t("drag-invert"));
                });
            }
        }
        if self.drag != before
            && let Some(dir) = &self.settings_dir
            && let Err(e) = settings::write(dir, DRAG_FILE, &self.drag.to_json())
        {
            self.toasts.push(ctx, Tone::Error, e);
        }
    }

    fn keymap_contents(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        texts: &mut [String],
        error: &mut Option<String>,
    ) {
        let t = |key: &str| tr(ctx, key);
        let mut changed = false;
        // Two columns so every field starts at the same place, with room between the
        // name and the field, and a field only as wide as a key chord needs.
        egui::Grid::new("tsukumoe-keymap")
            .num_columns(2)
            .spacing([20.0, 6.0])
            .show(ui, |ui| {
                for (action, text) in ACTIONS.iter().zip(texts.iter_mut()) {
                    ui.scope(|ui| {
                        ui.set_min_width(300.0);
                        ui.label(t(&actions::label_key(action)));
                    });
                    let r = ui
                        .scope(|ui| {
                            ui.set_width(180.0);
                            text_input(ui, text, action, "")
                        })
                        .inner;
                    ui.end_row();
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
                }
            });
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
        if button(ui, &t("keymap-reset"), Variant::Secondary).clicked() {
            self.keymap = actions::cubism_compatible();
            for (action, text) in ACTIONS.iter().zip(texts.iter_mut()) {
                *text = self
                    .keymap
                    .keys_text(action, actions::context_of(action))
                    .unwrap_or_default();
            }
            changed = true;
        }
        if changed {
            self.save_keymap(ctx);
        }
    }

    /// Writes the autosave file when it is due: switched on, something unsaved
    /// that was not in the last autosave, and the interval passed.
    pub(super) fn autosave_tick(&mut self, ctx: &Context) {
        if !self.options.autosave {
            return;
        }
        // Wake up now and then even when nothing else repaints.
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
        let Some(doc) = &mut self.doc else { return };
        if !doc.dirty() || !autosave_due(self.autosave.0.elapsed(), self.options.autosave_minutes) {
            return;
        }
        let revision = doc.dispatcher.revision();
        self.autosave.0 = std::time::Instant::now();
        if self.autosave.1 == Some(revision) {
            return;
        }
        match doc.autosave() {
            Ok(_) => self.autosave.1 = Some(revision),
            Err(e) => self.toasts.push(ctx, Tone::Error, e),
        }
    }

    fn save_options(&mut self, ctx: &Context) {
        if let Some(dir) = &self.settings_dir
            && let Err(e) = settings::write(dir, OPTIONS_FILE, &self.options.to_json())
        {
            self.toasts.push(ctx, Tone::Error, e);
        }
    }

    /// Runs `draw` on the options; when it changed them, puts them in range and saves.
    fn edit_options(&mut self, ctx: &Context, draw: impl FnOnce(&mut Options)) {
        let before = self.options.clone();
        draw(&mut self.options);
        if self.options != before {
            self.options = self.options.clone().clamped();
            self.save_options(ctx);
        }
    }

    fn view_contents(&mut self, ctx: &Context, ui: &mut Ui) {
        let t = |key: &str| tr(ctx, key);
        let names = [
            (Background::Theme, t("background-theme")),
            (Background::White, t("background-white")),
            (Background::Black, t("background-black")),
            (Background::Checker, t("background-checker")),
        ];
        let choices: Vec<(Background, &str)> =
            names.iter().map(|(b, n)| (*b, n.as_str())).collect();
        self.edit_options(ctx, |o| {
            labeled_select(
                ui,
                "settings-background",
                &t("settings-background"),
                &mut o.background,
                &choices,
            );
            let d = Options::default();
            for (key, value, default) in [
                (
                    "settings-vertex-radius",
                    &mut o.vertex_radius,
                    d.vertex_radius,
                ),
                ("settings-edge-radius", &mut o.edge_radius, d.edge_radius),
            ] {
                form_row(ui, &t(key), |ui| {
                    let range = options::RADIUS;
                    tsukumoe_ui::number_or(
                        ui,
                        value,
                        range.0..=range.1,
                        0.25,
                        &t(key),
                        Some(default),
                    );
                });
            }
            ui.add_space(8.0);
            if button(ui, &t("settings-reset"), Variant::Secondary).clicked() {
                let d = Options::default();
                (o.background, o.vertex_radius, o.edge_radius) =
                    (d.background, d.vertex_radius, d.edge_radius);
            }
        });
    }

    fn edit_contents(&mut self, ctx: &Context, ui: &mut Ui) {
        let t = |key: &str| tr(ctx, key);
        self.edit_options(ctx, |o| {
            hint(ui, t("settings-history-hint"));
            let mut limit = o.history_limit as f32;
            form_row(ui, &t("settings-history-limit"), |ui| {
                tsukumoe_ui::number_or(
                    ui,
                    &mut limit,
                    0.0..=options::HISTORY_LIMIT.1 as f32,
                    1.0,
                    &t("settings-history-limit"),
                    Some(Options::default().history_limit as f32),
                );
            });
            o.history_limit = limit.round() as u32;
            let d = Options::default();
            for (key, value, range, default) in [
                (
                    "settings-weld-ratio",
                    &mut o.weld_ratio,
                    options::WELD_RATIO,
                    d.weld_ratio,
                ),
                (
                    "settings-mirror-tolerance",
                    &mut o.mirror_tolerance,
                    options::MIRROR_TOLERANCE,
                    d.mirror_tolerance,
                ),
            ] {
                form_row(ui, &t(key), |ui| {
                    tsukumoe_ui::number_or(
                        ui,
                        value,
                        range.0..=range.1,
                        0.0005,
                        &t(key),
                        Some(default),
                    );
                });
            }
            ui.add_space(8.0);
            if button(ui, &t("settings-reset"), Variant::Secondary).clicked() {
                let d = Options::default();
                (o.history_limit, o.weld_ratio, o.mirror_tolerance) =
                    (d.history_limit, d.weld_ratio, d.mirror_tolerance);
            }
        });
    }

    fn files_contents(&mut self, ctx: &Context, ui: &mut Ui) {
        let t = |key: &str| tr(ctx, key);
        self.edit_options(ctx, |o| {
            hint(ui, t("settings-autosave-hint"));
            checkbox(ui, &mut o.autosave, &t("settings-autosave"));
            let mut minutes = o.autosave_minutes as f32;
            form_row(ui, &t("settings-autosave-minutes"), |ui| {
                tsukumoe_ui::number_or(
                    ui,
                    &mut minutes,
                    options::AUTOSAVE_MINUTES.0 as f32..=options::AUTOSAVE_MINUTES.1 as f32,
                    1.0,
                    &t("settings-autosave-minutes"),
                    Some(Options::default().autosave_minutes as f32),
                );
            });
            o.autosave_minutes = minutes.round() as u32;
            ui.add_space(8.0);
            if button(ui, &t("settings-reset"), Variant::Secondary).clicked() {
                let d = Options::default();
                (o.autosave, o.autosave_minutes) = (d.autosave, d.autosave_minutes);
            }
        });
        self.edit_options(ctx, |o| {
            ui.add_space(8.0);
            checkbox(ui, &mut o.atlas, &t("settings-atlas"));
            let mut size = o.atlas_size as f32;
            let mut padding = o.atlas_padding as f32;
            form_row(ui, &t("settings-atlas-size"), |ui| {
                tsukumoe_ui::number_or(
                    ui,
                    &mut size,
                    options::ATLAS_SIZE.0 as f32..=options::ATLAS_SIZE.1 as f32,
                    64.0,
                    &t("settings-atlas-size"),
                    Some(Options::default().atlas_size as f32),
                );
            });
            form_row(ui, &t("settings-atlas-padding"), |ui| {
                tsukumoe_ui::number_or(
                    ui,
                    &mut padding,
                    options::ATLAS_PADDING.0 as f32..=options::ATLAS_PADDING.1 as f32,
                    1.0,
                    &t("settings-atlas-padding"),
                    Some(Options::default().atlas_padding as f32),
                );
            });
            o.atlas_size = size.round() as u32;
            o.atlas_padding = padding.round() as u32;
            ui.add_space(8.0);
            if button(ui, &t("settings-reset"), Variant::Secondary).clicked() {
                let d = Options::default();
                (o.atlas, o.atlas_size, o.atlas_padding) = (d.atlas, d.atlas_size, d.atlas_padding);
            }
        });
        if let Some(dir) = self.config_dir.clone() {
            ui.add_space(8.0);
            if button(ui, &t("settings-open-folder"), Variant::Secondary).clicked()
                && let Err(e) = settings::reveal(&dir)
            {
                self.toasts.push(ctx, Tone::Error, e);
            }
        }
    }
}

/// Whether `elapsed` since the last auto-save is at least the interval.
/// `text` cut to `keep` characters, the cut marked.
fn shorten(text: &str, keep: usize) -> String {
    if text.chars().count() <= keep {
        return text.to_owned();
    }
    let head: String = text.chars().take(keep).collect();
    format!("{head}…")
}

fn autosave_due(elapsed: std::time::Duration, minutes: u32) -> bool {
    elapsed >= std::time::Duration::from_secs(u64::from(minutes) * 60)
}

/// A drop-down with its name on the left: `select` keeps the name for screen
/// readers only, so alone it shows just the current value ("Dark").
fn labeled_select<T: PartialEq + Clone>(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    name: &str,
    value: &mut T,
    options: &[(T, &str)],
) -> egui::Response {
    form_row(ui, name, |ui| {
        tsukumoe_ui::select(ui, id, name, value, options)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn an_autosave_is_due_once_the_interval_has_passed() {
        assert!(!autosave_due(Duration::from_secs(299), 5));
        assert!(autosave_due(Duration::from_secs(300), 5));
        assert!(autosave_due(Duration::from_secs(3600), 1));
    }
}
