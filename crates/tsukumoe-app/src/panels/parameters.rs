// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The parameter list: a name filter and, per parameter, a slider and a
//! number field you can type into or drag. The values are view state, not
//! model data: moving one never creates an undo step.

use egui::{Ui, Vec2};
use tsukumoe_core::command::Command;
use tsukumoe_core::elements::Parameter;
use tsukumoe_core::model::Model;
use tsukumoe_ui::{Icon, Variant, button, hint, icon_button, number, slider, text_input, tr};

use crate::mesh_tool::Op;
use crate::param_ops;
use crate::tree_model::matches;
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
/// A slider snaps to a key within this many points.
const SNAP_POINTS: f32 = 6.0;

/// What the panel asks the editor to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// A command and its history name.
    Op(Op),
    /// A parameter rename; the editor moves the view state along with it.
    Rename { from: String, to: String },
    /// Something that cannot be done, as a translation key for a toast.
    Error(&'static str),
}

/// Draws a tick at each key over the slider `rect`.
fn paint_keys(ui: &Ui, rect: egui::Rect, keys: &[f32], min: f32, max: f32, value: f32) {
    let theme = tsukumoe_ui::Theme::get(ui.ctx());
    let r = rect.height() / 2.5;
    let span = (max - min).max(1e-6);
    for &k in keys {
        let t = ((k - min) / span).clamp(0.0, 1.0);
        let x = rect.left() + r + t * (rect.width() - 2.0 * r);
        let on = (k - value).abs() <= span * 1e-4;
        let color = if on {
            theme.colors.warning
        } else {
            theme.colors.text_muted
        };
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.bottom() - 1.0),
                egui::pos2(x, rect.bottom() + 3.0),
            ],
            egui::Stroke::new(if on { 2.0 } else { 1.0 }, color),
        );
    }
}

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Vec<Out> {
    let ctx = ui.ctx().clone();
    let m = tsukumoe_ui::Theme::get(&ctx).metrics;
    let mut outs = Vec::new();
    let selection = view.selected();
    let name = tr(&ctx, "filter-name");
    ui.horizontal(|ui| {
        if button(ui, &tr(&ctx, "parameters-reset"), Variant::Secondary).clicked() {
            view.reset_params();
        }
        if icon_button(ui, Icon::Plus, &tr(&ctx, "param-new"), Variant::Ghost).clicked() {
            outs.push(Out::Op(param_ops::create_parameter(model)));
        }
    });
    text_input(ui, &mut view.param_filter, &name, &name);
    if let Some(focus) = view.param_focus.clone()
        && let Some(id) = model.parameters().find(&focus)
        && let Some(p) = model.parameters().get(id)
    {
        edit_section(ui, model, view, &focus, p, &selection, &mut outs);
    }
    let shown = names(model, &view.param_filter);
    let shift = ui.input(|i| i.modifiers.shift);
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
                let keys = param_ops::primary(model, &name, &selection).map(|(_, k)| k);
                let mut value = view.param(&name, p);
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(LABEL_WIDTH, m.control),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let focused = view.param_focus.as_deref() == Some(&name);
                            let text = if focused {
                                egui::RichText::new(&name).strong()
                            } else {
                                egui::RichText::new(&name)
                            };
                            let r = ui.add(
                                egui::Label::new(text)
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            );
                            if r.clicked() {
                                view.param_focus = Some(name.clone());
                            }
                        },
                    );
                    let width = (ui.available_width() - 72.0).max(40.0);
                    let slider_rect = ui
                        .allocate_ui(Vec2::new(width, m.control), |ui| {
                            let r = slider(ui, &mut value, p.min..=p.max, &name);
                            changed |= r.changed();
                            r.rect
                        })
                        .inner;
                    if let Some(keys) = &keys {
                        if changed && !shift {
                            let threshold =
                                (p.max - p.min) * SNAP_POINTS / slider_rect.width().max(1.0);
                            value = param_ops::snap(keys, value, threshold);
                        }
                        paint_keys(ui, slider_rect, keys, p.min, p.max, value);
                    }
                    let speed = f64::from((p.max - p.min).abs()) / 200.0;
                    changed |=
                        number(ui, &mut value, p.min..=p.max, speed.max(1e-4), &name).changed();
                });
                if changed {
                    view.set_param(&name, p, value);
                }
            }
        });
    outs
}

/// The section for the focused parameter: its name and range, and the keys of
/// the selected element.
fn edit_section(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    name: &str,
    p: &Parameter,
    selection: &[Selection],
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    ui.separator();
    // The name: edited in a buffer, sent when the field is left.
    let buffer_id = ui.make_persistent_id(("param-name", name));
    let mut text: String = ui
        .data_mut(|d| d.get_temp(buffer_id))
        .unwrap_or_else(|| name.to_owned());
    ui.horizontal(|ui| {
        // The delete button first: the name field takes the width that is left.
        if icon_button(ui, Icon::Trash, &t("param-delete"), Variant::Danger).clicked() {
            match param_ops::delete_parameter(model, name) {
                Some(op) => outs.push(Out::Op(op)),
                None => outs.push(Out::Error("param-err-not-bound")),
            }
        }
        let response = text_input(ui, &mut text, &t("param-name"), &t("param-name"));
        if response.lost_focus() {
            if text != name && !text.is_empty() {
                outs.push(Out::Rename {
                    from: name.to_owned(),
                    to: text.clone(),
                });
            }
            text = name.to_owned();
        }
    });
    ui.data_mut(|d| d.insert_temp(buffer_id, text));
    // The range and the default: edited in a buffer, sent when a drag or an entry ends.
    let range_id = ui.make_persistent_id(("param-range", name));
    let mut vals: [f32; 3] = ui
        .data_mut(|d| d.get_temp(range_id))
        .unwrap_or([p.min, p.default, p.max]);
    let mut done = false;
    let mut touched = false;
    let mut repeat = p.repeat;
    ui.horizontal(|ui| {
        for (i, key) in ["param-min", "param-default", "param-max"]
            .into_iter()
            .enumerate()
        {
            ui.label(t(key));
            let r = ui
                .add(
                    egui::DragValue::new(&mut vals[i])
                        .speed(0.01)
                        .max_decimals(3),
                )
                .accessible_name(t(key));
            touched |= r.changed();
            done |= r.drag_stopped() || r.lost_focus();
        }
        if ui.checkbox(&mut repeat, t("param-repeat")).changed() {
            outs.push(Out::Op((
                Command::SetParameterRepeat {
                    name: name.to_owned(),
                    repeat,
                },
                format!("param_repeat: {name}"),
            )));
        }
    });
    if done && vals != [p.min, p.default, p.max] {
        outs.push(Out::Op((
            Command::SetParameterRange {
                name: name.to_owned(),
                min: vals[0],
                max: vals[2],
                default: vals[1],
            },
            format!("param_range: {name}"),
        )));
    }
    ui.data_mut(|d| {
        if done || !touched && vals == [p.min, p.default, p.max] {
            d.remove::<[f32; 3]>(range_id);
        } else {
            d.insert_temp(range_id, vals);
        }
    });
    // The keys of the selected element.
    let value = view.param(name, p);
    let keys = param_ops::primary(model, name, selection).map(|(_, k)| k);
    let range = p.max - p.min;
    ui.horizontal(|ui| {
        let step = |dir: i32, view: &mut ViewState| {
            if let Some(k) = keys
                .as_deref()
                .and_then(|k| param_ops::neighbour(k, value, range, dir))
            {
                view.set_param(name, p, k);
            }
        };
        if icon_button(ui, Icon::ChevronLeft, &t("param-key-prev"), Variant::Ghost).clicked() {
            step(-1, view);
        }
        if icon_button(ui, Icon::ChevronRight, &t("param-key-next"), Variant::Ghost).clicked() {
            step(1, view);
        }
        if icon_button(ui, Icon::Plus, &t("param-key-add"), Variant::Ghost).clicked() {
            outs.push(match param_ops::add_key(model, name, value, selection) {
                Ok(op) => Out::Op(op),
                Err(e) => Out::Error(e.key()),
            });
        }
        if icon_button(ui, Icon::Minus, &t("param-key-remove"), Variant::Ghost).clicked() {
            outs.push(match param_ops::remove_key(model, name, value, selection) {
                Ok(op) => Out::Op(op),
                Err(e) => Out::Error(e.key()),
            });
        }
        if button(ui, &t("param-bind"), Variant::Secondary).clicked() {
            outs.push(match param_ops::bind(model, name, selection) {
                Ok(op) => Out::Op(op),
                Err(e) => Out::Error(e.key()),
            });
        }
        if keys.is_some() && button(ui, &t("param-unbind"), Variant::Secondary).clicked() {
            outs.push(match param_ops::unbind(model, name, selection) {
                Ok(op) => Out::Op(op),
                Err(e) => Out::Error(e.key()),
            });
        }
    });
    if let Some(keys) = &keys {
        let on = param_ops::on_key(keys, value, range).is_some();
        let text = keys
            .iter()
            .map(|k| format!("{k:.2}"))
            .collect::<Vec<_>>()
            .join("  ");
        hint(ui, format!("{}{}", if on { "\u{25c6} " } else { "" }, text));
    }
    ui.separator();
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
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

    struct Panel {
        model: Model,
        view: ViewState,
        outs: Vec<Out>,
    }

    fn panel() -> Panel {
        let mut model = model();
        model
            .apply(Command::AddPart {
                name: "Part".into(),
                parent: None,
            })
            .unwrap();
        let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        model
            .apply(Command::AddMesh {
                name: "Skin".into(),
                part: "Part".into(),
                positions: pts.clone(),
                uvs: pts,
                indices: vec![0, 1, 2],
            })
            .unwrap();
        let mut view = ViewState::default();
        view.select_only(Selection::Mesh("Skin".into()));
        view.param_focus = Some("ParamAngleX".into());
        Panel {
            model,
            view,
            outs: Vec::new(),
        }
    }

    fn harness(p: Panel) -> Harness<'static, Panel> {
        Harness::builder()
            .with_size(egui::vec2(420.0, 420.0))
            .build_ui_state(
                |ui, p: &mut Panel| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    let outs = show(ui, &p.model, &mut p.view);
                    p.outs.extend(outs);
                },
                p,
            )
    }

    #[test]
    fn the_buttons_send_bind_keys_and_a_new_parameter_through_the_param_ops() {
        let mut h = harness(panel());
        h.run();
        // Unbound: bind it.
        h.get_by_label("Bind").click();
        h.run();
        let Out::Op((command, label)) = h.state().outs.last().unwrap().clone() else {
            panic!("an op");
        };
        assert_eq!(label, "param_bind: ParamAngleX");
        h.state_mut().model.apply(command).unwrap();
        h.state_mut().outs.clear();
        h.run();
        // Bound: add a key at the slider (the default 0 is a key: refused with a message).
        h.get_by_label("Add a key here").click();
        h.run();
        assert_eq!(
            h.state().outs.last(),
            Some(&Out::Error("param-err-key-exists"))
        );
        // Move off the keys and add.
        let p = h
            .state()
            .model
            .parameters()
            .get(h.state().model.parameters().find("ParamAngleX").unwrap())
            .unwrap()
            .clone();
        h.state_mut().view.set_param("ParamAngleX", &p, 0.5);
        h.run();
        h.get_by_label("Add a key here").click();
        h.run();
        assert!(matches!(
            h.state().outs.last(),
            Some(Out::Op((Command::AddParameterKey { value, .. }, _))) if (*value - 0.5).abs() < 1e-6
        ));
        // A new parameter.
        h.get_by_label("New parameter").click();
        h.run();
        assert!(matches!(
            h.state().outs.last(),
            Some(Out::Op((Command::AddParameter { .. }, _)))
        ));
    }

    #[test]
    fn previous_and_next_key_move_the_slider_to_the_neighbouring_key() {
        let mut h = harness(panel());
        h.run();
        h.get_by_label("Bind").click();
        h.run();
        let Out::Op((command, _)) = h.state().outs.last().unwrap().clone() else {
            panic!("an op");
        };
        h.state_mut().model.apply(command).unwrap();
        h.run();
        let value = |h: &Harness<Panel>| {
            let m = &h.state().model;
            let p = m
                .parameters()
                .get(m.parameters().find("ParamAngleX").unwrap())
                .unwrap();
            h.state().view.param("ParamAngleX", p)
        };
        h.get_by_label("Next key").click();
        h.run();
        assert_eq!(value(&h), 1.0);
        h.get_by_label("Previous key").click();
        h.run();
        assert_eq!(value(&h), 0.0);
    }
}
