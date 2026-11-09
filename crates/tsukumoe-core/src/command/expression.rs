// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The commands that make and change expressions (M5-08): a named set of
//! parameter items, each adding to, multiplying or overwriting the parameter.

use crate::elements::Parameter;
use crate::model::Model;
use crate::playback::{Expression, ExpressionParameter};

use super::keyform::refused;
use super::{Change, Command, CommandError};

fn fade_ok(v: f32) -> bool {
    v.is_finite() && v >= 0.0
}

impl Model {
    pub(super) fn run_expression(
        &mut self,
        command: &Command,
        changes: &mut Vec<Change>,
    ) -> Result<(), CommandError> {
        match command {
            Command::AddExpression {
                name,
                fade_in,
                fade_out,
            } => {
                if !fade_ok(*fade_in) || !fade_ok(*fade_out) {
                    return Err(refused(name, "bad-range"));
                }
                let expression = Expression {
                    fade_in: *fade_in,
                    fade_out: *fade_out,
                    parameters: Vec::new(),
                };
                self.insert(changes, name, expression)?;
            }
            Command::RemoveExpression { name } => {
                let id = self.resolve::<Expression>(name)?;
                self.remove(changes, id);
            }
            Command::RenameExpression { name, new_name } => {
                let id = self.resolve::<Expression>(name)?;
                self.rename(changes, id, new_name)?;
            }
            Command::SetExpressionFade {
                name,
                fade_in,
                fade_out,
            } => {
                let id = self.resolve::<Expression>(name)?;
                if !fade_ok(*fade_in) || !fade_ok(*fade_out) {
                    return Err(refused(name, "bad-range"));
                }
                self.modify(changes, id, |e| {
                    e.fade_in = *fade_in;
                    e.fade_out = *fade_out;
                });
            }
            Command::SetExpressionParameter {
                expression,
                parameter,
                blend,
                value,
            } => {
                let id = self.resolve::<Expression>(expression)?;
                let target = self.resolve::<Parameter>(parameter)?;
                if !value.is_finite() {
                    return Err(refused(expression, "bad-range"));
                }
                let item = ExpressionParameter {
                    parameter: target,
                    blend: *blend,
                    value: *value,
                };
                self.modify(changes, id, |e| {
                    match e.parameters.iter_mut().find(|p| p.parameter == target) {
                        Some(p) => *p = item,
                        None => e.parameters.push(item),
                    }
                });
            }
            Command::RemoveExpressionParameter {
                expression,
                parameter,
            } => {
                let id = self.resolve::<Expression>(expression)?;
                let target = self.resolve::<Parameter>(parameter)?;
                let has = self
                    .expressions
                    .get(id)
                    .is_some_and(|e| e.parameters.iter().any(|p| p.parameter == target));
                if !has {
                    return Err(refused(expression, "no-such-item"));
                }
                self.modify(changes, id, |e| {
                    e.parameters.retain(|p| p.parameter != target)
                });
            }
            _ => unreachable!("run_expression gets expression commands only"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::ExpressionBlend;

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
        m.apply(Command::AddExpression {
            name: "E".into(),
            fade_in: 0.5,
            fade_out: 0.5,
        })
        .unwrap();
        m
    }

    fn item(m: &Model) -> Vec<ExpressionParameter> {
        let id = m.expressions().find("E").unwrap();
        m.expressions().get(id).unwrap().parameters.clone()
    }

    fn set(blend: ExpressionBlend, value: f32) -> Command {
        Command::SetExpressionParameter {
            expression: "E".into(),
            parameter: "P".into(),
            blend,
            value,
        }
    }

    #[test]
    fn setting_an_item_twice_replaces_it_and_removing_it_clears_it() {
        let mut m = model();
        m.apply(set(ExpressionBlend::Add, 0.5)).unwrap();
        m.apply(set(ExpressionBlend::Overwrite, 1.0)).unwrap();
        let items = item(&m);
        assert_eq!(items.len(), 1);
        assert_eq!(
            (items[0].blend, items[0].value),
            (ExpressionBlend::Overwrite, 1.0)
        );
        m.apply(Command::RemoveExpressionParameter {
            expression: "E".into(),
            parameter: "P".into(),
        })
        .unwrap();
        assert!(item(&m).is_empty());
        assert!(
            m.apply(Command::RemoveExpressionParameter {
                expression: "E".into(),
                parameter: "P".into(),
            })
            .is_err()
        );
    }

    #[test]
    fn bad_fades_and_values_are_refused() {
        let mut m = model();
        assert!(
            m.apply(Command::SetExpressionFade {
                name: "E".into(),
                fade_in: -1.0,
                fade_out: 0.0,
            })
            .is_err()
        );
        assert!(m.apply(set(ExpressionBlend::Add, f32::NAN)).is_err());
    }

    #[test]
    fn a_parameter_an_expression_names_cannot_be_removed() {
        let mut m = model();
        m.apply(set(ExpressionBlend::Add, 0.5)).unwrap();
        assert!(
            m.apply(Command::RemoveParameter { name: "P".into() })
                .is_err()
        );
    }
}
