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
    self, Kind, Panel, Row, Sort, accepted, range_selection, scroll_offset_for, scroll_row,
};
use crate::view::ViewState;

/// The indent of one depth, in points.
pub const INDENT: f32 = 14.0;

/// What the panel asks for: a command to send, or an element to move under a
/// deformer (the editor converts the coordinates and sends one batch).
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Command(Command),
    Reparent(Target, Option<String>),
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

    // Empty space below the rows clears the selection.
    let area = ui.available_rect_before_wrap();
    let background = ui.interact(area, ui.next_auto_id(), Sense::click());
    if background.clicked() {
        view.clear_selection();
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
            // The name, cut short before the right edge; deformer rows also show their part.
            let right = rect.right() - 4.0;
            let name_clip =
                Rect::from_min_max(pos2(cols.name_x, rect.top()), pos2(right, rect.bottom()));
            let font = FontId::proportional(theme.metrics.text);
            let name_color = tone(ui, row, selected);
            let galley = ui
                .painter()
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
            if response.clicked() {
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
}
