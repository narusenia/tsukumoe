// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Parameter editing as commands: creating and deleting parameters, binding the
//! selected elements to one, and adding or removing keys. Pure functions of the
//! model and the selection; the parameter panel only shows what they find and
//! the editor sends what they make.
//!
//! Deleting a parameter takes every element off it first (the shape at the
//! default stays), in one `Batch` with the removal, so one undo brings all of
//! it back. A parameter something else still names (a driver, physics, a
//! motion, a glue or a bone binding) makes the removal fail, and nothing changes.

use tsukumoe_core::command::{Command, ElementRef};
use tsukumoe_core::elements::ParameterId;
use tsukumoe_core::keyform::Binding;
use tsukumoe_core::model::Model;

use crate::mesh_tool::Op;
use crate::view::Selection;

/// A value counts as on a key when it is this close, in fractions of the range.
const KEY_TOLERANCE: f32 = 1e-4;

/// Why an operation does not apply; `key` is its translation key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamError {
    /// No mesh or deformer is selected.
    NoElement,
    /// Every selected element already has keyforms.
    AlreadyBound,
    /// The selected element is not bound to the parameter.
    NotBound,
    /// The slider is not on a key.
    NotOnKey,
    /// There is a key here already.
    KeyExists,
    /// The last key of an axis stays.
    LastKey,
}

impl ParamError {
    pub fn key(self) -> &'static str {
        match self {
            Self::NoElement => "param-err-no-element",
            Self::AlreadyBound => "param-err-already-bound",
            Self::NotBound => "param-err-not-bound",
            Self::NotOnKey => "param-err-not-on-key",
            Self::KeyExists => "param-err-key-exists",
            Self::LastKey => "param-err-last-key",
        }
    }
}

/// The meshes and deformers in `selection`, as command targets.
pub fn elements(selection: &[Selection]) -> Vec<ElementRef> {
    selection
        .iter()
        .filter_map(|s| match s {
            Selection::Mesh(n) => Some(ElementRef::Mesh { name: n.clone() }),
            Selection::Warp(n) | Selection::Rotation(n) => {
                Some(ElementRef::Deformer { name: n.clone() })
            }
            _ => None,
        })
        .collect()
}

/// The binding `element` reacts through, if it has keyforms.
pub fn binding_of<'a>(model: &'a Model, element: &ElementRef) -> Option<&'a Binding> {
    let id = match element {
        ElementRef::Mesh { name } => {
            model
                .meshes()
                .get(model.meshes().find(name)?)?
                .keyforms
                .as_ref()?
                .binding
        }
        ElementRef::Deformer { name } => {
            if let Some(w) = model.warps().find(name) {
                model.warps().get(w)?.keyforms.as_ref()?.binding
            } else {
                let r = model.rotations().find(name)?;
                model.rotations().get(r)?.keyforms.as_ref()?.binding
            }
        }
    };
    model.bindings().get(id)
}

/// The keys of the axis of `parameter` in `element`'s binding.
pub fn keys_of(model: &Model, parameter: &str, element: &ElementRef) -> Option<Vec<f32>> {
    let pid = model.parameters().find(parameter)?;
    binding_of(model, element)?
        .axes()
        .iter()
        .find(|a| a.parameter == pid)
        .map(|a| a.keys.clone())
}

/// The element the keys shown and edited belong to: the last selected one
/// that reacts to `parameter`, with its keys.
pub fn primary(
    model: &Model,
    parameter: &str,
    selection: &[Selection],
) -> Option<(ElementRef, Vec<f32>)> {
    elements(selection)
        .into_iter()
        .rev()
        .find_map(|e| keys_of(model, parameter, &e).map(|k| (e, k)))
}

/// The key `value` is on (within a hair of the range), if any.
pub fn on_key(keys: &[f32], value: f32, range: f32) -> Option<f32> {
    let tolerance = range.abs().max(1e-6) * KEY_TOLERANCE;
    keys.iter()
        .copied()
        .find(|k| (k - value).abs() <= tolerance)
}

/// `value` pulled onto the nearest key when it is within `threshold`.
pub fn snap(keys: &[f32], value: f32, threshold: f32) -> f32 {
    keys.iter()
        .copied()
        .filter(|k| (k - value).abs() <= threshold)
        .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
        .unwrap_or(value)
}

/// The key before (`dir` < 0) or after (`dir` > 0) `value`.
pub fn neighbour(keys: &[f32], value: f32, range: f32, dir: i32) -> Option<f32> {
    let tolerance = range.abs().max(1e-6) * KEY_TOLERANCE;
    if dir < 0 {
        keys.iter().rev().copied().find(|k| *k < value - tolerance)
    } else {
        keys.iter().copied().find(|k| *k > value + tolerance)
    }
}

/// Binds the selected elements that have no keyforms to `parameter`.
pub fn bind(model: &Model, parameter: &str, selection: &[Selection]) -> Result<Op, ParamError> {
    let all = elements(selection);
    if all.is_empty() {
        return Err(ParamError::NoElement);
    }
    let free: Vec<ElementRef> = all
        .into_iter()
        .filter(|e| binding_of(model, e).is_none())
        .collect();
    if free.is_empty() {
        return Err(ParamError::AlreadyBound);
    }
    let label = format!("param_bind: {parameter}");
    Ok((
        Command::BindParameter {
            parameter: parameter.to_owned(),
            elements: free,
        },
        label,
    ))
}

/// Takes the selected elements off `parameter`.
pub fn unbind(model: &Model, parameter: &str, selection: &[Selection]) -> Result<Op, ParamError> {
    let bound: Vec<ElementRef> = elements(selection)
        .into_iter()
        .filter(|e| keys_of(model, parameter, e).is_some())
        .collect();
    if bound.is_empty() {
        return Err(ParamError::NotBound);
    }
    Ok((
        Command::UnbindParameter {
            parameter: parameter.to_owned(),
            elements: bound,
        },
        format!("param_unbind: {parameter}"),
    ))
}

/// A key at `value` for the selected element's binding of `parameter`.
pub fn add_key(
    model: &Model,
    parameter: &str,
    value: f32,
    selection: &[Selection],
) -> Result<Op, ParamError> {
    let range = range_of(model, parameter);
    let (element, keys) = primary(model, parameter, selection).ok_or(ParamError::NotBound)?;
    if on_key(&keys, value, range).is_some() {
        return Err(ParamError::KeyExists);
    }
    Ok((
        Command::AddParameterKey {
            parameter: parameter.to_owned(),
            value,
            element,
        },
        format!("param_add_key: {parameter} = {value:.3}"),
    ))
}

/// Removes the key the slider is on.
pub fn remove_key(
    model: &Model,
    parameter: &str,
    value: f32,
    selection: &[Selection],
) -> Result<Op, ParamError> {
    let range = range_of(model, parameter);
    let (element, keys) = primary(model, parameter, selection).ok_or(ParamError::NotBound)?;
    let key = on_key(&keys, value, range).ok_or(ParamError::NotOnKey)?;
    if keys.len() <= 1 {
        return Err(ParamError::LastKey);
    }
    Ok((
        Command::RemoveParameterKey {
            parameter: parameter.to_owned(),
            value: key,
            element,
        },
        format!("param_remove_key: {parameter} = {key:.3}"),
    ))
}

fn range_of(model: &Model, parameter: &str) -> f32 {
    model
        .parameters()
        .find(parameter)
        .and_then(|id| model.parameters().get(id))
        .map_or(1.0, |p| p.max - p.min)
}

/// Every mesh and deformer that reacts to `parameter`.
pub fn bound_elements(model: &Model, parameter: ParameterId) -> Vec<ElementRef> {
    let reacts = |b: Option<tsukumoe_core::keyform::BindingId>| {
        b.and_then(|id| model.bindings().get(id))
            .is_some_and(|b| b.axes().iter().any(|a| a.parameter == parameter))
    };
    let mut out = Vec::new();
    for (_, name, m) in model.meshes().iter() {
        if reacts(m.keyforms.as_ref().map(|g| g.binding)) {
            out.push(ElementRef::Mesh {
                name: name.to_owned(),
            });
        }
    }
    for (_, name, w) in model.warps().iter() {
        if reacts(w.keyforms.as_ref().map(|g| g.binding)) {
            out.push(ElementRef::Deformer {
                name: name.to_owned(),
            });
        }
    }
    for (_, name, r) in model.rotations().iter() {
        if reacts(r.keyforms.as_ref().map(|g| g.binding)) {
            out.push(ElementRef::Deformer {
                name: name.to_owned(),
            });
        }
    }
    out
}

/// Removes the parameter after taking everything off it, as one step.
pub fn delete_parameter(model: &Model, name: &str) -> Option<Op> {
    let id = model.parameters().find(name)?;
    let mut commands = Vec::new();
    let bound = bound_elements(model, id);
    if !bound.is_empty() {
        commands.push(Command::UnbindParameter {
            parameter: name.to_owned(),
            elements: bound,
        });
    }
    commands.push(Command::RemoveParameter {
        name: name.to_owned(),
    });
    Some((Command::Batch { commands }, format!("param_delete: {name}")))
}

/// A new parameter named `Param`, `Param2`, ... from -1 to 1, default 0.
pub fn create_parameter(model: &Model) -> Op {
    let name = (1..)
        .map(|i| {
            if i == 1 {
                "Param".to_owned()
            } else {
                format!("Param{i}")
            }
        })
        .find(|n| model.parameters().find(n).is_none())
        .expect("endless range");
    (
        Command::AddParameter {
            name: name.clone(),
            min: -1.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        },
        format!("param_create: {name}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        let mut m = Model::new();
        for c in [
            Command::AddParameter {
                name: "P".into(),
                min: -1.0,
                max: 1.0,
                default: 0.0,
                repeat: false,
            },
            Command::AddPart {
                name: "Part".into(),
                parent: None,
            },
        ] {
            m.apply(c).unwrap();
        }
        for n in ["A", "B"] {
            let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
            m.apply(Command::AddMesh {
                name: n.into(),
                part: "Part".into(),
                positions: pts.clone(),
                uvs: pts,
                indices: vec![0, 1, 2],
            })
            .unwrap();
        }
        m
    }

    fn sel(names: &[&str]) -> Vec<Selection> {
        names.iter().map(|n| Selection::Mesh((*n).into())).collect()
    }

    fn apply(m: &mut Model, op: Result<Op, ParamError>) {
        m.apply(op.unwrap().0).unwrap();
    }

    #[test]
    fn bind_takes_the_unbound_selected_elements_and_says_why_it_cannot() {
        let mut m = model();
        assert_eq!(bind(&m, "P", &[]), Err(ParamError::NoElement));
        assert_eq!(
            bind(&m, "P", &[Selection::Part("Part".into())]),
            Err(ParamError::NoElement)
        );
        let op = bind(&m, "P", &sel(&["A", "B"]));
        apply(&mut m, op);
        assert_eq!(
            bind(&m, "P", &sel(&["A", "B"])),
            Err(ParamError::AlreadyBound)
        );
        // Only the unbound one of a mixed selection is sent.
        let mut m = model();
        let op = bind(&m, "P", &sel(&["A"]));
        apply(&mut m, op);
        let (command, _) = bind(&m, "P", &sel(&["A", "B"])).unwrap();
        let Command::BindParameter { elements, .. } = command else {
            panic!("a bind");
        };
        assert_eq!(elements, [ElementRef::Mesh { name: "B".into() }]);
    }

    #[test]
    fn keys_are_added_on_the_selected_binding_and_removed_where_the_slider_is() {
        let mut m = model();
        let op = bind(&m, "P", &sel(&["A", "B"]));
        apply(&mut m, op);
        assert_eq!(add_key(&m, "P", 0.5, &sel(&[])), Err(ParamError::NotBound));
        assert_eq!(
            add_key(&m, "P", 0.0, &sel(&["A"])),
            Err(ParamError::KeyExists)
        );
        let op = add_key(&m, "P", 0.5, &sel(&["A"]));
        apply(&mut m, op);
        let (_, keys) = primary(&m, "P", &sel(&["B"])).unwrap();
        assert_eq!(keys, [-1.0, 0.0, 0.5, 1.0], "B got it too (shared)");
        assert_eq!(
            remove_key(&m, "P", 0.25, &sel(&["A"])),
            Err(ParamError::NotOnKey)
        );
        let op = remove_key(&m, "P", 0.50001, &sel(&["A"]));
        apply(&mut m, op);
        assert_eq!(primary(&m, "P", &sel(&["A"])).unwrap().1, [-1.0, 0.0, 1.0]);
    }

    #[test]
    fn the_last_key_stays() {
        let mut m = model();
        let op = bind(&m, "P", &sel(&["A"]));
        apply(&mut m, op);
        for v in [-1.0, 1.0] {
            let op = remove_key(&m, "P", v, &sel(&["A"]));
            apply(&mut m, op);
        }
        assert_eq!(primary(&m, "P", &sel(&["A"])).unwrap().1, [0.0]);
        assert_eq!(
            remove_key(&m, "P", 0.0, &sel(&["A"])),
            Err(ParamError::LastKey)
        );
    }

    #[test]
    fn key_helpers_snap_step_and_find() {
        let keys = [-1.0, 0.0, 0.5, 1.0];
        assert_eq!(snap(&keys, 0.47, 0.05), 0.5);
        assert_eq!(snap(&keys, 0.3, 0.05), 0.3);
        assert_eq!(snap(&[], 0.3, 1.0), 0.3);
        assert_eq!(neighbour(&keys, 0.2, 2.0, 1), Some(0.5));
        assert_eq!(neighbour(&keys, 0.2, 2.0, -1), Some(0.0));
        assert_eq!(neighbour(&keys, 0.5, 2.0, 1), Some(1.0));
        assert_eq!(neighbour(&keys, 1.0, 2.0, 1), None);
        assert_eq!(neighbour(&keys, -1.0, 2.0, -1), None);
        assert_eq!(on_key(&keys, 0.50001, 2.0), Some(0.5));
        assert_eq!(on_key(&keys, 0.4, 2.0), None);
    }

    #[test]
    fn deleting_a_parameter_takes_its_elements_off_first_in_one_step() {
        let mut m = model();
        let op = bind(&m, "P", &sel(&["A", "B"]));
        apply(&mut m, op);
        let blank = model();
        let (command, label) = delete_parameter(&m, "P").unwrap();
        assert_eq!(label, "param_delete: P");
        m.apply(command).unwrap();
        assert!(m.parameters().find("P").is_none());
        assert_eq!(m.bindings().len(), 0);
        let a = m.meshes().get(m.meshes().find("A").unwrap()).unwrap();
        assert!(a.keyforms.is_none());
        assert!(m.undo());
        assert!(m.parameters().find("P").is_some());
        assert_eq!(m.bindings().len(), 1);
        assert!(delete_parameter(&blank, "Nope").is_none());
    }

    #[test]
    fn a_new_parameter_gets_a_free_name() {
        let mut m = model();
        let (c, _) = create_parameter(&m);
        m.apply(c).unwrap();
        let (c, label) = create_parameter(&m);
        assert_eq!(label, "param_create: Param2");
        m.apply(c).unwrap();
        assert!(m.parameters().find("Param2").is_some());
    }
}
