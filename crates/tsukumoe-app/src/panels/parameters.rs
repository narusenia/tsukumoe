// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The parameter list: a name filter and, per parameter, a slider and a
//! number field you can type into or drag. The values are view state, not
//! model data: moving one never creates an undo step.

use egui::{Ui, Vec2};
use tsukumoe_core::model::Model;
use tsukumoe_ui::{Variant, button, number, slider, text_input, tr};

use super::tree::matches;
use crate::view::{Selection, ViewState};

/// Names of the parameters matching the filter (name or display name).
pub fn names(model: &Model, filter: &str) -> Vec<String> {
    model
        .parameters()
        .iter()
        .filter(|(_, name, p)| {
            matches(name, filter) || matches(&p.display_name, filter) && !p.display_name.is_empty()
        })
        .map(|(_, name, _)| name.to_owned())
        .collect()
}

const LABEL_WIDTH: f32 = 120.0;

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) {
    let ctx = ui.ctx().clone();
    let m = tsukumoe_ui::Theme::get(&ctx).metrics;
    let name = tr(&ctx, "filter-name");
    ui.horizontal(|ui| {
        if button(ui, &tr(&ctx, "parameters-reset"), Variant::Secondary).clicked() {
            view.reset_params();
        }
    });
    text_input(ui, &mut view.param_filter, &name, &name);
    let shown = names(model, &view.param_filter);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for name in shown {
                let Some(id) = model.parameters().find(&name) else {
                    continue;
                };
                let Some(p) = model.parameters().get(id) else {
                    continue;
                };
                let mut value = view.param(&name, p);
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(LABEL_WIDTH, m.control),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let r = ui.add(
                                egui::Label::new(&name)
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            );
                            if r.clicked() {
                                view.selection = Some(Selection::Parameter(name.clone()));
                                view.touch();
                            }
                        },
                    );
                    let width = (ui.available_width() - 72.0).max(40.0);
                    changed |= ui
                        .allocate_ui(Vec2::new(width, m.control), |ui| {
                            slider(ui, &mut value, p.min..=p.max, &name).changed()
                        })
                        .inner;
                    let speed = f64::from((p.max - p.min).abs()) / 200.0;
                    changed |=
                        number(ui, &mut value, p.min..=p.max, speed.max(1e-4), &name).changed();
                });
                if changed {
                    view.set_param(&name, p, value);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::command::Command;

    fn model() -> Model {
        let mut m = Model::new();
        for n in ["ParamAngleX", "ParamAngleY", "ParamEyeOpen"] {
            m.apply(Command::AddParameter {
                name: n.into(),
                min: -1.0,
                max: 1.0,
                default: 0.0,
                repeat: false,
            })
            .unwrap();
        }
        m
    }

    #[test]
    fn the_filter_matches_names_case_insensitively() {
        let m = model();
        assert_eq!(names(&m, "").len(), 3);
        assert_eq!(names(&m, "angle"), ["ParamAngleX", "ParamAngleY"]);
        assert_eq!(names(&m, "EYE"), ["ParamEyeOpen"]);
        assert!(names(&m, "nope").is_empty());
    }

    #[test]
    fn changing_a_value_does_not_touch_the_model_or_history() {
        let m = model();
        let mut view = ViewState::default();
        let id = m.parameters().find("ParamAngleX").unwrap();
        let p = m.parameters().get(id).unwrap();
        view.set_param("ParamAngleX", p, 0.5);
        assert_eq!(view.param("ParamAngleX", p), 0.5);
        assert_eq!(m.history().commands().count(), 3, "only the three adds");
        assert_eq!(p.default, 0.0);
    }
}
