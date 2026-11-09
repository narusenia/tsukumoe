// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Presets and the three "feel" knobs of a pendulum group (M5-07 ②). The knobs
//! stand for the raw particle values: sway for the acceleration and delay,
//! stickiness for the mobility, length for the radius. They are read back from
//! particle 1, so a group edited by hand still shows where it is.

use tsukumoe_core::physics::{Channel, Particle};

/// How a chain of particles moves, each from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    pub sway: f32,
    pub stickiness: f32,
    pub length: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    LongHair,
    ShortHair,
    Skirt,
    Ribbon,
    Ear,
}

impl Preset {
    pub const ALL: [Self; 5] = [
        Self::LongHair,
        Self::ShortHair,
        Self::Skirt,
        Self::Ribbon,
        Self::Ear,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::LongHair => "physics-preset-long-hair",
            Self::ShortHair => "physics-preset-short-hair",
            Self::Skirt => "physics-preset-skirt",
            Self::Ribbon => "physics-preset-ribbon",
            Self::Ear => "physics-preset-ear",
        }
    }

    /// The particles of the group, the root included.
    pub fn count(self) -> usize {
        match self {
            Self::LongHair => 6,
            Self::ShortHair => 3,
            Self::Skirt => 3,
            Self::Ribbon => 4,
            Self::Ear => 2,
        }
    }

    pub fn feel(self) -> Feel {
        let (sway, stickiness, length) = match self {
            Self::LongHair => (0.6, 0.4, 0.7),
            Self::ShortHair => (0.5, 0.5, 0.3),
            Self::Skirt => (0.4, 0.6, 0.6),
            Self::Ribbon => (0.8, 0.2, 0.5),
            Self::Ear => (0.7, 0.3, 0.2),
        };
        Feel {
            sway,
            stickiness,
            length,
        }
    }
}

/// The parameters whose angle usually drives hair, with the channel and the
/// weight of the input that takes them.
pub const HEAD_INPUTS: [(&str, Channel, f32); 4] = [
    ("ParamAngleX", Channel::X, 60.0),
    ("ParamAngleZ", Channel::Angle, 60.0),
    ("ParamBodyAngleX", Channel::X, 40.0),
    ("ParamBodyAngleZ", Channel::Angle, 40.0),
];

const ACCELERATION: (f32, f32) = (0.8, 2.5);
const DELAY: (f32, f32) = (0.6, 1.0);
const MOBILITY: (f32, f32) = (0.99, 0.80);
const RADIUS: (f32, f32) = (4.0, 20.0);

fn lerp((a, b): (f32, f32), t: f32) -> f32 {
    a + (b - a) * t
}

fn unlerp((a, b): (f32, f32), v: f32) -> f32 {
    ((v - a) / (b - a)).clamp(0.0, 1.0)
}

/// `count` particles hanging down from a root at the origin.
pub fn particles(feel: Feel, count: usize) -> Vec<Particle> {
    let radius = lerp(RADIUS, feel.length);
    let mut out = vec![Particle {
        position: [0.0, 0.0],
        mobility: 1.0,
        delay: 1.0,
        acceleration: 1.0,
        radius: 0.0,
    }];
    for i in 1..count.max(2) {
        out.push(Particle {
            position: [0.0, radius * i as f32],
            mobility: lerp(MOBILITY, feel.stickiness),
            delay: lerp(DELAY, feel.sway),
            acceleration: lerp(ACCELERATION, feel.sway),
            radius,
        });
    }
    out
}

/// The knobs a group's particle 1 stands at.
pub fn feel_of(particles: &[Particle]) -> Feel {
    match particles.get(1) {
        Some(p) => Feel {
            sway: unlerp(ACCELERATION, p.acceleration),
            stickiness: unlerp(MOBILITY, p.mobility),
            length: unlerp(RADIUS, p.radius),
        },
        None => Feel {
            sway: 0.5,
            stickiness: 0.5,
            length: 0.5,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_knobs_are_read_back_from_what_they_made() {
        for preset in Preset::ALL {
            let feel = preset.feel();
            let ps = particles(feel, preset.count());
            assert_eq!(ps.len(), preset.count());
            let back = feel_of(&ps);
            assert!((back.sway - feel.sway).abs() < 1e-4, "{preset:?}");
            assert!(
                (back.stickiness - feel.stickiness).abs() < 1e-4,
                "{preset:?}"
            );
            assert!((back.length - feel.length).abs() < 1e-4, "{preset:?}");
        }
    }

    #[test]
    fn more_sway_is_more_acceleration_and_the_root_stays_put() {
        let calm = particles(
            Feel {
                sway: 0.0,
                stickiness: 0.5,
                length: 0.5,
            },
            3,
        );
        let lively = particles(
            Feel {
                sway: 1.0,
                stickiness: 0.5,
                length: 0.5,
            },
            3,
        );
        assert!(lively[1].acceleration > calm[1].acceleration);
        assert_eq!(calm[0], lively[0]);
        assert_eq!(calm[0].position, [0.0, 0.0]);
        // A group always keeps a root and one particle.
        assert_eq!(
            particles(
                Feel {
                    sway: 0.5,
                    stickiness: 0.5,
                    length: 0.5
                },
                0
            )
            .len(),
            2
        );
    }
}
