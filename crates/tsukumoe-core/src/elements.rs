// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Parameters and parts: the elements everything else refers to.

use crate::arena::Id;

pub type Point = [f32; 2];

pub type ParameterId = Id<Parameter>;
pub type PartId = Id<Part>;

/// A model input such as `ParamAngleX`. Its persistent id is the table name.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub display_name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// The value wraps around between `min` and `max` (angles).
    pub repeat: bool,
    pub group: Option<String>,
}

impl Parameter {
    pub fn new(min: f32, max: f32, default: f32) -> Self {
        Self {
            display_name: String::new(),
            min,
            max,
            default,
            repeat: false,
            group: None,
        }
    }
}

/// Groups drawables and carries their shared opacity.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub parent: Option<PartId>,
    pub opacity: f32,
    pub visible: bool,
}

impl Default for Part {
    fn default() -> Self {
        Self {
            parent: None,
            opacity: 1.0,
            visible: true,
        }
    }
}
