// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The parts and deformers tree: parts nested by parent, each with the
//! meshes and deformers that belong to it. A filter keeps the rows whose name
//! matches and the ancestors that lead to them.

use egui::Ui;
use tsukumoe_core::arena::Id;
use tsukumoe_core::elements::Part;
use tsukumoe_core::model::Model;
use tsukumoe_ui::{Icon, selectable_row, text_input, tr};

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
            collect(model, id, 0, filter, &mut out);
        }
    }
    out
}

/// Pushes the part's subtree; returns whether anything in it matched.
fn collect(model: &Model, part: Id<Part>, depth: usize, filter: &str, out: &mut Vec<Row>) -> bool {
    let name = model.parts().name(part).unwrap_or_default().to_owned();
    let start = out.len();
    let mut any = matches(&name, filter);
    out.push(Row {
        depth,
        kind: Kind::Part,
        name,
    });
    for (id, _, p) in model.parts().iter() {
        if p.parent == Some(part) {
            any |= collect(model, id, depth + 1, filter, out);
        }
    }
    let mut leaf = |kind, name: &str| {
        if matches(name, filter) {
            any = true;
            out.push(Row {
                depth: depth + 1,
                kind,
                name: name.to_owned(),
            });
        }
    };
    for (_, name, _) in model.meshes().iter().filter(|(_, _, m)| m.part == part) {
        leaf(Kind::Mesh, name);
    }
    for (_, name, _) in model.warps().iter().filter(|(_, _, w)| w.part == part) {
        leaf(Kind::Warp, name);
    }
    for (_, name, _) in model.rotations().iter().filter(|(_, _, r)| r.part == part) {
        leaf(Kind::Rotation, name);
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

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) {
    let ctx = ui.ctx().clone();
    let name = tr(&ctx, "filter-name");
    text_input(ui, &mut view.tree_filter, &name, &name);
    let rows = rows(model, &view.tree_filter);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for row in &rows {
                let selection = selection_of(row);
                let icon = match row.kind {
                    Kind::Part => Icon::Folder,
                    Kind::Mesh => Icon::Image,
                    Kind::Warp | Kind::Rotation => Icon::Settings,
                };
                ui.horizontal(|ui| {
                    ui.add_space(row.depth as f32 * 12.0);
                    let selected = view.selection.as_ref() == Some(&selection);
                    if selectable_row(ui, selected, Some(icon), &row.name).clicked() {
                        view.selection = Some(selection);
                        view.touch();
                    }
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::command::Command;

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
}
