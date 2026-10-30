// Copyright 2026 Seimei Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Physics stepping. The rules are in `docs/specifications/physics.md`.

use crate::compiled::{CompiledModel, PhysicsDef, PhysicsKind, PhysicsParticle};
use crate::elements::Point;
use crate::physics::Channel;

/// Seconds per physics step.
pub const STEP: f32 = 1.0 / 60.0;
/// Steps one `advance` takes at most; the time beyond that is dropped.
pub const MAX_STEPS: u32 = 10;
/// How many units per second squared a pendulum particle's `acceleration` of 1 is.
const GRAVITY_UNIT: f32 = 100.0;
/// Degrees a chain's angle input reaches at the end of the parameter's range.
const CHAIN_ANGLE_SPAN: f32 = 90.0;

/// Particle state and the time not yet stepped. Sized once from the model.
pub(crate) struct State {
    pos: Vec<f32>,
    vel: Vec<f32>,
    started: Vec<bool>,
    /// Per group: the translation and angle the inputs gave this frame.
    target: Vec<[f32; 3]>,
    accum: f32,
}

impl State {
    pub fn new(model: &CompiledModel) -> Self {
        let n = model.physics_particles.len();
        Self {
            pos: vec![0.0; n * 2],
            vel: vec![0.0; n * 2],
            started: vec![false; model.physics.len()],
            target: vec![[0.0; 3]; model.physics.len()],
            accum: 0.0,
        }
    }
}

/// Steps every group by `dt` seconds and writes the output parameters into
/// `values`.
pub(crate) fn advance(model: &CompiledModel, state: &mut State, values: &mut [f32], dt: f32) {
    if model.physics.is_empty() {
        return;
    }
    state.accum += if dt.is_finite() && dt > 0.0 { dt } else { 0.0 };
    let mut steps = (state.accum / STEP) as u32;
    if steps > MAX_STEPS {
        steps = MAX_STEPS;
        state.accum = 0.0;
    } else {
        state.accum -= steps as f32 * STEP;
    }

    for (g, def) in model.physics.iter().enumerate() {
        let (t, a) = inputs(model, def, values);
        state.target[g] = [t[0], t[1], a];
        let range = def.particle_start * 2..(def.particle_start + def.particle_count) * 2;
        let ps =
            &model.physics_particles[def.particle_start..def.particle_start + def.particle_count];
        let (pos, vel) = (&mut state.pos[range.clone()], &mut state.vel[range]);
        let started = std::mem::replace(&mut state.started[g], true);
        match def.kind {
            PhysicsKind::Pendulum { .. } => pendulum(ps, pos, vel, t, a, started, steps),
            PhysicsKind::Chain { gravity, wind } => {
                chain(ps, pos, vel, t, a, [gravity, wind], started, steps)
            }
        }
    }

    // Outputs restart from the default, so inputs above saw last frame's values.
    for o in &model.physics_outputs {
        values[o.parameter] = model.parameters[o.parameter].default;
    }
    for (g, def) in model.physics.iter().enumerate() {
        let outputs = &model.physics_outputs[def.output_start..def.output_start + def.output_count];
        for o in outputs {
            let at = (def.particle_start + o.particle) * 2;
            let (parent, rest_dir) = parent_and_rest(model, def, state, g, o.particle);
            let rel = [state.pos[at] - parent[0], state.pos[at + 1] - parent[1]];
            let base = match o.channel {
                Channel::X => rel[0],
                Channel::Y => rel[1],
                Channel::Angle => match def.kind {
                    PhysicsKind::Pendulum { .. } => rel[0].atan2(-rel[1]).to_degrees(),
                    PhysicsKind::Chain { .. } => {
                        let cross = rest_dir[0] * rel[1] - rest_dir[1] * rel[0];
                        let dot = rest_dir[0] * rel[0] + rest_dir[1] * rel[1];
                        cross.atan2(dot).to_degrees()
                    }
                },
            };
            let sign = if o.reflect { -1.0 } else { 1.0 };
            values[o.parameter] += base * o.scale * sign * o.weight / 100.0;
        }
    }
}

/// The point particle `k` hangs from, and the chain's rest direction.
fn parent_and_rest(
    model: &CompiledModel,
    def: &PhysicsDef,
    state: &State,
    group: usize,
    k: usize,
) -> (Point, Point) {
    let [tx, ty, a] = state.target[group];
    let base = def.particle_start;
    match def.kind {
        PhysicsKind::Pendulum { .. } if k == 0 => (model.physics_particles[base].rest, [0.0, -1.0]),
        PhysicsKind::Chain { gravity, .. } if k == 0 => ([tx, ty], rest_direction(gravity, a)),
        PhysicsKind::Pendulum { .. } => (
            [
                state.pos[(base + k - 1) * 2],
                state.pos[(base + k - 1) * 2 + 1],
            ],
            [0.0, -1.0],
        ),
        PhysicsKind::Chain { gravity, .. } => (
            [
                state.pos[(base + k - 1) * 2],
                state.pos[(base + k - 1) * 2 + 1],
            ],
            rest_direction(gravity, a),
        ),
    }
}

/// Maps `v` from `[min, def, max]` onto `[lo, tdef, hi]`, piecewise linearly.
fn normalize(v: f32, from: [f32; 3], to: [f32; 3]) -> f32 {
    let [min, def, max] = from;
    let [lo, tdef, hi] = to;
    let v = v.clamp(min, max);
    if v >= def {
        let span = max - def;
        if span > 0.0 {
            tdef + (v - def) / span * (hi - tdef)
        } else {
            tdef
        }
    } else {
        let span = def - min;
        if span > 0.0 {
            tdef + (def - v) / span * (lo - tdef)
        } else {
            tdef
        }
    }
}

fn inputs(model: &CompiledModel, def: &PhysicsDef, values: &[f32]) -> (Point, f32) {
    let mut t = [0.0; 2];
    let mut a = 0.0;
    let ios = &model.physics_inputs[def.input_start..def.input_start + def.input_count];
    for io in ios {
        let p = &model.parameters[io.parameter];
        let to = match (def.kind, io.channel) {
            (PhysicsKind::Pendulum { angle, .. }, Channel::Angle) => {
                [angle.min, angle.default, angle.max]
            }
            (PhysicsKind::Pendulum { position, .. }, _) => {
                [position.min, position.default, position.max]
            }
            (PhysicsKind::Chain { .. }, Channel::Angle) => {
                [-CHAIN_ANGLE_SPAN, 0.0, CHAIN_ANGLE_SPAN]
            }
            (PhysicsKind::Chain { .. }, _) => [-1.0, 0.0, 1.0],
        };
        let v = p.apply(values[io.parameter]);
        let sign = if io.reflect { -1.0 } else { 1.0 };
        let n = normalize(v, [p.min, p.default, p.max], to) * io.weight / 100.0 * sign;
        match io.channel {
            Channel::X => t[0] += n,
            Channel::Y => t[1] += n,
            Channel::Angle => a += n,
        }
    }
    (t, a)
}

fn unit(v: Point, fallback: Point) -> Point {
    let len = v[0].hypot(v[1]);
    if len > 1e-6 {
        [v[0] / len, v[1] / len]
    } else {
        fallback
    }
}

/// Moves `p` onto the circle of `length` around `parent`.
fn constrain(parent: Point, p: Point, length: f32, fallback: Point) -> Point {
    let d = unit([p[0] - parent[0], p[1] - parent[1]], fallback);
    [parent[0] + d[0] * length, parent[1] + d[1] * length]
}

fn pendulum(
    ps: &[PhysicsParticle],
    pos: &mut [f32],
    vel: &mut [f32],
    t: Point,
    angle: f32,
    started: bool,
    steps: u32,
) {
    let Some(root) = ps.first() else { return };
    if !started {
        for (i, p) in ps.iter().enumerate() {
            pos[2 * i] = p.rest[0] + t[0];
            pos[2 * i + 1] = p.rest[1] + t[1];
        }
        vel.fill(0.0);
        return;
    }
    pos[0] = root.rest[0] + t[0];
    pos[1] = root.rest[1] + t[1];
    let (sin, cos) = angle.to_radians().sin_cos();
    let g = [sin, -cos];
    for _ in 0..steps {
        for (i, q) in ps.iter().enumerate().skip(1) {
            let parent = [pos[2 * i - 2], pos[2 * i - 1]];
            let eff = STEP * q.delay.max(0.01);
            let keep = q.mobility.clamp(0.0, 1.0);
            let pull = q.acceleration * GRAVITY_UNIT * STEP;
            let v = [
                (vel[2 * i] + g[0] * pull) * keep,
                (vel[2 * i + 1] + g[1] * pull) * keep,
            ];
            let p = [pos[2 * i], pos[2 * i + 1]];
            let moved = constrain(parent, [p[0] + v[0] * eff, p[1] + v[1] * eff], q.length, g);
            vel[2 * i] = (moved[0] - p[0]) / eff;
            vel[2 * i + 1] = (moved[1] - p[1]) / eff;
            pos[2 * i] = moved[0];
            pos[2 * i + 1] = moved[1];
        }
    }
}

/// The direction a chain hangs in at rest: along gravity (down when there is
/// none), turned by the angle input.
fn rest_direction(gravity: Point, angle: f32) -> Point {
    let d = unit(gravity, [0.0, -1.0]);
    let (sin, cos) = angle.to_radians().sin_cos();
    [d[0] * cos - d[1] * sin, d[0] * sin + d[1] * cos]
}

#[allow(clippy::too_many_arguments)]
fn chain(
    joints: &[PhysicsParticle],
    pos: &mut [f32],
    vel: &mut [f32],
    root: Point,
    angle: f32,
    [gravity, wind]: [Point; 2],
    started: bool,
    steps: u32,
) {
    let rest = rest_direction(gravity, angle);
    if !started {
        let mut at = root;
        for (i, j) in joints.iter().enumerate() {
            at = [at[0] + rest[0] * j.length, at[1] + rest[1] * j.length];
            pos[2 * i] = at[0];
            pos[2 * i + 1] = at[1];
        }
        vel.fill(0.0);
        return;
    }
    let accel = [gravity[0] + wind[0], gravity[1] + wind[1]];
    for _ in 0..steps {
        let mut parent = root;
        for (i, j) in joints.iter().enumerate() {
            let keep = 1.0 - j.damping.clamp(0.0, 1.0);
            let v = [
                (vel[2 * i] + accel[0] * STEP) * keep,
                (vel[2 * i + 1] + accel[1] * STEP) * keep,
            ];
            let p = [pos[2 * i], pos[2 * i + 1]];
            let free = [p[0] + v[0] * STEP, p[1] + v[1] * STEP];
            let k = j.stiffness.clamp(0.0, 1.0);
            let away = unit([free[0] - parent[0], free[1] - parent[1]], rest);
            let dir = unit(
                [
                    away[0] * (1.0 - k) + rest[0] * k,
                    away[1] * (1.0 - k) + rest[1] * k,
                ],
                rest,
            );
            let moved = [parent[0] + dir[0] * j.length, parent[1] + dir[1] * j.length];
            vel[2 * i] = (moved[0] - p[0]) / STEP;
            vel[2 * i + 1] = (moved[1] - p[1]) / STEP;
            pos[2 * i] = moved[0];
            pos[2 * i + 1] = moved[1];
            parent = moved;
        }
    }
}

/// A two-particle pendulum hanging from the origin, driven by `In` (x) and
/// `Tilt` (angle), writing `OutX`, `OutY` and `OutAngle` from particle 1.
#[cfg(test)]
pub(crate) fn sample_model(mobility: f32) -> crate::model::Model {
    use crate::elements::Parameter;
    use crate::physics::{Particle, PendulumGroup, PhysicsInput, PhysicsOutput, Range};

    let mut m = crate::model::Model::new();
    let mut p = |name: &str, lo: f32, hi: f32| {
        m.parameters
            .insert(name, Parameter::new(lo, hi, 0.0))
            .unwrap()
    };
    let (x, tilt) = (p("In", -1.0, 1.0), p("Tilt", -1.0, 1.0));
    let outs = [
        p("OutX", -100.0, 100.0),
        p("OutY", -100.0, 100.0),
        p("OutAngle", -100.0, 100.0),
    ];
    let input = |parameter, channel| PhysicsInput {
        parameter,
        channel,
        weight: 100.0,
        reflect: false,
    };
    let output = |parameter, channel| PhysicsOutput {
        parameter,
        channel,
        particle: 1,
        scale: 1.0,
        weight: 100.0,
        reflect: false,
    };
    let particle = |y: f32, radius: f32| Particle {
        position: [0.0, y],
        mobility,
        delay: 1.0,
        acceleration: 1.0,
        radius,
    };
    m.physics.pendulums.push(PendulumGroup {
        name: "G".into(),
        inputs: vec![input(x, Channel::X), input(tilt, Channel::Angle)],
        outputs: vec![
            output(outs[0], Channel::X),
            output(outs[1], Channel::Y),
            output(outs[2], Channel::Angle),
        ],
        particles: vec![particle(0.0, 0.0), particle(-10.0, 10.0)],
        position_range: Range {
            min: -10.0,
            max: 10.0,
            default: 0.0,
        },
        angle_range: Range {
            min: -30.0,
            max: 30.0,
            default: 0.0,
        },
    });
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiled::CompileError;
    use crate::driver::Driver;
    use crate::eval::Frame;
    use crate::physics::{ChainJoint, PhysicsChain, PhysicsOutput};

    fn read(c: &CompiledModel, f: &mut Frame, name: &str) -> f32 {
        f.parameters_mut()[c.parameter_index(name).unwrap()]
    }

    fn set(c: &CompiledModel, f: &mut Frame, name: &str, v: f32) {
        f.set_parameter(c.parameter_index(name).unwrap(), v);
    }

    fn run(f: &mut Frame, c: &CompiledModel, seconds: f32) {
        for _ in 0..(seconds / STEP) as usize {
            f.advance(c, STEP);
        }
    }

    #[test]
    fn a_pendulum_at_rest_hangs_straight_down() {
        let c = sample_model(0.95).compile().unwrap();
        let mut f = Frame::new(&c);
        f.advance(&c, STEP);
        assert!((read(&c, &mut f, "OutX")).abs() < 1e-5);
        assert!((read(&c, &mut f, "OutY") + 10.0).abs() < 1e-4);
        assert!((read(&c, &mut f, "OutAngle")).abs() < 1e-4);
    }

    #[test]
    fn a_moved_root_swings_the_particle_and_it_settles() {
        let c = sample_model(0.95).compile().unwrap();
        let mut f = Frame::new(&c);
        f.evaluate(&c);
        set(&c, &mut f, "In", 1.0);
        f.advance(&c, STEP);
        // The root jumped 10 to the right; the particle is pulled after it.
        assert!(read(&c, &mut f, "OutAngle") < -20.0);
        run(&mut f, &c, 20.0);
        assert!(read(&c, &mut f, "OutAngle").abs() < 0.1);
        assert!((read(&c, &mut f, "OutY") + 10.0).abs() < 0.01);
    }

    #[test]
    fn the_angle_input_tilts_the_resting_direction() {
        let c = sample_model(0.95).compile().unwrap();
        let mut f = Frame::new(&c);
        f.evaluate(&c);
        set(&c, &mut f, "Tilt", 1.0);
        run(&mut f, &c, 20.0);
        assert!((read(&c, &mut f, "OutAngle") - 30.0).abs() < 0.1);
    }

    #[test]
    fn the_result_does_not_depend_on_the_frame_rate() {
        let c = sample_model(0.95).compile().unwrap();
        let outcome = |dts: &[f32]| {
            let mut f = Frame::new(&c);
            f.evaluate(&c);
            set(&c, &mut f, "In", 1.0);
            for &dt in dts {
                f.advance(&c, dt);
            }
            read(&c, &mut f, "OutAngle")
        };
        assert_eq!(outcome(&[STEP]), outcome(&[STEP / 2.0, STEP / 2.0]));
        assert_eq!(outcome(&[2.0 * STEP]), outcome(&[STEP, STEP]));
        assert_ne!(outcome(&[STEP]), outcome(&[2.0 * STEP]));
    }

    #[test]
    fn a_long_pause_takes_at_most_the_step_limit() {
        let c = sample_model(0.95).compile().unwrap();
        let mut f = Frame::new(&c);
        f.evaluate(&c);
        set(&c, &mut f, "In", 1.0);
        f.advance(&c, 1000.0);
        let capped = read(&c, &mut f, "OutAngle");
        let mut g = Frame::new(&c);
        g.evaluate(&c);
        set(&c, &mut g, "In", 1.0);
        g.advance(&c, MAX_STEPS as f32 * STEP);
        assert_eq!(capped, read(&c, &mut g, "OutAngle"));
    }

    #[test]
    fn evaluate_alone_does_not_advance_physics() {
        let c = sample_model(0.95).compile().unwrap();
        let mut f = Frame::new(&c);
        f.evaluate(&c);
        set(&c, &mut f, "In", 1.0);
        f.evaluate(&c);
        f.evaluate(&c);
        // The root moved but nothing stepped: the particle still hangs from where it was.
        let held = read(&c, &mut f, "OutAngle");
        f.advance(&c, STEP);
        assert_ne!(held, read(&c, &mut f, "OutAngle"));
    }

    fn chain_model(stiffness: f32, wind: Point) -> crate::model::Model {
        use crate::elements::Parameter;
        let mut m = crate::model::Model::new();
        let out = m
            .parameters
            .insert("Out", Parameter::new(-100.0, 100.0, 0.0))
            .unwrap();
        m.physics.chains.push(PhysicsChain {
            name: "Hair".into(),
            joints: vec![ChainJoint {
                length: 1.0,
                stiffness,
                damping: 0.2,
            }],
            gravity: [0.0, -9.8],
            wind,
            inputs: vec![],
            outputs: vec![PhysicsOutput {
                parameter: out,
                channel: Channel::Angle,
                particle: 0,
                scale: 1.0,
                weight: 100.0,
                reflect: false,
            }],
        });
        m
    }

    #[test]
    fn a_chain_leans_with_the_wind_unless_stiff() {
        let c = chain_model(0.0, [5.0, 0.0]).compile().unwrap();
        let mut f = Frame::new(&c);
        f.evaluate(&c);
        run(&mut f, &c, 10.0);
        let lean = (5.0f32 / 9.8).atan().to_degrees();
        assert!((read(&c, &mut f, "Out") - lean).abs() < 1.0);

        let c = chain_model(1.0, [5.0, 0.0]).compile().unwrap();
        let mut f = Frame::new(&c);
        f.evaluate(&c);
        run(&mut f, &c, 10.0);
        assert!(read(&c, &mut f, "Out").abs() < 1e-3);
    }

    #[test]
    fn a_driver_cannot_write_a_physics_output() {
        let mut m = sample_model(0.95);
        let out = m.parameters.find("OutX").unwrap();
        m.drivers
            .insert(
                "D",
                Driver {
                    output: out,
                    expression: "1".into(),
                },
            )
            .unwrap();
        assert!(matches!(
            m.compile(),
            Err(CompileError::PhysicsOutputConflict { group }) if group == "G"
        ));
    }
}
