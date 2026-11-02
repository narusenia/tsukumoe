// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What the tree's menus and keys do to the model, as commands: rename,
//! delete (a part takes what is under it), duplicate and move to another part.
//! Each function reads the model and returns commands for the dispatcher; a
//! group of them is one `Batch`, so one undo step (ADR-0019).

use std::fmt;

use tsukumoe_core::command::{Command, RotationBase};
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::DeformerRef;

use crate::tree_model::Kind;
use crate::view::Selection;

fn kind_of(s: &Selection) -> Option<(Kind, &str)> {
    match s {
        Selection::Part(n) => Some((Kind::Part, n)),
        Selection::Mesh(n) => Some((Kind::Mesh, n)),
        Selection::Warp(n) => Some((Kind::Warp, n)),
        Selection::Rotation(n) => Some((Kind::Rotation, n)),
        Selection::Parameter(_) => None,
    }
}

/// The command that gives `name` of `kind` the name `new_name`.
pub fn rename_command(kind: Kind, name: &str, new_name: &str) -> Command {
    let (name, new_name) = (name.to_owned(), new_name.to_owned());
    match kind {
        Kind::Part => Command::RenamePart { name, new_name },
        Kind::Mesh => Command::RenameMesh { name, new_name },
        Kind::Warp => Command::RenameWarp { name, new_name },
        Kind::Rotation => Command::RenameRotation { name, new_name },
    }
}

/// The selection a renamed element has afterwards.
pub fn renamed(kind: Kind, new_name: &str) -> Selection {
    let n = new_name.to_owned();
    match kind {
        Kind::Part => Selection::Part(n),
        Kind::Mesh => Selection::Mesh(n),
        Kind::Warp => Selection::Warp(n),
        Kind::Rotation => Selection::Rotation(n),
    }
}

/// What deleting a selection removes.
#[derive(Clone, Debug, PartialEq)]
pub struct DeletePlan {
    /// The removals, children before what holds them.
    pub commands: Vec<Command>,
    /// How many elements go.
    pub count: usize,
    /// A part with something under it is going: ask first.
    pub needs_confirm: bool,
}

fn deformer_depth(model: &Model, r: DeformerRef) -> usize {
    let parent = |r: DeformerRef| match r {
        DeformerRef::Warp(id) => model.warps().get(id).and_then(|d| d.parent),
        DeformerRef::Rotation(id) => model.rotations().get(id).and_then(|d| d.parent),
    };
    let (mut depth, mut at) = (0, parent(r));
    while let Some(p) = at {
        depth += 1;
        if depth > 64 {
            break;
        }
        at = parent(p);
    }
    depth
}

/// The commands that delete `items`. A part goes with the parts below it and
/// all their meshes and deformers, leaves first. Removals that the model
/// refuses (a mask still in use, a deformer with children elsewhere) fail
/// when applied, and the batch leaves the model untouched.
pub fn delete_plan(model: &Model, items: &[Selection]) -> DeletePlan {
    let mut meshes: Vec<String> = Vec::new();
    let mut deformers: Vec<DeformerRef> = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    let mut under_parts = 0;
    for item in items {
        let Some((kind, name)) = kind_of(item) else {
            continue;
        };
        match kind {
            Kind::Mesh => {
                if !meshes.iter().any(|m| m == name) {
                    meshes.push(name.to_owned());
                }
            }
            Kind::Warp | Kind::Rotation => {
                let r = model
                    .warps()
                    .find(name)
                    .map(DeformerRef::Warp)
                    .or_else(|| model.rotations().find(name).map(DeformerRef::Rotation));
                if let Some(r) = r
                    && !deformers.contains(&r)
                {
                    deformers.push(r);
                }
            }
            Kind::Part => {
                let Some(root) = model.parts().find(name) else {
                    continue;
                };
                let mut tree = vec![root];
                let mut i = 0;
                while i < tree.len() {
                    let at = tree[i];
                    let below: Vec<_> = model
                        .parts()
                        .iter()
                        .filter(|(id, _, p)| p.parent == Some(at) && !tree.contains(id))
                        .map(|(id, _, _)| id)
                        .collect();
                    tree.extend(below);
                    i += 1;
                }
                for &part in &tree {
                    for (_, n, m) in model.meshes().iter() {
                        if m.part == part && !meshes.iter().any(|x| x == n) {
                            meshes.push(n.to_owned());
                            under_parts += 1;
                        }
                    }
                    let owned = model
                        .warps()
                        .iter()
                        .filter(|(_, _, d)| d.part == part)
                        .map(|(id, _, _)| DeformerRef::Warp(id))
                        .chain(
                            model
                                .rotations()
                                .iter()
                                .filter(|(_, _, d)| d.part == part)
                                .map(|(id, _, _)| DeformerRef::Rotation(id)),
                        );
                    for r in owned {
                        if !deformers.contains(&r) {
                            deformers.push(r);
                            under_parts += 1;
                        }
                    }
                }
                // Deepest parts first, so a part goes before the one holding it.
                let names: Vec<String> = tree
                    .iter()
                    .rev()
                    .filter_map(|&id| model.parts().name(id).map(str::to_owned))
                    .collect();
                under_parts += names.len() - 1;
                for n in names {
                    if !parts.contains(&n) {
                        parts.push(n);
                    }
                }
            }
        }
    }
    deformers.sort_by_key(|&r| std::cmp::Reverse(deformer_depth(model, r)));
    let mut commands: Vec<Command> = meshes
        .iter()
        .map(|n| Command::RemoveMesh { name: n.clone() })
        .collect();
    commands.extend(deformers.iter().filter_map(|&r| {
        crate::deformer_tool::name_of(model, r).map(|name| Command::RemoveDeformer { name })
    }));
    // A part under another deleted part: leaves first (the list is already so per root).
    commands.extend(
        parts
            .iter()
            .map(|n| Command::RemovePart { name: n.clone() }),
    );
    DeletePlan {
        count: commands.len(),
        needs_confirm: under_parts > 0,
        commands,
    }
}

/// Why something cannot be duplicated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DuplicateError {
    /// Keyforms or bone influences would not carry over.
    Unsupported(String),
    Missing(String),
    /// Parts and parameters are not duplicated.
    Kind,
}

impl DuplicateError {
    pub fn key(&self) -> &'static str {
        match self {
            Self::Unsupported(_) => "tree-duplicate-unsupported",
            Self::Missing(_) => "deformer-missing",
            Self::Kind => "tree-duplicate-kind",
        }
    }
}

impl fmt::Display for DuplicateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(n) | Self::Missing(n) => write!(f, "{n}"),
            Self::Kind => f.write_str(self.key()),
        }
    }
}

/// `<name>.copy`, or `<name>.copy2`, ... the first the model does not have
/// (and the batch has not taken yet).
fn copy_name(taken: impl Fn(&str) -> bool, name: &str) -> String {
    let first = format!("{name}.copy");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|i| format!("{first}{i}"))
        .find(|n| !taken(n))
        .expect("an unused name exists")
}

/// A copy of each mesh and deformer of `items` (children of a deformer are
/// not copied), as one batch, with the names the copies get. Anything with
/// keyforms, or a mesh with bone influences, is refused.
pub fn duplicate_commands(
    model: &Model,
    items: &[Selection],
) -> Result<(Vec<Command>, Vec<Selection>), DuplicateError> {
    let mut commands = Vec::new();
    let mut made: Vec<Selection> = Vec::new();
    let taken = |made: &[Selection], n: &str| {
        model.meshes().find(n).is_some()
            || model.warps().find(n).is_some()
            || model.rotations().find(n).is_some()
            || made.iter().any(|s| kind_of(s).is_some_and(|(_, m)| m == n))
    };
    for item in items {
        let (kind, name) = kind_of(item).ok_or(DuplicateError::Kind)?;
        let copy = copy_name(|n| taken(&made, n), name);
        match kind {
            Kind::Part => return Err(DuplicateError::Kind),
            Kind::Mesh => {
                let id = model
                    .meshes()
                    .find(name)
                    .ok_or_else(|| DuplicateError::Missing(name.to_owned()))?;
                let m = model.meshes().get(id).expect("found above");
                if m.keyforms.is_some() || !m.influences.is_empty() {
                    return Err(DuplicateError::Unsupported(name.to_owned()));
                }
                let part = model.parts().name(m.part).unwrap_or_default().to_owned();
                commands.push(Command::AddMesh {
                    name: copy.clone(),
                    part,
                    positions: m.positions.clone(),
                    uvs: m.uvs.clone(),
                    indices: m.indices.clone(),
                });
                commands.push(Command::SetMeshTexture {
                    name: copy.clone(),
                    texture: m.texture,
                });
                commands.push(Command::SetMeshBlendMode {
                    name: copy.clone(),
                    blend_mode: m.blend_mode,
                });
                commands.push(Command::SetMeshOpacity {
                    name: copy.clone(),
                    opacity: m.opacity,
                });
                commands.push(Command::SetMeshDrawOrder {
                    name: copy.clone(),
                    draw_order: m.draw_order,
                });
                if !m.visible {
                    commands.push(Command::SetMeshVisible {
                        name: copy.clone(),
                        visible: false,
                    });
                }
                if !m.masks.is_empty() {
                    let masks: Vec<String> = m
                        .masks
                        .iter()
                        .filter_map(|&id| model.meshes().name(id).map(str::to_owned))
                        .collect();
                    commands.push(Command::SetMeshMasks {
                        name: copy.clone(),
                        masks,
                        inverted: m.mask_inverted,
                    });
                }
                if let Some(parent) = m
                    .parent
                    .and_then(|r| crate::deformer_tool::name_of(model, r))
                {
                    commands.push(Command::SetMeshParent {
                        name: copy.clone(),
                        parent: Some(parent),
                    });
                }
                made.push(Selection::Mesh(copy));
            }
            Kind::Warp | Kind::Rotation => {
                let r = model
                    .warps()
                    .find(name)
                    .map(DeformerRef::Warp)
                    .or_else(|| model.rotations().find(name).map(DeformerRef::Rotation))
                    .ok_or_else(|| DuplicateError::Missing(name.to_owned()))?;
                match r {
                    DeformerRef::Warp(id) => {
                        let d = model.warps().get(id).expect("found above");
                        if d.keyforms.is_some() {
                            return Err(DuplicateError::Unsupported(name.to_owned()));
                        }
                        commands.push(Command::AddWarpDeformer {
                            name: copy.clone(),
                            part: model.parts().name(d.part).unwrap_or_default().to_owned(),
                            parent: d
                                .parent
                                .and_then(|p| crate::deformer_tool::name_of(model, p)),
                            rows: d.rows,
                            columns: d.columns,
                            base_points: d.base_points.clone(),
                        });
                        made.push(Selection::Warp(copy));
                    }
                    DeformerRef::Rotation(id) => {
                        let d = model.rotations().get(id).expect("found above");
                        if d.keyforms.is_some() {
                            return Err(DuplicateError::Unsupported(name.to_owned()));
                        }
                        commands.push(Command::AddRotationDeformer {
                            name: copy.clone(),
                            part: model.parts().name(d.part).unwrap_or_default().to_owned(),
                            parent: d
                                .parent
                                .and_then(|p| crate::deformer_tool::name_of(model, p)),
                            base: RotationBase::of(&d.base),
                        });
                        made.push(Selection::Rotation(copy));
                    }
                }
            }
        }
    }
    Ok((commands, made))
}

/// The command that moves `item` to `part` (a part name; for a part, its new
/// parent part, or none).
pub fn set_part_command(item: &Selection, part: Option<&str>) -> Option<Command> {
    let (kind, name) = kind_of(item)?;
    let name = name.to_owned();
    match (kind, part) {
        (Kind::Mesh, Some(p)) => Some(Command::SetMeshPart {
            name,
            part: p.to_owned(),
        }),
        (Kind::Warp | Kind::Rotation, Some(p)) => Some(Command::SetDeformerPart {
            name,
            part: p.to_owned(),
        }),
        (Kind::Part, p) => Some(Command::SetPartParent {
            name,
            parent: p.map(str::to_owned),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::Dispatcher;

    fn tri(x: f32) -> Vec<[f32; 2]> {
        vec![[x, 0.0], [x + 1.0, 0.0], [x, 1.0]]
    }

    fn model() -> Model {
        let mut m = Model::new();
        let base = RotationBase {
            origin: [0.0, 0.0],
            angle_degrees: 10.0,
            scale: 1.5,
            reflect_x: false,
            reflect_y: false,
        };
        let mesh = |name: &str, part: &str, x: f32| Command::AddMesh {
            name: name.into(),
            part: part.into(),
            positions: tri(x),
            uvs: tri(0.0),
            indices: vec![0, 1, 2],
        };
        for c in [
            Command::AddPart {
                name: "Head".into(),
                parent: None,
            },
            Command::AddPart {
                name: "Hair".into(),
                parent: Some("Head".into()),
            },
            Command::AddPart {
                name: "Arm".into(),
                parent: None,
            },
            mesh("Skin", "Head", 0.0),
            mesh("Fringe", "Hair", 2.0),
            mesh("Sleeve", "Arm", 4.0),
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "Head".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            },
            Command::AddRotationDeformer {
                name: "R".into(),
                part: "Hair".into(),
                parent: Some("W".into()),
                base,
            },
            Command::SetMeshParent {
                name: "Fringe".into(),
                parent: Some("R".into()),
            },
            Command::SetMeshOpacity {
                name: "Fringe".into(),
                opacity: 0.5,
            },
            Command::SetMeshMasks {
                name: "Fringe".into(),
                masks: vec!["Skin".into()],
                inverted: true,
            },
        ] {
            m.apply(c).unwrap();
        }
        m
    }

    #[test]
    fn deleting_a_part_removes_what_is_under_it_in_one_undoable_batch() {
        let mut d = Dispatcher::new(model());
        let plan = delete_plan(d.model(), &[Selection::Part("Head".into())]);
        // Skin, Fringe; R before W; Hair before Head.
        assert_eq!(plan.count, 6);
        assert!(plan.needs_confirm);
        let pos = |c: &Command| plan.commands.iter().position(|x| x == c).unwrap();
        assert!(
            pos(&Command::RemoveDeformer { name: "R".into() })
                < pos(&Command::RemoveDeformer { name: "W".into() })
        );
        assert!(
            pos(&Command::RemovePart {
                name: "Hair".into()
            }) < pos(&Command::RemovePart {
                name: "Head".into()
            })
        );
        // The mask of Fringe is Skin, which goes too, so the batch holds together.
        let before = d.model().meshes().len();
        d.submit_labeled(
            Command::Batch {
                commands: plan.commands,
            },
            Some("tree_delete: Head".into()),
        )
        .unwrap();
        assert_eq!(d.model().meshes().len(), before - 2);
        assert!(d.model().parts().find("Head").is_none() && d.model().warps().find("W").is_none());
        assert!(d.model().validate().is_empty());
        assert_eq!(d.log().len(), 1);
        assert!(d.undo());
        assert!(
            d.model().parts().find("Hair").is_some() && d.model().rotations().find("R").is_some()
        );
        assert!(d.model().validate().is_empty());
    }

    #[test]
    fn deleting_a_mesh_or_a_deformer_needs_no_confirmation_and_failures_leave_the_model() {
        let mut d = Dispatcher::new(model());
        let plan = delete_plan(d.model(), &[Selection::Mesh("Sleeve".into())]);
        assert!(!plan.needs_confirm);
        assert_eq!(
            plan.commands,
            [Command::RemoveMesh {
                name: "Sleeve".into()
            }]
        );
        // A deformer with children cannot go: the command fails and nothing changes.
        let plan = delete_plan(d.model(), &[Selection::Warp("W".into())]);
        assert!(!plan.needs_confirm);
        let steps = d.log().len();
        assert!(
            d.submit(Command::Batch {
                commands: plan.commands
            })
            .is_err()
        );
        assert_eq!(d.log().len(), steps);
        // A mesh used as a mask cannot go either.
        let plan = delete_plan(d.model(), &[Selection::Mesh("Skin".into())]);
        assert!(
            d.submit(Command::Batch {
                commands: plan.commands
            })
            .is_err()
        );
        // Several at once, each once.
        let two = [
            Selection::Mesh("Sleeve".into()),
            Selection::Mesh("Sleeve".into()),
            Selection::Parameter("P".into()),
        ];
        assert_eq!(delete_plan(d.model(), &two).count, 1);
    }

    #[test]
    fn a_duplicate_looks_like_the_original_and_takes_a_fresh_name() {
        let mut d = Dispatcher::new(model());
        let (commands, made) = duplicate_commands(
            d.model(),
            &[
                Selection::Mesh("Fringe".into()),
                Selection::Warp("W".into()),
            ],
        )
        .unwrap();
        assert_eq!(
            made,
            [
                Selection::Mesh("Fringe.copy".into()),
                Selection::Warp("W.copy".into())
            ]
        );
        d.submit_labeled(
            Command::Batch { commands },
            Some("tree_duplicate: Fringe".into()),
        )
        .unwrap();
        let m = d.model();
        let copy = m
            .meshes()
            .get(m.meshes().find("Fringe.copy").unwrap())
            .unwrap();
        let orig = m.meshes().get(m.meshes().find("Fringe").unwrap()).unwrap();
        assert_eq!(
            (
                copy.positions.clone(),
                copy.opacity,
                copy.parent,
                copy.masks.clone(),
                copy.mask_inverted,
                copy.part
            ),
            (
                orig.positions.clone(),
                0.5,
                orig.parent,
                orig.masks.clone(),
                true,
                orig.part
            )
        );
        assert!(m.warps().find("W.copy").is_some());
        assert!(m.validate().is_empty());
        // The numbered name when the copy exists.
        let (_, again) = duplicate_commands(m, &[Selection::Mesh("Fringe".into())]).unwrap();
        assert_eq!(again, [Selection::Mesh("Fringe.copy2".into())]);
        // The same mesh twice in one go gets two names.
        let (_, twice) = duplicate_commands(
            m,
            &[
                Selection::Mesh("Sleeve".into()),
                Selection::Mesh("Sleeve".into()),
            ],
        )
        .unwrap();
        assert_eq!(
            twice,
            [
                Selection::Mesh("Sleeve.copy".into()),
                Selection::Mesh("Sleeve.copy2".into())
            ]
        );
        assert!(d.undo());
        assert!(
            d.model().meshes().find("Fringe.copy").is_none()
                && d.model().warps().find("W.copy").is_none()
        );
    }

    #[test]
    fn parts_keyforms_and_unknown_things_are_not_duplicated() {
        let m = model();
        assert_eq!(
            duplicate_commands(&m, &[Selection::Part("Head".into())]),
            Err(DuplicateError::Kind)
        );
        assert_eq!(
            duplicate_commands(&m, &[Selection::Mesh("Nope".into())]),
            Err(DuplicateError::Missing("Nope".into()))
        );
        assert_eq!(
            duplicate_commands(&m, &[Selection::Parameter("P".into())]),
            Err(DuplicateError::Kind)
        );
    }

    #[test]
    fn keyframed_elements_are_refused() {
        use tsukumoe_core::elements::{Parameter, Part};
        use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
        use tsukumoe_core::model::ModelParts;
        use tsukumoe_core::rig::{ArtMesh, MeshKeyform, WarpDeformer, WarpKeyform};
        let mut parts = ModelParts::default();
        let p = parts
            .parameters
            .insert("P", Parameter::new(0.0, 1.0, 0.0))
            .unwrap();
        let part = parts.parts.insert("Part", Part::default()).unwrap();
        let binding = parts.bindings.insert(
            Binding::new(vec![Axis {
                parameter: p,
                keys: vec![0.0, 1.0],
            }])
            .unwrap(),
        );
        let mut mesh = ArtMesh::new(part, tri(0.0), tri(0.0), vec![0, 1, 2]);
        let key = MeshKeyform {
            positions: tri(0.0),
            opacity: 1.0,
            draw_order: 500.0,
            multiply_color: [1.0; 3],
            screen_color: [0.0; 3],
        };
        mesh.keyforms = Some(KeyformGrid::new(binding, vec![key.clone(), key]));
        parts.meshes.insert("Keyed", mesh).unwrap();
        let grid = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let wk = WarpKeyform {
            points: grid.clone(),
            opacity: 1.0,
        };
        parts
            .warps
            .insert(
                "KeyedWarp",
                WarpDeformer {
                    part,
                    parent: None,
                    rows: 1,
                    columns: 1,
                    bezier: false,
                    base_points: grid,
                    rest_points: None,
                    keyforms: Some(KeyformGrid::new(binding, vec![wk.clone(), wk])),
                },
            )
            .unwrap();
        let m = Model::from_parts(parts).unwrap();
        assert_eq!(
            duplicate_commands(&m, &[Selection::Mesh("Keyed".into())]),
            Err(DuplicateError::Unsupported("Keyed".into()))
        );
        assert_eq!(
            duplicate_commands(&m, &[Selection::Warp("KeyedWarp".into())]),
            Err(DuplicateError::Unsupported("KeyedWarp".into()))
        );
    }

    #[test]
    fn rename_and_part_commands_are_built_for_each_kind() {
        assert_eq!(
            rename_command(Kind::Warp, "a", "b"),
            Command::RenameWarp {
                name: "a".into(),
                new_name: "b".into()
            }
        );
        assert_eq!(
            renamed(Kind::Rotation, "x"),
            Selection::Rotation("x".into())
        );
        let mesh = Selection::Mesh("m".into());
        assert_eq!(
            set_part_command(&mesh, Some("P")),
            Some(Command::SetMeshPart {
                name: "m".into(),
                part: "P".into()
            })
        );
        assert_eq!(set_part_command(&mesh, None), None);
        assert_eq!(
            set_part_command(&Selection::Part("Q".into()), None),
            Some(Command::SetPartParent {
                name: "Q".into(),
                parent: None
            })
        );
        assert_eq!(
            set_part_command(&Selection::Parameter("p".into()), Some("P")),
            None
        );
    }
}
