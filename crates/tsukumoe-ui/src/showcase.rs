// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The gallery: every component in every state. The `gallery` example shows it
//! in a window; the snapshot tests render each [`Section`] in each theme.

use egui::{self, Color32, ScrollArea, Ui};

use crate::{
    Density, Icon, Mode, Theme, Toasts, Tone, Variant, badge, button, checkbox, color_picker,
    dialog, hint, icon, icon_button, icon_text_button, label, list, menu_item, number, panel,
    progress, radio, section_header, select, selectable_row, set_density, set_mode, slider, switch,
    tabs, text_input, tip,
};

/// A group of components in the gallery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Theme,
    Buttons,
    Toggles,
    Fields,
    Feedback,
    Structure,
    Icons,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::Theme,
        Section::Buttons,
        Section::Toggles,
        Section::Fields,
        Section::Feedback,
        Section::Structure,
        Section::Icons,
    ];
}

#[derive(PartialEq, Clone, Copy)]
enum Size {
    Small,
    Large,
}

pub struct Showcase {
    mode: Mode,
    density: Density,
    on: bool,
    checked: bool,
    radio: u8,
    value: f32,
    number: f32,
    text: String,
    size: Size,
    color: Color32,
    tab: usize,
    selected: usize,
    dialog_open: bool,
    toasts: Toasts,
}

fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    section_header(ui, title);
    panel(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
    ui.add_space(Theme::get(ui.ctx()).metrics.pad);
}

fn states(ui: &mut Ui, add: impl Fn(&mut Ui, bool)) {
    ui.horizontal(|ui| {
        add(ui, true);
        ui.add_enabled_ui(false, |ui| add(ui, false));
    });
}

impl Default for Showcase {
    fn default() -> Self {
        Self::new()
    }
}

impl Showcase {
    pub fn new() -> Self {
        Self {
            mode: Mode::Dark,
            density: Density::Compact,
            on: true,
            checked: true,
            radio: 0,
            value: 0.3,
            number: 0.6,
            text: String::new(),
            size: Size::Small,
            color: Color32::from_rgb(0x5b, 0x7c, 0xfa),
            tab: 0,
            selected: 2,
            dialog_open: false,
            toasts: Toasts::default(),
        }
    }

    /// Draws one section.
    pub fn show(&mut self, ui: &mut Ui, section: Section) {
        let ctx = ui.ctx().clone();
        match section {
            Section::Theme => self.theme_section(ui, &ctx),
            Section::Buttons => self.buttons(ui, &ctx),
            Section::Toggles => self.toggles(ui),
            Section::Fields => self.fields(ui),
            Section::Feedback => self.feedback(ui, &ctx),
            Section::Structure => self.structure(ui),
            Section::Icons => self.icons(ui),
        }
    }

    /// Draws every section in a scrolling column.
    pub fn show_all(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for section in Section::ALL {
                self.show(ui, section);
            }
        });
    }

    /// Draws the dialog and the toasts; call once per frame, after the rest.
    pub fn overlays(&mut self, ctx: &egui::Context) {
        dialog(ctx, "demo", "Dialog", &mut self.dialog_open, |ui| {
            label(ui, "Escape, the close button or a click outside closes it.");
        });
        self.toasts.show(ctx);
    }

    fn theme_section(&mut self, ui: &mut Ui, ctx: &egui::Context) {
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
                        set_mode(ctx, mode);
                    }
                }
                ui.separator();
                for (density, name) in
                    [(Density::Compact, "Compact"), (Density::Regular, "Regular")]
                {
                    let variant = if self.density == density {
                        Variant::Primary
                    } else {
                        Variant::Secondary
                    };
                    if button(ui, name, variant).clicked() {
                        self.density = density;
                        set_density(ctx, density);
                    }
                }
            });
        });
    }

    fn buttons(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        section(ui, "Button", |ui| {
            for variant in [
                Variant::Primary,
                Variant::Secondary,
                Variant::Ghost,
                Variant::Danger,
            ] {
                states(ui, |ui, _| {
                    let name = format!("{variant:?}");
                    button(ui, &name, variant);
                    icon_text_button(ui, Icon::Plus, &name, variant);
                    icon_button(ui, Icon::Settings, "Settings", variant);
                });
            }
            ui.horizontal(|ui| {
                if button(ui, "Open dialog", Variant::Secondary).clicked() {
                    self.dialog_open = true;
                }
                for (tone, text) in [
                    (Tone::Success, "Saved"),
                    (Tone::Warning, "Check this"),
                    (Tone::Error, "Failed"),
                ] {
                    if button(ui, &format!("Toast: {text}"), Variant::Ghost).clicked() {
                        self.toasts.push(ctx, tone, text);
                    }
                }
            });
        });
    }

    fn toggles(&mut self, ui: &mut Ui) {
        section(ui, "Switch, checkbox, radio", |ui| {
            states(ui, |ui, _| {
                let mut on = self.on;
                switch(ui, &mut on, "Switch");
                let mut checked = self.checked;
                checkbox(ui, &mut checked, "Checkbox");
                radio(ui, true, "Radio");
            });
            ui.horizontal(|ui| {
                switch(ui, &mut self.on, "Visible");
                label(ui, if self.on { "On" } else { "Off" });
                checkbox(ui, &mut self.checked, "Checkbox");
                for (n, name) in [(0, "One"), (1, "Two"), (2, "Three")] {
                    if radio(ui, self.radio == n, name).clicked() {
                        self.radio = n;
                    }
                }
            });
        });
    }

    fn fields(&mut self, ui: &mut Ui) {
        section(ui, "Slider, number, text, select, color", |ui| {
            ui.horizontal(|ui| {
                ui.allocate_ui(egui::vec2(260.0, 24.0), |ui| {
                    slider(ui, &mut self.value, 0.0..=1.0, "Value");
                });
                number(ui, &mut self.value, 0.0..=1.0, 0.005, "Value");
                ui.add_enabled_ui(false, |ui| {
                    number(ui, &mut self.number, 0.0..=1.0, 0.005, "Disabled value");
                });
            });
            ui.horizontal(|ui| {
                ui.allocate_ui(egui::vec2(260.0, 24.0), |ui| {
                    let hint = if crate::has_cjk_font(ui.ctx()) {
                        "ParamAngleX / 日本語も入力できる"
                    } else {
                        "ParamAngleX"
                    };
                    text_input(ui, &mut self.text, "Name", hint);
                });
                select(
                    ui,
                    "size",
                    "Size",
                    &mut self.size,
                    &[(Size::Small, "Small"), (Size::Large, "Large")],
                );
                color_picker(ui, &mut self.color, "Color");
            });
            ui.add_enabled_ui(false, |ui| {
                ui.allocate_ui(egui::vec2(260.0, 24.0), |ui| {
                    text_input(ui, &mut String::from("Disabled"), "Disabled", "");
                });
            });
        });
    }

    fn feedback(&mut self, ui: &mut Ui, _ctx: &egui::Context) {
        section(ui, "Badge, progress, tooltip", |ui| {
            ui.horizontal(|ui| {
                for (tone, name) in [
                    (Tone::Neutral, "Neutral"),
                    (Tone::Accent, "Accent"),
                    (Tone::Success, "Success"),
                    (Tone::Warning, "Warning"),
                    (Tone::Error, "Error"),
                ] {
                    badge(ui, name, tone);
                }
            });
            progress(ui, self.value);
            tip(
                hint(ui, "Hover this line for a tooltip"),
                "Tooltips use the raised surface",
            );
        });
    }

    fn structure(&mut self, ui: &mut Ui) {
        section(ui, "Tabs, list, menu", |ui| {
            tabs(ui, &mut self.tab, &["Parameters", "Parts", "Physics"]);
            ui.separator();
            ui.allocate_ui(egui::vec2(ui.available_width(), 120.0), |ui| {
                list(ui, "rows", 40, |ui, i| {
                    let icon = if i % 3 == 0 { Icon::Folder } else { Icon::File };
                    if selectable_row(ui, self.selected == i, Some(icon), &format!("ParamHead{i}"))
                        .clicked()
                    {
                        self.selected = i;
                    }
                });
            });
            ui.separator();
            ui.menu_button("Menu", |ui| {
                menu_item(ui, "Undo", Some("⌘Z"));
                menu_item(ui, "Redo", Some("⇧⌘Z"));
                ui.separator();
                menu_item(ui, "Delete", None);
            });
        });
    }

    fn icons(&mut self, ui: &mut Ui) {
        let t = Theme::get(ui.ctx());
        section(ui, "Icons", |ui| {
            ui.horizontal_wrapped(|ui| {
                for i in Icon::ALL {
                    let r = icon(ui, *i, t.colors.text);
                    tip(r, i.name());
                }
            });
        });
    }
}
