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
use crate::rig::ArtMesh;
use crate::table::InsertError;
use crate::validate::ModelError;

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
    /// The command would leave the model with problems it did not have.
    Invalid(Vec<ModelError>),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { kind, name } => write!(f, "no {kind} named '{name}'"),
            Self::AlreadyExists { kind, name } => write!(f, "{kind} '{name}' already exists"),
            Self::EmptyName { kind } => write!(f, "{kind} name is empty"),
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
            Command::Batch { commands } => {
                for command in commands {
                    self.run(command, changes)?;
                }
            }
        }
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
        ]
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
}
