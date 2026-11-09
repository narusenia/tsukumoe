// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The expressions panel: the expressions of the model, the items of one
//! (parameter, blend, value), and a preview that applies it to the view's
//! parameter values at a weight. The preview changes the view, never the
//! model; every edit goes out as a command.

use egui::Ui;
use tsukumoe_core::command::Command;
use tsukumoe_core::model::Model;
use tsukumoe_core::playback::{Expression, ExpressionBlend};
use tsukumoe_ui::{
    Icon, Variant, button, form_row, hint, icon_button, number, number_edit, section_header,
    slider, tr,
};

use crate::view::ViewState;

/// A value this far from the default counts as moved when items are taken
/// from the current values.
const MOVED: f32 = 1e-4;

/// What the panel remembers between frames. Not saved, not in the history.
#[derive(Clone, Debug)]
pub struct State {
    pub name: Option<String>,
    /// How much of the expression the preview applies, 0 to 1.
    pub weight: f32,
    /// The values the preview started from, while it runs.
    base: Option<Vec<(String, f32)>>,
    /// The expression as last applied, to notice an edit.
    applied: Option<(Expression, f32)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            name: None,
            weight: 1.0,
            base: None,
            applied: None,
        }
    }
}

pub enum Out {
    Command(Command, String),
}

/// The value a parameter takes under an expression of weight `weight`, from
/// the value `base` it had before.
pub fn blended(blend: ExpressionBlend, base: f32, value: f32, weight: f32) -> f32 {
    match blend {
        ExpressionBlend::Add => base + value * weight,
        ExpressionBlend::Multiply => base * (1.0 + (value - 1.0) * weight),
        ExpressionBlend::Overwrite => base * (1.0 - weight) + value * weight,
    }
}

impl State {
    pub fn previewing(&self) -> bool {
        self.base.is_some()
    }

    /// Puts the values back and ends the preview.
    pub fn stop(&mut self, model: &Model, view: &mut ViewState) {
        for (name, value) in self.base.take().unwrap_or_default() {
            if let Some(id) = model.parameters().find(&name)
                && let Some(p) = model.parameters().get(id)
            {
                view.set_param(&name, p, value);
            }
        }
        self.applied = None;
    }

    fn start(&mut self, model: &Model, view: &ViewState, expression: &Expression) {
        self.base = Some(
            expression
                .parameters
                .iter()
                .filter_map(|i| {
                    let name = model.parameters().name(i.parameter)?;
                    let p = model.parameters().get(i.parameter)?;
                    Some((name.to_owned(), view.param(name, p)))
                })
                .collect(),
        );
        self.applied = None;
    }

    /// Applies the expression to the view's values if it or the weight
    /// changed since the last time.
    fn apply(&mut self, model: &Model, view: &mut ViewState, expression: &Expression) {
        if self.applied.as_ref() == Some(&(expression.clone(), self.weight)) {
            return;
        }
        let Some(base) = self.base.as_mut() else {
            return;
        };
        for item in &expression.parameters {
            let (Some(name), Some(p)) = (
                model.parameters().name(item.parameter),
                model.parameters().get(item.parameter),
            ) else {
                continue;
            };
            // An item added during the preview starts from the current value.
            let at = match base.iter().position(|(n, _)| n == name) {
                Some(at) => at,
                None => {
                    base.push((name.to_owned(), view.param(name, p)));
                    base.len() - 1
                }
            };
            let value = blended(item.blend, base[at].1, item.value, self.weight);
            view.set_param(name, p, value);
        }
        self.applied = Some((expression.clone(), self.weight));
    }
}

/// A name for a new expression that no expression has.
pub fn unique_name(model: &Model) -> String {
    (1..)
        .map(|n| format!("Expression{n}"))
        .find(|name| model.expressions().find(name).is_none())
        .expect("endless range")
}

/// The commands that make an item of every parameter whose value in the view
/// is not the default, overwriting it with that value.
pub fn capture(model: &Model, view: &ViewState, expression: &str) -> Vec<Command> {
    model
        .parameters()
        .iter()
        .filter_map(|(_, name, p)| {
            let v = view.param(name, p);
            ((v - p.default).abs() > MOVED).then(|| Command::SetExpressionParameter {
                expression: expression.to_owned(),
                parameter: name.to_owned(),
                blend: ExpressionBlend::Overwrite,
                value: v,
            })
        })
        .collect()
}

fn blend_key(blend: ExpressionBlend) -> &'static str {
    match blend {
        ExpressionBlend::Add => "expression-blend-add",
        ExpressionBlend::Multiply => "expression-blend-multiply",
        ExpressionBlend::Overwrite => "expression-blend-overwrite",
    }
}

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Vec<Out> {
    let mut outs = Vec::new();
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let mut st = std::mem::take(&mut view.expression);
    // The expression may be gone (undo, another document).
    if st
        .name
        .as_deref()
        .is_some_and(|n| model.expressions().find(n).is_none())
    {
        st.stop(model, view);
        st.name = None;
    }
    toolbar(ui, model, view, &mut st, &mut outs);
    let current = st.name.clone().and_then(|n| {
        let id = model.expressions().find(&n)?;
        Some((n, model.expressions().get(id)?.clone()))
    });
    let Some((name, expression)) = current else {
        hint(ui, t("expressions-empty"));
        view.expression = st;
        return outs;
    };
    fades(ui, &name, &expression, &mut outs);
    items(ui, model, &name, &expression, &mut outs);
    ui.add_space(4.0);
    actions(ui, model, view, &name, &expression, &mut st, &mut outs);
    if st.previewing() {
        st.apply(model, view, &expression);
    }
    view.expression = st;
    outs
}

fn toolbar(ui: &mut Ui, model: &Model, view: &mut ViewState, st: &mut State, outs: &mut Vec<Out>) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    ui.horizontal(|ui| {
        let names: Vec<String> = model
            .expressions()
            .iter()
            .map(|(_, n, _)| n.to_owned())
            .collect();
        let shown = st.name.clone().unwrap_or_else(|| t("expressions-pick"));
        egui::ComboBox::from_id_salt("expressions-pick")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for n in &names {
                    let on = st.name.as_deref() == Some(n.as_str());
                    if ui.selectable_label(on, n).clicked() && !on {
                        st.stop(model, view);
                        st.name = Some(n.clone());
                    }
                }
            })
            .response
            .accessible_name(t("expressions-pick"));
        if icon_button(ui, Icon::Plus, &t("expressions-new"), Variant::Ghost).clicked() {
            let name = unique_name(model);
            outs.push(Out::Command(
                Command::AddExpression {
                    name: name.clone(),
                    fade_in: 0.5,
                    fade_out: 0.5,
                },
                format!("expression_add: {name}"),
            ));
            st.stop(model, view);
            st.name = Some(name);
        }
        if let Some(name) = st.name.clone()
            && icon_button(ui, Icon::Trash, &t("expressions-delete"), Variant::Ghost).clicked()
        {
            st.stop(model, view);
            outs.push(Out::Command(
                Command::RemoveExpression { name: name.clone() },
                format!("expression_remove: {name}"),
            ));
            st.name = None;
        }
    });
}

fn fades(ui: &mut Ui, name: &str, expression: &Expression, outs: &mut Vec<Out>) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    {
        let mut next = (expression.fade_in, expression.fade_out);
        let a = form_row(ui, &t("expressions-fade-in"), |ui| {
            number_edit(ui, next.0, 0.0..=60.0, 0.05, &t("expressions-fade-in"))
        });
        let b = form_row(ui, &t("expressions-fade-out"), |ui| {
            number_edit(ui, next.1, 0.0..=60.0, 0.05, &t("expressions-fade-out"))
        });
        next = (a.unwrap_or(next.0), b.unwrap_or(next.1));
        if a.is_some() || b.is_some() {
            outs.push(Out::Command(
                Command::SetExpressionFade {
                    name: name.to_owned(),
                    fade_in: next.0,
                    fade_out: next.1,
                },
                format!("expression_fade: {name}"),
            ));
        }
    }
}

fn items(ui: &mut Ui, model: &Model, name: &str, expression: &Expression, outs: &mut Vec<Out>) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    section_header(ui, &t("expressions-items"));
    for (i, item) in expression.parameters.iter().enumerate() {
        let Some(param) = model.parameters().name(item.parameter) else {
            continue;
        };
        ui.push_id(i, |ui| {
            ui.horizontal(|ui| {
                ui.label(param);
                let mut blend = item.blend;
                egui::ComboBox::from_id_salt("blend")
                    .selected_text(t(blend_key(blend)))
                    .show_ui(ui, |ui| {
                        for b in [
                            ExpressionBlend::Add,
                            ExpressionBlend::Multiply,
                            ExpressionBlend::Overwrite,
                        ] {
                            ui.selectable_value(&mut blend, b, t(blend_key(b)));
                        }
                    })
                    .response
                    .accessible_name(t("expressions-blend"));
                let typed = number_edit(
                    ui,
                    item.value,
                    -1.0e6..=1.0e6,
                    0.01,
                    &t("expressions-value"),
                );
                let value = typed.unwrap_or(item.value);
                if blend != item.blend || typed.is_some() {
                    outs.push(Out::Command(
                        Command::SetExpressionParameter {
                            expression: name.to_owned(),
                            parameter: param.to_owned(),
                            blend,
                            value,
                        },
                        format!("expression_item: {param}"),
                    ));
                }
                if icon_button(
                    ui,
                    Icon::Trash,
                    &t("expressions-item-delete"),
                    Variant::Ghost,
                )
                .clicked()
                {
                    outs.push(Out::Command(
                        Command::RemoveExpressionParameter {
                            expression: name.to_owned(),
                            parameter: param.to_owned(),
                        },
                        format!("expression_item_remove: {param}"),
                    ));
                }
            });
        });
    }
}

fn actions(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    name: &str,
    expression: &Expression,
    st: &mut State,
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    ui.horizontal_wrapped(|ui| {
        let focus = view
            .param_focus
            .clone()
            .filter(|f| model.parameters().find(f).is_some());
        let has = |f: &str| {
            model
                .parameters()
                .find(f)
                .is_some_and(|id| expression.parameters.iter().any(|p| p.parameter == id))
        };
        ui.add_enabled_ui(focus.as_deref().is_some_and(|f| !has(f)), |ui| {
            if button(ui, &t("expressions-add-item"), Variant::Secondary).clicked()
                && let Some(f) = &focus
            {
                outs.push(Out::Command(
                    Command::SetExpressionParameter {
                        expression: name.to_owned(),
                        parameter: f.clone(),
                        blend: ExpressionBlend::Add,
                        value: 0.0,
                    },
                    format!("expression_item: {f}"),
                ));
            }
        });
        if button(ui, &t("expressions-capture"), Variant::Secondary).clicked() {
            let commands = capture(model, view, name);
            if !commands.is_empty() {
                outs.push(Out::Command(
                    Command::Batch { commands },
                    format!("expression_capture: {name}"),
                ));
            }
        }
    });
    ui.horizontal(|ui| {
        if st.previewing() {
            if button(ui, &t("expressions-preview-stop"), Variant::Secondary).clicked() {
                st.stop(model, view);
            }
        } else if button(ui, &t("expressions-preview"), Variant::Primary).clicked() {
            st.start(model, view, expression);
        }
        hint(ui, t("expressions-weight"));
        ui.allocate_ui(
            egui::vec2((ui.available_width() - 70.0).max(40.0), 24.0),
            |ui| {
                slider(ui, &mut st.weight, 0.0..=1.0, &t("expressions-weight"));
            },
        );
        number(
            ui,
            &mut st.weight,
            0.0..=1.0,
            0.01,
            &t("expressions-weight"),
        );
    });
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
                name: "Smile".into(),
                min: 0.0,
                max: 1.0,
                default: 0.0,
                repeat: false,
            },
            Command::AddExpression {
                name: "E".into(),
                fade_in: 0.5,
                fade_out: 0.5,
            },
            Command::SetExpressionParameter {
                expression: "E".into(),
                parameter: "Smile".into(),
                blend: ExpressionBlend::Add,
                value: 0.8,
            },
        ] {
            m.apply(c).unwrap();
        }
        m
    }

    #[test]
    fn the_three_blends_follow_the_weight() {
        // Weight 0 leaves the value; weight 1 gives the full effect.
        for b in [
            ExpressionBlend::Add,
            ExpressionBlend::Multiply,
            ExpressionBlend::Overwrite,
        ] {
            assert_eq!(blended(b, 0.4, 0.9, 0.0), 0.4, "{b:?}");
        }
        assert!((blended(ExpressionBlend::Add, 0.4, 0.5, 1.0) - 0.9).abs() < 1e-6);
        assert!((blended(ExpressionBlend::Multiply, 0.4, 2.0, 1.0) - 0.8).abs() < 1e-6);
        assert_eq!(blended(ExpressionBlend::Overwrite, 0.4, 0.9, 1.0), 0.9);
        assert!((blended(ExpressionBlend::Overwrite, 0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn capture_takes_the_parameters_that_moved() {
        let m = model();
        let mut view = ViewState::default();
        assert!(capture(&m, &view, "E").is_empty());
        let id = m.parameters().find("Smile").unwrap();
        view.set_param("Smile", m.parameters().get(id).unwrap(), 0.6);
        let commands = capture(&m, &view, "E");
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            Command::SetExpressionParameter { value, blend: ExpressionBlend::Overwrite, .. }
                if (*value - 0.6).abs() < 1e-6
        ));
    }

    struct Panel {
        model: Model,
        view: ViewState,
        outs: Vec<Out>,
    }

    fn harness() -> Harness<'static, Panel> {
        let mut view = ViewState::default();
        view.expression.name = Some("E".into());
        Harness::builder()
            .with_size(egui::vec2(520.0, 320.0))
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

    fn smile(h: &Harness<'_, Panel>) -> f32 {
        let id = h.state().model.parameters().find("Smile").unwrap();
        h.state()
            .view
            .param("Smile", h.state().model.parameters().get(id).unwrap())
    }

    #[test]
    fn preview_applies_the_expression_and_stop_restores() {
        let mut h = harness();
        h.run();
        h.get_by_label("Preview").click();
        h.run_steps(3);
        assert!((smile(&h) - 0.8).abs() < 1e-5, "{}", smile(&h));
        h.get_by_label("Stop preview").click();
        h.run_steps(3);
        assert_eq!(smile(&h), 0.0);
    }

    #[test]
    fn the_new_button_makes_an_expression() {
        let mut h = harness();
        h.state_mut().view.expression.name = None;
        h.run();
        h.get_by_label("Add expression").click();
        h.run();
        let Some(Out::Command(Command::AddExpression { name, .. }, _)) = h.state().outs.last()
        else {
            panic!("an AddExpression");
        };
        assert_eq!(name, "Expression1");
    }
}
