// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The inspector: the selected element's properties. Parameter ranges and part
//! opacity can be edited; each finished edit is one `Command` handed back to
//! the caller (and so to the dispatcher). Everything else is read-only here;
//! editing tools come in later units.

use egui::{Id, Ui};
use tsukumoe_core::command::{Command, RotationBase};
use tsukumoe_core::model::Model;
use tsukumoe_ui::{checkbox, hint, label, number, section_header, tr};

use crate::deformer_tool::DeformerTool;
use crate::view::Selection;

/// A numeric field that edits a local copy while it is dragged or typed into,
/// and returns the value once when the edit ends and differs from `original`.
fn commit_field(
    ui: &mut Ui,
    key: &str,
    original: f32,
    range: std::ops::RangeInclusive<f32>,
    speed: f64,
    name: &str,
) -> Option<f32> {
    let id = Id::unique(("inspector", key));
    let mut value = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(original);
    let response = number(ui, &mut value, range, speed, name);
    if response.changed() {
        ui.data_mut(|d| d.insert_temp(id, value));
    }
    let ended = response.drag_stopped() || response.lost_focus();
    if ended {
        ui.data_mut(|d| d.remove::<f32>(id));
        if value != original {
            return Some(value);
        }
    }
    None
}

/// The field of a rotation deformer's base shape an inspector row edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotationField {
    OriginX,
    OriginY,
    Angle,
    Scale,
    ReflectX,
    ReflectY,
}

/// `base` with `field` set to `value` (a flag is on above 0.5).
pub fn with_field(mut base: RotationBase, field: RotationField, value: f32) -> RotationBase {
    match field {
        RotationField::OriginX => base.origin[0] = value,
        RotationField::OriginY => base.origin[1] = value,
        RotationField::Angle => base.angle_degrees = value,
        RotationField::Scale => base.scale = value,
        RotationField::ReflectX => base.reflect_x = value > 0.5,
        RotationField::ReflectY => base.reflect_y = value > 0.5,
    }
    base
}

pub fn show(
    ui: &mut Ui,
    model: &Model,
    selection: Option<&Selection>,
    deformer: Option<&DeformerTool>,
) -> Option<Command> {
    let ctx = ui.ctx().clone();
    let Some(selection) = selection else {
        hint(ui, tr(&ctx, "inspector-none"));
        return None;
    };
    let mut command = None;
    match selection {
        Selection::Parameter(name) => {
            let p = model
                .parameters()
                .find(name)
                .and_then(|id| model.parameters().get(id))?;
            section_header(ui, name);
            let (min, max, default) = (p.min, p.max, p.default);
            for (key, field, original) in [
                ("inspector-min", 0, min),
                ("inspector-max", 1, max),
                ("inspector-default", 2, default),
            ] {
                ui.horizontal(|ui| {
                    label(ui, tr(&ctx, key));
                    let speed = f64::from((max - min).abs().max(1.0)) / 200.0;
                    if let Some(v) = commit_field(
                        ui,
                        &format!("{name}/{field}"),
                        original,
                        f32::MIN..=f32::MAX,
                        speed,
                        &tr(&ctx, key),
                    ) {
                        let mut r = [min, max, default];
                        r[field] = v;
                        command = Some(Command::SetParameterRange {
                            name: name.clone(),
                            min: r[0],
                            max: r[1],
                            default: r[2],
                        });
                    }
                });
            }
        }
        Selection::Part(name) => {
            let p = model
                .parts()
                .find(name)
                .and_then(|id| model.parts().get(id))?;
            section_header(ui, name);
            ui.horizontal(|ui| {
                label(ui, tr(&ctx, "inspector-opacity"));
                if let Some(v) = commit_field(
                    ui,
                    &format!("{name}/opacity"),
                    p.opacity,
                    0.0..=1.0,
                    0.005,
                    &tr(&ctx, "inspector-opacity"),
                ) {
                    command = Some(Command::SetPartOpacity {
                        name: name.clone(),
                        opacity: v,
                    });
                }
            });
        }
        Selection::Mesh(name) => {
            let id = model.meshes().find(name)?;
            let mesh = model.meshes().get(id)?;
            section_header(ui, name);
            hint(
                ui,
                format!(
                    "{}: {}",
                    tr(&ctx, "inspector-vertices"),
                    mesh.positions.len()
                ),
            );
            hint(
                ui,
                format!(
                    "{}: {}",
                    tr(&ctx, "inspector-triangles"),
                    mesh.indices.len() / 3
                ),
            );
        }
        Selection::Warp(name) => {
            let w = model
                .warps()
                .find(name)
                .and_then(|id| model.warps().get(id))?;
            section_header(ui, name);
            // The grid size is fixed; a different one means making a new warp.
            hint(
                ui,
                format!("{}: {} x {}", tr(&ctx, "inspector-grid"), w.rows, w.columns),
            );
            // The selected control points, in the parent's space (the stored base).
            let selected: Vec<u32> = deformer
                .filter(|t| t.deformer() == Some(name.as_str()))
                .map(|t| t.points().iter().copied().take(8).collect())
                .unwrap_or_default();
            for i in selected {
                let Some(&p) = w.base_points.get(i as usize) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    label(ui, format!("{} {i}", tr(&ctx, "inspector-point")));
                    for (axis, original) in [(0usize, p[0]), (1, p[1])] {
                        let key = format!("{name}/point{i}/{axis}");
                        if let Some(v) = commit_field(
                            ui,
                            &key,
                            original,
                            f32::MIN..=f32::MAX,
                            0.01,
                            &tr(&ctx, "inspector-point"),
                        ) {
                            let mut points = w.base_points.clone();
                            points[i as usize][axis] = v;
                            command = Some(Command::SetWarpBasePoints {
                                name: name.clone(),
                                points,
                            });
                        }
                    }
                });
            }
        }
        Selection::Rotation(name) => {
            let r = model
                .rotations()
                .find(name)
                .and_then(|id| model.rotations().get(id))?;
            section_header(ui, name);
            let base = RotationBase::of(&r.base);
            let rows = [
                (
                    "inspector-origin-x",
                    RotationField::OriginX,
                    base.origin[0],
                    f32::MIN..=f32::MAX,
                    0.01,
                ),
                (
                    "inspector-origin-y",
                    RotationField::OriginY,
                    base.origin[1],
                    f32::MIN..=f32::MAX,
                    0.01,
                ),
                (
                    "inspector-angle",
                    RotationField::Angle,
                    base.angle_degrees,
                    -3600.0..=3600.0,
                    0.5,
                ),
                (
                    "inspector-scale",
                    RotationField::Scale,
                    base.scale,
                    0.01..=100.0,
                    0.01,
                ),
            ];
            for (key, field, original, range, speed) in rows {
                ui.horizontal(|ui| {
                    label(ui, tr(&ctx, key));
                    if let Some(v) = commit_field(
                        ui,
                        &format!("{name}/{key}"),
                        original,
                        range,
                        speed,
                        &tr(&ctx, key),
                    ) {
                        command = Some(Command::SetRotationBase {
                            name: name.clone(),
                            base: with_field(base, field, v),
                        });
                    }
                });
            }
            for (key, field, on) in [
                (
                    "inspector-reflect-x",
                    RotationField::ReflectX,
                    base.reflect_x,
                ),
                (
                    "inspector-reflect-y",
                    RotationField::ReflectY,
                    base.reflect_y,
                ),
            ] {
                let mut flag = on;
                if checkbox(ui, &mut flag, &tr(&ctx, key)).changed() {
                    command = Some(Command::SetRotationBase {
                        name: name.clone(),
                        base: with_field(base, field, f32::from(u8::from(flag))),
                    });
                }
            }
        }
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    fn model() -> Model {
        let mut m = Model::new();
        m.apply(Command::AddParameter {
            name: "P".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        m
    }

    #[test]
    fn nothing_selected_and_a_selected_parameter_render_without_commands() {
        let m = model();
        for selection in [
            None,
            Some(Selection::Parameter("P".into())),
            Some(Selection::Part("gone".into())),
        ] {
            let mut out = None;
            let mut h = Harness::new_ui(|ui| {
                tsukumoe_ui::init(ui.ctx());
                out = show(ui, &m, selection.as_ref(), None);
            });
            h.run();
            drop(h);
            assert!(out.is_none());
        }
    }

    fn deformer_model() -> Model {
        let mut m = Model::new();
        for command in [
            Command::AddPart {
                name: "P".into(),
                parent: None,
            },
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "P".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            },
            Command::AddRotationDeformer {
                name: "R".into(),
                part: "P".into(),
                parent: None,
                base: RotationBase {
                    origin: [1.0, 2.0],
                    angle_degrees: 10.0,
                    scale: 1.5,
                    reflect_x: false,
                    reflect_y: false,
                },
            },
        ] {
            m.apply(command).unwrap();
        }
        m
    }

    #[test]
    fn each_rotation_field_changes_only_itself() {
        let base = RotationBase {
            origin: [1.0, 2.0],
            angle_degrees: 10.0,
            scale: 1.5,
            reflect_x: false,
            reflect_y: false,
        };
        assert_eq!(
            with_field(base, RotationField::OriginX, 5.0).origin,
            [5.0, 2.0]
        );
        assert_eq!(
            with_field(base, RotationField::OriginY, 5.0).origin,
            [1.0, 5.0]
        );
        assert_eq!(
            with_field(base, RotationField::Angle, 45.0).angle_degrees,
            45.0
        );
        assert_eq!(with_field(base, RotationField::Scale, 2.0).scale, 2.0);
        assert!(with_field(base, RotationField::ReflectX, 1.0).reflect_x);
        assert!(!with_field(base, RotationField::ReflectY, 0.0).reflect_y);
        assert_eq!(with_field(base, RotationField::Angle, 10.0), base);
    }

    #[test]
    fn a_reflect_box_sends_the_new_base_and_a_warp_shows_its_points() {
        use egui_kittest::kittest::Queryable;
        let m = deformer_model();
        let mut h = Harness::builder()
            .with_size(egui::vec2(300.0, 300.0))
            .build_ui_state(
                |ui, state: &mut (Model, Selection, Option<Command>)| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    if let Some(c) = show(ui, &state.0, Some(&state.1), None) {
                        state.2 = Some(c);
                    }
                },
                (m, Selection::Rotation("R".into()), None),
            );
        h.run();
        h.get_by_label("Reflect X").click();
        h.run();
        let Some(Command::SetRotationBase { name, base }) = h.state().2.clone() else {
            panic!("the box sends SetRotationBase");
        };
        assert_eq!(name, "R");
        assert!(base.reflect_x && base.angle_degrees == 10.0 && base.origin == [1.0, 2.0]);
        // The warp shows its size, read only.
        h.state_mut().1 = Selection::Warp("W".into());
        h.run();
        h.get_by_label("Grid (rows x columns): 1 x 1");
    }
}
