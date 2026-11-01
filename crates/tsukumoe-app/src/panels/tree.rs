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
use tsukumoe_core::rig::DeformerRef;
use tsukumoe_ui::{Icon, Theme, Variant, icon_button, selectable_row, text_input, tr};

use crate::deformer_ops::Target;
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
    /// The deformer this mesh or deformer hangs from.
    pub parent: Option<String>,
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
        parent: None,
    });
    for (id, _, p) in model.parts().iter() {
        if p.parent == Some(part) {
            any |= collect(model, id, depth + 1, hidden, filter, out);
        }
    }
    // Meshes and deformers hang under a deformer of the same part; a parent in
    // another part leaves them at the top of this one.
    let part_of = |r: DeformerRef| match r {
        DeformerRef::Warp(id) => model.warps().get(id).map(|d| d.part),
        DeformerRef::Rotation(id) => model.rotations().get(id).map(|d| d.part),
    };
    let nested = |parent: Option<DeformerRef>| parent.and_then(part_of) == Some(part);
    for (_, name, m) in model
        .meshes()
        .iter()
        .filter(|(_, _, m)| m.part == part && !nested(m.parent))
    {
        if matches(name, filter) {
            any = true;
            out.push(mesh_row(
                model,
                name,
                m.visible,
                m.parent,
                depth + 1,
                hidden,
            ));
        }
    }
    let roots: Vec<DeformerRef> = model
        .warps()
        .iter()
        .filter(|(_, _, d)| d.part == part && !nested(d.parent))
        .map(|(id, _, _)| DeformerRef::Warp(id))
        .chain(
            model
                .rotations()
                .iter()
                .filter(|(_, _, d)| d.part == part && !nested(d.parent))
                .map(|(id, _, _)| DeformerRef::Rotation(id)),
        )
        .collect();
    for r in roots {
        any |= collect_deformer(model, r, part, depth + 1, hidden, filter, out);
    }
    if !any {
        out.truncate(start);
    }
    any
}

fn deformer_name(model: &Model, r: DeformerRef) -> Option<String> {
    crate::deformer_tool::name_of(model, r)
}

fn mesh_row(
    model: &Model,
    name: &str,
    visible: bool,
    parent: Option<DeformerRef>,
    depth: usize,
    hidden: bool,
) -> Row {
    Row {
        depth,
        kind: Kind::Mesh,
        name: name.to_owned(),
        visible: Some(visible),
        dimmed: hidden || !visible,
        parent: parent.and_then(|r| deformer_name(model, r)),
    }
}

/// Pushes a deformer and what hangs under it in the same part (child
/// deformers, then meshes); returns whether anything in it matched.
fn collect_deformer(
    model: &Model,
    deformer: DeformerRef,
    part: Id<Part>,
    depth: usize,
    hidden: bool,
    filter: &str,
    out: &mut Vec<Row>,
) -> bool {
    let Some(name) = deformer_name(model, deformer) else {
        return false;
    };
    let (kind, parent) = match deformer {
        DeformerRef::Warp(id) => (Kind::Warp, model.warps().get(id).and_then(|d| d.parent)),
        DeformerRef::Rotation(id) => (
            Kind::Rotation,
            model.rotations().get(id).and_then(|d| d.parent),
        ),
    };
    let start = out.len();
    let mut any = matches(&name, filter);
    out.push(Row {
        depth,
        kind,
        name,
        visible: None,
        dimmed: hidden,
        parent: parent.and_then(|r| deformer_name(model, r)),
    });
    let children: Vec<DeformerRef> = model
        .warps()
        .iter()
        .filter(|(_, _, d)| d.part == part && d.parent == Some(deformer))
        .map(|(id, _, _)| DeformerRef::Warp(id))
        .chain(
            model
                .rotations()
                .iter()
                .filter(|(_, _, d)| d.part == part && d.parent == Some(deformer))
                .map(|(id, _, _)| DeformerRef::Rotation(id)),
        )
        .collect();
    for child in children {
        any |= collect_deformer(model, child, part, depth + 1, hidden, filter, out);
    }
    for (_, name, m) in model
        .meshes()
        .iter()
        .filter(|(_, _, m)| m.part == part && m.parent == Some(deformer))
    {
        if matches(name, filter) {
            any = true;
            out.push(mesh_row(
                model,
                name,
                m.visible,
                m.parent,
                depth + 1,
                hidden,
            ));
        }
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

/// What the tree asks for: a command to send, or an element to move under a
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

/// The meshes from `anchor` to `target` among the rows shown (a Shift click),
/// ending with `target`. Without an anchor in the rows, just `target`. Rows
/// that a filter hides are not in `rows`, so they are not in the range.
pub fn range_names(rows: &[Row], anchor: Option<&str>, target: &str) -> Vec<String> {
    let meshes: Vec<&str> = rows
        .iter()
        .filter(|r| r.kind == Kind::Mesh)
        .map(|r| r.name.as_str())
        .collect();
    let (Some(a), Some(t)) = (
        anchor.and_then(|a| meshes.iter().position(|&n| n == a)),
        meshes.iter().position(|&n| n == target),
    ) else {
        return vec![target.to_owned()];
    };
    let mut range: Vec<String> = meshes[a.min(t)..=a.max(t)]
        .iter()
        .map(|&n| n.to_owned())
        .collect();
    if t < a {
        range.reverse();
    }
    range
}

/// Draws the tree. Only the rows in view are built (`show_rows`), so a model
/// with hundreds of meshes scrolls cheaply. Returns what the user asked for,
/// to be sent through the editor's dispatcher. Meshes and deformers can be
/// dragged onto a deformer row to hang them from it, and a right click gives
/// a menu to set or clear the parent and to remove a deformer.
pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Vec<Out> {
    let ctx = ui.ctx().clone();
    let name = tr(&ctx, "filter-name");
    text_input(ui, &mut view.tree_filter, &name, &name);
    let rows = rows(model, &view.tree_filter);
    let all = rows.clone();
    let control = Theme::get(&ctx).metrics.control;
    let row_height = control + ui.spacing().item_spacing.y;
    let mut outs = Vec::new();
    let deformers: Vec<String> = model
        .warps()
        .iter()
        .map(|(_, n, _)| n.to_owned())
        .chain(model.rotations().iter().map(|(_, n, _)| n.to_owned()))
        .collect();
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
                            outs.extend(toggle_command(row).map(Out::Command));
                        }
                    }
                    let selected = view.is_selected(&selection);
                    let response = selectable_row(ui, selected, Some(icon), &row.name);
                    if response.clicked() {
                        let modifiers = ui.input(|i| i.modifiers);
                        match &selection {
                            Selection::Mesh(n) if modifiers.command => view.toggle_mesh(n),
                            Selection::Mesh(n) if modifiers.shift => {
                                let from = view
                                    .anchor
                                    .clone()
                                    .or_else(|| view.selected_meshes().last().cloned());
                                let names = range_names(&all, from.as_deref(), n);
                                view.select_meshes(names);
                            }
                            _ => view.select_only(selection),
                        }
                    }
                    let Some(target) = target_of(row) else {
                        return;
                    };
                    // Drag a row onto a deformer row to hang it there.
                    let response = response.interact(egui::Sense::click_and_drag());
                    response.dnd_set_drag_payload(target.clone());
                    if matches!(row.kind, Kind::Warp | Kind::Rotation) {
                        if response.dnd_hover_payload::<Target>().is_some() {
                            ui.painter().rect_stroke(
                                response.rect,
                                2.0,
                                egui::Stroke::new(1.5, Theme::get(&ctx).colors.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if let Some(dropped) = response.dnd_release_payload::<Target>()
                            && *dropped != target
                        {
                            outs.push(Out::Reparent((*dropped).clone(), Some(row.name.clone())));
                        }
                    }
                    response.context_menu(|ui| {
                        ui.menu_button(tr(&ctx, "tree-set-parent"), |ui| {
                            for parent in deformers.iter().filter(|d| **d != row.name) {
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
                        if matches!(row.kind, Kind::Warp | Kind::Rotation)
                            && ui.button(tr(&ctx, "tree-remove-deformer")).clicked()
                        {
                            outs.push(Out::Command(Command::RemoveDeformer {
                                name: row.name.clone(),
                            }));
                            ui.close();
                        }
                    });
                });
            }
        },
    );
    outs
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
                    for out in show(ui, &state.0, &mut state.1) {
                        if let Out::Command(c) = out {
                            state.2 = Some(c);
                        }
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

    fn deformer_model() -> Model {
        let mut m = model();
        m.apply(Command::AddPart {
            name: "Face".into(),
            parent: None,
        })
        .unwrap();
        let grid = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let tri = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let base = tsukumoe_core::command::RotationBase {
            origin: [0.0, 0.0],
            angle_degrees: 0.0,
            scale: 1.0,
            reflect_x: false,
            reflect_y: false,
        };
        for command in [
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "Face".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: grid,
            },
            Command::AddRotationDeformer {
                name: "R".into(),
                part: "Face".into(),
                parent: Some("W".into()),
                base,
            },
            // A deformer of another part under W: it stays at the top of its part.
            Command::AddRotationDeformer {
                name: "Far".into(),
                part: "Arm".into(),
                parent: Some("W".into()),
                base,
            },
        ] {
            m.apply(command).unwrap();
        }
        for (name, parent) in [("M1", Some("R")), ("M2", None)] {
            m.apply(Command::AddMesh {
                name: name.into(),
                part: "Face".into(),
                positions: tri.clone(),
                uvs: tri.clone(),
                indices: vec![0, 1, 2],
            })
            .unwrap();
            if let Some(p) = parent {
                m.apply(Command::SetMeshParent {
                    name: name.into(),
                    parent: Some(p.into()),
                })
                .unwrap();
            }
        }
        m
    }

    #[test]
    fn deformers_nest_under_their_parent_in_the_same_part_with_their_meshes() {
        let m = deformer_model();
        let r = rows(&m, "");
        let face: Vec<_> = r
            .iter()
            .skip_while(|x| x.name != "Face")
            .map(|x| (x.depth, x.name.as_str(), x.kind))
            .collect();
        assert_eq!(
            face,
            [
                (0, "Face", Kind::Part),
                (1, "M2", Kind::Mesh),
                (1, "W", Kind::Warp),
                (2, "R", Kind::Rotation),
                (3, "M1", Kind::Mesh),
            ]
        );
        // The parent is on the row; a parent in another part leaves the row at the top.
        let get = |n: &str| r.iter().find(|x| x.name == n).unwrap();
        assert_eq!(get("R").parent.as_deref(), Some("W"));
        assert_eq!(get("M1").parent.as_deref(), Some("R"));
        assert_eq!(get("M2").parent, None);
        assert_eq!(
            (get("Far").depth, get("Far").parent.as_deref()),
            (2, Some("W"))
        );
        assert_eq!(get("Arm").depth, 1);
    }

    #[test]
    fn a_filter_keeps_the_deformers_above_a_match() {
        let m = deformer_model();
        let r = rows(&m, "m1");
        let names: Vec<_> = r.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["Face", "W", "R", "M1"]);
    }

    #[test]
    fn the_context_menu_sends_a_reparent_and_a_removal() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let m = deformer_model();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 400.0))
            .build_ui_state(
                |ui, state: &mut (Model, ViewState, Vec<Out>)| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    let outs = show(ui, &state.0, &mut state.1);
                    state.2.extend(outs);
                },
                (m, ViewState::default(), Vec::new()),
            );
        harness.run();
        // Remove: a right click on the row of R.
        harness.get_by_label("R").click_secondary();
        harness.run();
        harness.get_by_label("Remove deformer").click();
        harness.run();
        assert_eq!(
            harness.state().2,
            [Out::Command(Command::RemoveDeformer { name: "R".into() })]
        );
        // Clear the parent of M1.
        harness.state_mut().2.clear();
        harness.get_by_label("M1").click_secondary();
        harness.run();
        harness.get_by_label("Clear parent").click();
        harness.run();
        assert_eq!(
            harness.state().2,
            [Out::Reparent(Target::Mesh("M1".into()), None)]
        );
        // Set the parent of M2 to W.
        harness.state_mut().2.clear();
        harness.get_by_label("M2").click_secondary();
        harness.run();
        harness.get_by_label("Set parent").click();
        harness.run();
        // The row of W and the menu entry share the label; the menu is on top.
        harness.get_all_by_label("W").next().unwrap().click();
        harness.run();
        assert_eq!(
            harness.state().2,
            [Out::Reparent(Target::Mesh("M2".into()), Some("W".into()))]
        );
    }

    fn many_meshes() -> Model {
        let mut m = model();
        for n in ["a1", "a2", "b1", "a3", "b2"] {
            m.apply(Command::AddMesh {
                name: n.into(),
                part: "Arm".into(),
                positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                indices: vec![0, 1, 2],
            })
            .unwrap();
        }
        m
    }

    #[test]
    fn a_range_runs_between_the_anchor_and_the_target_over_the_rows_shown() {
        let m = many_meshes();
        let all = rows(&m, "");
        assert_eq!(range_names(&all, Some("a1"), "b1"), ["a1", "a2", "b1"]);
        // Upwards: the target is still last.
        assert_eq!(
            range_names(&all, Some("b2"), "a2"),
            ["b2", "a3", "b1", "a2"]
        );
        // No anchor, or one that is not shown: just the target.
        assert_eq!(range_names(&all, None, "a3"), ["a3"]);
        assert_eq!(range_names(&all, Some("gone"), "a3"), ["a3"]);
        // A filter hides rows, and they are not in the range.
        let shown = rows(&m, "a");
        assert_eq!(range_names(&shown, Some("a1"), "a3"), ["a1", "a2", "a3"]);
        assert_eq!(range_names(&shown, Some("a1"), "b1"), ["b1"]);
    }

    #[test]
    fn ctrl_and_shift_clicks_select_several_meshes_in_the_drawn_tree() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 400.0))
            .build_ui_state(
                |ui, state: &mut (Model, ViewState)| {
                    tsukumoe_ui::init(ui.ctx());
                    show(ui, &state.0, &mut state.1);
                },
                (many_meshes(), ViewState::default()),
            );
        harness.run();
        harness.get_by_label("a1").click();
        harness.run();
        harness
            .get_by_label("b1")
            .click_modifiers(egui::Modifiers::COMMAND);
        harness.run();
        assert_eq!(harness.state().1.selected_meshes(), ["a1", "b1"]);
        harness
            .get_by_label("a3")
            .click_modifiers(egui::Modifiers::SHIFT);
        harness.run();
        // The anchor is a1 (the last plain or Ctrl click was on b1, so b1).
        assert_eq!(harness.state().1.selected_meshes(), ["b1", "a3"]);
        harness.get_by_label("a2").click();
        harness.run();
        assert_eq!(harness.state().1.selected_meshes(), ["a2"]);
    }
}
