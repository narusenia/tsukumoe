// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The settings dialog (M5-02f): one window with a tab for each group of
//! settings. The tabs draw what used to be separate dialogs (drag, pie menu,
//! deletion confirmations, shortcuts) and the look of the editor (language,
//! theme, density, tab style). A change is applied and saved at once.

use crate::options::{self, Background, Options};
use egui::ScrollArea;
use tsukumoe_ui::{Density, Language, Mode, Preferences, builtin_themes, section_header};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Controls,
    Shortcuts,
    View,
    Edit,
    Files,
}

impl SettingsTab {
    const ALL: [Self; 6] = [
        Self::General,
        Self::Controls,
        Self::Shortcuts,
        Self::View,
        Self::Edit,
        Self::Files,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::General => "settings-tab-general",
            Self::Controls => "settings-tab-controls",
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
}

const WIDTH: f32 = 680.0;
const TAB_COLUMN: f32 = 140.0;
const BODY_HEIGHT: f32 = 460.0;

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
        })));
    }

    pub(super) fn settings_dialog(
        &mut self,
        ctx: &Context,
        state: &mut SettingsState,
        open: &mut bool,
    ) {
        let t = |key: &str| tr(ctx, key);
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
                        ScrollArea::vertical()
                            .max_height(BODY_HEIGHT)
                            .auto_shrink([false, true])
                            .show(ui, |ui| match state.tab {
                                SettingsTab::General => self.general_contents(ctx, ui),
                                SettingsTab::Controls => {
                                    section_header(ui, &t("drag-title"));
                                    self.drag_contents(ctx, ui);
                                    ui.add_space(12.0);
                                    section_header(ui, &t("pie-title"));
                                    self.pie_contents(ctx, ui);
                                    ui.add_space(12.0);
                                    section_header(ui, &t("confirm-settings-title"));
                                    self.confirm_contents(ctx, ui);
                                }
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
        let theme_changed = tsukumoe_ui::select(
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
        tsukumoe_ui::select(
            ui,
            "settings-density",
            &t("host-density"),
            &mut self.prefs.density,
            &density_options,
        );

        let auto = t("language-auto");
        let mut languages = vec![(None, auto.as_str())];
        languages.extend(Language::ALL.map(|l| (Some(l), l.native_name())));
        tsukumoe_ui::select(
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
        tsukumoe_ui::select(
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
        hint(ui, t("confirm-settings-hint"));
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

    fn pie_contents(&mut self, ctx: &Context, ui: &mut Ui) {
        let t = |key: &str| tr(ctx, key);
        let before = self.pie_config.clone();
        let names: Vec<(String, String)> = ACTIONS
            .iter()
            .map(|a| ((*a).to_owned(), t(&actions::label_key(a))))
            .collect();
        let options: Vec<(String, &str)> =
            names.iter().map(|(a, n)| (a.clone(), n.as_str())).collect();
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
                    Entry::Group { label: name, items } => {
                        label(ui, t(name));
                        ui.indent(id.clone(), |ui| slots(ui, items, options, &id, t));
                    }
                }
            }
        }
        slots(ui, &mut self.pie_config.items, &options, "pie", &t);
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
            if tsukumoe_ui::select(ui, key, &t("drag-parameter"), &mut choice, &options).changed() {
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
        hint(ui, t("keymap-hint"));
        for (action, text) in ACTIONS.iter().zip(texts.iter_mut()) {
            ui.horizontal(|ui| {
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
            tsukumoe_ui::select(
                ui,
                "settings-background",
                &t("settings-background"),
                &mut o.background,
                &choices,
            );
            hint(ui, t("settings-radius-hint"));
            for (key, value) in [
                ("settings-vertex-radius", &mut o.vertex_radius),
                ("settings-edge-radius", &mut o.edge_radius),
            ] {
                ui.horizontal(|ui| {
                    label(ui, t(key));
                    let range = options::RADIUS;
                    tsukumoe_ui::number(ui, value, range.0..=range.1, 0.25, &t(key));
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
            ui.horizontal(|ui| {
                label(ui, t("settings-history-limit"));
                tsukumoe_ui::number(
                    ui,
                    &mut limit,
                    0.0..=options::HISTORY_LIMIT.1 as f32,
                    1.0,
                    &t("settings-history-limit"),
                );
            });
            o.history_limit = limit.round() as u32;
            hint(ui, t("settings-threshold-hint"));
            for (key, value, range) in [
                (
                    "settings-weld-ratio",
                    &mut o.weld_ratio,
                    options::WELD_RATIO,
                ),
                (
                    "settings-mirror-tolerance",
                    &mut o.mirror_tolerance,
                    options::MIRROR_TOLERANCE,
                ),
            ] {
                ui.horizontal(|ui| {
                    label(ui, t(key));
                    tsukumoe_ui::number(ui, value, range.0..=range.1, 0.0005, &t(key));
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
            ui.horizontal(|ui| {
                label(ui, t("settings-autosave-minutes"));
                tsukumoe_ui::number(
                    ui,
                    &mut minutes,
                    options::AUTOSAVE_MINUTES.0 as f32..=options::AUTOSAVE_MINUTES.1 as f32,
                    1.0,
                    &t("settings-autosave-minutes"),
                );
            });
            o.autosave_minutes = minutes.round() as u32;
            ui.add_space(8.0);
            if button(ui, &t("settings-reset"), Variant::Secondary).clicked() {
                let d = Options::default();
                (o.autosave, o.autosave_minutes) = (d.autosave, d.autosave_minutes);
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
fn autosave_due(elapsed: std::time::Duration, minutes: u32) -> bool {
    elapsed >= std::time::Duration::from_secs(u64::from(minutes) * 60)
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
