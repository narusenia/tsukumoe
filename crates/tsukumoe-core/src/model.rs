// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The editing model.
//!
//! Fields are crate-private on purpose: ADR-0018 routes every write through
//! `Model::apply` (M1-02), so there is no public way to mutate a model yet.

use crate::arena::Arena;
use crate::driver::Driver;
use crate::elements::{Parameter, Part};
use crate::history::History;
use crate::keyform::Binding;
use crate::physics::PhysicsRig;
use crate::playback::{Expression, Motion, Pose};
use crate::rig::{ArtMesh, BlendShape, Bone, Glue, RotationDeformer, WarpDeformer};
use crate::table::Table;

/// Equality ignores the history: two models are equal when their contents are.
#[derive(Default)]
pub struct Model {
    pub(crate) parameters: Table<Parameter>,
    pub(crate) parts: Table<Part>,
    pub(crate) meshes: Table<ArtMesh>,
    pub(crate) warps: Table<WarpDeformer>,
    pub(crate) rotations: Table<RotationDeformer>,
    pub(crate) glues: Table<Glue>,
    pub(crate) bones: Table<Bone>,
    pub(crate) blend_shapes: Table<BlendShape>,
    pub(crate) drivers: Table<Driver>,
    pub(crate) motions: Table<Motion>,
    pub(crate) expressions: Table<Expression>,
    pub(crate) bindings: Arena<Binding>,
    pub(crate) pose: Option<Pose>,
    pub(crate) physics: PhysicsRig,
    pub(crate) history: History,
}

impl Model {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parameters(&self) -> &Table<Parameter> {
        &self.parameters
    }

    pub fn parts(&self) -> &Table<Part> {
        &self.parts
    }

    pub fn meshes(&self) -> &Table<ArtMesh> {
        &self.meshes
    }

    pub fn warps(&self) -> &Table<WarpDeformer> {
        &self.warps
    }

    pub fn rotations(&self) -> &Table<RotationDeformer> {
        &self.rotations
    }

    pub fn glues(&self) -> &Table<Glue> {
        &self.glues
    }

    pub fn bones(&self) -> &Table<Bone> {
        &self.bones
    }

    pub fn blend_shapes(&self) -> &Table<BlendShape> {
        &self.blend_shapes
    }

    pub fn drivers(&self) -> &Table<Driver> {
        &self.drivers
    }

    pub fn motions(&self) -> &Table<Motion> {
        &self.motions
    }

    pub fn expressions(&self) -> &Table<Expression> {
        &self.expressions
    }

    pub fn bindings(&self) -> &Arena<Binding> {
        &self.bindings
    }

    pub fn pose(&self) -> Option<&Pose> {
        self.pose.as_ref()
    }

    pub fn physics(&self) -> &PhysicsRig {
        &self.physics
    }

    pub fn history(&self) -> &History {
        &self.history
    }
}

impl PartialEq for Model {
    fn eq(&self, other: &Self) -> bool {
        self.parameters == other.parameters
            && self.parts == other.parts
            && self.meshes == other.meshes
            && self.warps == other.warps
            && self.rotations == other.rotations
            && self.glues == other.glues
            && self.bones == other.bones
            && self.blend_shapes == other.blend_shapes
            && self.drivers == other.drivers
            && self.motions == other.motions
            && self.expressions == other.expressions
            && self.bindings == other.bindings
            && self.pose == other.pose
            && self.physics == other.physics
    }
}
