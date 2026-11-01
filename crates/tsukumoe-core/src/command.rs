// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Edit commands and the only way to change a model: [`Model::apply`].
//!
//! Commands name elements by their persistent id, never by internal [`Id`],
//! so they survive a save and load (ADR-0019).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::arena::Id;
use crate::elements::{Parameter, Part, Point};
use crate::history::{Change, Edit, Editable, Record};
use crate::model::Model;
use crate::rig::{
    ArtMesh, BlendMode, DeformerRef, RotationDeformer, RotationKeyform, WarpDeformer,
};
use crate::table::InsertError;
use crate::validate::ModelError;

/// The base shape of a rotation deformer: origin, angle in degrees, scale and
/// reflections (the part of a rotation keyform that is not opacity).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RotationBase {
    pub origin: Point,
    pub angle_degrees: f32,
    pub scale: f32,
    #[serde(default)]
    pub reflect_x: bool,
    #[serde(default)]
    pub reflect_y: bool,
}

impl RotationBase {
    pub fn of(base: &RotationKeyform) -> Self {
        Self {
            origin: base.origin,
            angle_degrees: base.angle_degrees,
            scale: base.scale,
            reflect_x: base.reflect_x,
            reflect_y: base.reflect_y,
        }
    }

    fn apply_to(&self, base: &mut RotationKeyform) {
        base.origin = self.origin;
        base.angle_degrees = self.angle_degrees;
        base.scale = self.scale;
        base.reflect_x = self.reflect_x;
        base.reflect_y = self.reflect_y;
    }
}

/// An edit to a model. In JSON, `type` names the variant in snake case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Adds a parameter with a value range and default.
    AddParameter {
        name: String,
        min: f32,
        max: f32,
        default: f32,
        #[serde(default)]
        repeat: bool,
    },
    /// Removes a parameter by name.
    RemoveParameter { name: String },
    /// Changes the range and default of a parameter.
    SetParameterRange {
        name: String,
        min: f32,
        max: f32,
        default: f32,
    },
    /// Adds a part, optionally under a parent part.
    AddPart {
        name: String,
        #[serde(default)]
        parent: Option<String>,
    },
    /// Removes a part by name; fails while a mesh uses it.
    RemovePart { name: String },
    /// Sets the opacity of a part.
    SetPartOpacity { name: String, opacity: f32 },
    /// Shows or hides a part; a hidden part hides every mesh under it.
    SetPartVisible { name: String, visible: bool },
    /// Adds an art mesh to a part; positions and uvs are [x, y] pairs, indices are triangle vertex indices.
    AddMesh {
        name: String,
        part: String,
        positions: Vec<Point>,
        uvs: Vec<Point>,
        indices: Vec<u32>,
    },
    /// Removes an art mesh by name.
    RemoveMesh { name: String },
    /// Replaces the vertex positions of a mesh; the count must match its uvs.
    SetMeshPositions { name: String, positions: Vec<Point> },
    /// Replaces the vertices, UVs and triangles of a mesh at once; counts must match as in `add_mesh`. Fails if the vertex count changes while keyforms, bone influences, blend shapes or glue depend on the vertices.
    SetMeshGeometry {
        name: String,
        positions: Vec<Point>,
        uvs: Vec<Point>,
        indices: Vec<u32>,
    },
    /// Sets the texture index of a mesh.
    SetMeshTexture { name: String, texture: u32 },
    /// Sets how a mesh blends with what is behind it.
    SetMeshBlendMode { name: String, blend_mode: BlendMode },
    /// Sets the opacity of a mesh.
    SetMeshOpacity { name: String, opacity: f32 },
    /// Sets the draw order of a mesh (ascending order is back to front).
    SetMeshDrawOrder { name: String, draw_order: f32 },
    /// Replaces the UVs of a mesh; the count must match its positions.
    SetMeshUvs { name: String, uvs: Vec<Point> },
    /// Sets the meshes that clip a mesh (by name) and whether the clip is inverted; an empty list removes the clipping.
    SetMeshMasks {
        name: String,
        masks: Vec<String>,
        #[serde(default)]
        inverted: bool,
    },
    /// Shows or hides a mesh; a hidden mesh is not in the draw list.
    SetMeshVisible { name: String, visible: bool },
    /// Adds a warp deformer: a grid of `(rows + 1) * (columns + 1)` control points
    /// (row by row) in the space of its parent deformer, or in the drawing
    /// space without one. Deformer names are shared by warp and rotation deformers.
    AddWarpDeformer {
        name: String,
        part: String,
        #[serde(default)]
        parent: Option<String>,
        rows: u32,
        columns: u32,
        base_points: Vec<Point>,
    },
    /// Adds a rotation deformer with its base shape.
    AddRotationDeformer {
        name: String,
        part: String,
        #[serde(default)]
        parent: Option<String>,
        base: RotationBase,
    },
    /// Removes a deformer (warp or rotation) by name; fails while a mesh or a deformer hangs from it.
    RemoveDeformer { name: String },
    /// Sets the parent deformer of a deformer (`null` for none). The command
    /// does not convert coordinates; fails if it makes a cycle or if the
    /// deformer has keyforms.
    SetDeformerParent {
        name: String,
        #[serde(default)]
        parent: Option<String>,
    },
    /// Sets the parent deformer of a mesh (`null` for none). Fails if the mesh has keyforms.
    SetMeshParent {
        name: String,
        #[serde(default)]
        parent: Option<String>,
    },
    /// Replaces the base control points of a warp deformer; the count must stay `(rows + 1) * (columns + 1)`. Fails if it has keyforms.
    SetWarpBasePoints { name: String, points: Vec<Point> },
    /// Replaces the base shape of a rotation deformer. Fails if it has keyforms.
    SetRotationBase { name: String, base: RotationBase },
    /// Renames a part. Everything that refers to it keeps pointing at it.
    RenamePart { name: String, new_name: String },
    /// Renames a mesh.
    RenameMesh { name: String, new_name: String },
    /// Renames a warp deformer. Warp and rotation deformers share their names.
    RenameWarp { name: String, new_name: String },
    /// Renames a rotation deformer.
    RenameRotation { name: String, new_name: String },
    /// Moves a mesh to another part.
    SetMeshPart { name: String, part: String },
    /// Moves a deformer (warp or rotation) to another part.
    SetDeformerPart { name: String, part: String },
    /// Sets the parent part of a part (`null` for none); fails if it makes a cycle.
    SetPartParent {
        name: String,
        #[serde(default)]
        parent: Option<String>,
    },
    /// Applies the commands in order as one undo step; if one fails, none
    /// of them is applied.
    Batch { commands: Vec<Command> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandError {
    NotFound {
        kind: &'static str,
        name: String,
    },
    AlreadyExists {
        kind: &'static str,
        name: String,
    },
    EmptyName {
        kind: &'static str,
    },
    /// The element has keyforms, whose coordinates this edit would invalidate.
    HasKeyforms {
        kind: &'static str,
        name: String,
    },
    /// The command would leave the model with problems it did not have.
    Invalid(Vec<ModelError>),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { kind, name } => write!(f, "no {kind} named '{name}'"),
            Self::AlreadyExists { kind, name } => write!(f, "{kind} '{name}' already exists"),
            Self::EmptyName { kind } => write!(f, "{kind} name is empty"),
            Self::HasKeyforms { kind, name } => {
                write!(
                    f,
                    "{kind} '{name}' has keyforms; its shape cannot be edited here"
                )
            }
            Self::Invalid(errors) => write!(f, "command would break the model: {errors:?}"),
        }
    }
}

impl std::error::Error for CommandError {}

impl Model {
    /// Applies `command` as one undoable step. On error the model is left
    /// exactly as it was.
    pub fn apply(&mut self, command: Command) -> Result<(), CommandError> {
        let mut changes = Vec::new();
        if let Err(error) = self.run(&command, &mut changes) {
            self.rollback(&changes);
            return Err(error);
        }
        let after = self.validate();
        if !after.is_empty() {
            // Only problems the command introduced count against it.
            self.rollback(&changes);
            let before = self.validate();
            let introduced: Vec<_> = after.into_iter().filter(|e| !before.contains(e)).collect();
            if !introduced.is_empty() {
                return Err(CommandError::Invalid(introduced));
            }
            for change in &changes {
                change.redo(self);
            }
        }
        self.history.undo.push(Record { command, changes });
        self.history.redo.clear();
        Ok(())
    }

    /// Reverts the last applied command. False if there is none.
    pub fn undo(&mut self) -> bool {
        let Some(record) = self.history.undo.pop() else {
            return false;
        };
        self.rollback(&record.changes);
        self.history.redo.push(record);
        true
    }

    /// Re-applies the last undone command. False if there is none.
    pub fn redo(&mut self) -> bool {
        let Some(record) = self.history.redo.pop() else {
            return false;
        };
        for change in &record.changes {
            change.redo(self);
        }
        self.history.undo.push(record);
        true
    }

    fn rollback(&mut self, changes: &[Change]) {
        for change in changes.iter().rev() {
            change.undo(self);
        }
    }

    fn run(&mut self, command: &Command, changes: &mut Vec<Change>) -> Result<(), CommandError> {
        match command {
            Command::AddParameter {
                name,
                min,
                max,
                default,
                repeat,
            } => {
                let mut parameter = Parameter::new(*min, *max, *default);
                parameter.repeat = *repeat;
                self.insert(changes, name, parameter)?;
            }
            Command::RemoveParameter { name } => {
                let id = self.resolve::<Parameter>(name)?;
                self.remove(changes, id);
            }
            Command::SetParameterRange {
                name,
                min,
                max,
                default,
            } => {
                let id = self.resolve::<Parameter>(name)?;
                self.modify(changes, id, |p| {
                    p.min = *min;
                    p.max = *max;
                    p.default = *default;
                });
            }
            Command::AddPart { name, parent } => {
                let parent = parent
                    .as_deref()
                    .map(|n| self.resolve::<Part>(n))
                    .transpose()?;
                self.insert(
                    changes,
                    name,
                    Part {
                        parent,
                        ..Part::default()
                    },
                )?;
            }
            Command::RemovePart { name } => {
                let id = self.resolve::<Part>(name)?;
                self.remove(changes, id);
            }
            Command::SetPartOpacity { name, opacity } => {
                let id = self.resolve::<Part>(name)?;
                self.modify(changes, id, |p| p.opacity = *opacity);
            }
            Command::SetPartVisible { name, visible } => {
                let id = self.resolve::<Part>(name)?;
                self.modify(changes, id, |p| p.visible = *visible);
            }
            Command::SetMeshTexture { name, texture } => {
                let id = self.resolve::<ArtMesh>(name)?;
                self.modify(changes, id, |m| m.texture = *texture);
            }
            Command::SetMeshBlendMode { name, blend_mode } => {
                let id = self.resolve::<ArtMesh>(name)?;
                self.modify(changes, id, |m| m.blend_mode = *blend_mode);
            }
            Command::SetMeshOpacity { name, opacity } => {
                let id = self.resolve::<ArtMesh>(name)?;
                self.modify(changes, id, |m| m.opacity = *opacity);
            }
            Command::SetMeshDrawOrder { name, draw_order } => {
                let id = self.resolve::<ArtMesh>(name)?;
                self.modify(changes, id, |m| m.draw_order = *draw_order);
            }
            Command::SetMeshUvs { name, uvs } => {
                let id = self.resolve::<ArtMesh>(name)?;
                self.modify(changes, id, |m| m.uvs = uvs.clone());
            }
            Command::SetMeshMasks {
                name,
                masks,
                inverted,
            } => {
                let id = self.resolve::<ArtMesh>(name)?;
                let masks = masks
                    .iter()
                    .map(|n| self.resolve::<ArtMesh>(n))
                    .collect::<Result<Vec<_>, _>>()?;
                self.modify(changes, id, |m| {
                    m.masks = masks;
                    m.mask_inverted = *inverted;
                });
            }
            Command::SetMeshVisible { name, visible } => {
                let id = self.resolve::<ArtMesh>(name)?;
                self.modify(changes, id, |m| m.visible = *visible);
            }
            Command::AddMesh {
                name,
                part,
                positions,
                uvs,
                indices,
            } => {
                let part = self.resolve::<Part>(part)?;
                let mesh = ArtMesh::new(part, positions.clone(), uvs.clone(), indices.clone());
                self.insert(changes, name, mesh)?;
            }
            Command::RemoveMesh { name } => {
                let id = self.resolve::<ArtMesh>(name)?;
                self.remove(changes, id);
            }
            Command::SetMeshPositions { name, positions } => {
                let id = self.resolve::<ArtMesh>(name)?;
                self.modify(changes, id, |m| m.positions = positions.clone());
            }
            Command::SetMeshGeometry {
                name,
                positions,
                uvs,
                indices,
            } => {
                let id = self.resolve::<ArtMesh>(name)?;
                let count_changes = self
                    .meshes
                    .get(id)
                    .is_some_and(|m| m.positions.len() != positions.len());
                // Keyforms, influences and blend shapes are caught by validation;
                // glue nodes stay in range after a removal but would point at the wrong vertices.
                if count_changes {
                    let glued: Vec<_> = self
                        .glues
                        .iter()
                        .filter(|(_, _, g)| {
                            (g.mesh_a == id || g.mesh_b == id) && !g.nodes.is_empty()
                        })
                        .map(|(_, glue, _)| ModelError::GeometryMismatch {
                            owner: format!("glue '{glue}'"),
                            field: "nodes",
                        })
                        .collect();
                    if !glued.is_empty() {
                        return Err(CommandError::Invalid(glued));
                    }
                }
                self.modify(changes, id, |m| {
                    m.positions = positions.clone();
                    m.uvs = uvs.clone();
                    m.indices = indices.clone();
                });
            }
            Command::AddWarpDeformer {
                name,
                part,
                parent,
                rows,
                columns,
                base_points,
            } => {
                self.check_deformer_name(name)?;
                let part = self.resolve::<Part>(part)?;
                let parent = self.resolve_parent(parent.as_deref())?;
                if *rows == 0 || *columns == 0 {
                    return Err(CommandError::Invalid(vec![ModelError::GeometryMismatch {
                        owner: format!("warp deformer '{name}'"),
                        field: "rows",
                    }]));
                }
                let warp = WarpDeformer {
                    part,
                    parent,
                    rows: *rows,
                    columns: *columns,
                    bezier: false,
                    base_points: base_points.clone(),
                    keyforms: None,
                };
                self.insert(changes, name, warp)?;
            }
            Command::AddRotationDeformer {
                name,
                part,
                parent,
                base,
            } => {
                self.check_deformer_name(name)?;
                let part = self.resolve::<Part>(part)?;
                let parent = self.resolve_parent(parent.as_deref())?;
                let mut keyform = RotationKeyform::default();
                base.apply_to(&mut keyform);
                let rotation = RotationDeformer {
                    part,
                    parent,
                    base: keyform,
                    keyforms: None,
                };
                self.insert(changes, name, rotation)?;
            }
            Command::RemoveDeformer { name } => match self.resolve_deformer(name)? {
                DeformerRef::Warp(id) => self.remove(changes, id),
                DeformerRef::Rotation(id) => self.remove(changes, id),
            },
            Command::SetDeformerParent { name, parent } => {
                let parent = self.resolve_parent(parent.as_deref())?;
                match self.resolve_deformer(name)? {
                    DeformerRef::Warp(id) => {
                        self.reject_keyforms::<WarpDeformer>(id, name, |d| d.keyforms.is_some())?;
                        self.modify(changes, id, |d| d.parent = parent);
                    }
                    DeformerRef::Rotation(id) => {
                        self.reject_keyforms::<RotationDeformer>(id, name, |d| {
                            d.keyforms.is_some()
                        })?;
                        self.modify(changes, id, |d| d.parent = parent);
                    }
                }
            }
            Command::SetMeshParent { name, parent } => {
                let parent = self.resolve_parent(parent.as_deref())?;
                let id = self.resolve::<ArtMesh>(name)?;
                self.reject_keyforms::<ArtMesh>(id, name, |m| m.keyforms.is_some())?;
                self.modify(changes, id, |m| m.parent = parent);
            }
            Command::SetWarpBasePoints { name, points } => {
                let id = self.resolve::<WarpDeformer>(name)?;
                self.reject_keyforms::<WarpDeformer>(id, name, |d| d.keyforms.is_some())?;
                self.modify(changes, id, |d| d.base_points = points.clone());
            }
            Command::SetRotationBase { name, base } => {
                let id = self.resolve::<RotationDeformer>(name)?;
                self.reject_keyforms::<RotationDeformer>(id, name, |d| d.keyforms.is_some())?;
                self.modify(changes, id, |d| base.apply_to(&mut d.base));
            }
            Command::RenamePart { name, new_name } => {
                let id = self.resolve::<Part>(name)?;
                self.rename(changes, id, new_name)?;
            }
            Command::RenameMesh { name, new_name } => {
                let id = self.resolve::<ArtMesh>(name)?;
                self.rename(changes, id, new_name)?;
            }
            Command::RenameWarp { name, new_name } => {
                let id = self.resolve::<WarpDeformer>(name)?;
                self.check_deformer_name(new_name)?;
                self.rename(changes, id, new_name)?;
            }
            Command::RenameRotation { name, new_name } => {
                let id = self.resolve::<RotationDeformer>(name)?;
                self.check_deformer_name(new_name)?;
                self.rename(changes, id, new_name)?;
            }
            Command::SetMeshPart { name, part } => {
                let part = self.resolve::<Part>(part)?;
                let id = self.resolve::<ArtMesh>(name)?;
                self.modify(changes, id, |m| m.part = part);
            }
            Command::SetDeformerPart { name, part } => {
                let part = self.resolve::<Part>(part)?;
                match self.resolve_deformer(name)? {
                    DeformerRef::Warp(id) => self.modify(changes, id, |d| d.part = part),
                    DeformerRef::Rotation(id) => self.modify(changes, id, |d| d.part = part),
                }
            }
            Command::SetPartParent { name, parent } => {
                let parent = parent
                    .as_deref()
                    .map(|n| self.resolve::<Part>(n))
                    .transpose()?;
                let id = self.resolve::<Part>(name)?;
                self.modify(changes, id, |p| p.parent = parent);
            }
            Command::Batch { commands } => {
                for command in commands {
                    self.run(command, changes)?;
                }
            }
        }
        Ok(())
    }

    /// A deformer by name: warp and rotation deformers share one namespace.
    fn resolve_deformer(&self, name: &str) -> Result<DeformerRef, CommandError> {
        if let Some(id) = self.warps.find(name) {
            Ok(DeformerRef::Warp(id))
        } else if let Some(id) = self.rotations.find(name) {
            Ok(DeformerRef::Rotation(id))
        } else {
            Err(CommandError::NotFound {
                kind: "deformer",
                name: name.to_owned(),
            })
        }
    }

    fn resolve_parent(&self, name: Option<&str>) -> Result<Option<DeformerRef>, CommandError> {
        name.map(|n| self.resolve_deformer(n)).transpose()
    }

    fn check_deformer_name(&self, name: &str) -> Result<(), CommandError> {
        if self.warps.find(name).is_some() || self.rotations.find(name).is_some() {
            return Err(CommandError::AlreadyExists {
                kind: "deformer",
                name: name.to_owned(),
            });
        }
        Ok(())
    }

    fn reject_keyforms<T: Editable>(
        &self,
        id: Id<T>,
        name: &str,
        has: impl Fn(&T) -> bool,
    ) -> Result<(), CommandError> {
        if T::table(self).get(id).is_some_and(has) {
            return Err(CommandError::HasKeyforms {
                kind: T::KIND,
                name: name.to_owned(),
            });
        }
        Ok(())
    }

    /// Gives an element a new name, keeping its id. History sees it as the
    /// removal of the old name and the insertion of the new one at the same id.
    fn rename<T: Editable>(
        &mut self,
        changes: &mut Vec<Change>,
        id: Id<T>,
        new_name: &str,
    ) -> Result<(), CommandError> {
        if new_name.is_empty() {
            return Err(CommandError::EmptyName { kind: T::KIND });
        }
        let table = T::table_mut(self);
        let Some(old) = table.name(id).map(str::to_owned) else {
            return Ok(());
        };
        if old == new_name {
            return Ok(());
        }
        if table.find(new_name).is_some() {
            return Err(CommandError::AlreadyExists {
                kind: T::KIND,
                name: new_name.to_owned(),
            });
        }
        let Some(value) = table.remove(id) else {
            return Ok(());
        };
        changes.push(T::change(Edit {
            id,
            name: old,
            before: Some(value.clone()),
            after: None,
        }));
        T::table_mut(self).put(id, new_name, value.clone());
        changes.push(T::change(Edit {
            id,
            name: new_name.to_owned(),
            before: None,
            after: Some(value),
        }));
        Ok(())
    }

    fn resolve<T: Editable>(&self, name: &str) -> Result<Id<T>, CommandError> {
        T::table(self)
            .find(name)
            .ok_or_else(|| CommandError::NotFound {
                kind: T::KIND,
                name: name.to_owned(),
            })
    }

    fn insert<T: Editable>(
        &mut self,
        changes: &mut Vec<Change>,
        name: &str,
        value: T,
    ) -> Result<Id<T>, CommandError> {
        let id = T::table_mut(self)
            .insert(name, value.clone())
            .map_err(|error| match error {
                InsertError::EmptyName => CommandError::EmptyName { kind: T::KIND },
                InsertError::DuplicateName => CommandError::AlreadyExists {
                    kind: T::KIND,
                    name: name.to_owned(),
                },
            })?;
        changes.push(T::change(Edit {
            id,
            name: name.to_owned(),
            before: None,
            after: Some(value),
        }));
        Ok(id)
    }

    fn remove<T: Editable>(&mut self, changes: &mut Vec<Change>, id: Id<T>) {
        let table = T::table_mut(self);
        let Some(name) = table.name(id).map(str::to_owned) else {
            return;
        };
        let Some(value) = table.remove(id) else {
            return;
        };
        changes.push(T::change(Edit {
            id,
            name,
            before: Some(value),
            after: None,
        }));
    }

    fn modify<T: Editable>(
        &mut self,
        changes: &mut Vec<Change>,
        id: Id<T>,
        edit: impl FnOnce(&mut T),
    ) {
        let table = T::table_mut(self);
        let Some(name) = table.name(id).map(str::to_owned) else {
            return;
        };
        let Some(value) = table.get_mut(id) else {
            return;
        };
        let before = value.clone();
        edit(value);
        let after = value.clone();
        changes.push(T::change(Edit {
            id,
            name,
            before: Some(before),
            after: Some(after),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri() -> (Vec<Point>, Vec<Point>, Vec<u32>) {
        let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        (pts.clone(), pts, vec![0, 1, 2])
    }

    fn add_part(name: &str) -> Command {
        Command::AddPart {
            name: name.into(),
            parent: None,
        }
    }

    fn add_mesh(name: &str, part: &str) -> Command {
        let (positions, uvs, indices) = tri();
        Command::AddMesh {
            name: name.into(),
            part: part.into(),
            positions,
            uvs,
            indices,
        }
    }

    fn add_param(name: &str) -> Command {
        Command::AddParameter {
            name: name.into(),
            min: -30.0,
            max: 30.0,
            default: 0.0,
            repeat: false,
        }
    }

    fn scene() -> Vec<Command> {
        vec![
            add_param("ParamAngleX"),
            add_part("Face"),
            add_mesh("Skin", "Face"),
            Command::SetPartOpacity {
                name: "Face".into(),
                opacity: 0.5,
            },
            Command::SetMeshPositions {
                name: "Skin".into(),
                positions: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]],
            },
            Command::SetMeshTexture {
                name: "Skin".into(),
                texture: 2,
            },
            Command::SetMeshBlendMode {
                name: "Skin".into(),
                blend_mode: BlendMode::Multiply,
            },
            Command::SetMeshOpacity {
                name: "Skin".into(),
                opacity: 0.25,
            },
            Command::SetMeshDrawOrder {
                name: "Skin".into(),
                draw_order: 700.0,
            },
            Command::SetMeshUvs {
                name: "Skin".into(),
                uvs: vec![[0.0, 0.0], [0.5, 0.0], [0.0, 0.5]],
            },
            Command::SetMeshGeometry {
                name: "Skin".into(),
                positions: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [2.0, 2.0]],
                uvs: vec![[0.0, 0.0], [0.5, 0.0], [0.0, 0.5], [0.5, 0.5]],
                indices: vec![0, 1, 2, 2, 1, 3],
            },
            add_mesh("Clip", "Face"),
            Command::SetMeshMasks {
                name: "Skin".into(),
                masks: vec!["Clip".into()],
                inverted: true,
            },
            Command::SetMeshVisible {
                name: "Skin".into(),
                visible: false,
            },
            Command::SetPartVisible {
                name: "Face".into(),
                visible: false,
            },
            warp("Warp"),
            rotation("Rot", Some("Warp")),
            Command::SetWarpBasePoints {
                name: "Warp".into(),
                points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 2.0], [1.0, 2.0]],
            },
            Command::SetRotationBase {
                name: "Rot".into(),
                base: RotationBase {
                    origin: [0.5, 0.5],
                    angle_degrees: 30.0,
                    scale: 2.0,
                    reflect_x: true,
                    reflect_y: false,
                },
            },
            Command::SetMeshParent {
                name: "Skin".into(),
                parent: Some("Rot".into()),
            },
            Command::SetDeformerParent {
                name: "Rot".into(),
                parent: None,
            },
            Command::SetMeshParent {
                name: "Skin".into(),
                parent: None,
            },
            Command::RemoveDeformer { name: "Rot".into() },
            add_part("Hair"),
            Command::SetPartParent {
                name: "Hair".into(),
                parent: Some("Face".into()),
            },
            Command::SetMeshPart {
                name: "Skin".into(),
                part: "Hair".into(),
            },
            Command::RenameMesh {
                name: "Skin".into(),
                new_name: "Scalp".into(),
            },
            Command::RenamePart {
                name: "Hair".into(),
                new_name: "Fringe".into(),
            },
            Command::RenameWarp {
                name: "Warp".into(),
                new_name: "HeadWarp".into(),
            },
            rotation("Spin", None),
            Command::RenameRotation {
                name: "Spin".into(),
                new_name: "Turn".into(),
            },
            Command::SetDeformerPart {
                name: "Turn".into(),
                part: "Fringe".into(),
            },
        ]
    }

    fn warp(name: &str) -> Command {
        Command::AddWarpDeformer {
            name: name.into(),
            part: "Face".into(),
            parent: None,
            rows: 1,
            columns: 1,
            base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        }
    }

    fn rotation(name: &str, parent: Option<&str>) -> Command {
        Command::AddRotationDeformer {
            name: name.into(),
            part: "Face".into(),
            parent: parent.map(Into::into),
            base: RotationBase {
                origin: [0.0, 0.0],
                angle_degrees: 0.0,
                scale: 1.0,
                reflect_x: false,
                reflect_y: false,
            },
        }
    }

    fn built(commands: Vec<Command>) -> Model {
        let mut model = Model::new();
        for command in commands {
            model.apply(command).unwrap();
        }
        model
    }

    #[test]
    fn every_command_undoes_and_redoes() {
        let commands = scene();
        for n in 1..=commands.len() {
            let mut model = built(commands[..n].to_vec());
            let expected = built(commands[..n - 1].to_vec());
            let full = built(commands[..n].to_vec());
            assert!(model.undo());
            assert!(model == expected, "undo of command {n}");
            assert!(model.redo());
            assert!(model == full, "redo of command {n}");
        }
    }

    #[test]
    fn replay_through_json_gives_the_same_model() {
        let original = built(scene());
        let json =
            serde_json::to_string(&original.history().commands().collect::<Vec<_>>()).unwrap();
        let commands: Vec<Command> = serde_json::from_str(&json).unwrap();
        assert_eq!(commands, scene());
        assert!(built(commands) == original);
    }

    #[test]
    fn json_form_is_tagged_by_type() {
        let json = serde_json::to_value(add_part("Face")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "add_part", "name": "Face", "parent": null})
        );
        let parsed: Command = serde_json::from_str(
            r#"{"type":"add_parameter","name":"P","min":0,"max":1,"default":0}"#,
        )
        .unwrap();
        assert_eq!(
            parsed,
            Command::AddParameter {
                name: "P".into(),
                min: 0.0,
                max: 1.0,
                default: 0.0,
                repeat: false,
            }
        );
    }

    #[test]
    fn undoing_a_removal_restores_the_same_id() {
        let mut model = built(vec![add_part("Face"), add_mesh("Skin", "Face")]);
        let part = model.parts().find("Face").unwrap();
        model
            .apply(Command::RemoveMesh {
                name: "Skin".into(),
            })
            .unwrap();
        let mesh_count = model.meshes().len();
        assert_eq!(mesh_count, 0);
        assert!(model.undo());
        let mesh = model
            .meshes()
            .get(model.meshes().find("Skin").unwrap())
            .unwrap();
        assert_eq!(mesh.part, part);
        assert!(model.validate().is_empty());
    }

    #[test]
    fn removing_a_part_in_use_is_rejected_and_changes_nothing() {
        let mut model = built(vec![add_part("Face"), add_mesh("Skin", "Face")]);
        let expected = built(vec![add_part("Face"), add_mesh("Skin", "Face")]);
        let result = model.apply(Command::RemovePart {
            name: "Face".into(),
        });
        assert!(matches!(result, Err(CommandError::Invalid(_))));
        assert!(model == expected);
        assert_eq!(model.history().commands().count(), 2);
    }

    #[test]
    fn failed_batch_changes_nothing() {
        let mut model = built(vec![add_param("A")]);
        let result = model.apply(Command::Batch {
            commands: vec![add_param("B"), add_param("A")],
        });
        assert_eq!(
            result,
            Err(CommandError::AlreadyExists {
                kind: "parameter",
                name: "A".into()
            })
        );
        assert!(model.parameters().find("B").is_none());
        assert!(model == built(vec![add_param("A")]));
    }

    #[test]
    fn batch_is_one_undo_step() {
        let mut model = Model::new();
        model
            .apply(Command::Batch {
                commands: vec![add_param("A"), add_param("B")],
            })
            .unwrap();
        assert_eq!(model.parameters().len(), 2);
        assert!(model.undo());
        assert_eq!(model.parameters().len(), 0);
        assert!(!model.undo());
    }

    #[test]
    fn new_command_clears_redo() {
        let mut model = built(vec![add_param("A")]);
        model.undo();
        assert!(model.history().can_redo());
        model.apply(add_param("B")).unwrap();
        assert!(!model.history().can_redo());
        assert!(!model.redo());
    }

    #[test]
    fn unknown_names_and_empty_names_are_errors() {
        let mut model = Model::new();
        assert_eq!(
            model.apply(Command::RemoveMesh { name: "X".into() }),
            Err(CommandError::NotFound {
                kind: "mesh",
                name: "X".into()
            })
        );
        assert_eq!(
            model.apply(add_param("")),
            Err(CommandError::EmptyName { kind: "parameter" })
        );
    }

    #[test]
    fn edit_that_breaks_geometry_is_rejected() {
        let mut model = built(vec![add_part("Face"), add_mesh("Skin", "Face")]);
        let result = model.apply(Command::SetMeshPositions {
            name: "Skin".into(),
            positions: vec![[0.0, 0.0]],
        });
        assert!(matches!(result, Err(CommandError::Invalid(_))));
    }

    #[test]
    fn edits_that_add_no_new_problem_pass_on_an_already_broken_model() {
        let mut model = built(vec![add_param("A")]);
        model
            .parameters
            .get_mut(model.parameters.find("A").unwrap())
            .unwrap()
            .max = -1.0;
        assert!(!model.validate().is_empty());
        model.apply(add_param("B")).unwrap();
    }

    #[test]
    fn mesh_style_commands_change_only_their_field() {
        let mut model = built(vec![
            add_part("Face"),
            add_mesh("A", "Face"),
            add_mesh("B", "Face"),
        ]);
        let id = model.meshes().find("A").unwrap();
        model
            .apply(Command::SetMeshBlendMode {
                name: "A".into(),
                blend_mode: BlendMode::Additive,
            })
            .unwrap();
        model
            .apply(Command::SetMeshMasks {
                name: "A".into(),
                masks: vec!["B".into()],
                inverted: false,
            })
            .unwrap();
        let mesh = model.meshes().get(id).unwrap();
        assert_eq!(mesh.blend_mode, BlendMode::Additive);
        assert_eq!(mesh.masks, vec![model.meshes().find("B").unwrap()]);
        assert_eq!(
            (mesh.texture, mesh.opacity, mesh.draw_order),
            (0, 1.0, 500.0)
        );
        assert_eq!(
            serde_json::to_value(Command::SetMeshBlendMode {
                name: "A".into(),
                blend_mode: BlendMode::Additive
            })
            .unwrap(),
            serde_json::json!({"type":"set_mesh_blend_mode","name":"A","blend_mode":"additive"})
        );
    }

    #[test]
    fn bad_mask_and_uv_edits_are_rejected_and_change_nothing() {
        let mut model = built(vec![add_part("Face"), add_mesh("A", "Face")]);
        let expected = built(vec![add_part("Face"), add_mesh("A", "Face")]);
        assert_eq!(
            model.apply(Command::SetMeshMasks {
                name: "A".into(),
                masks: vec!["Nope".into()],
                inverted: false
            }),
            Err(CommandError::NotFound {
                kind: "mesh",
                name: "Nope".into()
            })
        );
        assert!(matches!(
            model.apply(Command::SetMeshUvs {
                name: "A".into(),
                uvs: vec![[0.0, 0.0]]
            }),
            Err(CommandError::Invalid(_))
        ));
        assert!(model == expected);
    }

    #[test]
    fn bad_geometry_is_rejected_and_changes_nothing() {
        let mut model = built(vec![add_part("Face"), add_mesh("A", "Face")]);
        let expected = built(vec![add_part("Face"), add_mesh("A", "Face")]);
        let (positions, uvs, indices) = tri();
        let set =
            |positions: Vec<Point>, uvs: Vec<Point>, indices: Vec<u32>| Command::SetMeshGeometry {
                name: "A".into(),
                positions,
                uvs,
                indices,
            };
        let bad = [
            set(positions.clone(), uvs[..2].to_vec(), indices.clone()),
            set(positions.clone(), uvs.clone(), vec![0, 1, 3]),
            set(positions.clone(), uvs.clone(), vec![0, 1]),
        ];
        for command in bad {
            assert!(matches!(
                model.apply(command),
                Err(CommandError::Invalid(_))
            ));
            assert!(model == expected);
        }
        assert_eq!(
            model.apply(Command::SetMeshGeometry {
                name: "Nope".into(),
                positions,
                uvs,
                indices
            }),
            Err(CommandError::NotFound {
                kind: "mesh",
                name: "Nope".into()
            })
        );
    }

    #[test]
    fn changing_the_vertex_count_is_rejected_while_glue_uses_the_mesh() {
        use crate::rig::{Glue, GlueNode};
        let mut model = built(vec![
            add_part("Face"),
            add_mesh("A", "Face"),
            add_mesh("B", "Face"),
        ]);
        let (a, b) = (
            model.meshes().find("A").unwrap(),
            model.meshes().find("B").unwrap(),
        );
        model
            .glues
            .insert(
                "G",
                Glue {
                    mesh_a: a,
                    mesh_b: b,
                    nodes: vec![GlueNode {
                        index_a: 0,
                        index_b: 0,
                        weight: 0.5,
                    }],
                    intensity: 1.0,
                    keyforms: None,
                },
            )
            .unwrap();
        let grow = Command::SetMeshGeometry {
            name: "A".into(),
            positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            indices: vec![0, 1, 2, 2, 1, 3],
        };
        assert!(matches!(model.apply(grow), Err(CommandError::Invalid(_))));
        let (positions, uvs, _) = tri();
        model
            .apply(Command::SetMeshGeometry {
                name: "A".into(),
                positions,
                uvs,
                indices: vec![0, 2, 1],
            })
            .unwrap();
    }

    #[test]
    fn changing_the_vertex_count_is_rejected_for_a_mesh_with_keyforms() {
        use crate::keyform::{Axis, Binding, KeyformGrid};
        use crate::rig::MeshKeyform;
        let mut model = built(vec![
            add_param("P"),
            add_part("Face"),
            add_mesh("A", "Face"),
        ]);
        let p = model.parameters().find("P").unwrap();
        let binding = model.bindings.insert(
            Binding::new(vec![Axis {
                parameter: p,
                keys: vec![0.0, 1.0],
            }])
            .unwrap(),
        );
        let keyform = MeshKeyform {
            positions: tri().0,
            opacity: 1.0,
            draw_order: 500.0,
            multiply_color: [1.0; 3],
            screen_color: [0.0; 3],
        };
        let id = model.meshes().find("A").unwrap();
        model.meshes.get_mut(id).unwrap().keyforms =
            Some(KeyformGrid::new(binding, vec![keyform; 2]));
        let steps = model.history().commands().count();
        let grow = Command::SetMeshGeometry {
            name: "A".into(),
            positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            indices: vec![0, 1, 2, 2, 1, 3],
        };
        assert!(matches!(model.apply(grow), Err(CommandError::Invalid(_))));
        assert_eq!(model.history().commands().count(), steps);
        assert_eq!(model.meshes().get(id).unwrap().positions.len(), 3);
    }

    fn deformer_scene() -> Model {
        built(vec![
            add_part("Face"),
            add_mesh("Skin", "Face"),
            warp("Warp"),
            rotation("Rot", Some("Warp")),
        ])
    }

    #[test]
    fn deformer_commands_that_cannot_apply_change_nothing() {
        let mut model = deformer_scene();
        let steps = model.history().commands().count();
        let bad_points = Command::SetWarpBasePoints {
            name: "Warp".into(),
            points: vec![[0.0, 0.0]],
        };
        let cycle = Command::SetDeformerParent {
            name: "Warp".into(),
            parent: Some("Rot".into()),
        };
        let unknown_parent = Command::SetMeshParent {
            name: "Skin".into(),
            parent: Some("Nope".into()),
        };
        // Warp and rotation deformers share their names.
        let clash = rotation("Warp", None);
        let in_use = Command::RemoveDeformer {
            name: "Warp".into(),
        };
        let no_cells = Command::AddWarpDeformer {
            name: "Flat".into(),
            part: "Face".into(),
            parent: None,
            rows: 0,
            columns: 1,
            base_points: vec![],
        };
        for command in [bad_points, cycle, in_use, no_cells] {
            assert!(matches!(
                model.apply(command),
                Err(CommandError::Invalid(_))
            ));
        }
        assert_eq!(
            model.apply(unknown_parent),
            Err(CommandError::NotFound {
                kind: "deformer",
                name: "Nope".into()
            })
        );
        assert_eq!(
            model.apply(clash),
            Err(CommandError::AlreadyExists {
                kind: "deformer",
                name: "Warp".into()
            })
        );
        assert_eq!(model.history().commands().count(), steps);
        assert!(model.validate().is_empty());
    }

    #[test]
    fn a_deformer_with_children_cannot_be_removed_until_they_are_gone() {
        let mut model = deformer_scene();
        model
            .apply(Command::SetMeshParent {
                name: "Skin".into(),
                parent: Some("Rot".into()),
            })
            .unwrap();
        let remove = || Command::RemoveDeformer { name: "Rot".into() };
        assert!(model.apply(remove()).is_err());
        model
            .apply(Command::SetMeshParent {
                name: "Skin".into(),
                parent: None,
            })
            .unwrap();
        model.apply(remove()).unwrap();
        assert!(model.rotations().find("Rot").is_none());
        assert!(model.undo());
        assert!(model.rotations().find("Rot").is_some());
    }

    #[test]
    fn deformers_with_keyforms_are_not_edited() {
        use crate::keyform::{Axis, Binding, KeyformGrid};
        use crate::rig::WarpKeyform;
        let mut model = deformer_scene();
        model.apply(add_param("P")).unwrap();
        let p = model.parameters().find("P").unwrap();
        let binding = model.bindings.insert(
            Binding::new(vec![Axis {
                parameter: p,
                keys: vec![-30.0, 30.0],
            }])
            .unwrap(),
        );
        let keyform = WarpKeyform {
            points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            opacity: 1.0,
        };
        let id = model.warps().find("Warp").unwrap();
        model.warps.get_mut(id).unwrap().keyforms =
            Some(KeyformGrid::new(binding, vec![keyform; 2]));
        for command in [
            Command::SetWarpBasePoints {
                name: "Warp".into(),
                points: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [2.0, 2.0]],
            },
            Command::SetDeformerParent {
                name: "Warp".into(),
                parent: None,
            },
        ] {
            assert!(matches!(
                model.apply(command),
                Err(CommandError::HasKeyforms { .. })
            ));
        }
    }

    #[test]
    fn deformer_commands_are_tagged_in_json() {
        let json = serde_json::to_value(rotation("Rot", None)).unwrap();
        assert_eq!(json["type"], "add_rotation_deformer");
        assert_eq!(json["base"]["scale"], 1.0);
        let parsed: Command =
            serde_json::from_str(r#"{"type":"set_mesh_parent","name":"Skin"}"#).unwrap();
        assert_eq!(
            parsed,
            Command::SetMeshParent {
                name: "Skin".into(),
                parent: None
            }
        );
    }

    #[test]
    fn renaming_keeps_every_reference_and_fails_on_empty_duplicate_and_unknown_names() {
        let mut model = built(vec![
            add_part("Face"),
            add_part("Hair"),
            add_mesh("Skin", "Face"),
            add_mesh("Clip", "Face"),
            warp("W"),
            rotation("R", Some("W")),
            Command::SetMeshMasks {
                name: "Skin".into(),
                masks: vec!["Clip".into()],
                inverted: false,
            },
            Command::SetMeshParent {
                name: "Skin".into(),
                parent: Some("R".into()),
            },
        ]);
        let renames = [
            Command::RenameMesh {
                name: "Clip".into(),
                new_name: "Mask".into(),
            },
            Command::RenameRotation {
                name: "R".into(),
                new_name: "Spin".into(),
            },
            Command::RenameWarp {
                name: "W".into(),
                new_name: "Bend".into(),
            },
            Command::RenamePart {
                name: "Face".into(),
                new_name: "Head".into(),
            },
        ];
        for r in renames {
            model.apply(r).unwrap();
        }
        // The names moved and nothing was left pointing at the old ones.
        assert!(model.validate().is_empty());
        let skin = model
            .meshes()
            .get(model.meshes().find("Skin").unwrap())
            .unwrap();
        assert_eq!(skin.masks, vec![model.meshes().find("Mask").unwrap()]);
        assert_eq!(
            skin.parent,
            Some(DeformerRef::Rotation(
                model.rotations().find("Spin").unwrap()
            ))
        );
        assert_eq!(skin.part, model.parts().find("Head").unwrap());
        assert!(model.meshes().find("Clip").is_none() && model.parts().find("Face").is_none());
        // Each rename is one undo step that brings the old name back.
        for _ in 0..4 {
            assert!(model.undo());
        }
        assert!(model.meshes().find("Clip").is_some() && model.rotations().find("R").is_some());
        assert!(model.warps().find("W").is_some() && model.parts().find("Face").is_some());
        assert!(model.validate().is_empty());
        assert!(model.redo());
        assert!(model.meshes().find("Mask").is_some());
        // Failures change nothing.
        let steps = model.history().commands().count();
        let bad = [
            (
                Command::RenameMesh {
                    name: "Skin".into(),
                    new_name: String::new(),
                },
                CommandError::EmptyName { kind: "mesh" },
            ),
            (
                Command::RenameMesh {
                    name: "Skin".into(),
                    new_name: "Mask".into(),
                },
                CommandError::AlreadyExists {
                    kind: "mesh",
                    name: "Mask".into(),
                },
            ),
            (
                Command::RenameMesh {
                    name: "Nope".into(),
                    new_name: "X".into(),
                },
                CommandError::NotFound {
                    kind: "mesh",
                    name: "Nope".into(),
                },
            ),
            // A warp may not take the name of a rotation deformer.
            (
                Command::RenameWarp {
                    name: "W".into(),
                    new_name: "R".into(),
                },
                CommandError::AlreadyExists {
                    kind: "deformer",
                    name: "R".into(),
                },
            ),
            (
                Command::RenamePart {
                    name: "Face".into(),
                    new_name: "Hair".into(),
                },
                CommandError::AlreadyExists {
                    kind: "part",
                    name: "Hair".into(),
                },
            ),
        ];
        for (command, error) in bad {
            assert_eq!(model.apply(command), Err(error));
        }
        assert_eq!(model.history().commands().count(), steps);
        // Renaming to the same name is nothing.
        model
            .apply(Command::RenamePart {
                name: "Face".into(),
                new_name: "Face".into(),
            })
            .unwrap();
    }

    #[test]
    fn parts_and_deformers_move_between_parts_and_parts_cannot_loop() {
        let mut model = built(vec![
            add_part("Face"),
            add_part("A"),
            add_part("B"),
            add_part("C"),
            add_mesh("Skin", "A"),
            warp("W"),
        ]);
        model
            .apply(Command::SetMeshPart {
                name: "Skin".into(),
                part: "B".into(),
            })
            .unwrap();
        model
            .apply(Command::SetDeformerPart {
                name: "W".into(),
                part: "C".into(),
            })
            .unwrap();
        let skin = model
            .meshes()
            .get(model.meshes().find("Skin").unwrap())
            .unwrap();
        assert_eq!(skin.part, model.parts().find("B").unwrap());
        assert!(model.undo() && model.undo());
        model
            .apply(Command::SetPartParent {
                name: "B".into(),
                parent: Some("A".into()),
            })
            .unwrap();
        model
            .apply(Command::SetPartParent {
                name: "C".into(),
                parent: Some("B".into()),
            })
            .unwrap();
        let steps = model.history().commands().count();
        // A under C would close the loop A > B > C > A.
        let cycle = Command::SetPartParent {
            name: "A".into(),
            parent: Some("C".into()),
        };
        assert!(matches!(model.apply(cycle), Err(CommandError::Invalid(_))));
        assert_eq!(
            model.apply(Command::SetPartParent {
                name: "A".into(),
                parent: Some("Nope".into())
            }),
            Err(CommandError::NotFound {
                kind: "part",
                name: "Nope".into()
            })
        );
        assert_eq!(
            model.apply(Command::SetMeshPart {
                name: "Skin".into(),
                part: "Nope".into()
            }),
            Err(CommandError::NotFound {
                kind: "part",
                name: "Nope".into()
            })
        );
        assert_eq!(model.history().commands().count(), steps);
        // Taking a part out of its parent.
        model
            .apply(Command::SetPartParent {
                name: "C".into(),
                parent: None,
            })
            .unwrap();
        assert!(model.validate().is_empty());
    }
}
