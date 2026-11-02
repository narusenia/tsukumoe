// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The commands that bind elements to parameters and edit the keys of a
//! binding (M5-06a). Every change to a keyform grid is a resampling (see
//! `keyform_edit`), so nothing that is drawn changes when a key is added.
//!
//! Bindings live in an arena and are shared by the elements that react to the
//! same parameters at the same keys. A command never changes a binding that
//! others may use: it makes the new binding (or finds an equal one), moves the
//! elements over, and drops the old one if no element is left on it. History
//! records the arena changes with the element changes.

use crate::arena::Id;
use crate::elements::{Parameter, ParameterId};
use crate::history::{BindingEdit, Change, Editable};
use crate::keyform::{Binding, BindingId, KeyformGrid};
use crate::keyform_edit::{self, Blend, resample, sample};
use crate::model::Model;
use crate::rig::{
    ArtMesh, DeformerRef, MeshKeyform, RotationDeformer, RotationKeyform, WarpDeformer, WarpKeyform,
};

use super::{Command, CommandError, ElementRef};

/// An element kind that can hold keyforms on a binding.
trait Bound: Editable {
    type Kf: Blend;
    fn grid(&self) -> Option<&KeyformGrid<Self::Kf>>;
    fn set_grid(&mut self, grid: Option<KeyformGrid<Self::Kf>>);
    /// The base shape as a keyform.
    fn form(&self) -> Self::Kf;
    /// Takes a keyform as the base shape (when the keyforms go away).
    fn set_form(&mut self, kf: &Self::Kf);
}

impl Bound for ArtMesh {
    type Kf = MeshKeyform;
    fn grid(&self) -> Option<&KeyformGrid<MeshKeyform>> {
        self.keyforms.as_ref()
    }
    fn set_grid(&mut self, grid: Option<KeyformGrid<MeshKeyform>>) {
        self.keyforms = grid;
    }
    fn form(&self) -> MeshKeyform {
        MeshKeyform {
            positions: self.positions.clone(),
            opacity: self.opacity,
            draw_order: self.draw_order,
            multiply_color: self.multiply_color,
            screen_color: self.screen_color,
        }
    }
    fn set_form(&mut self, kf: &MeshKeyform) {
        self.positions = kf.positions.clone();
        self.opacity = kf.opacity;
        self.draw_order = kf.draw_order;
        self.multiply_color = kf.multiply_color;
        self.screen_color = kf.screen_color;
    }
}

impl Bound for WarpDeformer {
    type Kf = WarpKeyform;
    fn grid(&self) -> Option<&KeyformGrid<WarpKeyform>> {
        self.keyforms.as_ref()
    }
    fn set_grid(&mut self, grid: Option<KeyformGrid<WarpKeyform>>) {
        self.keyforms = grid;
    }
    fn form(&self) -> WarpKeyform {
        WarpKeyform {
            points: self.base_points.clone(),
            opacity: 1.0,
        }
    }
    fn set_form(&mut self, kf: &WarpKeyform) {
        self.base_points = kf.points.clone();
    }
}

impl Bound for RotationDeformer {
    type Kf = RotationKeyform;
    fn grid(&self) -> Option<&KeyformGrid<RotationKeyform>> {
        self.keyforms.as_ref()
    }
    fn set_grid(&mut self, grid: Option<KeyformGrid<RotationKeyform>>) {
        self.keyforms = grid;
    }
    fn form(&self) -> RotationKeyform {
        self.base
    }
    fn set_form(&mut self, kf: &RotationKeyform) {
        self.base = *kf;
    }
}

#[derive(Clone, Copy)]
enum Elem {
    Mesh(Id<ArtMesh>),
    Warp(Id<WarpDeformer>),
    Rotation(Id<RotationDeformer>),
}

fn refused(name: &str, reason: &'static str) -> CommandError {
    CommandError::Refused {
        name: name.to_owned(),
        reason,
    }
}

impl Model {
    fn element(&self, r: &ElementRef) -> Result<(Elem, String), CommandError> {
        match r {
            ElementRef::Mesh { name } => {
                Ok((Elem::Mesh(self.resolve::<ArtMesh>(name)?), name.clone()))
            }
            ElementRef::Deformer { name } => Ok((
                match self.resolve_deformer(name)? {
                    DeformerRef::Warp(id) => Elem::Warp(id),
                    DeformerRef::Rotation(id) => Elem::Rotation(id),
                },
                name.clone(),
            )),
        }
    }

    fn binding_of(&self, e: Elem) -> Option<BindingId> {
        match e {
            Elem::Mesh(id) => self.meshes.get(id)?.keyforms.as_ref().map(|g| g.binding),
            Elem::Warp(id) => self.warps.get(id)?.keyforms.as_ref().map(|g| g.binding),
            Elem::Rotation(id) => self.rotations.get(id)?.keyforms.as_ref().map(|g| g.binding),
        }
    }

    /// Every mesh, warp and rotation on `binding`.
    fn sharers(&self, binding: BindingId) -> Vec<Elem> {
        let mut out = Vec::new();
        for (id, _, m) in self.meshes.iter() {
            if m.keyforms.as_ref().is_some_and(|g| g.binding == binding) {
                out.push(Elem::Mesh(id));
            }
        }
        for (id, _, w) in self.warps.iter() {
            if w.keyforms.as_ref().is_some_and(|g| g.binding == binding) {
                out.push(Elem::Warp(id));
            }
        }
        for (id, _, r) in self.rotations.iter() {
            if r.keyforms.as_ref().is_some_and(|g| g.binding == binding) {
                out.push(Elem::Rotation(id));
            }
        }
        out
    }

    /// True if a glue or a bone (whose keyforms these commands do not edit) is on `binding`.
    fn used_by_others(&self, binding: BindingId) -> bool {
        self.glues
            .iter()
            .any(|(_, _, g)| g.keyforms.as_ref().is_some_and(|k| k.binding == binding))
            || self
                .bones
                .iter()
                .any(|(_, _, b)| b.keyforms.as_ref().is_some_and(|k| k.binding == binding))
    }

    /// The id of a binding equal to `binding`, adding one if there is none.
    fn intern_binding(&mut self, changes: &mut Vec<Change>, binding: Binding) -> BindingId {
        if let Some((id, _)) = self.bindings.iter().find(|(_, b)| **b == binding) {
            return id;
        }
        let id = self.bindings.insert(binding.clone());
        changes.push(Change::Binding(BindingEdit {
            id,
            before: None,
            after: Some(binding),
        }));
        id
    }

    /// Removes the bindings in `candidates` that nothing uses any more.
    fn drop_unused(&mut self, changes: &mut Vec<Change>, candidates: &[BindingId]) {
        let mut done: Vec<BindingId> = Vec::new();
        for &id in candidates {
            if done.contains(&id) {
                continue;
            }
            done.push(id);
            if !self.sharers(id).is_empty() || self.used_by_others(id) {
                continue;
            }
            if let Some(binding) = self.bindings.remove(id) {
                changes.push(Change::Binding(BindingEdit {
                    id,
                    before: Some(binding),
                    after: None,
                }));
            }
        }
    }

    /// Gives element `id` the grid on `new` (or no keyforms, `None`), resampled
    /// from what it has; an old axis `new` does not have is read at
    /// `fallback(parameter)`. An element with no keyforms gets copies of its shape.
    fn rebuild<T: Bound>(
        &mut self,
        changes: &mut Vec<Change>,
        id: Id<T>,
        new: Option<(BindingId, &Binding)>,
        fallback: &dyn Fn(ParameterId) -> f32,
    ) {
        let Some(element) = T::table(self).get(id) else {
            return;
        };
        let old = element
            .grid()
            .and_then(|g| Some((self.bindings.get(g.binding)?.clone(), g.keyforms.clone())));
        let form = element.form();
        let grid = new.map(|(bid, binding)| {
            let keyforms = match &old {
                Some((ob, kfs)) => resample(ob, kfs, binding, fallback),
                None => vec![form.clone(); binding.keyform_count()],
            };
            KeyformGrid::new(bid, keyforms)
        });
        // No grid left: the shape at the fallback values becomes the base shape.
        let base = match (&grid, &old) {
            (None, Some((ob, kfs))) => {
                let values: Vec<f32> = ob.axes().iter().map(|a| fallback(a.parameter)).collect();
                Some(sample(ob, kfs, &values))
            }
            _ => None,
        };
        self.modify(changes, id, |e| {
            if let Some(kf) = &base {
                e.set_form(kf);
            }
            e.set_grid(grid);
        });
    }

    fn rebuild_elem(
        &mut self,
        changes: &mut Vec<Change>,
        e: Elem,
        new: Option<(BindingId, &Binding)>,
        fallback: &dyn Fn(ParameterId) -> f32,
    ) {
        match e {
            Elem::Mesh(id) => self.rebuild(changes, id, new, fallback),
            Elem::Warp(id) => self.rebuild(changes, id, new, fallback),
            Elem::Rotation(id) => self.rebuild(changes, id, new, fallback),
        }
    }

    pub(super) fn run_keyform(
        &mut self,
        command: &Command,
        changes: &mut Vec<Change>,
    ) -> Result<(), CommandError> {
        match command {
            Command::RenameParameter { name, new_name } => {
                let id = self.resolve::<Parameter>(name)?;
                if new_name.is_empty() {
                    return Err(CommandError::EmptyName { kind: "parameter" });
                }
                if name == new_name {
                    return Ok(());
                }
                // Expressions name parameters in their text: rewrite them too.
                let rewrites: Vec<(Id<crate::driver::Driver>, String)> = self
                    .drivers
                    .iter()
                    .filter_map(|(did, _, d)| {
                        crate::expr::rename_name(&d.expression, name, new_name).map(|t| (did, t))
                    })
                    .collect();
                if !rewrites.is_empty() && !crate::expr::is_identifier(new_name) {
                    return Err(refused(name, "not-an-identifier"));
                }
                self.rename(changes, id, new_name)?;
                for (did, text) in rewrites {
                    self.modify(changes, did, |d| d.expression = text);
                }
            }
            Command::SetParameterRepeat { name, repeat } => {
                let id = self.resolve::<Parameter>(name)?;
                self.modify(changes, id, |p| p.repeat = *repeat);
            }
            Command::BindParameter {
                parameter,
                elements,
            } => {
                let pid = self.resolve::<Parameter>(parameter)?;
                let p = self.parameters.get(pid).cloned().expect("resolved");
                let binding = keyform_edit::single_axis(pid, p.min, p.default, p.max)
                    .map_err(|_| refused(parameter, "bad-range"))?;
                let mut elems = Vec::new();
                for r in elements {
                    let (e, name) = self.element(r)?;
                    if self.binding_of(e).is_some() {
                        return Err(refused(&name, "already-bound"));
                    }
                    elems.push(e);
                }
                let bid = self.intern_binding(changes, binding.clone());
                for e in elems {
                    self.rebuild_elem(changes, e, Some((bid, &binding)), &|_| p.default);
                }
                self.drop_unused(changes, &[bid]);
            }
            Command::UnbindParameter {
                parameter,
                elements,
            } => {
                let pid = self.resolve::<Parameter>(parameter)?;
                let default = self.parameters.get(pid).map_or(0.0, |p| p.default);
                let mut old_ids = Vec::new();
                for r in elements {
                    let (e, name) = self.element(r)?;
                    let bid = self
                        .binding_of(e)
                        .ok_or_else(|| refused(&name, "not-bound"))?;
                    let binding = self.bindings.get(bid).cloned().expect("on a binding");
                    let next = keyform_edit::without_axis(&binding, pid)
                        .ok_or_else(|| refused(&name, "not-bound"))?;
                    match next {
                        Some(nb) => {
                            let nid = self.intern_binding(changes, nb.clone());
                            self.rebuild_elem(changes, e, Some((nid, &nb)), &|_| default);
                        }
                        None => self.rebuild_elem(changes, e, None, &|_| default),
                    }
                    old_ids.push(bid);
                }
                self.drop_unused(changes, &old_ids);
            }
            Command::AddParameterKey {
                parameter,
                value,
                element,
            }
            | Command::RemoveParameterKey {
                parameter,
                value,
                element,
            } => {
                let adding = matches!(command, Command::AddParameterKey { .. });
                let pid = self.resolve::<Parameter>(parameter)?;
                let (e, name) = self.element(element)?;
                let bid = self
                    .binding_of(e)
                    .ok_or_else(|| refused(&name, "not-bound"))?;
                let binding = self.bindings.get(bid).cloned().expect("on a binding");
                let axis = binding
                    .axes()
                    .iter()
                    .find(|a| a.parameter == pid)
                    .ok_or_else(|| refused(&name, "not-bound"))?;
                if self.used_by_others(bid) {
                    return Err(refused(&name, "shared-with-other-kinds"));
                }
                let next = if adding {
                    keyform_edit::with_key(&binding, pid, *value)
                        .ok_or_else(|| refused(&name, "bad-key"))?
                } else if axis.keys.len() <= 1 {
                    return Err(refused(&name, "last-key"));
                } else {
                    keyform_edit::without_key(&binding, pid, *value)
                        .ok_or_else(|| refused(&name, "no-such-key"))?
                };
                let nid = self.intern_binding(changes, next.clone());
                for s in self.sharers(bid) {
                    self.rebuild_elem(changes, s, Some((nid, &next)), &|_| 0.0);
                }
                self.drop_unused(changes, &[bid]);
            }
            _ => unreachable!("not a keyform command"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::driver::Driver;
    use crate::eval::Frame;

    fn mesh_ref(n: &str) -> ElementRef {
        ElementRef::Mesh { name: n.into() }
    }

    fn deformer_ref(n: &str) -> ElementRef {
        ElementRef::Deformer { name: n.into() }
    }

    fn base() -> Model {
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

    fn bind(m: &mut Model, elements: Vec<ElementRef>) -> Result<(), CommandError> {
        m.apply(Command::BindParameter {
            parameter: "P".into(),
            elements,
        })
    }

    fn kf_of(m: &Model, mesh: &str) -> KeyformGrid<MeshKeyform> {
        let id = m.meshes().find(mesh).unwrap();
        m.meshes().get(id).unwrap().keyforms.clone().unwrap()
    }

    /// The vertices of every mesh with `P` at `value`.
    fn drawn(m: &Model, value: f32) -> Vec<f32> {
        let compiled = m.compile().unwrap();
        let mut frame = Frame::new(&compiled);
        let index = (0..compiled.parameter_names.len())
            .find(|&i| compiled.parameter_name(i) == "P")
            .unwrap();
        frame.set_parameter(index, value);
        frame.evaluate(&compiled);
        frame.positions().to_vec()
    }

    fn same(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-4, "{x} vs {y}");
        }
    }

    /// A mesh with two different shapes at the two ends, bound to `P`.
    fn bent() -> Model {
        let mut m = base();
        bind(&mut m, vec![mesh_ref("A"), mesh_ref("B")]).unwrap();
        for (name, shift) in [("A", 1.0), ("B", 2.0)] {
            let id = m.meshes().find(name).unwrap();
            let mut grid = m.meshes().get(id).unwrap().keyforms.clone().unwrap();
            // keys are -1, 0, 1: move the end keys apart.
            for (k, d) in [(0, -shift), (2, shift)] {
                for p in &mut grid.keyforms[k].positions {
                    p[0] += d;
                }
            }
            m.meshes.get_mut(id).unwrap().keyforms = Some(grid);
        }
        m
    }

    #[test]
    fn binding_copies_the_shape_to_every_key_shares_the_binding_and_undoes() {
        let mut m = base();
        let before = drawn(&m, 0.3);
        let blank = base();
        bind(&mut m, vec![mesh_ref("A"), mesh_ref("B")]).unwrap();
        let (a, b) = (kf_of(&m, "A"), kf_of(&m, "B"));
        assert_eq!(a.binding, b.binding, "one shared binding");
        assert_eq!(m.bindings().len(), 1);
        let axis = &m.bindings().get(a.binding).unwrap().axes()[0];
        assert_eq!(axis.keys, [-1.0, 0.0, 1.0]);
        assert_eq!(a.keyforms.len(), 3);
        assert!(
            a.keyforms
                .iter()
                .all(|k| k.positions == a.keyforms[0].positions)
        );
        same(&drawn(&m, 0.3), &before);
        // Binding them one at a time lands on the same binding.
        let mut one_by_one = base();
        bind(&mut one_by_one, vec![mesh_ref("A")]).unwrap();
        bind(&mut one_by_one, vec![mesh_ref("B")]).unwrap();
        assert_eq!(one_by_one.bindings().len(), 1);
        // A second bind of a bound element is refused and changes nothing.
        let snapshot = drawn(&m, 0.0);
        assert_eq!(
            bind(&mut m, vec![mesh_ref("A")]),
            Err(CommandError::Refused {
                name: "A".into(),
                reason: "already-bound"
            })
        );
        same(&drawn(&m, 0.0), &snapshot);
        assert!(m.undo());
        assert!(m == blank);
        assert!(m.redo());
        assert_eq!(m.bindings().len(), 1);
    }

    #[test]
    fn a_default_at_an_end_gives_two_keys() {
        let mut m = Model::new();
        for c in [
            Command::AddParameter {
                name: "P".into(),
                min: 0.0,
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
        let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        m.apply(Command::AddMesh {
            name: "A".into(),
            part: "Part".into(),
            positions: pts.clone(),
            uvs: pts,
            indices: vec![0, 1, 2],
        })
        .unwrap();
        bind(&mut m, vec![mesh_ref("A")]).unwrap();
        let axis = m.bindings().iter().next().unwrap().1.axes()[0].clone();
        assert_eq!(axis.keys, [0.0, 1.0]);
    }

    #[test]
    fn adding_a_key_changes_nothing_drawn_and_every_sharer_gets_it() {
        let mut m = bent();
        let values = [-2.0, -1.0, -0.4, 0.0, 0.3, 0.5, 1.0, 3.0];
        let before: Vec<Vec<f32>> = values.iter().map(|&v| drawn(&m, v)).collect();
        let was = kf_of(&m, "A").binding;
        m.apply(Command::AddParameterKey {
            parameter: "P".into(),
            value: 0.5,
            element: mesh_ref("A"),
        })
        .unwrap();
        for (v, expected) in values.iter().zip(&before) {
            same(&drawn(&m, *v), expected);
        }
        let (a, b) = (kf_of(&m, "A"), kf_of(&m, "B"));
        assert_eq!(a.binding, b.binding);
        assert_ne!(a.binding, was);
        assert_eq!(m.bindings().len(), 1, "the old binding is gone");
        assert_eq!(
            m.bindings().get(a.binding).unwrap().axes()[0].keys,
            [-1.0, 0.0, 0.5, 1.0]
        );
        assert_eq!((a.keyforms.len(), b.keyforms.len()), (4, 4));
        // Already a key, or not a number: refused.
        for v in [0.5, f32::NAN] {
            assert!(matches!(
                m.apply(Command::AddParameterKey {
                    parameter: "P".into(),
                    value: v,
                    element: mesh_ref("B"),
                }),
                Err(CommandError::Refused {
                    reason: "bad-key",
                    ..
                })
            ));
        }
        assert!(m.undo());
        assert_eq!(kf_of(&m, "A").binding, was);
        assert_eq!(kf_of(&m, "B").keyforms.len(), 3);
        assert_eq!(m.bindings().len(), 1);
        for (v, expected) in values.iter().zip(&before) {
            same(&drawn(&m, *v), expected);
        }
    }

    #[test]
    fn removing_a_key_drops_its_forms_and_the_last_key_stays() {
        let mut m = bent();
        let remove = |m: &mut Model, v: f32| {
            m.apply(Command::RemoveParameterKey {
                parameter: "P".into(),
                value: v,
                element: mesh_ref("A"),
            })
        };
        assert!(matches!(
            remove(&mut m, 0.25),
            Err(CommandError::Refused {
                reason: "no-such-key",
                ..
            })
        ));
        remove(&mut m, 0.0).unwrap();
        let (a, b) = (kf_of(&m, "A"), kf_of(&m, "B"));
        assert_eq!((a.keyforms.len(), b.keyforms.len()), (2, 2));
        assert_eq!(a.binding, b.binding);
        // Between the two ends the shape is now a straight blend.
        let mid = drawn(&m, 0.0);
        let id = m.meshes().find("A").unwrap();
        let ends = m.meshes().get(id).unwrap().keyforms.as_ref().unwrap();
        let expect = (ends.keyforms[0].positions[0][0] + ends.keyforms[1].positions[0][0]) / 2.0;
        assert!((mid[0] - expect).abs() < 1e-4);
        remove(&mut m, -1.0).unwrap();
        assert!(matches!(
            remove(&mut m, 1.0),
            Err(CommandError::Refused {
                reason: "last-key",
                ..
            })
        ));
        assert!(m.undo() && m.undo());
        assert_eq!(kf_of(&m, "A").keyforms.len(), 3);
    }

    #[test]
    fn unbinding_keeps_the_shape_at_the_default_and_drops_the_binding() {
        let mut m = bent();
        let at_default = drawn(&m, 0.0);
        let blank = bent();
        m.apply(Command::UnbindParameter {
            parameter: "P".into(),
            elements: vec![mesh_ref("A")],
        })
        .unwrap();
        let id = m.meshes().find("A").unwrap();
        assert!(m.meshes().get(id).unwrap().keyforms.is_none());
        // B still has its binding.
        assert_eq!(m.bindings().len(), 1);
        same(&drawn(&m, 0.0), &at_default);
        // Unbinding what is not bound is refused.
        assert!(matches!(
            m.apply(Command::UnbindParameter {
                parameter: "P".into(),
                elements: vec![mesh_ref("A")],
            }),
            Err(CommandError::Refused {
                reason: "not-bound",
                ..
            })
        ));
        m.apply(Command::UnbindParameter {
            parameter: "P".into(),
            elements: vec![mesh_ref("B")],
        })
        .unwrap();
        assert_eq!(m.bindings().len(), 0);
        assert!(m.undo() && m.undo());
        assert!(m == blank);
    }

    #[test]
    fn a_deformer_can_be_bound_and_keyed_too() {
        let mut m = base();
        m.apply(Command::AddWarpDeformer {
            name: "W".into(),
            part: "Part".into(),
            parent: None,
            rows: 1,
            columns: 1,
            base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        })
        .unwrap();
        m.apply(Command::AddRotationDeformer {
            name: "R".into(),
            part: "Part".into(),
            parent: None,
            base: crate::command::RotationBase {
                origin: [0.0, 0.0],
                angle_degrees: 12.0,
                scale: 1.0,
                reflect_x: false,
                reflect_y: false,
            },
        })
        .unwrap();
        bind(&mut m, vec![deformer_ref("W"), deformer_ref("R")]).unwrap();
        m.apply(Command::AddParameterKey {
            parameter: "P".into(),
            value: 0.5,
            element: deformer_ref("R"),
        })
        .unwrap();
        let w = m.warps().get(m.warps().find("W").unwrap()).unwrap();
        let r = m.rotations().get(m.rotations().find("R").unwrap()).unwrap();
        assert_eq!(w.keyforms.as_ref().unwrap().keyforms.len(), 4);
        assert_eq!(r.keyforms.as_ref().unwrap().keyforms.len(), 4);
        assert!(
            r.keyforms
                .as_ref()
                .unwrap()
                .keyforms
                .iter()
                .all(|k| k.angle_degrees == 12.0)
        );
        m.apply(Command::UnbindParameter {
            parameter: "P".into(),
            elements: vec![deformer_ref("W"), deformer_ref("R")],
        })
        .unwrap();
        let r = m.rotations().get(m.rotations().find("R").unwrap()).unwrap();
        assert!(r.keyforms.is_none() && r.base.angle_degrees == 12.0);
        assert_eq!(m.bindings().len(), 0);
    }

    #[test]
    fn renaming_a_parameter_rewrites_driver_expressions_and_undoes() {
        let mut m = base();
        m.apply(Command::AddParameter {
            name: "Q".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        let q = m.parameters().find("Q").unwrap();
        m.drivers
            .insert(
                "D",
                Driver {
                    output: q,
                    expression: "P * 2 + sin(P)".into(),
                },
            )
            .unwrap();
        let rename = |m: &mut Model, to: &str| {
            m.apply(Command::RenameParameter {
                name: "P".into(),
                new_name: to.into(),
            })
        };
        // A name an expression cannot write breaks the expression: refused.
        assert!(matches!(
            rename(&mut m, "not a name"),
            Err(CommandError::Refused {
                reason: "not-an-identifier",
                ..
            })
        ));
        assert!(m.parameters().find("P").is_some());
        rename(&mut m, "Angle").unwrap();
        let d = m.drivers().get(m.drivers().find("D").unwrap()).unwrap();
        assert_eq!(d.expression, "Angle * 2 + sin(Angle)");
        assert!(m.compile().is_ok(), "the expression still compiles");
        assert!(matches!(
            m.apply(Command::RenameParameter {
                name: "Angle".into(),
                new_name: "Q".into(),
            }),
            Err(CommandError::AlreadyExists { .. })
        ));
        assert!(m.undo());
        let d = m.drivers().get(m.drivers().find("D").unwrap()).unwrap();
        assert_eq!(d.expression, "P * 2 + sin(P)");
        assert!(m.parameters().find("P").is_some());
        // A name nothing reads can be anything (not empty).
        m.drivers.remove(m.drivers().find("D").unwrap());
        rename(&mut m, "not a name").unwrap();
        assert!(matches!(
            m.apply(Command::RenameParameter {
                name: "not a name".into(),
                new_name: String::new(),
            }),
            Err(CommandError::EmptyName { .. })
        ));
    }

    #[test]
    fn the_new_commands_round_trip_through_json() {
        let c = Command::AddParameterKey {
            parameter: "P".into(),
            value: 0.5,
            element: deformer_ref("W"),
        };
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type":"add_parameter_key","parameter":"P","value":0.5,
                "element":{"kind":"deformer","name":"W"}})
        );
        assert_eq!(serde_json::from_value::<Command>(json).unwrap(), c);
    }
}
