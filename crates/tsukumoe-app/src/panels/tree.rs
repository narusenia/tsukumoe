// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The parts panel and the deformers panel: one drawing for both, over the
//! rows of [`crate::tree_model`]. Each row has a fixed column for the eye, then
//! the indent, a folder arrow, the icon and the name, so rows of one depth line
//! up whatever they are; thin guide lines run down the indent. Only the rows
//! in view are built (`show_rows`).

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use tsukumoe_core::command::Command;
use tsukumoe_core::model::Model;
use tsukumoe_ui::{Icon, Theme, Variant, button, hint, select, text_input, tr};

use crate::deformer_ops::Target;
use crate::tree_model::{
    self, Kind, Panel, Renaming, Row, Sort, accepted, range_selection, scroll_offset_for,
    scroll_row,
};
use crate::view::{Selection, ViewState};

/// The indent of one depth, in points.
pub const INDENT: f32 = 14.0;

/// What the panel asks for: a command to send, or an element to move under a
/// deformer (the editor converts the coordinates and sends one batch).
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Command(Command),
    Reparent(Target, Option<String>),
    /// Rename an element; an empty or unchanged name is dropped by the panel.
    Rename {
        kind: Kind,
        from: String,
        to: String,
    },
    /// Delete these (a part takes what is under it; the editor asks first).
    Delete(Vec<Selection>),
    /// Copy these.
    Duplicate(Vec<Selection>),
    /// Move the element to a part (a part: to a parent part, or none).
    SetPart(Selection, Option<String>),
}

/// The element a mesh or deformer row stands for.
pub fn target_of(row: &Row) -> Option<Target> {
    match row.kind {
        Kind::Mesh => Some(Target::Mesh(row.name.clone())),
        Kind::Warp | Kind::Rotation => Some(Target::Deformer(row.name.clone())),
        Kind::Part => None,
    }
}

/// The command an eye click on `row` sends: the opposite of its flag.
pub fn toggle_command(row: &Row) -> Option<Command> {
    let visible = !row.visible?;
    let name = row.name.clone();
    match row.kind {
        Kind::Part => Some(Command::SetPartVisible { name, visible }),
        Kind::Mesh => Some(Command::SetMeshVisible { name, visible }),
        Kind::Warp | Kind::Rotation => None,
    }
}

/// The x of every column of a row whose rectangle starts at `left`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Columns {
    /// The eye button's square.
    pub eye: Rect,
    /// The folder arrow's square.
    pub arrow: Rect,
    /// The kind icon's square.
    pub icon: Rect,
    /// Where the name starts.
    pub name_x: f32,
}

/// Lays the columns of a row of `depth` in `row`: the eye column has a fixed
/// width (empty for rows without an eye); the indent comes after it.
pub fn columns(row: Rect, depth: usize, control: f32, icon: f32, gap: f32) -> Columns {
    let eye = Rect::from_min_size(row.min, vec2(control, row.height()));
    let indent = row.left() + control + depth as f32 * INDENT;
    let arrow = Rect::from_min_size(pos2(indent, row.top()), vec2(icon + 2.0, row.height()));
    let icon_x = arrow.right() + 2.0;
    let icon_rect =
        Rect::from_center_size(pos2(icon_x + icon / 2.0, row.center().y), vec2(icon, icon));
    Columns {
        eye,
        arrow,
        icon: icon_rect,
        name_x: icon_rect.right() + gap + 2.0,
    }
}

/// The x of the guide line of indent level `level` (1 is the first indent).
pub fn guide_x(row: Rect, level: usize, control: f32, icon: f32) -> f32 {
    row.left() + control + (level - 1) as f32 * INDENT + (icon + 2.0) / 2.0
}

/// For each row, the indexes of the rows above it that hold it (its
/// ancestors, outermost first).
pub fn ancestors(rows: &[Row]) -> Vec<Vec<usize>> {
    let mut stack: Vec<usize> = Vec::new();
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            stack.truncate(r.depth);
            let out = stack.clone();
            stack.push(i);
            out
        })
        .collect()
}

fn tone(ui: &Ui, row: &Row, selected: bool) -> Color32 {
    let c = Theme::get(ui.ctx()).colors;
    if selected {
        c.text
    } else if row.dimmed {
        c.disabled
    } else {
        c.text_muted
    }
}

fn kind_icon(kind: Kind) -> Icon {
    match kind {
        Kind::Part => Icon::Folder,
        Kind::Mesh => Icon::Image,
        Kind::Warp | Kind::Rotation => Icon::Settings,
    }
}

/// Draws a panel. Returns what the user asked for, to be sent through the
/// editor's dispatcher.
pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState, panel: Panel) -> Vec<Out> {
    let ctx = ui.ctx().clone();
    let theme = Theme::get(&ctx);
    let mut outs = Vec::new();
    header(ui, model, view, panel);
    // A selection made elsewhere opens the folders above it, then scrolls to it.
    if let Some(sel) = view.selection.clone()
        && view.tree(panel).seen.as_ref() != Some(&sel)
    {
        view.tree_mut(panel).open_to(model, panel, &sel);
    }
    let rows = tree_model::rows(model, view.tree(panel), panel);
    let control = theme.metrics.control;
    let icon_size = theme.metrics.icon;
    let row_height = control + ui.spacing().item_spacing.y;
    let stride = row_height + ui.spacing().item_spacing.y;
    let held = ancestors(&rows);
    let primary_ancestors: Vec<usize> = view
        .selection
        .as_ref()
        .and_then(|s| rows.iter().position(|r| r.selection() == *s))
        .map(|i| held[i].clone())
        .unwrap_or_default();

    let lists = Lists {
        parts: model.parts().iter().map(|(_, n, _)| n.to_owned()).collect(),
        deformers: model
            .warps()
            .iter()
            .map(|(_, n, _)| n.to_owned())
            .chain(model.rotations().iter().map(|(_, n, _)| n.to_owned()))
            .collect(),
    };
    // Empty space below the rows clears the selection.
    let area = ui.available_rect_before_wrap();
    let background = ui.interact(area, ui.next_auto_id(), Sense::click());
    let panel_id = background.id;
    if background.clicked() {
        view.clear_selection();
        ui.memory_mut(|m| m.request_focus(panel_id));
    }
    // The keys work while the panel has the keyboard (and no name is being typed).
    let focused = ui.memory(|m| m.has_focus(panel_id));
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                panel_id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    tab: false,
                    escape: false,
                },
            );
        });
        if view.tree(panel).renaming.is_none() {
            for key in pressed_keys(ui) {
                outs.extend(apply_key(&rows, view, panel, key));
            }
        }
    }

    let mut scroll = egui::ScrollArea::vertical().auto_shrink(false);
    if let Some(index) = scroll_row(
        &rows,
        view.selection.as_ref(),
        view.tree(panel).seen.as_ref(),
    ) && let Some(offset) = scroll_offset_for(
        index,
        rows.len(),
        stride,
        view.tree(panel).scroll.0,
        view.tree(panel).scroll.1,
    ) {
        scroll = scroll.vertical_scroll_offset(offset);
    }
    let shown = scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
        for index in range {
            let row = &rows[index];
            let selection = row.selection();
            let selected = view.is_selected(&selection);
            let primary = view.selection.as_ref() == Some(&selection);
            let (rect, response) = ui
                .allocate_exact_size(vec2(ui.available_width(), control), Sense::click_and_drag());
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::Button, true, selected, &row.name)
            });
            let cols = columns(rect, row.depth, control, icon_size, theme.metrics.gap);
            let painter = ui.painter_at(rect);
            // The selection, stronger than a hover.
            if selected {
                let [r, g, b, _] = theme.colors.accent.to_array();
                let alpha = if theme.dark { 95 } else { 70 };
                painter.rect_filled(
                    rect,
                    theme.metrics.radius,
                    Color32::from_rgba_unmultiplied(r, g, b, alpha),
                );
                let bar = Rect::from_min_size(
                    rect.min,
                    vec2(if primary { 3.0 } else { 2.0 }, rect.height()),
                );
                painter.rect_filled(bar, 0.0, theme.colors.accent);
            } else if response.hovered() {
                painter.rect_filled(rect, theme.metrics.radius, theme.colors.control_hover);
            }
            // Guide lines, one per indent; the ones above the selected row are stronger.
            for level in 1..=row.depth {
                let x = guide_x(rect, level, control, icon_size);
                let holder = held[index].get(level - 1);
                let strong = holder.is_some_and(|h| primary_ancestors.contains(h));
                let color = if strong {
                    theme.colors.border_strong
                } else {
                    theme.colors.border
                };
                painter.line_segment(
                    [pos2(x, rect.top()), pos2(x, rect.bottom())],
                    Stroke::new(1.0, color),
                );
            }
            // The eye, in its own column (nothing in it for rows without one).
            if let Some(visible) = row.visible {
                let eye = ui.interact(cols.eye, response.id.with("eye"), Sense::click());
                let eye_label = tr(&ctx, if visible { "tree-hide" } else { "tree-show" });
                eye.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, eye_label.clone())
                });
                let icon = if visible { Icon::Eye } else { Icon::EyeOff };
                let color = if eye.hovered() {
                    theme.colors.text
                } else {
                    theme.colors.text_muted
                };
                icon.image(color, icon_size).paint_at(
                    ui,
                    Rect::from_center_size(cols.eye.center(), vec2(icon_size, icon_size)),
                );
                eye.clone()
                    .on_hover_text(tr(&ctx, if visible { "tree-hide" } else { "tree-show" }));
                if eye.clicked() {
                    outs.extend(toggle_command(row).map(Out::Command));
                }
            }
            // The folder arrow.
            if row.expandable {
                let arrow = ui.interact(cols.arrow, response.id.with("arrow"), Sense::click());
                let icon = if row.open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                };
                icon.image(theme.colors.text_muted, icon_size - 2.0)
                    .paint_at(
                        ui,
                        Rect::from_center_size(
                            cols.arrow.center(),
                            vec2(icon_size - 2.0, icon_size - 2.0),
                        ),
                    );
                if arrow.clicked() {
                    let key = row.key();
                    let open = !row.open;
                    view.tree_mut(panel).set_open(&key, open);
                }
            }
            kind_icon(row.kind)
                .image(tone(ui, row, selected), icon_size)
                .paint_at(ui, cols.icon);
            // The name, cut short before the right edge (and before the "..." button);
            // deformer rows also show their part. While renamed, a field takes its place.
            let more_rect = Rect::from_min_size(
                pos2(rect.right() - control, rect.top()),
                vec2(control, rect.height()),
            );
            let hovered = response.hovered() || ui.rect_contains_pointer(more_rect);
            let right = if hovered {
                more_rect.left()
            } else {
                rect.right() - 4.0
            };
            let name_clip =
                Rect::from_min_max(pos2(cols.name_x, rect.top()), pos2(right, rect.bottom()));
            let renaming_here = view
                .tree(panel)
                .renaming
                .as_ref()
                .is_some_and(|r| r.kind == row.kind && r.name == row.name);
            if renaming_here {
                let mut done = None;
                if let Some(r) = view.tree_mut(panel).renaming.as_mut() {
                    let edit = ui.put(
                        name_clip,
                        egui::TextEdit::singleline(&mut r.text).desired_width(name_clip.width()),
                    );
                    let edit = edit.accessible_name(tr(&ctx, "tree-rename"));
                    if !r.focused {
                        edit.request_focus();
                        r.focused = true;
                    }
                    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    if escape {
                        done = Some(false);
                    } else if edit.lost_focus() {
                        done = Some(true);
                    }
                }
                if let Some(commit) = done
                    && let Some(r) = view.tree_mut(panel).renaming.take()
                    && commit
                    && !r.text.trim().is_empty()
                    && r.text != r.name
                {
                    outs.push(Out::Rename {
                        kind: r.kind,
                        from: r.name,
                        to: r.text.trim().to_owned(),
                    });
                }
            } else {
                let font = FontId::proportional(theme.metrics.text);
                let name_color = tone(ui, row, selected);
                let galley =
                    ui.painter()
                        .layout_no_wrap(row.name.clone(), font.clone(), name_color);
                let name_width = galley.size().x;
                ui.painter_at(name_clip).galley(
                    pos2(cols.name_x, rect.center().y - galley.size().y / 2.0),
                    galley,
                    name_color,
                );
                if let Some(part) = &row.part {
                    let x = cols.name_x + name_width + 8.0;
                    if x < right - 20.0 {
                        ui.painter_at(name_clip).text(
                            pos2(x, rect.center().y),
                            Align2::LEFT_CENTER,
                            part,
                            FontId::proportional(theme.metrics.text - 1.0),
                            theme.colors.disabled,
                        );
                    }
                }
            }
            // The "..." button, with what the right-click menu has.
            if hovered && !renaming_here {
                let more = ui.interact(more_rect, response.id.with("more"), Sense::click());
                let color = if more.hovered() {
                    theme.colors.text
                } else {
                    theme.colors.text_muted
                };
                Icon::More.image(color, icon_size).paint_at(
                    ui,
                    Rect::from_center_size(more_rect.center(), vec2(icon_size, icon_size)),
                );
                more.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "..."));
                egui::Popup::menu(&more).show(|ui| {
                    row_menu(ui, model, view, panel, row, &lists, &mut outs);
                });
            }
            // Drag a row onto another to move it: onto a deformer row it hangs from
            // it, onto a part row it goes into that part (a part under it).
            response.dnd_set_drag_payload(selection.clone());
            let can_drop = |dragged: &Selection| match row.kind {
                Kind::Warp | Kind::Rotation => {
                    matches!(
                        dragged,
                        Selection::Mesh(_) | Selection::Warp(_) | Selection::Rotation(_)
                    ) && *dragged != selection
                }
                Kind::Part => !matches!(dragged, Selection::Parameter(_)) && *dragged != selection,
                Kind::Mesh => false,
            };
            if let Some(dragged) = response.dnd_hover_payload::<Selection>()
                && can_drop(&dragged)
            {
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    Stroke::new(1.5, theme.colors.accent),
                    egui::StrokeKind::Inside,
                );
            }
            if let Some(dragged) = response.dnd_release_payload::<Selection>()
                && can_drop(&dragged)
            {
                match row.kind {
                    Kind::Warp | Kind::Rotation => {
                        let target = match &*dragged {
                            Selection::Mesh(n) => Some(Target::Mesh(n.clone())),
                            Selection::Warp(n) | Selection::Rotation(n) => {
                                Some(Target::Deformer(n.clone()))
                            }
                            _ => None,
                        };
                        outs.extend(target.map(|t| Out::Reparent(t, Some(row.name.clone()))));
                    }
                    _ => outs.push(Out::SetPart((*dragged).clone(), Some(row.name.clone()))),
                }
            }
            response.context_menu(|ui| {
                row_menu(ui, model, view, panel, row, &lists, &mut outs);
            });
            if response.double_clicked() {
                activate(model, view, panel, row);
            }
            if response.clicked() {
                ui.memory_mut(|m| m.request_focus(panel_id));
                let modifiers = ui.input(|i| i.modifiers);
                let tool = view.tool;
                if modifiers.command {
                    if accepted(tool, row.kind) {
                        view.toggle(selection);
                    }
                } else if modifiers.shift {
                    let items = range_selection(&rows, view.anchor.as_ref(), &selection, tool);
                    view.select_all(items);
                } else {
                    view.select_only(selection);
                }
            }
        }
    });
    view.tree_mut(panel).scroll = (shown.state.clamped_offset().y, shown.inner_rect.height());
    view.tree_mut(panel).seen = view.selection.clone();
    outs
}

/// A key the panel handles while it has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKey {
    Up,
    Down,
    Left,
    Right,
    /// Enter or F2: change the name of the selected row.
    Rename,
    Delete,
    /// Ctrl / Cmd + D.
    Duplicate,
}

/// What a key does: it moves the selection, opens or closes a folder, starts
/// renaming, or asks to delete or duplicate the selection (the returned `Out`).
pub fn apply_key(rows: &[Row], view: &mut ViewState, panel: Panel, key: PanelKey) -> Option<Out> {
    let nav = match key {
        PanelKey::Up => Some(tree_model::NavKey::Up),
        PanelKey::Down => Some(tree_model::NavKey::Down),
        PanelKey::Left => Some(tree_model::NavKey::Left),
        PanelKey::Right => Some(tree_model::NavKey::Right),
        _ => None,
    };
    if let Some(nav) = nav {
        let from = view.selection.clone();
        if let Some(to) = tree_model::navigate(rows, from.as_ref(), nav, view.tree_mut(panel)) {
            view.select_only(to);
        }
        return None;
    }
    match key {
        PanelKey::Rename => {
            let row = view
                .selection
                .as_ref()
                .and_then(|s| rows.iter().find(|r| r.selection() == *s))?;
            view.tree_mut(panel).renaming = Some(Renaming::new(row.kind, &row.name));
            None
        }
        PanelKey::Delete => {
            let items = view.selected();
            (!items.is_empty()).then_some(Out::Delete(items))
        }
        PanelKey::Duplicate => {
            let items: Vec<Selection> = view
                .selected()
                .into_iter()
                .filter(|s| {
                    matches!(
                        s,
                        Selection::Mesh(_) | Selection::Warp(_) | Selection::Rotation(_)
                    )
                })
                .collect();
            (!items.is_empty()).then_some(Out::Duplicate(items))
        }
        _ => None,
    }
}

/// The keys pressed this frame that the panel handles.
fn pressed_keys(ui: &mut Ui) -> Vec<PanelKey> {
    use egui::{Key, Modifiers};
    let table = [
        (Modifiers::NONE, Key::ArrowUp, PanelKey::Up),
        (Modifiers::NONE, Key::ArrowDown, PanelKey::Down),
        (Modifiers::NONE, Key::ArrowLeft, PanelKey::Left),
        (Modifiers::NONE, Key::ArrowRight, PanelKey::Right),
        (Modifiers::NONE, Key::Enter, PanelKey::Rename),
        (Modifiers::NONE, Key::F2, PanelKey::Rename),
        (Modifiers::NONE, Key::Delete, PanelKey::Delete),
        (Modifiers::NONE, Key::Backspace, PanelKey::Delete),
        (Modifiers::COMMAND, Key::D, PanelKey::Duplicate),
    ];
    ui.input_mut(|i| {
        table
            .iter()
            .filter(|(m, k, _)| i.consume_key(*m, *k))
            .map(|(_, _, key)| *key)
            .collect()
    })
}

/// A double click on a row: a part selects the meshes under it, anything else
/// starts renaming.
pub fn activate(model: &Model, view: &mut ViewState, panel: Panel, row: &Row) {
    if row.kind == Kind::Part {
        let meshes = tree_model::meshes_under_part(model, &row.name);
        if !meshes.is_empty() {
            view.select_meshes(meshes);
        }
    } else {
        view.tree_mut(panel).renaming = Some(Renaming::new(row.kind, &row.name));
    }
}

/// The names the row menus offer as parents and parts.
struct Lists {
    parts: Vec<String>,
    deformers: Vec<String>,
}

/// What a row's menu (right click, or the "..." button) offers. It acts on
/// the whole selection when the row is part of it, else on the row alone.
fn row_menu(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    panel: Panel,
    row: &Row,
    lists: &Lists,
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let me = row.selection();
    let targets: Vec<Selection> = if view.is_selected(&me) {
        view.selected()
    } else {
        vec![me.clone()]
    };
    let target = target_of(row);
    if ui.button(tr(&ctx, "tree-rename")).clicked() {
        view.tree_mut(panel).renaming = Some(Renaming::new(row.kind, &row.name));
        ui.close();
    }
    if row.kind != Kind::Part && ui.button(tr(&ctx, "tree-duplicate")).clicked() {
        outs.push(Out::Duplicate(targets.clone()));
        ui.close();
    }
    if ui.button(tr(&ctx, "tree-delete")).clicked() {
        outs.push(Out::Delete(targets));
        ui.close();
    }
    ui.separator();
    if let Some(target) = target {
        ui.menu_button(tr(&ctx, "tree-set-parent"), |ui| {
            for parent in lists.deformers.iter().filter(|d| **d != row.name) {
                if ui.button(parent).clicked() {
                    outs.push(Out::Reparent(target.clone(), Some(parent.clone())));
                    ui.close();
                }
            }
        });
        let unparent = ui.add_enabled(
            row.parent.is_some(),
            egui::Button::new(tr(&ctx, "tree-unparent")),
        );
        if unparent.clicked() {
            outs.push(Out::Reparent(target.clone(), None));
            ui.close();
        }
    }
    let part_key = if row.kind == Kind::Part {
        "tree-set-parent-part"
    } else {
        "tree-set-part"
    };
    ui.menu_button(tr(&ctx, part_key), |ui| {
        if row.kind == Kind::Part && ui.button(tr(&ctx, "tree-no-part")).clicked() {
            outs.push(Out::SetPart(me.clone(), None));
            ui.close();
        }
        for part in lists.parts.iter().filter(|p| **p != row.name) {
            if ui.button(part).clicked() {
                outs.push(Out::SetPart(me.clone(), Some(part.clone())));
                ui.close();
            }
        }
    });
    if row.kind == Kind::Part && ui.button(tr(&ctx, "tree-select-children")).clicked() {
        let meshes = tree_model::meshes_under_part(model, &row.name);
        if !meshes.is_empty() {
            view.select_meshes(meshes);
        }
        ui.close();
    }
    if row.expandable {
        let key = row.key();
        if ui.button(tr(&ctx, "tree-open-children")).clicked() {
            view.tree_mut(panel)
                .set_subtree_open(model, panel, &key, true);
            ui.close();
        }
        if ui.button(tr(&ctx, "tree-close-children")).clicked() {
            view.tree_mut(panel)
                .set_subtree_open(model, panel, &key, false);
            ui.close();
        }
    }
}

/// The filter field, the kind toggles, the sort order and the folder buttons.
fn header(ui: &mut Ui, model: &Model, view: &mut ViewState, panel: Panel) {
    let ctx = ui.ctx().clone();
    let name = tr(&ctx, "filter-name");
    text_input(ui, &mut view.tree_mut(panel).filter, &name, &name);
    ui.horizontal_wrapped(|ui| {
        for &kind in panel.kinds() {
            let on = view.tree(panel).kinds.contains(&kind);
            let variant = if on {
                Variant::Primary
            } else {
                Variant::Secondary
            };
            if button(ui, &tr(&ctx, kind_key(kind)), variant).clicked() {
                view.tree_mut(panel).toggle_kind(kind);
            }
        }
        let on = view.tree(panel).hidden_only;
        let variant = if on {
            Variant::Primary
        } else {
            Variant::Secondary
        };
        if button(ui, &tr(&ctx, "tree-hidden-only"), variant).clicked() {
            view.tree_mut(panel).hidden_only = !on;
        }
    });
    ui.horizontal_wrapped(|ui| {
        let mut sort = view.tree(panel).sort;
        let options = [
            (Sort::Model, tr(&ctx, "tree-sort-model")),
            (Sort::Name, tr(&ctx, "tree-sort-name")),
            (Sort::Draw, tr(&ctx, "tree-sort-draw")),
        ];
        let options: Vec<(Sort, &str)> = options.iter().map(|(s, n)| (*s, n.as_str())).collect();
        if select(
            ui,
            ("tree-sort", panel as u8),
            &tr(&ctx, "tree-sort"),
            &mut sort,
            &options,
        )
        .changed()
        {
            view.tree_mut(panel).sort = sort;
        }
        if button(ui, &tr(&ctx, "tree-open-all"), Variant::Ghost).clicked() {
            view.tree_mut(panel).open_all();
        }
        if button(ui, &tr(&ctx, "tree-close-all"), Variant::Ghost).clicked() {
            view.tree_mut(panel).close_all(model, panel);
        }
        if button(ui, &tr(&ctx, "tree-open-selection"), Variant::Ghost).clicked()
            && let Some(sel) = view.selection.clone()
        {
            view.tree_mut(panel).open_to(model, panel, &sel);
        }
    });
    let count = view.selected_count();
    if count > 1 {
        let text = tsukumoe_ui::tr_args(
            &ctx,
            "tree-selected-count",
            &[("count", tsukumoe_ui::Arg::from(count))],
        );
        hint(ui, text);
    }
}

fn kind_key(kind: Kind) -> &'static str {
    match kind {
        Kind::Part => "tree-kind-part",
        Kind::Mesh => "tree-kind-mesh",
        Kind::Warp => "tree-kind-warp",
        Kind::Rotation => "tree-kind-rotation",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Selection;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use tsukumoe_core::command::RotationBase;

    fn mesh(name: &str, part: &str) -> Command {
        let tri = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        Command::AddMesh {
            name: name.into(),
            part: part.into(),
            positions: tri.clone(),
            uvs: tri,
            indices: vec![0, 1, 2],
        }
    }

    fn model() -> Model {
        let mut m = Model::new();
        let add = |m: &mut Model, name: &str, parent: Option<&str>| {
            m.apply(Command::AddPart {
                name: name.into(),
                parent: parent.map(str::to_owned),
            })
            .unwrap();
        };
        add(&mut m, "Body", None);
        add(&mut m, "Head", Some("Body"));
        add(&mut m, "Hair", Some("Head"));
        add(&mut m, "Arm", Some("Body"));
        m.apply(mesh("HairFront", "Hair")).unwrap();
        m
    }

    fn row(m: &Model, name: &str) -> Row {
        tree_model::rows(m, &Default::default(), Panel::Parts)
            .into_iter()
            .find(|r| r.name == name)
            .unwrap()
    }

    #[test]
    fn rows_of_one_depth_line_up_whatever_they_are() {
        let area = Rect::from_min_size(pos2(10.0, 100.0), vec2(300.0, 24.0));
        let (control, icon, gap) = (24.0, 14.0, 6.0);
        for depth in 0..4 {
            let a = columns(area, depth, control, icon, gap);
            // The same for a row with an eye and a row without one.
            let b = columns(area, depth, control, icon, gap);
            assert_eq!((a.icon, a.name_x), (b.icon, b.name_x));
            // The eye column is fixed and the indent starts after it.
            assert_eq!(a.eye.left(), area.left());
            assert!(a.arrow.left() >= area.left() + control);
            assert!(
                (a.arrow.left() - (area.left() + control + depth as f32 * INDENT)).abs() < 1e-4
            );
            assert!(a.icon.left() > a.arrow.right() && a.name_x > a.icon.right());
        }
        // One level deeper moves icon and name right by exactly one indent.
        let (d1, d2) = (
            columns(area, 1, control, icon, gap),
            columns(area, 2, control, icon, gap),
        );
        assert!((d2.name_x - d1.name_x - INDENT).abs() < 1e-4);
        assert!((d2.icon.left() - d1.icon.left() - INDENT).abs() < 1e-4);
    }

    #[test]
    fn a_guide_line_runs_under_the_arrow_of_the_folder_it_belongs_to() {
        let area = Rect::from_min_size(pos2(10.0, 100.0), vec2(300.0, 24.0));
        let (control, icon, gap) = (24.0, 14.0, 6.0);
        for level in 1..5 {
            let folder = columns(area, level - 1, control, icon, gap);
            assert!((guide_x(area, level, control, icon) - folder.arrow.center().x).abs() < 1e-4);
        }
    }

    #[test]
    fn every_row_knows_the_rows_that_hold_it() {
        let m = model();
        let rows = tree_model::rows(&m, &Default::default(), Panel::Parts);
        let held = ancestors(&rows);
        let at = |n: &str| rows.iter().position(|r| r.name == n).unwrap();
        assert!(held[at("Body")].is_empty());
        assert_eq!(held[at("HairFront")], [at("Body"), at("Head"), at("Hair")]);
        assert_eq!(held[at("Arm")], [at("Body")]);
    }

    #[test]
    fn the_eye_sends_the_opposite_flag_and_undo_restores_it() {
        let mut d = crate::dispatcher::Dispatcher::new(model());
        let cmd = toggle_command(&row(d.model(), "HairFront")).unwrap();
        assert_eq!(
            cmd,
            Command::SetMeshVisible {
                name: "HairFront".into(),
                visible: false
            }
        );
        d.submit(cmd).unwrap();
        assert!(d.dirty());
        assert_eq!(row(d.model(), "HairFront").visible, Some(false));
        let cmd = toggle_command(&row(d.model(), "Hair")).unwrap();
        assert!(matches!(
            cmd,
            Command::SetPartVisible { visible: false, .. }
        ));
        d.submit(cmd).unwrap();
        assert!(d.undo() && d.undo());
        assert!(!d.dirty());
        assert_eq!(
            toggle_command(&Row {
                visible: None,
                ..row(d.model(), "Hair")
            }),
            None
        );
    }

    fn harness(m: Model, panel: Panel) -> Harness<'static, (Model, ViewState)> {
        Harness::builder()
            .with_size(egui::vec2(320.0, 400.0))
            .build_ui_state(
                move |ui, state: &mut (Model, ViewState)| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    show(ui, &state.0, &mut state.1, panel);
                },
                (m, ViewState::default()),
            )
    }

    fn many() -> Model {
        let mut m = model();
        for n in ["a1", "a2", "b1", "a3", "b2"] {
            m.apply(mesh(n, "Arm")).unwrap();
        }
        m
    }

    #[test]
    fn ctrl_and_shift_clicks_select_several_rows_in_the_drawn_panel() {
        let mut h = harness(many(), Panel::Parts);
        h.run();
        h.get_by_label("a1").click();
        h.run();
        h.get_by_label("b1")
            .click_modifiers(egui::Modifiers::COMMAND);
        h.run();
        assert_eq!(h.state().1.selected_meshes(), ["a1", "b1"]);
        h.get_by_label("a3").click_modifiers(egui::Modifiers::SHIFT);
        h.run();
        // The anchor is the last plain or Ctrl click: b1.
        assert_eq!(h.state().1.selected_meshes(), ["b1", "a3"]);
        h.get_by_label("a2").click();
        h.run();
        assert_eq!(h.state().1.selected_meshes(), ["a2"]);
        // The header shows how many are selected.
        h.get_by_label("a1")
            .click_modifiers(egui::Modifiers::COMMAND);
        h.run();
        h.get_by_label("2 selected");
    }

    #[test]
    fn a_closed_folder_hides_its_rows_and_a_filter_shows_them_again() {
        let mut h = harness(many(), Panel::Parts);
        h.run();
        h.get_by_label("a2");
        h.state_mut().1.parts.set_open("p:Arm", false);
        h.run();
        assert!(h.query_by_label("a2").is_none());
        h.state_mut().1.parts.filter = "a2".into();
        h.run();
        h.get_by_label("a2");
        h.state_mut().1.parts.filter.clear();
        h.run();
        assert!(
            h.query_by_label("a2").is_none(),
            "the folder is closed again"
        );
    }

    #[test]
    fn a_click_in_empty_space_clears_the_selection() {
        let mut h = harness(model(), Panel::Parts);
        h.run();
        h.get_by_label("Arm").click();
        h.run();
        assert!(h.state().1.selection.is_some());
        h.state_mut().1.clear_selection();
        assert!(h.state().1.selection.is_none());
    }

    #[test]
    fn the_deformers_panel_draws_the_hierarchy() {
        let mut m = model();
        for c in [
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "Arm".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            },
            Command::AddRotationDeformer {
                name: "R".into(),
                part: "Arm".into(),
                parent: Some("W".into()),
                base: RotationBase {
                    origin: [0.0, 0.0],
                    angle_degrees: 0.0,
                    scale: 1.0,
                    reflect_x: false,
                    reflect_y: false,
                },
            },
        ] {
            m.apply(c).unwrap();
        }
        let mut h = harness(m, Panel::Deformers);
        h.run();
        h.get_by_label("W");
        h.get_by_label("R");
        assert!(h.query_by_label("Arm").is_none(), "no part rows here");
        h.get_by_label("Warp");
    }

    #[test]
    fn a_selection_made_elsewhere_opens_the_folders_and_scrolls_the_big_panel_but_a_click_does_not()
    {
        let mut m = model();
        for i in 0..631 {
            m.apply(mesh(&format!("m{i}"), "Arm")).unwrap();
        }
        let mut h = harness(m, Panel::Parts);
        h.run();
        let eyes = h.get_all_by_label("Hide").count();
        assert!(eyes > 0 && eyes < 40, "{eyes} rows built");
        assert!(
            h.query_by_label("m600").is_none(),
            "far below the first rows"
        );
        // The folder is closed, and the selection is made in the viewport.
        h.state_mut().1.parts.set_open("p:Arm", false);
        h.state_mut().1.select_only(Selection::Mesh("m600".into()));
        h.run();
        h.run();
        h.get_by_label("m600");
        let offset = h.state().1.parts.scroll.0;
        assert!(offset > 1000.0, "{offset}");
        // A click on a row in view does not move the panel.
        h.get_by_label("m601").click();
        h.run();
        h.run();
        assert_eq!(h.state().1.selection, Some(Selection::Mesh("m601".into())));
        assert!((h.state().1.parts.scroll.0 - offset).abs() < 1e-3);
        // A selection hidden by the filter leaves the panel alone.
        h.state_mut().1.parts.filter = "m1".into();
        h.run();
        let before = h.state().1.parts.scroll.0;
        h.state_mut().1.select_only(Selection::Mesh("m500".into()));
        h.run();
        assert_eq!(h.state().1.parts.filter, "m1");
        assert_eq!(h.state().1.parts.scroll.0, before);
    }

    #[test]
    fn clicking_an_eye_in_the_drawn_panel_returns_the_command() {
        let mut m = model();
        for i in 0..631 {
            m.apply(mesh(&format!("m{i}"), "Arm")).unwrap();
        }
        let mut h = Harness::builder()
            .with_size(egui::vec2(320.0, 400.0))
            .build_ui_state(
                |ui, state: &mut (Model, ViewState, Option<Command>)| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    for out in show(ui, &state.0, &mut state.1, Panel::Parts) {
                        if let Out::Command(c) = out {
                            state.2 = Some(c);
                        }
                    }
                },
                (m, ViewState::default(), None),
            );
        h.run();
        h.get_all_by_label("Hide").next().unwrap().click();
        h.run();
        assert!(matches!(h.state().2, Some(Command::SetPartVisible { .. })));
    }

    fn collecting(m: Model, panel: Panel) -> Harness<'static, (Model, ViewState, Vec<Out>)> {
        Harness::builder()
            .with_size(egui::vec2(320.0, 400.0))
            .build_ui_state(
                move |ui, state: &mut (Model, ViewState, Vec<Out>)| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    let outs = show(ui, &state.0, &mut state.1, panel);
                    state.2.extend(outs);
                },
                (m, ViewState::default(), Vec::new()),
            )
    }

    #[test]
    fn the_menu_deletes_and_duplicates_the_selection_when_the_row_is_in_it_else_the_row() {
        let mut h = collecting(many(), Panel::Parts);
        h.run();
        h.get_by_label("a1").click();
        h.get_by_label("a2")
            .click_modifiers(egui::Modifiers::COMMAND);
        h.run();
        // Right click on a selected row: both.
        h.get_by_label("a2").click_secondary();
        h.run();
        h.get_by_label("Delete").click();
        h.run();
        let deleted = h.state().2.clone();
        assert_eq!(
            deleted,
            [Out::Delete(vec![
                Selection::Mesh("a1".into()),
                Selection::Mesh("a2".into())
            ])]
        );
        // Right click on a row outside the selection: that row alone.
        h.state_mut().2.clear();
        h.get_by_label("b1").click_secondary();
        h.run();
        h.get_by_label("Duplicate").click();
        h.run();
        assert_eq!(
            h.state().2,
            [Out::Duplicate(vec![Selection::Mesh("b1".into())])]
        );
        // A part has no Duplicate, but can select what is under it.
        h.state_mut().2.clear();
        h.get_by_label("Arm").click_secondary();
        h.run();
        assert!(h.query_by_label("Duplicate").is_none());
        h.get_by_label("Select children").click();
        h.run();
        assert_eq!(h.state().1.selected_meshes().len(), 5);
    }

    #[test]
    fn a_double_click_renames_a_mesh_and_selects_the_meshes_under_a_part() {
        let m = many();
        let mut view = ViewState::default();
        let row = |n: &str| {
            tree_model::rows(&m, &Default::default(), Panel::Parts)
                .into_iter()
                .find(|r| r.name == n)
                .unwrap()
        };
        activate(&m, &mut view, Panel::Parts, &row("a3"));
        let renaming = view.parts.renaming.clone().unwrap();
        assert_eq!(
            (
                renaming.kind,
                renaming.name.as_str(),
                renaming.text.as_str()
            ),
            (Kind::Mesh, "a3", "a3")
        );
        activate(&m, &mut view, Panel::Parts, &row("Arm"));
        assert_eq!(view.selected_meshes(), ["a1", "a2", "b1", "a3", "b2"]);
        // The field is drawn; changing the text and leaving it commits.
        let mut h = collecting(many(), Panel::Parts);
        h.run();
        h.state_mut().1.parts.renaming = Some(Renaming {
            kind: Kind::Mesh,
            name: "a3".into(),
            text: "a3x".into(),
            focused: false,
        });
        h.run();
        h.run();
        h.get_by_label("Body").click();
        h.run();
        assert!(h.state().2.contains(&Out::Rename {
            kind: Kind::Mesh,
            from: "a3".into(),
            to: "a3x".into()
        }));
        assert!(h.state().1.parts.renaming.is_none());
    }

    #[test]
    fn an_empty_or_unchanged_name_is_dropped() {
        let mut h = collecting(many(), Panel::Parts);
        h.run();
        for text in ["", "   ", "a1"] {
            h.state_mut().2.clear();
            h.state_mut().1.parts.renaming = Some(Renaming {
                kind: Kind::Mesh,
                name: "a1".into(),
                text: text.into(),
                focused: true,
            });
            h.run();
            h.get_by_label("Body").click();
            h.run();
            assert!(
                h.state().2.iter().all(|o| !matches!(o, Out::Rename { .. })),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_keys_move_rename_delete_and_duplicate_as_state_functions() {
        let m = many();
        let mut view = ViewState::default();
        let rows = |v: &ViewState| tree_model::rows(&m, v.tree(Panel::Parts), Panel::Parts);
        let sel = |n: &str| Selection::Mesh(n.into());
        view.select_only(sel("a1"));
        assert_eq!(
            apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Down),
            None
        );
        assert_eq!(view.selection, Some(sel("a2")));
        apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Up);
        assert_eq!(view.selection, Some(sel("a1")));
        // Left on a leaf goes to the folder, Left again closes it, Right opens it.
        apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Left);
        assert_eq!(view.selection, Some(Selection::Part("Arm".into())));
        apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Left);
        assert!(!view.parts.is_open("p:Arm"));
        apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Right);
        assert!(view.parts.is_open("p:Arm"));
        // Enter / F2 starts renaming the selected row.
        view.select_only(sel("a3"));
        assert_eq!(
            apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Rename),
            None
        );
        assert_eq!(
            view.parts.renaming.as_ref().map(|r| r.name.as_str()),
            Some("a3")
        );
        view.parts.renaming = None;
        // Delete and Ctrl+D ask for the whole selection; a part is not duplicated.
        view.select_all(vec![sel("a1"), Selection::Part("Arm".into())]);
        assert_eq!(
            apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Delete),
            Some(Out::Delete(vec![sel("a1"), Selection::Part("Arm".into())]))
        );
        assert_eq!(
            apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Duplicate),
            Some(Out::Duplicate(vec![sel("a1")]))
        );
        view.clear_selection();
        assert_eq!(
            apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Delete),
            None
        );
        assert_eq!(
            apply_key(&rows(&view), &mut view, Panel::Parts, PanelKey::Rename),
            None
        );
    }

    #[test]
    fn the_keys_work_only_while_the_panel_has_the_keyboard() {
        let mut h = collecting(many(), Panel::Parts);
        h.run();
        // Selected from elsewhere: the panel has no focus, so Down does nothing.
        h.state_mut().1.select_only(Selection::Mesh("a1".into()));
        h.run();
        h.key_press(egui::Key::ArrowDown);
        h.run();
        assert_eq!(h.state().1.selection, Some(Selection::Mesh("a1".into())));
        // A click gives it the keyboard.
        h.get_by_label("a1").click();
        h.run();
        h.key_press(egui::Key::ArrowDown);
        h.run();
        assert_eq!(h.state().1.selection, Some(Selection::Mesh("a2".into())));
        h.key_press(egui::Key::Delete);
        h.run();
        assert_eq!(
            h.state().2,
            [Out::Delete(vec![Selection::Mesh("a2".into())])]
        );
    }
}
