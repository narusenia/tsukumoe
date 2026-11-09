// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The command palette: a box that opens at the pointer, searches every editor
//! action by its name (with its group and shortcut) and the raw edit commands
//! by their schema, and hands back what the user picks. A raw command asks for
//! its arguments in a form generated from the schema.

use egui::{Align2, Area, Context, Id, Key, Modifiers, Order, Pos2, Rect, Ui, pos2};
use tsukumoe_core::command::Command;
use tsukumoe_ui::{
    Icon, Keymap, Variant, button, checkbox, form_row, hint, selectable_row, text_input, tr,
};

use crate::actions::{ACTIONS, icon_of, label_key};
use crate::commands::{CommandInfo, FieldKind, build};

const WIDTH: f32 = 440.0;
const VISIBLE_ROWS: f32 = 10.0;
/// How many used actions are remembered for the empty search.
const RECENT: usize = 5;

/// What the user picked.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Action(String),
    Command(Command),
}

enum Kind {
    Action(&'static str),
    Raw(usize),
}

struct Item {
    kind: Kind,
    label: String,
    group: String,
    shortcut: Option<String>,
    icon: Option<Icon>,
    /// Lower case text the search looks in.
    haystack: String,
}

pub struct Palette {
    raw: Vec<CommandInfo>,
    open: bool,
    anchor: Pos2,
    pub query: String,
    cursor: usize,
    /// The raw command whose form is shown, with the field texts.
    form: Option<(usize, Vec<String>)>,
    pub error: Option<String>,
    recent: Vec<&'static str>,
    focus: bool,
    moved: bool,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            raw: crate::commands::all(),
            open: false,
            anchor: Pos2::ZERO,
            query: String::new(),
            cursor: 0,
            form: None,
            error: None,
            recent: Vec::new(),
            focus: false,
            moved: false,
        }
    }
}

impl Palette {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the box with its corner at `at`, empty.
    pub fn open_at(&mut self, at: Pos2) {
        self.open = true;
        self.anchor = at;
        self.query.clear();
        self.cursor = 0;
        self.form = None;
        self.error = None;
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.form = None;
    }

    pub fn commands(&self) -> &[CommandInfo] {
        &self.raw
    }

    /// Shows the form of the raw command `name`.
    pub fn select(&mut self, name: &str) {
        self.form = self
            .raw
            .iter()
            .position(|c| c.name == name)
            .map(|i| (i, vec![String::new(); self.raw[i].fields.len()]));
        self.error = None;
    }

    pub fn selected(&self) -> Option<&CommandInfo> {
        self.form.as_ref().map(|(i, _)| &self.raw[*i])
    }

    /// Builds the command from the form; the error is also kept for display.
    pub fn build(&mut self) -> Option<Command> {
        let (i, inputs) = self.form.as_ref()?;
        match build(&self.raw[*i], inputs) {
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

    fn items(&self, ctx: &Context, keymap: &Keymap) -> Vec<Item> {
        let group = |prefix: &str| {
            let key = format!("palette-group-{prefix}");
            let text = tr(ctx, &key);
            if text == key { prefix.to_owned() } else { text }
        };
        let mut items: Vec<Item> = ACTIONS
            .iter()
            .filter(|a| **a != "view.palette")
            .map(|&action| {
                let label = tr(ctx, &label_key(action));
                let group = group(action.split('.').next().unwrap_or(action));
                Item {
                    haystack: format!("{label} {group} {action}").to_lowercase(),
                    kind: Kind::Action(action),
                    label,
                    group,
                    shortcut: keymap.shortcut_text(ctx, action),
                    icon: icon_of(action),
                }
            })
            .collect();
        let raw_group = group("command");
        items.extend(self.raw.iter().enumerate().map(|(i, c)| Item {
            haystack:
                format!("{} {} {}", c.name, c.name.replace('_', " "), c.description).to_lowercase(),
            kind: Kind::Raw(i),
            label: c.name.replace('_', " "),
            group: raw_group.clone(),
            shortcut: None,
            icon: None,
        }));
        items
    }

    /// The items that match every word of the query. Without a query the used
    /// actions come first. A name that starts with a word ranks above one that
    /// merely contains it.
    fn matches<'a>(&self, items: &'a [Item]) -> Vec<&'a Item> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut found: Vec<(usize, &Item)> = items
            .iter()
            .filter(|it| words.iter().all(|w| it.haystack.contains(w)))
            .map(|it| {
                let label = it.label.to_lowercase();
                let rank = if words.is_empty() {
                    match it.kind {
                        Kind::Action(a) => self
                            .recent
                            .iter()
                            .position(|r| *r == a)
                            .unwrap_or(RECENT + 1),
                        Kind::Raw(_) => RECENT + 2,
                    }
                } else if matches!(it.kind, Kind::Raw(_)) {
                    // The raw commands follow every action.
                    10
                } else if words.iter().all(|w| label.starts_with(w)) {
                    0
                } else if words.iter().all(|w| label.contains(w)) {
                    1
                } else {
                    2
                };
                (rank, it)
            })
            .collect();
        found.sort_by_key(|(rank, _)| *rank);
        found.into_iter().map(|(_, it)| it).collect()
    }

    /// Draws the box while it is open; returns what the user picked.
    pub fn show(&mut self, ctx: &Context, keymap: &Keymap) -> Option<Out> {
        if !self.open {
            return None;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            if self.form.is_some() {
                self.form = None;
                ctx.request_repaint();
            } else {
                self.close();
            }
            return None;
        }
        let screen = ctx.content_rect();
        let height = 52.0 + VISIBLE_ROWS * 28.0;
        let pos = pos2(
            self.anchor
                .x
                .clamp(screen.left() + 8.0, (screen.right() - WIDTH - 8.0).max(8.0)),
            self.anchor.y.clamp(
                screen.top() + 8.0,
                (screen.bottom() - height - 8.0).max(8.0),
            ),
        );
        let items = self.items(ctx, keymap);
        let found = self.matches(&items);
        let last = found.len().saturating_sub(1);
        let down = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
        let up = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp));
        let enter =
            self.form.is_none() && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        if down {
            self.cursor = (self.cursor + 1).min(last);
            self.moved = true;
        }
        if up {
            self.cursor = self.cursor.saturating_sub(1);
            self.moved = true;
        }
        self.cursor = self.cursor.min(last);
        let mut picked: Option<usize> = enter.then_some(self.cursor);
        let mut out = None;
        let theme = tsukumoe_ui::Theme::get(ctx);
        let area = Area::new(Id::unique("tsukumoe-palette"))
            .fixed_pos(pos)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme.colors.surface_raised)
                    .stroke(egui::Stroke::new(1.0, theme.colors.border_strong))
                    .corner_radius(theme.metrics.radius + 2.0)
                    .inner_margin(egui::Margin::same(theme.metrics.pad as i8))
                    .show(ui, |ui| {
                        ui.set_width(WIDTH);
                        if self.form.is_some() {
                            out = self.form_ui(ui);
                        } else {
                            self.search_ui(ui, &found, &mut picked);
                        }
                    });
            });
        if let Some(i) = picked
            && let Some(item) = found.get(i)
        {
            match item.kind {
                Kind::Action(a) => {
                    self.recent.retain(|r| *r != a);
                    self.recent.insert(0, a);
                    self.recent.truncate(RECENT);
                    self.close();
                    return Some(Out::Action(a.to_owned()));
                }
                Kind::Raw(index) => {
                    let name = self.raw[index].name.clone();
                    self.select(&name);
                    self.focus = true;
                    ctx.request_repaint();
                }
            }
        }
        // A press outside the box closes it.
        if ctx.input(|i| i.pointer.any_pressed())
            && let Some(p) = ctx.pointer_interact_pos()
            && !area.response.rect.contains(p)
        {
            self.close();
        }
        if out.is_some() {
            self.close();
        }
        out
    }

    fn search_ui(&mut self, ui: &mut Ui, found: &[&Item], picked: &mut Option<usize>) {
        let ctx = ui.ctx().clone();
        let name = tr(&ctx, "palette-search");
        let before = self.query.clone();
        let field = text_input(ui, &mut self.query, &name, &name);
        if std::mem::take(&mut self.focus) {
            field.request_focus();
        }
        if self.query != before {
            self.cursor = 0;
            ui.ctx().request_repaint();
        }
        ui.add_space(4.0);
        let moved = std::mem::take(&mut self.moved);
        egui::ScrollArea::vertical()
            .id_salt("palette-list")
            // A fixed height, so the box does not shrink with a narrowed list and
            // lag behind when the list grows back.
            .min_scrolled_height(VISIBLE_ROWS * 28.0)
            .max_height(VISIBLE_ROWS * 28.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if found.is_empty() {
                    hint(ui, tr(&ctx, "palette-none"));
                }
                for (i, item) in found.iter().enumerate() {
                    let response = selectable_row(
                        ui,
                        i == self.cursor,
                        // Every row takes an icon's room, so the names line up.
                        Some(item.icon.unwrap_or(Icon::ChevronRight)),
                        &item.label,
                    );
                    paint_tail(ui, &response.rect, item);
                    if i == self.cursor && moved {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        *picked = Some(i);
                    }
                    if response.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        self.cursor = i;
                    }
                }
            });
    }

    fn form_ui(&mut self, ui: &mut Ui) -> Option<Out> {
        let ctx = ui.ctx().clone();
        let (index, mut inputs) = self.form.take()?;
        let info = self.raw[index].clone();
        let mut run = false;
        let mut back = false;
        ui.horizontal(|ui| {
            if tsukumoe_ui::icon_button(
                ui,
                Icon::ChevronLeft,
                &tr(&ctx, "palette-back"),
                Variant::Ghost,
            )
            .clicked()
            {
                back = true;
            }
            ui.label(egui::RichText::new(info.name.replace('_', " ")).strong());
        });
        hint(ui, info.description.clone());
        ui.add_space(4.0);
        for (field, text) in info.fields.iter().zip(inputs.iter_mut()) {
            let name = format!("{}{}", field.name, if field.required { " *" } else { "" });
            form_row(ui, &name, |ui| {
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
        if let Some(e) = &self.error {
            hint(ui, e.clone());
        }
        ui.add_space(4.0);
        if button(ui, &tr(&ctx, "palette-run"), Variant::Primary).clicked() {
            run = true;
        }
        if !back {
            self.form = Some((index, inputs));
        }
        if run {
            self.build().map(Out::Command)
        } else {
            None
        }
    }
}

/// The group name and the shortcut at the right end of a row.
fn paint_tail(ui: &Ui, rect: &Rect, item: &Item) {
    let theme = tsukumoe_ui::Theme::get(ui.ctx());
    let font = egui::FontId::proportional(theme.metrics.text - 1.0);
    let color = theme.colors.text_muted;
    let mut right = rect.right() - theme.metrics.pad;
    if let Some(shortcut) = &item.shortcut {
        let r = ui.painter().text(
            pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            shortcut,
            font.clone(),
            color,
        );
        right = r.left() - theme.metrics.pad;
    }
    ui.painter().text(
        pos2(right, rect.center().y),
        Align2::RIGHT_CENTER,
        &item.group,
        font,
        color.gamma_multiply(0.7),
    );
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
        p.form.as_mut().unwrap().1[0] = "Arm".into();
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

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn harness() -> Harness<'static, (Palette, Keymap, Vec<Out>)> {
        let mut palette = Palette::default();
        palette.open_at(pos2(40.0, 40.0));
        Harness::builder()
            .with_size(egui::vec2(700.0, 500.0))
            .build_ui_state(
                |ui, (palette, keymap, outs): &mut (Palette, Keymap, Vec<Out>)| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    outs.extend(palette.show(ui.ctx(), keymap));
                },
                (palette, crate::actions::cubism_compatible(), Vec::new()),
            )
    }

    #[test]
    fn typing_narrows_the_list_by_name_and_enter_runs_the_top_action() {
        let mut h = harness();
        h.run();
        h.get_by_label("Undo");
        h.get_by_label("Search commands").type_text("undo");
        h.run();
        assert!(h.query_by_label("Redo").is_none());
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(h.state().2, vec![Out::Action("edit.undo".to_owned())]);
        assert!(!h.state().0.is_open());
    }

    #[test]
    fn escape_closes_the_box_and_a_raw_command_opens_its_form() {
        let mut h = harness();
        h.run();
        h.get_by_label("Search commands").type_text("add part");
        h.run();
        h.get_by_label("add part").click();
        h.run();
        assert_eq!(h.state().0.selected().unwrap().name, "add_part");
        h.key_press(Key::Escape);
        h.run();
        assert!(h.state().0.selected().is_none() && h.state().0.is_open());
        h.key_press(Key::Escape);
        h.run();
        assert!(!h.state().0.is_open());
    }
}
