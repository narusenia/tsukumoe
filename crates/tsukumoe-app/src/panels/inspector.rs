// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The inspector: the selected element's properties. Parameter ranges and part
//! opacity can be edited; each finished edit is one `Command` handed back to
//! the caller (and so to the dispatcher). Everything else is read-only here;
//! editing tools come in later units.

use egui::{Id, Ui};
use tsukumoe_core::command::Command;
use tsukumoe_core::model::Model;
use tsukumoe_ui::{hint, label, number, section_header, tr};

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

pub fn show(ui: &mut Ui, model: &Model, selection: Option<&Selection>) -> Option<Command> {
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
        Selection::Warp(name) | Selection::Rotation(name) => {
            section_header(ui, name);
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
                out = show(ui, &m, selection.as_ref());
            });
            h.run();
            drop(h);
            assert!(out.is_none());
        }
    }
}
