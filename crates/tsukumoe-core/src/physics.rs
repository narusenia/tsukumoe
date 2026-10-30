// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Physics: pendulum groups (physics3 compatible) and physics chains.
//!
//! The simulation is in `simulate.rs`.

use crate::elements::{ParameterId, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    X,
    Y,
    Angle,
}

impl Channel {
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::X,
            1 => Self::Y,
            2 => Self::Angle,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsInput {
    pub parameter: ParameterId,
    pub channel: Channel,
    pub weight: f32,
    pub reflect: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsOutput {
    pub parameter: ParameterId,
    pub channel: Channel,
    /// Index into the group's particles.
    pub particle: usize,
    pub scale: f32,
    pub weight: f32,
    pub reflect: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    pub position: Point,
    pub mobility: f32,
    pub delay: f32,
    pub acceleration: f32,
    pub radius: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendulumGroup {
    pub name: String,
    pub inputs: Vec<PhysicsInput>,
    pub outputs: Vec<PhysicsOutput>,
    pub particles: Vec<Particle>,
    pub position_range: Range,
    pub angle_range: Range,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainJoint {
    pub length: f32,
    pub stiffness: f32,
    pub damping: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsChain {
    pub name: String,
    pub joints: Vec<ChainJoint>,
    pub gravity: Point,
    pub wind: Point,
    pub inputs: Vec<PhysicsInput>,
    pub outputs: Vec<PhysicsOutput>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhysicsRig {
    pub pendulums: Vec<PendulumGroup>,
    pub chains: Vec<PhysicsChain>,
}
