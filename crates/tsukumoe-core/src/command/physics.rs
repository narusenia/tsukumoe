// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The commands that make and change pendulum groups and physics chains
//! (M5-07). A group is edited as a whole: the `Set` commands replace its
//! inputs, outputs and particles (or joints). The history keeps the whole rig
//! before and after (`Change::Physics`).

use serde::{Deserialize, Serialize};

use crate::elements::Parameter;
use crate::model::Model;
use crate::physics::{
    ChainJoint, Channel, Particle, PendulumGroup, PhysicsChain, PhysicsInput, PhysicsOutput, Range,
};

use super::keyform::refused;
use super::{Change, Command, CommandError};

/// An input of a physics group, the parameter by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PhysicsInputDef {
    pub parameter: String,
    pub channel: Channel,
    pub weight: f32,
    #[serde(default)]
    pub reflect: bool,
}

/// An output of a physics group, the parameter by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PhysicsOutputDef {
    pub parameter: String,
    pub channel: Channel,
    /// Index into the group's particles (joints, for a chain).
    pub particle: usize,
    pub scale: f32,
    pub weight: f32,
    #[serde(default)]
    pub reflect: bool,
}

fn finite(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn range_ok(r: &Range) -> bool {
    finite(&[r.min, r.max, r.default]) && r.min <= r.default && r.default <= r.max
}

fn not_found(kind: &'static str, name: &str) -> CommandError {
    CommandError::NotFound {
        kind,
        name: name.to_owned(),
    }
}

fn taken(kind: &'static str, name: &str) -> CommandError {
    CommandError::AlreadyExists {
        kind,
        name: name.to_owned(),
    }
}

impl Model {
    fn physics_inputs(
        &self,
        owner: &str,
        defs: &[PhysicsInputDef],
    ) -> Result<Vec<PhysicsInput>, CommandError> {
        defs.iter()
            .map(|d| {
                if !finite(&[d.weight]) || d.weight < 0.0 {
                    return Err(refused(owner, "bad-range"));
                }
                Ok(PhysicsInput {
                    parameter: self.resolve::<Parameter>(&d.parameter)?,
                    channel: d.channel,
                    weight: d.weight,
                    reflect: d.reflect,
                })
            })
            .collect()
    }

    fn physics_outputs(
        &self,
        owner: &str,
        defs: &[PhysicsOutputDef],
        particles: usize,
    ) -> Result<Vec<PhysicsOutput>, CommandError> {
        defs.iter()
            .map(|d| {
                if !finite(&[d.scale, d.weight]) || d.weight < 0.0 || d.particle >= particles {
                    return Err(refused(owner, "bad-range"));
                }
                Ok(PhysicsOutput {
                    parameter: self.resolve::<Parameter>(&d.parameter)?,
                    channel: d.channel,
                    particle: d.particle,
                    scale: d.scale,
                    weight: d.weight,
                    reflect: d.reflect,
                })
            })
            .collect()
    }

    pub(super) fn run_physics(
        &mut self,
        command: &Command,
        changes: &mut Vec<Change>,
    ) -> Result<(), CommandError> {
        let before = self.physics.clone();
        match command {
            Command::AddPendulum { name } => {
                if self.physics.pendulums.iter().any(|g| &g.name == name) {
                    return Err(taken("pendulum", name));
                }
                self.physics.pendulums.push(PendulumGroup {
                    name: name.clone(),
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    particles: vec![
                        Particle {
                            position: [0.0, 0.0],
                            mobility: 1.0,
                            delay: 1.0,
                            acceleration: 1.0,
                            radius: 0.0,
                        },
                        Particle {
                            position: [0.0, 10.0],
                            mobility: 0.95,
                            delay: 0.9,
                            acceleration: 1.5,
                            radius: 10.0,
                        },
                    ],
                    position_range: Range {
                        min: -10.0,
                        max: 10.0,
                        default: 0.0,
                    },
                    angle_range: Range {
                        min: -10.0,
                        max: 10.0,
                        default: 0.0,
                    },
                });
            }
            Command::RemovePendulum { name } => {
                let at = self.pendulum_at(name)?;
                self.physics.pendulums.remove(at);
            }
            Command::RenamePendulum { name, new_name } => {
                let at = self.pendulum_at(name)?;
                if name != new_name && self.physics.pendulums.iter().any(|g| &g.name == new_name) {
                    return Err(taken("pendulum", new_name));
                }
                self.physics.pendulums[at].name = new_name.clone();
            }
            Command::SetPendulum {
                name,
                inputs,
                outputs,
                particles,
                position_range,
                angle_range,
            } => {
                let at = self.pendulum_at(name)?;
                let ok = !particles.is_empty()
                    && particles.iter().all(|p| {
                        finite(&[
                            p.position[0],
                            p.position[1],
                            p.mobility,
                            p.delay,
                            p.acceleration,
                            p.radius,
                        ]) && p.radius >= 0.0
                    })
                    && range_ok(position_range)
                    && range_ok(angle_range);
                if !ok {
                    return Err(refused(name, "bad-range"));
                }
                let inputs = self.physics_inputs(name, inputs)?;
                let outputs = self.physics_outputs(name, outputs, particles.len())?;
                let group = &mut self.physics.pendulums[at];
                group.inputs = inputs;
                group.outputs = outputs;
                group.particles = particles.clone();
                group.position_range = *position_range;
                group.angle_range = *angle_range;
            }
            Command::AddPhysicsChain { name } => {
                if self.physics.chains.iter().any(|c| &c.name == name) {
                    return Err(taken("physics chain", name));
                }
                self.physics.chains.push(PhysicsChain {
                    name: name.clone(),
                    joints: vec![
                        ChainJoint {
                            length: 1.0,
                            stiffness: 0.5,
                            damping: 0.1
                        };
                        3
                    ],
                    gravity: [0.0, -9.8],
                    wind: [0.0, 0.0],
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                });
            }
            Command::RemovePhysicsChain { name } => {
                let at = self.chain_at(name)?;
                self.physics.chains.remove(at);
            }
            Command::RenamePhysicsChain { name, new_name } => {
                let at = self.chain_at(name)?;
                if name != new_name && self.physics.chains.iter().any(|c| &c.name == new_name) {
                    return Err(taken("physics chain", new_name));
                }
                self.physics.chains[at].name = new_name.clone();
            }
            Command::SetPhysicsChain {
                name,
                joints,
                gravity,
                wind,
                inputs,
                outputs,
            } => {
                let at = self.chain_at(name)?;
                let ok = !joints.is_empty()
                    && joints
                        .iter()
                        .all(|j| finite(&[j.length, j.stiffness, j.damping]) && j.length >= 0.0)
                    && finite(&[gravity[0], gravity[1], wind[0], wind[1]]);
                if !ok {
                    return Err(refused(name, "bad-range"));
                }
                let inputs = self.physics_inputs(name, inputs)?;
                let outputs = self.physics_outputs(name, outputs, joints.len())?;
                let chain = &mut self.physics.chains[at];
                chain.joints = joints.clone();
                chain.gravity = *gravity;
                chain.wind = *wind;
                chain.inputs = inputs;
                chain.outputs = outputs;
            }
            _ => unreachable!("run_physics gets physics commands only"),
        }
        if self.physics != before {
            changes.push(Change::Physics(Box::new((before, self.physics.clone()))));
        }
        Ok(())
    }

    fn pendulum_at(&self, name: &str) -> Result<usize, CommandError> {
        self.physics
            .pendulums
            .iter()
            .position(|g| g.name == name)
            .ok_or_else(|| not_found("pendulum", name))
    }

    fn chain_at(&self, name: &str) -> Result<usize, CommandError> {
        self.physics
            .chains
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| not_found("physics chain", name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        let mut m = Model::new();
        m.apply(Command::AddParameter {
            name: "P".into(),
            min: -30.0,
            max: 30.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        m.apply(Command::AddPendulum { name: "G".into() }).unwrap();
        m
    }

    fn set(m: &Model, input: &str, particle: usize) -> Command {
        let g = &m.physics().pendulums[0];
        Command::SetPendulum {
            name: "G".into(),
            inputs: vec![PhysicsInputDef {
                parameter: input.into(),
                channel: Channel::Angle,
                weight: 50.0,
                reflect: false,
            }],
            outputs: vec![PhysicsOutputDef {
                parameter: "P".into(),
                channel: Channel::Angle,
                particle,
                scale: 1.0,
                weight: 100.0,
                reflect: false,
            }],
            particles: g.particles.clone(),
            position_range: g.position_range,
            angle_range: g.angle_range,
        }
    }

    #[test]
    fn a_pendulum_is_added_set_renamed_removed_and_each_step_undoes() {
        let mut m = model();
        assert_eq!(m.physics().pendulums[0].particles.len(), 2);
        let c = set(&m, "P", 1);
        m.apply(c).unwrap();
        assert_eq!(m.physics().pendulums[0].inputs.len(), 1);
        m.apply(Command::RenamePendulum {
            name: "G".into(),
            new_name: "H".into(),
        })
        .unwrap();
        assert_eq!(m.physics().pendulums[0].name, "H");
        assert!(m.undo());
        assert_eq!(m.physics().pendulums[0].name, "G");
        assert!(m.undo());
        assert!(m.physics().pendulums[0].inputs.is_empty());
        assert!(m.redo());
        assert_eq!(m.physics().pendulums[0].inputs.len(), 1);
        m.apply(Command::RemovePendulum { name: "G".into() })
            .unwrap();
        assert!(m.physics().pendulums.is_empty());
        assert!(m.undo());
        assert_eq!(m.physics().pendulums.len(), 1);
    }

    #[test]
    fn bad_names_parameters_and_numbers_are_refused() {
        let mut m = model();
        assert!(m.apply(Command::AddPendulum { name: "G".into() }).is_err());
        let c = set(&m, "Nope", 1);
        assert!(m.apply(c).is_err());
        let c = set(&m, "P", 2);
        assert!(m.apply(c).is_err(), "particle out of range");
        assert!(
            m.apply(Command::RemovePendulum { name: "X".into() })
                .is_err()
        );
        let mut c = set(&m, "P", 1);
        if let Command::SetPendulum { particles, .. } = &mut c {
            particles[1].mobility = f32::NAN;
        }
        assert!(m.apply(c).is_err());
        let mut c = set(&m, "P", 1);
        if let Command::SetPendulum { particles, .. } = &mut c {
            particles.clear();
        }
        assert!(m.apply(c).is_err());
    }

    #[test]
    fn a_parameter_a_group_names_cannot_be_removed() {
        let mut m = model();
        let c = set(&m, "P", 1);
        m.apply(c).unwrap();
        assert!(
            m.apply(Command::RemoveParameter { name: "P".into() })
                .is_err()
        );
    }

    #[test]
    fn a_chain_is_set_and_checked() {
        let mut m = model();
        m.apply(Command::AddPhysicsChain { name: "C".into() })
            .unwrap();
        assert_eq!(m.physics().chains[0].joints.len(), 3);
        let chain = |joints: usize, particle: usize| Command::SetPhysicsChain {
            name: "C".into(),
            joints: vec![
                ChainJoint {
                    length: 1.0,
                    stiffness: 0.5,
                    damping: 0.1,
                };
                joints
            ],
            gravity: [0.0, -9.8],
            wind: [1.0, 0.0],
            inputs: vec![],
            outputs: vec![PhysicsOutputDef {
                parameter: "P".into(),
                channel: Channel::X,
                particle,
                scale: 1.0,
                weight: 100.0,
                reflect: true,
            }],
        };
        m.apply(chain(2, 1)).unwrap();
        assert_eq!(m.physics().chains[0].joints.len(), 2);
        assert_eq!(m.physics().chains[0].wind, [1.0, 0.0]);
        assert!(m.apply(chain(2, 2)).is_err());
        assert!(m.apply(chain(0, 0)).is_err());
        assert!(m.undo());
        assert_eq!(m.physics().chains[0].joints.len(), 3);
    }
}
