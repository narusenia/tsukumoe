// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Recorded changes and the undo / redo stacks (ADR-0019).

use crate::arena::Id;
use crate::command::Command;
use crate::elements::{Parameter, Part};
use crate::model::Model;
use crate::rig::ArtMesh;
use crate::table::Table;

/// One element's state before and after a command. `before` empty is an
/// insertion, `after` empty is a removal, both set is a modification.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Edit<T> {
    pub(crate) id: Id<T>,
    pub(crate) name: String,
    pub(crate) before: Option<T>,
    pub(crate) after: Option<T>,
}

impl<T: Clone> Edit<T> {
    fn set(&self, table: &mut Table<T>, state: &Option<T>) {
        match state {
            Some(value) => table.put(self.id, &self.name, value.clone()),
            None => {
                table.remove(self.id);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Change {
    Parameter(Edit<Parameter>),
    Part(Edit<Part>),
    // Boxed: a mesh edit holds two copies of the mesh, far larger than the others.
    Mesh(Box<Edit<ArtMesh>>),
}

impl Change {
    pub(crate) fn undo(&self, model: &mut Model) {
        match self {
            Change::Parameter(e) => e.set(&mut model.parameters, &e.before),
            Change::Part(e) => e.set(&mut model.parts, &e.before),
            Change::Mesh(e) => e.set(&mut model.meshes, &e.before),
        }
    }

    pub(crate) fn redo(&self, model: &mut Model) {
        match self {
            Change::Parameter(e) => e.set(&mut model.parameters, &e.after),
            Change::Part(e) => e.set(&mut model.parts, &e.after),
            Change::Mesh(e) => e.set(&mut model.meshes, &e.after),
        }
    }
}

/// An element kind commands can edit.
pub(crate) trait Editable: Clone + Sized {
    /// Shown in errors.
    const KIND: &'static str;
    fn table(model: &Model) -> &Table<Self>;
    fn table_mut(model: &mut Model) -> &mut Table<Self>;
    fn change(edit: Edit<Self>) -> Change;
}

impl Editable for Parameter {
    const KIND: &'static str = "parameter";
    fn table(model: &Model) -> &Table<Self> {
        &model.parameters
    }
    fn table_mut(model: &mut Model) -> &mut Table<Self> {
        &mut model.parameters
    }
    fn change(edit: Edit<Self>) -> Change {
        Change::Parameter(edit)
    }
}

impl Editable for Part {
    const KIND: &'static str = "part";
    fn table(model: &Model) -> &Table<Self> {
        &model.parts
    }
    fn table_mut(model: &mut Model) -> &mut Table<Self> {
        &mut model.parts
    }
    fn change(edit: Edit<Self>) -> Change {
        Change::Part(edit)
    }
}

impl Editable for ArtMesh {
    const KIND: &'static str = "mesh";
    fn table(model: &Model) -> &Table<Self> {
        &model.meshes
    }
    fn table_mut(model: &mut Model) -> &mut Table<Self> {
        &mut model.meshes
    }
    fn change(edit: Edit<Self>) -> Change {
        Change::Mesh(Box::new(edit))
    }
}

#[derive(Debug)]
pub(crate) struct Record {
    pub(crate) command: Command,
    pub(crate) changes: Vec<Change>,
}

/// Applied commands, newest last, and the ones undone since.
#[derive(Debug, Default)]
pub struct History {
    pub(crate) undo: Vec<Record>,
    pub(crate) redo: Vec<Record>,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The commands currently applied, oldest first. Replaying them on an
    /// empty model gives the same model.
    pub fn commands(&self) -> impl Iterator<Item = &Command> {
        self.undo.iter().map(|record| &record.command)
    }
}
