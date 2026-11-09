// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The physics panel: the pendulum groups and physics chains of the model and
//! the settings of one. A finished edit of any field sends the whole group as
//! one `SetPendulum` / `SetPhysicsChain` (Undo 1 step). The model changes, so
//! the viewport rebuilds its scene and the physics starts settling again: the
//! change shows at once.

use egui::Ui;
use tsukumoe_core::command::{Command, PhysicsInputDef, PhysicsOutputDef};
use tsukumoe_core::model::Model;
use tsukumoe_core::physics::{ChainJoint, Channel, Particle, PendulumGroup, PhysicsChain, Range};
use tsukumoe_ui::{
    Icon, Variant, button, checkbox, hint, icon_button, number, section_header, slider, tr,
};

use crate::physics_preset::{self as preset, HEAD_INPUTS, Preset, feel_of};
use crate::view::ViewState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pendulum,
    Chain,
}

/// What the panel remembers between frames. Not saved, not in the history.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub selected: Option<(Kind, String)>,
    /// The test that wiggles the inputs of the picked group, while it runs.
    testing: Option<Test>,
}

#[derive(Clone, Debug)]
struct Test {
    group: (Kind, String),
    /// The values the inputs had when the test started.
    base: Vec<(String, f32)>,
    time: f32,
}

pub enum Out {
    Command(Command, String),
}

/// A name for a new group that no group of the kind has.
pub fn unique_name(model: &Model, kind: Kind) -> String {
    let (prefix, taken): (&str, Vec<&str>) = match kind {
        Kind::Pendulum => (
            "Pendulum",
            model
                .physics()
                .pendulums
                .iter()
                .map(|g| g.name.as_str())
                .collect(),
        ),
        Kind::Chain => (
            "Chain",
            model
                .physics()
                .chains
                .iter()
                .map(|c| c.name.as_str())
                .collect(),
        ),
    };
    (1..)
        .map(|n| format!("{prefix}{n}"))
        .find(|name| !taken.contains(&name.as_str()))
        .expect("endless range")
}

fn input_defs(
    model: &Model,
    inputs: &[tsukumoe_core::physics::PhysicsInput],
) -> Vec<PhysicsInputDef> {
    inputs
        .iter()
        .filter_map(|i| {
            Some(PhysicsInputDef {
                parameter: model.parameters().name(i.parameter)?.to_owned(),
                channel: i.channel,
                weight: i.weight,
                reflect: i.reflect,
            })
        })
        .collect()
}

fn output_defs(
    model: &Model,
    outputs: &[tsukumoe_core::physics::PhysicsOutput],
) -> Vec<PhysicsOutputDef> {
    outputs
        .iter()
        .filter_map(|o| {
            Some(PhysicsOutputDef {
                parameter: model.parameters().name(o.parameter)?.to_owned(),
                channel: o.channel,
                particle: o.particle,
                scale: o.scale,
                weight: o.weight,
                reflect: o.reflect,
            })
        })
        .collect()
}

fn channel_key(c: Channel) -> &'static str {
    match c {
        Channel::X => "physics-channel-x",
        Channel::Y => "physics-channel-y",
        Channel::Angle => "physics-channel-angle",
    }
}

/// Whether a number field finished an edit this frame.
fn done(r: &egui::Response) -> bool {
    r.drag_stopped() || (r.changed() && !r.dragged() && !r.has_focus())
}

/// A number field that is true when its edit ends.
fn field(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, name: &str) -> bool {
    done(&number(ui, value, range, 0.05, name))
}

fn param_combo(ui: &mut Ui, model: &Model, value: &mut String, name: &str) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(("param", name))
        .selected_text(value.clone())
        .width(110.0)
        .show_ui(ui, |ui| {
            for (_, p, _) in model.parameters().iter() {
                if ui.selectable_label(value == p, p).clicked() && value != p {
                    *value = p.to_owned();
                    changed = true;
                }
            }
        })
        .response
        .accessible_name(name);
    changed
}

fn channel_combo(ui: &mut Ui, ctx: &egui::Context, value: &mut Channel) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("channel")
        .selected_text(tr(ctx, channel_key(*value)))
        .width(70.0)
        .show_ui(ui, |ui| {
            for c in [Channel::X, Channel::Y, Channel::Angle] {
                if ui
                    .selectable_label(*value == c, tr(ctx, channel_key(c)))
                    .clicked()
                    && *value != c
                {
                    *value = c;
                    changed = true;
                }
            }
        })
        .response
        .accessible_name(tr(ctx, "physics-channel"));
    changed
}

/// The input and output tables shared by both kinds. `targets` is how many
/// particles (joints) an output may name.
fn io_tables(
    ui: &mut Ui,
    model: &Model,
    inputs: &mut Vec<PhysicsInputDef>,
    outputs: &mut Vec<PhysicsOutputDef>,
    targets: usize,
    focus: Option<&str>,
) -> bool {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    // A new row takes the parameter in focus in the parameters panel, else the first.
    let first = focus
        .filter(|f| model.parameters().find(f).is_some())
        .map(str::to_owned)
        .or_else(|| {
            model
                .parameters()
                .iter()
                .next()
                .map(|(_, n, _)| n.to_owned())
        });
    let mut changed = false;
    section_header(ui, &t("physics-inputs"));
    let mut remove = None;
    for (i, input) in inputs.iter_mut().enumerate() {
        ui.push_id(("input", i), |ui| {
            ui.horizontal_wrapped(|ui| {
                changed |= param_combo(ui, model, &mut input.parameter, &t("physics-parameter"));
                changed |= channel_combo(ui, &ctx, &mut input.channel);
                changed |= field(ui, &mut input.weight, 0.0..=1000.0, &t("physics-weight"));
                changed |= checkbox(ui, &mut input.reflect, &t("physics-reflect")).changed();
                if icon_button(ui, Icon::Trash, &t("physics-row-delete"), Variant::Ghost).clicked()
                {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        inputs.remove(i);
        changed = true;
    }
    ui.add_enabled_ui(first.is_some(), |ui| {
        if button(ui, &t("physics-add-input"), Variant::Secondary).clicked()
            && let Some(parameter) = first.clone()
        {
            inputs.push(PhysicsInputDef {
                parameter,
                channel: Channel::Angle,
                weight: 100.0,
                reflect: false,
            });
            changed = true;
        }
    });
    section_header(ui, &t("physics-outputs"));
    let mut remove = None;
    for (i, output) in outputs.iter_mut().enumerate() {
        ui.push_id(("output", i), |ui| {
            ui.horizontal_wrapped(|ui| {
                changed |= param_combo(ui, model, &mut output.parameter, &t("physics-parameter"));
                changed |= channel_combo(ui, &ctx, &mut output.channel);
                egui::ComboBox::from_id_salt("particle")
                    .selected_text(output.particle.to_string())
                    .width(48.0)
                    .show_ui(ui, |ui| {
                        for p in 0..targets {
                            if ui
                                .selectable_label(output.particle == p, p.to_string())
                                .clicked()
                                && output.particle != p
                            {
                                output.particle = p;
                                changed = true;
                            }
                        }
                    })
                    .response
                    .accessible_name(t("physics-particle"));
                changed |= field(ui, &mut output.scale, -1000.0..=1000.0, &t("physics-scale"));
                changed |= field(ui, &mut output.weight, 0.0..=1000.0, &t("physics-weight"));
                changed |= checkbox(ui, &mut output.reflect, &t("physics-reflect")).changed();
                if icon_button(ui, Icon::Trash, &t("physics-row-delete"), Variant::Ghost).clicked()
                {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        outputs.remove(i);
        changed = true;
    }
    ui.add_enabled_ui(first.is_some() && targets > 0, |ui| {
        if button(ui, &t("physics-add-output"), Variant::Secondary).clicked()
            && let Some(parameter) = first
        {
            outputs.push(PhysicsOutputDef {
                parameter,
                channel: Channel::Angle,
                particle: targets - 1,
                scale: 1.0,
                weight: 100.0,
                reflect: false,
            });
            changed = true;
        }
    });
    changed
}

fn range_row(ui: &mut Ui, ctx: &egui::Context, label: &str, range: &mut Range) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        changed |= field(ui, &mut range.min, -1.0e6..=1.0e6, &tr(ctx, "physics-min"));
        changed |= field(
            ui,
            &mut range.default,
            -1.0e6..=1.0e6,
            &tr(ctx, "physics-default"),
        );
        changed |= field(ui, &mut range.max, -1.0e6..=1.0e6, &tr(ctx, "physics-max"));
    });
    changed
}

/// Adds an input for each head parameter the model has and the group lacks.
fn add_head_inputs(model: &Model, inputs: &mut Vec<PhysicsInputDef>) {
    for (name, channel, weight) in HEAD_INPUTS {
        if model.parameters().find(name).is_some() && !inputs.iter().any(|i| i.parameter == name) {
            inputs.push(PhysicsInputDef {
                parameter: name.to_owned(),
                channel,
                weight,
                reflect: false,
            });
        }
    }
}

/// Drops the outputs on particle `removed` and moves the ones after it back.
fn forget_target(outputs: &mut Vec<PhysicsOutputDef>, removed: usize) {
    outputs.retain(|o| o.particle != removed);
    for o in outputs {
        if o.particle > removed {
            o.particle -= 1;
        }
    }
}

/// Edits a pendulum group; the command if anything finished changing.
fn pendulum(
    ui: &mut Ui,
    model: &Model,
    group: &PendulumGroup,
    focus: Option<&str>,
) -> Option<Command> {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let mut inputs = input_defs(model, &group.inputs);
    let mut outputs = output_defs(model, &group.outputs);
    let mut particles = group.particles.clone();
    let (mut position, mut angle) = (group.position_range, group.angle_range);
    let mut changed = false;
    // Presets: the particles of a kind of hair, and the head inputs it needs.
    ui.horizontal_wrapped(|ui| {
        ui.label(t("physics-preset"));
        for p in Preset::ALL {
            if button(ui, &t(p.key()), Variant::Secondary).clicked() {
                particles = preset::particles(p.feel(), p.count());
                outputs.retain(|o| o.particle < particles.len());
                if inputs.is_empty() {
                    add_head_inputs(model, &mut inputs);
                }
                changed = true;
            }
        }
    });
    // The three knobs, read back from particle 1.
    let mut feel = feel_of(&particles);
    let before = feel;
    let mut knob = false;
    for (value, key) in [
        (&mut feel.sway, "physics-sway"),
        (&mut feel.stickiness, "physics-stickiness"),
        (&mut feel.length, "physics-length-knob"),
    ] {
        ui.horizontal(|ui| {
            ui.label(t(key));
            knob |= done(&slider(ui, value, 0.0..=1.0, &t(key)));
        });
    }
    if knob && feel != before {
        particles = preset::particles(feel, particles.len());
        changed = true;
    }
    // Inputs by the head parameters the model has.
    let present: Vec<_> = HEAD_INPUTS
        .iter()
        .filter(|(n, _, _)| model.parameters().find(n).is_some())
        .collect();
    ui.horizontal_wrapped(|ui| {
        ui.label(t("physics-head-inputs"));
        for &&(name, channel, weight) in &present {
            let mut on = inputs.iter().any(|i| i.parameter == name);
            if checkbox(ui, &mut on, name).changed() {
                if on {
                    inputs.push(PhysicsInputDef {
                        parameter: name.to_owned(),
                        channel,
                        weight,
                        reflect: false,
                    });
                } else {
                    inputs.retain(|i| i.parameter != name);
                }
                changed = true;
            }
        }
        if present.is_empty() {
            hint(ui, t("physics-no-head-inputs"));
        }
    });
    changed |= io_tables(ui, model, &mut inputs, &mut outputs, particles.len(), focus);
    egui::CollapsingHeader::new(t("physics-details"))
        .id_salt("physics-details")
        .show(ui, |ui| {
            changed |= range_row(ui, &ctx, &t("physics-position-range"), &mut position);
            changed |= range_row(ui, &ctx, &t("physics-angle-range"), &mut angle);
            section_header(ui, &t("physics-particles"));
            let mut remove = None;
            let count = particles.len();
            for (i, p) in particles.iter_mut().enumerate() {
                ui.push_id(("particle", i), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(i.to_string());
                        changed |= field(ui, &mut p.position[0], -1.0e6..=1.0e6, &t("physics-x"));
                        changed |= field(ui, &mut p.position[1], -1.0e6..=1.0e6, &t("physics-y"));
                        changed |= field(ui, &mut p.mobility, 0.0..=1.0, &t("physics-mobility"));
                        changed |= field(ui, &mut p.delay, 0.0..=10.0, &t("physics-delay"));
                        changed |= field(
                            ui,
                            &mut p.acceleration,
                            0.0..=100.0,
                            &t("physics-acceleration"),
                        );
                        changed |= field(ui, &mut p.radius, 0.0..=1000.0, &t("physics-radius"));
                        ui.add_enabled_ui(count > 1, |ui| {
                            if icon_button(
                                ui,
                                Icon::Trash,
                                &t("physics-row-delete"),
                                Variant::Ghost,
                            )
                            .clicked()
                            {
                                remove = Some(i);
                            }
                        });
                    });
                });
            }
            if let Some(i) = remove {
                particles.remove(i);
                forget_target(&mut outputs, i);
                changed = true;
            }
            if button(ui, &t("physics-add-particle"), Variant::Secondary).clicked() {
                let last = particles.last().copied().unwrap_or(Particle {
                    position: [0.0, 0.0],
                    mobility: 0.95,
                    delay: 0.9,
                    acceleration: 1.5,
                    radius: 10.0,
                });
                particles.push(Particle {
                    position: [last.position[0], last.position[1] + last.radius.max(1.0)],
                    ..last
                });
                changed = true;
            }
        });
    changed.then(|| Command::SetPendulum {
        name: group.name.clone(),
        inputs,
        outputs,
        particles,
        position_range: position,
        angle_range: angle,
    })
}

/// Edits a physics chain; the command if anything finished changing.
fn chain(ui: &mut Ui, model: &Model, c: &PhysicsChain, focus: Option<&str>) -> Option<Command> {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let mut inputs = input_defs(model, &c.inputs);
    let mut outputs = output_defs(model, &c.outputs);
    let mut joints = c.joints.clone();
    let (mut gravity, mut wind) = (c.gravity, c.wind);
    let mut changed = false;
    for (label, v) in [
        (t("physics-gravity"), &mut gravity),
        (t("physics-wind"), &mut wind),
    ] {
        ui.horizontal_wrapped(|ui| {
            ui.label(label);
            changed |= field(ui, &mut v[0], -1.0e4..=1.0e4, &t("physics-x"));
            changed |= field(ui, &mut v[1], -1.0e4..=1.0e4, &t("physics-y"));
        });
    }
    changed |= io_tables(ui, model, &mut inputs, &mut outputs, joints.len(), focus);
    section_header(ui, &t("physics-joints"));
    let mut remove = None;
    let count = joints.len();
    for (i, j) in joints.iter_mut().enumerate() {
        ui.push_id(("joint", i), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(i.to_string());
                changed |= field(ui, &mut j.length, 0.0..=1.0e4, &t("physics-length"));
                changed |= field(ui, &mut j.stiffness, 0.0..=1.0, &t("physics-stiffness"));
                changed |= field(ui, &mut j.damping, 0.0..=1.0, &t("physics-damping"));
                ui.add_enabled_ui(count > 1, |ui| {
                    if icon_button(ui, Icon::Trash, &t("physics-row-delete"), Variant::Ghost)
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
            });
        });
    }
    if let Some(i) = remove {
        joints.remove(i);
        forget_target(&mut outputs, i);
        changed = true;
    }
    if button(ui, &t("physics-add-joint"), Variant::Secondary).clicked() {
        joints.push(joints.last().copied().unwrap_or(ChainJoint {
            length: 1.0,
            stiffness: 0.5,
            damping: 0.1,
        }));
        changed = true;
    }
    changed.then(|| Command::SetPhysicsChain {
        name: c.name.clone(),
        joints,
        gravity,
        wind,
        inputs,
        outputs,
    })
}

/// The names of the parameters a group takes as input.
fn input_names(model: &Model, kind: Kind, name: &str) -> Vec<String> {
    let inputs = match kind {
        Kind::Pendulum => model
            .physics()
            .pendulums
            .iter()
            .find(|g| g.name == name)
            .map(|g| &g.inputs),
        Kind::Chain => model
            .physics()
            .chains
            .iter()
            .find(|c| c.name == name)
            .map(|c| &c.inputs),
    };
    inputs
        .into_iter()
        .flatten()
        .filter_map(|i| model.parameters().name(i.parameter).map(str::to_owned))
        .collect()
}

/// Puts the inputs back to what they were and ends the test.
fn stop_test(model: &Model, view: &mut ViewState) {
    for (name, value) in view
        .physics
        .testing
        .take()
        .map(|t| t.base)
        .unwrap_or_default()
    {
        if let Some(p) = model
            .parameters()
            .find(&name)
            .and_then(|id| model.parameters().get(id))
        {
            view.set_param(&name, p, value);
        }
    }
}

/// The button that wiggles the inputs of the picked group, and the wiggle.
fn test_row(ui: &mut Ui, model: &Model, view: &mut ViewState, kind: Kind, name: &str) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let names = input_names(model, kind, name);
    let group = (kind, name.to_owned());
    if view
        .physics
        .testing
        .as_ref()
        .is_some_and(|test| test.group != group)
    {
        stop_test(model, view);
    }
    ui.horizontal(|ui| {
        if view.physics.testing.is_some() {
            if button(ui, &t("physics-test-stop"), Variant::Secondary).clicked() {
                stop_test(model, view);
            }
        } else {
            ui.add_enabled_ui(!names.is_empty(), |ui| {
                if button(ui, &t("physics-test"), Variant::Secondary)
                    .on_hover_text(t("physics-test-hint"))
                    .clicked()
                {
                    let base = names
                        .iter()
                        .filter_map(|n| {
                            let p = model.parameters().get(model.parameters().find(n)?)?;
                            Some((n.clone(), view.param(n, p)))
                        })
                        .collect();
                    view.physics.testing = Some(Test {
                        group,
                        base,
                        time: 0.0,
                    });
                }
            });
            if names.is_empty() {
                hint(ui, t("physics-test-no-input"));
            }
        }
    });
    if let Some(test) = view.physics.testing.as_mut() {
        test.time += ui.input(|i| i.stable_dt);
        let time = test.time;
        for (i, n) in names.iter().enumerate() {
            if let Some(p) = model
                .parameters()
                .find(n)
                .and_then(|id| model.parameters().get(id))
            {
                let swing = (p.max - p.default).min(p.default - p.min).max(0.0) * 0.8;
                let value = p.default + swing * (time * 2.4 + i as f32 * 1.3).sin();
                view.set_param(n, p, value);
            }
        }
        ui.ctx().request_repaint();
    }
}

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Vec<Out> {
    let mut outs = Vec::new();
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let st = &mut view.physics;
    // The group may be gone (undo, another document).
    if let Some((kind, name)) = &st.selected {
        let there = match kind {
            Kind::Pendulum => model.physics().pendulums.iter().any(|g| &g.name == name),
            Kind::Chain => model.physics().chains.iter().any(|c| &c.name == name),
        };
        if !there {
            st.selected = None;
        }
    }
    ui.horizontal_wrapped(|ui| {
        let shown = st
            .selected
            .as_ref()
            .map_or_else(|| t("physics-pick"), |(_, n)| n.clone());
        egui::ComboBox::from_id_salt("physics-pick")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for g in &model.physics().pendulums {
                    let on = st.selected == Some((Kind::Pendulum, g.name.clone()));
                    let label = format!("{} ({})", g.name, t("physics-pendulum"));
                    if ui.selectable_label(on, label).clicked() {
                        st.selected = Some((Kind::Pendulum, g.name.clone()));
                    }
                }
                for c in &model.physics().chains {
                    let on = st.selected == Some((Kind::Chain, c.name.clone()));
                    let label = format!("{} ({})", c.name, t("physics-chain"));
                    if ui.selectable_label(on, label).clicked() {
                        st.selected = Some((Kind::Chain, c.name.clone()));
                    }
                }
            })
            .response
            .accessible_name(t("physics-pick"));
        if button(ui, &t("physics-new-pendulum"), Variant::Secondary).clicked() {
            let name = unique_name(model, Kind::Pendulum);
            outs.push(Out::Command(
                Command::AddPendulum { name: name.clone() },
                format!("pendulum_add: {name}"),
            ));
            st.selected = Some((Kind::Pendulum, name));
        }
        if button(ui, &t("physics-new-chain"), Variant::Secondary).clicked() {
            let name = unique_name(model, Kind::Chain);
            outs.push(Out::Command(
                Command::AddPhysicsChain { name: name.clone() },
                format!("physics_chain_add: {name}"),
            ));
            st.selected = Some((Kind::Chain, name));
        }
        if let Some((kind, name)) = st.selected.clone()
            && icon_button(ui, Icon::Trash, &t("physics-delete"), Variant::Ghost).clicked()
        {
            let command = match kind {
                Kind::Pendulum => Command::RemovePendulum { name: name.clone() },
                Kind::Chain => Command::RemovePhysicsChain { name: name.clone() },
            };
            outs.push(Out::Command(command, format!("physics_remove: {name}")));
            st.selected = None;
        }
    });
    let Some((kind, name)) = st.selected.clone() else {
        hint(ui, t("physics-empty"));
        return outs;
    };
    test_row(ui, model, view, kind, &name);
    let focus = view.param_focus.clone();
    let focus = focus.as_deref();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .id_salt("physics-scroll")
        .show(ui, |ui| {
            let command = match kind {
                Kind::Pendulum => model
                    .physics()
                    .pendulums
                    .iter()
                    .find(|g| g.name == name)
                    .and_then(|g| pendulum(ui, model, g, focus)),
                Kind::Chain => model
                    .physics()
                    .chains
                    .iter()
                    .find(|c| c.name == name)
                    .and_then(|c| chain(ui, model, c, focus)),
            };
            if let Some(command) = command {
                outs.push(Out::Command(command, format!("physics_set: {name}")));
            }
        });
    outs
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn model() -> Model {
        let mut m = Model::new();
        for c in [
            Command::AddParameter {
                name: "AngleX".into(),
                min: -30.0,
                max: 30.0,
                default: 0.0,
                repeat: false,
            },
            Command::AddParameter {
                name: "ParamAngleX".into(),
                min: -30.0,
                max: 30.0,
                default: 0.0,
                repeat: false,
            },
            Command::AddPendulum {
                name: "Hair".into(),
            },
        ] {
            m.apply(c).unwrap();
        }
        m
    }

    struct Panel {
        model: Model,
        view: ViewState,
        outs: Vec<Out>,
    }

    fn harness(select: bool) -> Harness<'static, Panel> {
        let mut view = ViewState::default();
        if select {
            view.physics.selected = Some((Kind::Pendulum, "Hair".into()));
        }
        Harness::builder()
            .with_size(egui::vec2(560.0, 420.0))
            .build_ui_state(
                |ui, p: &mut Panel| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    let outs = show(ui, &p.model, &mut p.view);
                    p.outs.extend(outs);
                },
                Panel {
                    model: model(),
                    view,
                    outs: Vec::new(),
                },
            )
    }

    #[test]
    fn the_new_buttons_make_a_group_of_each_kind() {
        let mut h = harness(false);
        h.run();
        h.get_by_label("New pendulum").click();
        h.run();
        let Some(Out::Command(Command::AddPendulum { name }, _)) = h.state().outs.last() else {
            panic!("an AddPendulum");
        };
        assert_eq!(name, "Pendulum1");
        h.get_by_label("New chain").click();
        h.run();
        let Some(Out::Command(Command::AddPhysicsChain { name }, _)) = h.state().outs.last() else {
            panic!("an AddPhysicsChain");
        };
        assert_eq!(name, "Chain1");
    }

    #[test]
    fn adding_an_input_sends_the_whole_group() {
        let mut h = harness(true);
        h.run();
        h.get_by_label("Add input").click();
        h.run();
        let Some(Out::Command(
            Command::SetPendulum {
                name,
                inputs,
                particles,
                ..
            },
            _,
        )) = h.state().outs.last()
        else {
            panic!("a SetPendulum");
        };
        assert_eq!(name, "Hair");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].parameter, "AngleX");
        assert_eq!(particles.len(), 2);
    }

    #[test]
    fn a_preset_sets_the_particles_and_the_head_inputs() {
        let mut h = harness(true);
        h.run();
        h.get_by_label("Long hair").click();
        h.run();
        let Some(Out::Command(
            Command::SetPendulum {
                inputs, particles, ..
            },
            _,
        )) = h.state().outs.last()
        else {
            panic!("a SetPendulum");
        };
        assert_eq!(particles.len(), Preset::LongHair.count());
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].parameter, "ParamAngleX");
    }

    #[test]
    fn the_test_wiggles_the_inputs_and_stop_puts_them_back() {
        let mut h = harness(true);
        h.state_mut()
            .model
            .apply(Command::SetPendulum {
                name: "Hair".into(),
                inputs: vec![PhysicsInputDef {
                    parameter: "ParamAngleX".into(),
                    channel: Channel::X,
                    weight: 60.0,
                    reflect: false,
                }],
                outputs: vec![],
                particles: model().physics().pendulums[0].particles.clone(),
                position_range: model().physics().pendulums[0].position_range,
                angle_range: model().physics().pendulums[0].angle_range,
            })
            .unwrap();
        h.run();
        h.get_by_label("Wiggle to test").click();
        h.run_steps(8);
        let value = |h: &Harness<'_, Panel>| {
            let m = &h.state().model;
            let p = m
                .parameters()
                .get(m.parameters().find("ParamAngleX").unwrap())
                .unwrap();
            h.state().view.param("ParamAngleX", p)
        };
        assert!(value(&h) != 0.0, "the input moves");
        h.get_by_label("Stop the test").click();
        h.run_steps(3);
        assert_eq!(value(&h), 0.0);
    }

    #[test]
    fn removing_a_particle_drops_or_shifts_its_outputs() {
        let out = |particle| PhysicsOutputDef {
            parameter: "AngleX".into(),
            channel: Channel::Angle,
            particle,
            scale: 1.0,
            weight: 100.0,
            reflect: false,
        };
        let mut outputs = vec![out(0), out(1), out(2)];
        forget_target(&mut outputs, 1);
        let left: Vec<usize> = outputs.iter().map(|o| o.particle).collect();
        assert_eq!(left, vec![0, 1]);
    }
}
