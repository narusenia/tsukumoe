// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The parts and deformers tree: parts nested by parent, each with the
//! meshes and deformers that belong to it. A filter keeps the rows whose name
//! matches and the ancestors that lead to them.

use egui::Ui;
use tsukumoe_core::arena::Id;
use tsukumoe_core::command::Command;
use tsukumoe_core::elements::Part;
use tsukumoe_core::model::Model;
use tsukumoe_ui::{Icon, Theme, Variant, icon_button, selectable_row, text_input, tr};

use crate::view::{Selection, ViewState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Part,
    Mesh,
    Warp,
    Rotation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub depth: usize,
    pub kind: Kind,
    pub name: String,
    /// The row's own visible flag; parts and meshes only.
    pub visible: Option<bool>,
    /// Hidden itself or through a hidden part above it.
    pub dimmed: bool,
}

pub fn matches(name: &str, filter: &str) -> bool {
    filter.trim().is_empty() || name.to_lowercase().contains(&filter.trim().to_lowercase())
}

/// The rows to show, in tree order. The filter is a case-insensitive substring.
pub fn rows(model: &Model, filter: &str) -> Vec<Row> {
    let mut out = Vec::new();
    // Parts without a parent, and any whose parent is gone, are roots.
    for (id, _, part) in model.parts().iter() {
        if part.parent.is_none_or(|p| !model.parts().contains(p)) {
            collect(model, id, 0, false, filter, &mut out);
        }
    }
    out
}

/// Pushes the part's subtree; returns whether anything in it matched.
fn collect(
    model: &Model,
    part: Id<Part>,
    depth: usize,
    hidden_above: bool,
    filter: &str,
    out: &mut Vec<Row>,
) -> bool {
    let name = model.parts().name(part).unwrap_or_default().to_owned();
    let own = model.parts().get(part).is_none_or(|p| p.visible);
    let hidden = hidden_above || !own;
    let start = out.len();
    let mut any = matches(&name, filter);
    out.push(Row {
        depth,
        kind: Kind::Part,
        name,
        visible: Some(own),
        dimmed: hidden,
    });
    for (id, _, p) in model.parts().iter() {
        if p.parent == Some(part) {
            any |= collect(model, id, depth + 1, hidden, filter, out);
        }
    }
    let mut leaf = |kind, name: &str, visible: Option<bool>| {
        if matches(name, filter) {
            any = true;
            out.push(Row {
                depth: depth + 1,
                kind,
                name: name.to_owned(),
                visible,
                dimmed: hidden || visible == Some(false),
            });
        }
    };
    for (_, name, m) in model.meshes().iter().filter(|(_, _, m)| m.part == part) {
        leaf(Kind::Mesh, name, Some(m.visible));
    }
    for (_, name, _) in model.warps().iter().filter(|(_, _, w)| w.part == part) {
        leaf(Kind::Warp, name, None);
    }
    for (_, name, _) in model.rotations().iter().filter(|(_, _, r)| r.part == part) {
        leaf(Kind::Rotation, name, None);
    }
    if !any {
        out.truncate(start);
    }
    any
}

fn selection_of(row: &Row) -> Selection {
    let name = row.name.clone();
    match row.kind {
        Kind::Part => Selection::Part(name),
        Kind::Mesh => Selection::Mesh(name),
        Kind::Warp => Selection::Warp(name),
        Kind::Rotation => Selection::Rotation(name),
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

/// Draws the tree. Only the rows in view are built (`show_rows`), so a model
/// with hundreds of meshes scrolls cheaply. Returns the visibility command
/// the user asked for, to be sent through the editor's dispatcher.
pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Option<Command> {
    let ctx = ui.ctx().clone();
    let name = tr(&ctx, "filter-name");
    text_input(ui, &mut view.tree_filter, &name, &name);
    let rows = rows(model, &view.tree_filter);
    let control = Theme::get(&ctx).metrics.control;
    let row_height = control + ui.spacing().item_spacing.y;
    let mut command = None;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        row_height,
        rows.len(),
        |ui, range| {
            for row in &rows[range] {
                let selection = selection_of(row);
                let icon = match row.kind {
                    Kind::Part => Icon::Folder,
                    Kind::Mesh => Icon::Image,
                    Kind::Warp | Kind::Rotation => Icon::Settings,
                };
                ui.horizontal(|ui| {
                    if row.dimmed {
                        ui.set_opacity(0.45);
                    }
                    ui.add_space(row.depth as f32 * 12.0);
                    if let Some(visible) = row.visible {
                        let (eye, key) = if visible {
                            (Icon::Eye, "tree-hide")
                        } else {
                            (Icon::EyeOff, "tree-show")
                        };
                        if icon_button(ui, eye, &tr(&ctx, key), Variant::Ghost).clicked() {
                            command = toggle_command(row);
                        }
                    }
                    let selected = view.selection.as_ref() == Some(&selection);
                    if selectable_row(ui, selected, Some(icon), &row.name).clicked() {
                        view.selection = Some(selection);
                        view.touch();
                    }
                });
            }
        },
    );
    command
}

#[cfg(test)]
mod tests {
    use super::*;

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
        m.apply(Command::AddMesh {
            name: "HairFront".into(),
            part: "Hair".into(),
            positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
        })
        .unwrap();
        m
    }

    fn names(rows: &[Row]) -> Vec<(usize, &str)> {
        rows.iter().map(|r| (r.depth, r.name.as_str())).collect()
    }

    #[test]
    fn rows_nest_parts_and_list_their_meshes() {
        let m = model();
        assert_eq!(
            names(&rows(&m, "")),
            [
                (0, "Body"),
                (1, "Head"),
                (2, "Hair"),
                (3, "HairFront"),
                (1, "Arm")
            ]
        );
    }

    #[test]
    fn a_filter_keeps_matches_and_their_ancestors() {
        let m = model();
        assert_eq!(
            names(&rows(&m, "front")),
            [(0, "Body"), (1, "Head"), (2, "Hair"), (3, "HairFront")]
        );
        assert_eq!(names(&rows(&m, "ARM")), [(0, "Body"), (1, "Arm")]);
        assert!(rows(&m, "zzz").is_empty());
        assert_eq!(rows(&m, "  ").len(), 5);
    }

    #[test]
    fn rows_carry_their_flag_and_dim_through_a_hidden_part() {
        let mut m = model();
        m.apply(Command::SetPartVisible {
            name: "Head".into(),
            visible: false,
        })
        .unwrap();
        m.apply(Command::SetMeshVisible {
            name: "HairFront".into(),
            visible: false,
        })
        .unwrap();
        let r = rows(&m, "");
        let get = |n: &str| r.iter().find(|x| x.name == n).unwrap();
        assert_eq!(
            (get("Body").visible, get("Body").dimmed),
            (Some(true), false)
        );
        assert_eq!(
            (get("Head").visible, get("Head").dimmed),
            (Some(false), true)
        );
        // Under a hidden part: dimmed, but the eye shows the part's own flag.
        assert_eq!(
            (get("Hair").visible, get("Hair").dimmed),
            (Some(true), true)
        );
        assert_eq!(get("HairFront").visible, Some(false));
        assert_eq!((get("Arm").visible, get("Arm").dimmed), (Some(true), false));
        // Only the mesh's own flag is on its row when the part is hidden.
        m.apply(Command::SetMeshVisible {
            name: "HairFront".into(),
            visible: true,
        })
        .unwrap();
        let r = rows(&m, "front");
        assert_eq!(
            r.last().map(|x| (x.visible, x.dimmed)),
            Some((Some(true), true))
        );
    }

    #[test]
    fn the_eye_sends_the_opposite_flag_and_undo_restores_it() {
        let mut d = crate::dispatcher::Dispatcher::new(model());
        let row = |d: &crate::dispatcher::Dispatcher, n: &str| {
            rows(d.model(), "")
                .into_iter()
                .find(|x| x.name == n)
                .unwrap()
        };
        let cmd = toggle_command(&row(&d, "HairFront")).unwrap();
        assert_eq!(
            cmd,
            Command::SetMeshVisible {
                name: "HairFront".into(),
                visible: false
            }
        );
        d.submit(cmd).unwrap();
        assert!(d.dirty());
        assert_eq!(row(&d, "HairFront").visible, Some(false));
        let cmd = toggle_command(&row(&d, "Hair")).unwrap();
        assert_eq!(
            cmd,
            Command::SetPartVisible {
                name: "Hair".into(),
                visible: false
            }
        );
        d.submit(cmd).unwrap();
        assert!(d.undo() && d.undo());
        assert_eq!(row(&d, "HairFront").visible, Some(true));
        assert_eq!(row(&d, "Hair").visible, Some(true));
        assert!(!d.dirty());
    }

    #[test]
    fn clicking_an_eye_in_the_drawn_panel_returns_the_command_and_a_big_tree_draws() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let mut m = model();
        for i in 0..631 {
            m.apply(Command::AddMesh {
                name: format!("m{i}"),
                part: "Arm".into(),
                positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                indices: vec![0, 1, 2],
            })
            .unwrap();
        }
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 400.0))
            .build_ui_state(
                |ui, state: &mut (Model, ViewState, Option<Command>)| {
                    tsukumoe_ui::init(ui.ctx());
                    if let Some(c) = show(ui, &state.0, &mut state.1) {
                        state.2 = Some(c);
                    }
                },
                (m, ViewState::default(), None),
            );
        let start = std::time::Instant::now();
        harness.run();
        let drawn = start.elapsed();
        // Only the visible rows are built, not all 636.
        let label = tr(&harness.ctx, "tree-hide");
        let eyes = harness.get_all_by_label(&label).count();
        assert!(eyes > 0 && eyes < 40, "{eyes} eyes built");
        harness.get_all_by_label(&label).next().unwrap().click();
        harness.run();
        assert!(matches!(
            harness.state().2,
            Some(Command::SetPartVisible { .. })
        ));
        eprintln!("631-mesh tree: first frames {drawn:?}");
    }
}
