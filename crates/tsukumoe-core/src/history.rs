// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Recorded changes and the undo / redo stacks (ADR-0019).

use crate::arena::Id;
use crate::command::Command;
use crate::driver::Driver;
use crate::elements::{Parameter, Part};
use crate::keyform::Binding;
use crate::model::Model;
use crate::rig::{ArtMesh, RotationDeformer, WarpDeformer};
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

/// A binding in the arena before and after a command (no name: ids only).
/// `before` empty is an insertion, `after` empty is a removal.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BindingEdit {
    pub(crate) id: crate::keyform::BindingId,
    pub(crate) before: Option<Binding>,
    pub(crate) after: Option<Binding>,
}

impl BindingEdit {
    fn set(&self, model: &mut Model, state: &Option<Binding>) {
        match state {
            Some(value) => {
                model.bindings.restore(self.id, value.clone());
            }
            None => {
                model.bindings.remove(self.id);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Change {
    Binding(BindingEdit),
    Driver(Edit<Driver>),
    Parameter(Edit<Parameter>),
    Part(Edit<Part>),
    // Boxed: a mesh edit holds two copies of the mesh, far larger than the others.
    Mesh(Box<Edit<ArtMesh>>),
    Warp(Box<Edit<WarpDeformer>>),
    Rotation(Edit<RotationDeformer>),
}

impl Change {
    pub(crate) fn undo(&self, model: &mut Model) {
        match self {
            Change::Binding(e) => e.set(model, &e.before),
            Change::Driver(e) => e.set(&mut model.drivers, &e.before),
            Change::Parameter(e) => e.set(&mut model.parameters, &e.before),
            Change::Part(e) => e.set(&mut model.parts, &e.before),
            Change::Mesh(e) => e.set(&mut model.meshes, &e.before),
            Change::Warp(e) => e.set(&mut model.warps, &e.before),
            Change::Rotation(e) => e.set(&mut model.rotations, &e.before),
        }
    }

    pub(crate) fn redo(&self, model: &mut Model) {
        match self {
            Change::Binding(e) => e.set(model, &e.after),
            Change::Driver(e) => e.set(&mut model.drivers, &e.after),
            Change::Parameter(e) => e.set(&mut model.parameters, &e.after),
            Change::Part(e) => e.set(&mut model.parts, &e.after),
            Change::Mesh(e) => e.set(&mut model.meshes, &e.after),
            Change::Warp(e) => e.set(&mut model.warps, &e.after),
            Change::Rotation(e) => e.set(&mut model.rotations, &e.after),
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

impl Editable for Driver {
    const KIND: &'static str = "driver";
    fn table(model: &Model) -> &Table<Self> {
        &model.drivers
    }
    fn table_mut(model: &mut Model) -> &mut Table<Self> {
        &mut model.drivers
    }
    fn change(edit: Edit<Self>) -> Change {
        Change::Driver(edit)
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

impl Editable for WarpDeformer {
    const KIND: &'static str = "warp deformer";
    fn table(model: &Model) -> &Table<Self> {
        &model.warps
    }
    fn table_mut(model: &mut Model) -> &mut Table<Self> {
        &mut model.warps
    }
    fn change(edit: Edit<Self>) -> Change {
        Change::Warp(Box::new(edit))
    }
}

impl Editable for RotationDeformer {
    const KIND: &'static str = "rotation deformer";
    fn table(model: &Model) -> &Table<Self> {
        &model.rotations
    }
    fn table_mut(model: &mut Model) -> &mut Table<Self> {
        &mut model.rotations
    }
    fn change(edit: Edit<Self>) -> Change {
        Change::Rotation(edit)
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
