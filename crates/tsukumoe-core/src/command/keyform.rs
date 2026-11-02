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
use crate::keyform::{Axis, Binding, BindingId, KeyformGrid};
use crate::keyform_edit::{self, Blend, resample, sample};
use crate::model::Model;
use crate::rig::{
    ArtMesh, DeformerRef, Glue, MeshKeyform, RotationDeformer, RotationKeyform, WarpDeformer,
    WarpKeyform,
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

impl Bound for Glue {
    type Kf = f32;
    fn grid(&self) -> Option<&KeyformGrid<f32>> {
        self.keyforms.as_ref()
    }
    fn set_grid(&mut self, grid: Option<KeyformGrid<f32>>) {
        self.keyforms = grid;
    }
    fn form(&self) -> f32 {
        self.intensity
    }
    fn set_form(&mut self, kf: &f32) {
        self.intensity = *kf;
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
    Glue(Id<Glue>),
}

pub(super) fn refused(name: &str, reason: &'static str) -> CommandError {
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
            ElementRef::Glue { name } => {
                Ok((Elem::Glue(self.resolve::<Glue>(name)?), name.clone()))
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
            Elem::Glue(id) => self.glues.get(id)?.keyforms.as_ref().map(|g| g.binding),
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
        for (id, _, g) in self.glues.iter() {
            if g.keyforms.as_ref().is_some_and(|g| g.binding == binding) {
                out.push(Elem::Glue(id));
            }
        }
        out
    }

    /// True if a bone (whose keyforms these commands do not edit) is on `binding`.
    fn used_by_others(&self, binding: BindingId) -> bool {
        self.bones
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

    /// Puts the keyforms of element `id` on the binding `new`, which has the
    /// shape of the old one (same number of keys per axis).
    fn retarget<T: Bound>(&mut self, changes: &mut Vec<Change>, id: Id<T>, new: BindingId) {
        let Some(grid) = T::table(self).get(id).and_then(|e| e.grid()) else {
            return;
        };
        let grid = KeyformGrid::new(new, grid.keyforms.clone());
        self.modify(changes, id, |e| e.set_grid(Some(grid)));
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
            Elem::Glue(id) => self.rebuild(changes, id, new, fallback),
        }
    }

    /// Position in the dense grid of the key `key` (one index per axis).
    fn key_index<T>(
        &self,
        grid: Option<&KeyformGrid<T>>,
        key: &[usize],
        name: &str,
    ) -> Result<usize, CommandError> {
        let grid = grid.ok_or_else(|| refused(name, "not-bound"))?;
        let binding = self
            .bindings
            .get(grid.binding)
            .ok_or_else(|| refused(name, "not-bound"))?;
        binding.index(key).ok_or_else(|| refused(name, "bad-key"))
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
            Command::SetParameterGroup { name, group } => {
                let id = self.resolve::<Parameter>(name)?;
                self.modify(changes, id, |p| p.group = group.clone());
            }
            Command::MoveParameter { name, to } => {
                let id = self.resolve::<Parameter>(name)?;
                let from = self.parameters.position(id).expect("resolved");
                let to = (*to).min(self.parameters.len() - 1);
                if from != to {
                    self.parameters.move_to(id, to);
                    changes.push(Change::ParameterOrder { id, from, to });
                }
            }
            Command::MoveParameterKeyforms { from, to } => {
                let (f, t) = (
                    self.resolve::<Parameter>(from)?,
                    self.resolve::<Parameter>(to)?,
                );
                let (pf, pt) = (
                    self.parameters.get(f).cloned().expect("resolved"),
                    self.parameters.get(t).cloned().expect("resolved"),
                );
                let scale = |k: f32| {
                    let span = pf.max - pf.min;
                    if span > 0.0 {
                        pt.min + (k - pf.min) / span * (pt.max - pt.min)
                    } else {
                        pt.default
                    }
                };
                let mut moves = Vec::new();
                for (bid, b) in self.bindings.iter() {
                    let Some(at) = b.axes().iter().position(|a| a.parameter == f) else {
                        continue;
                    };
                    if b.axes().iter().any(|a| a.parameter == t) {
                        return Err(refused(to, "already-bound"));
                    }
                    if self.used_by_others(bid) {
                        return Err(refused(from, "shared-with-other-kinds"));
                    }
                    let mut axes = b.axes().to_vec();
                    axes[at] = Axis {
                        parameter: t,
                        keys: axes[at].keys.iter().map(|&k| scale(k)).collect(),
                    };
                    let next = Binding::new(axes).map_err(|_| refused(to, "bad-range"))?;
                    moves.push((bid, next));
                }
                if moves.is_empty() {
                    return Err(refused(from, "not-bound"));
                }
                let mut old = Vec::new();
                for (bid, next) in moves {
                    let nid = self.intern_binding(changes, next);
                    for e in self.sharers(bid) {
                        match e {
                            Elem::Mesh(id) => self.retarget(changes, id, nid),
                            Elem::Warp(id) => self.retarget(changes, id, nid),
                            Elem::Rotation(id) => self.retarget(changes, id, nid),
                            Elem::Glue(id) => self.retarget(changes, id, nid),
                        }
                    }
                    old.push(bid);
                }
                self.drop_unused(changes, &old);
            }
            Command::BindParameter {
                parameter,
                elements,
            } => {
                let pid = self.resolve::<Parameter>(parameter)?;
                let p = self.parameters.get(pid).cloned().expect("resolved");
                let axis = keyform_edit::single_axis(pid, p.min, p.default, p.max)
                    .map_err(|_| refused(parameter, "bad-range"))?;
                // An element with keyforms gets the parameter as one more
                // axis (the last, so the old grid keeps its order).
                let mut elems = Vec::new();
                for r in elements {
                    let (e, name) = self.element(r)?;
                    let binding = match self.binding_of(e) {
                        None => axis.clone(),
                        Some(bid) => {
                            let old = self.bindings.get(bid).expect("on a binding");
                            if old.axes().iter().any(|a| a.parameter == pid) {
                                return Err(refused(&name, "already-bound"));
                            }
                            let mut axes = old.axes().to_vec();
                            axes.extend(axis.axes().iter().cloned());
                            Binding::new(axes).map_err(|_| refused(&name, "bad-range"))?
                        }
                    };
                    elems.push((e, self.binding_of(e), binding));
                }
                let mut touched = Vec::new();
                for (e, old, binding) in elems {
                    let bid = self.intern_binding(changes, binding.clone());
                    self.rebuild_elem(changes, e, Some((bid, &binding)), &|_| p.default);
                    touched.extend(old);
                    touched.push(bid);
                }
                self.drop_unused(changes, &touched);
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
            Command::SetAxisOrder {
                element,
                parameters,
            } => {
                let (e, name) = self.element(element)?;
                let bid = self
                    .binding_of(e)
                    .ok_or_else(|| refused(&name, "not-bound"))?;
                let binding = self.bindings.get(bid).cloned().expect("on a binding");
                if self.used_by_others(bid) {
                    return Err(refused(&name, "shared-with-other-kinds"));
                }
                let mut axes = Vec::new();
                for p in parameters {
                    let pid = self.resolve::<Parameter>(p)?;
                    let axis = binding
                        .axes()
                        .iter()
                        .find(|a| a.parameter == pid)
                        .ok_or_else(|| refused(&name, "bad-order"))?;
                    axes.push(axis.clone());
                }
                if axes.len() != binding.axes().len() {
                    return Err(refused(&name, "bad-order"));
                }
                let next = Binding::new(axes).map_err(|_| refused(&name, "bad-order"))?;
                if next != binding {
                    let nid = self.intern_binding(changes, next.clone());
                    for s in self.sharers(bid) {
                        self.rebuild_elem(changes, s, Some((nid, &next)), &|_| 0.0);
                    }
                    self.drop_unused(changes, &[bid]);
                }
            }
            Command::SetMeshKeyform {
                name,
                key,
                positions,
                opacity,
                draw_order,
            } => {
                let id = self.resolve::<ArtMesh>(name)?;
                let at = self.key_index(self.meshes.get(id).and_then(|m| m.grid()), key, name)?;
                self.modify(changes, id, |m| {
                    if let Some(g) = m.keyforms.as_mut() {
                        let kf = &mut g.keyforms[at];
                        kf.positions = positions.clone();
                        kf.opacity = *opacity;
                        kf.draw_order = *draw_order;
                    }
                });
            }
            Command::SetMeshKeyformColors {
                name,
                key,
                multiply_color,
                screen_color,
            } => {
                let id = self.resolve::<ArtMesh>(name)?;
                let at = self.key_index(self.meshes.get(id).and_then(|m| m.grid()), key, name)?;
                self.modify(changes, id, |m| {
                    if let Some(g) = m.keyforms.as_mut() {
                        g.keyforms[at].multiply_color = *multiply_color;
                        g.keyforms[at].screen_color = *screen_color;
                    }
                });
            }
            Command::SetWarpKeyform {
                name,
                key,
                points,
                opacity,
            } => {
                let id = self.resolve::<WarpDeformer>(name)?;
                let at = self.key_index(self.warps.get(id).and_then(|w| w.grid()), key, name)?;
                self.modify(changes, id, |w| {
                    if let Some(g) = w.keyforms.as_mut() {
                        g.keyforms[at] = WarpKeyform {
                            points: points.clone(),
                            opacity: *opacity,
                        };
                    }
                });
            }
            Command::SetRotationKeyform {
                name,
                key,
                base,
                opacity,
            } => {
                let id = self.resolve::<RotationDeformer>(name)?;
                let at =
                    self.key_index(self.rotations.get(id).and_then(|r| r.grid()), key, name)?;
                self.modify(changes, id, |r| {
                    if let Some(g) = r.keyforms.as_mut() {
                        let kf = &mut g.keyforms[at];
                        base.apply_to(kf);
                        kf.opacity = *opacity;
                    }
                });
            }
            Command::SetGlueKeyform {
                name,
                key,
                intensity,
            } => {
                let id = self.resolve::<Glue>(name)?;
                if !intensity.is_finite() {
                    return Err(refused(name, "bad-range"));
                }
                let at = self.key_index(self.glues.get(id).and_then(|g| g.grid()), key, name)?;
                self.modify(changes, id, |g| {
                    if let Some(grid) = g.keyforms.as_mut() {
                        grid.keyforms[at] = *intensity;
                    }
                });
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

    #[test]
    fn a_second_parameter_becomes_a_second_axis_and_nothing_drawn_changes() {
        let mut m = bent();
        m.apply(Command::AddParameter {
            name: "Q".into(),
            min: 0.0,
            max: 2.0,
            default: 1.0,
            repeat: false,
        })
        .unwrap();
        let before: Vec<Vec<f32>> = [-1.0, -0.4, 0.0, 0.7, 1.0]
            .iter()
            .map(|&v| drawn(&m, v))
            .collect();
        let one_axis = kf_of(&m, "A");
        m.apply(Command::BindParameter {
            parameter: "Q".into(),
            elements: vec![mesh_ref("A"), mesh_ref("B")],
        })
        .unwrap();
        // P has 3 keys, Q has 3: nine keyforms, on one shared two-axis binding.
        assert_eq!(kf_of(&m, "A").keyforms.len(), 9);
        assert_eq!(m.bindings().len(), 1);
        for (v, b) in [-1.0, -0.4, 0.0, 0.7, 1.0].iter().zip(&before) {
            same(&drawn(&m, *v), b);
        }
        // Binding Q again is refused.
        assert_eq!(
            m.apply(Command::BindParameter {
                parameter: "Q".into(),
                elements: vec![mesh_ref("A")],
            }),
            Err(CommandError::Refused {
                name: "A".into(),
                reason: "already-bound"
            })
        );
        assert!(m.undo());
        assert_eq!(kf_of(&m, "A"), one_axis);
        assert_eq!(m.bindings().len(), 1);
    }

    #[test]
    fn swapping_the_axes_keeps_the_picture_and_refuses_a_wrong_list() {
        let mut m = bent();
        m.apply(Command::AddParameter {
            name: "Q".into(),
            min: 0.0,
            max: 2.0,
            default: 1.0,
            repeat: false,
        })
        .unwrap();
        m.apply(Command::BindParameter {
            parameter: "Q".into(),
            elements: vec![mesh_ref("A"), mesh_ref("B")],
        })
        .unwrap();
        let before: Vec<Vec<f32>> = [-1.0, -0.4, 0.7].iter().map(|&v| drawn(&m, v)).collect();
        let grid = kf_of(&m, "A");
        let order = |names: &[&str]| Command::SetAxisOrder {
            element: mesh_ref("A"),
            parameters: names.iter().map(|n| (*n).to_owned()).collect(),
        };
        m.apply(order(&["Q", "P"])).unwrap();
        assert_ne!(kf_of(&m, "A"), grid, "the grid is in the other order");
        assert_eq!(m.bindings().len(), 1, "B moved with A");
        for (v, b) in [-1.0, -0.4, 0.7].iter().zip(&before) {
            same(&drawn(&m, *v), b);
        }
        for bad in [&["P"][..], &["P", "P"], &["P", "Z"]] {
            assert!(m.apply(order(bad)).is_err());
        }
        assert!(m.undo());
        assert_eq!(kf_of(&m, "A"), grid);
    }

    #[test]
    fn a_color_command_changes_only_the_colors_of_its_key_and_undoes() {
        let mut m = bent();
        let blank = bent();
        let before = kf_of(&m, "A");
        m.apply(Command::SetMeshKeyformColors {
            name: "A".into(),
            key: vec![2],
            multiply_color: [0.5, 0.5, 0.5],
            screen_color: [0.0, 0.25, 0.0],
        })
        .unwrap();
        let after = kf_of(&m, "A");
        assert_eq!(after.keyforms[2].multiply_color, [0.5; 3]);
        assert_eq!(after.keyforms[2].screen_color, [0.0, 0.25, 0.0]);
        assert_eq!(after.keyforms[2].positions, before.keyforms[2].positions);
        assert_eq!(after.keyforms[0], before.keyforms[0]);
        assert!(m.undo());
        assert!(m == blank);
    }

    #[test]
    fn a_keyform_command_changes_only_its_key_and_undoes() {
        let mut m = bent();
        let blank = bent();
        let id = m.meshes().find("A").unwrap();
        let before = kf_of(&m, "A");
        let moved = vec![[5.0, 5.0], [6.0, 5.0], [5.0, 6.0]];
        m.apply(Command::SetMeshKeyform {
            name: "A".into(),
            key: vec![2],
            positions: moved.clone(),
            opacity: 0.25,
            draw_order: 650.0,
        })
        .unwrap();
        let after = kf_of(&m, "A");
        assert_eq!(after.keyforms[2].positions, moved);
        assert_eq!(after.keyforms[2].opacity, 0.25);
        assert_eq!(after.keyforms[2].draw_order, 650.0);
        assert_eq!(after.keyforms[0], before.keyforms[0]);
        assert_eq!(after.keyforms[1], before.keyforms[1]);
        assert_eq!(
            after.keyforms[2].multiply_color,
            before.keyforms[2].multiply_color
        );
        // At the key the picture shows it; B is untouched.
        same(&drawn(&m, 1.0)[..6], &[5.0, 5.0, 6.0, 5.0, 5.0, 6.0]);
        assert_eq!(kf_of(&m, "B"), kf_of(&blank, "B"));
        let _ = id;
        assert!(m.undo());
        assert!(m == blank);
    }

    #[test]
    fn keyform_commands_refuse_a_missing_key_an_unbound_element_and_a_wrong_size() {
        let mut m = bent();
        let set = |m: &mut Model, key: Vec<usize>| {
            m.apply(Command::SetMeshKeyform {
                name: "A".into(),
                key,
                positions: vec![[0.0, 0.0]; 3],
                opacity: 1.0,
                draw_order: 500.0,
            })
        };
        assert!(matches!(
            set(&mut m, vec![3]),
            Err(CommandError::Refused {
                reason: "bad-key",
                ..
            })
        ));
        assert!(matches!(
            set(&mut m, vec![0, 0]),
            Err(CommandError::Refused {
                reason: "bad-key",
                ..
            })
        ));
        // Wrong vertex count: the model check refuses it.
        assert!(
            m.apply(Command::SetMeshKeyform {
                name: "A".into(),
                key: vec![0],
                positions: vec![[0.0, 0.0]; 2],
                opacity: 1.0,
                draw_order: 500.0,
            })
            .is_err()
        );
        let mut plain = base();
        assert!(matches!(
            plain.apply(Command::SetMeshKeyform {
                name: "A".into(),
                key: vec![0],
                positions: vec![[0.0, 0.0]; 3],
                opacity: 1.0,
                draw_order: 500.0,
            }),
            Err(CommandError::Refused {
                reason: "not-bound",
                ..
            })
        ));
    }

    #[test]
    fn warp_and_rotation_keyforms_are_set_by_key_and_the_space_follows_the_values() {
        use crate::deformer_space::Space;
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
        bind(&mut m, vec![deformer_ref("W")]).unwrap();
        // At P = 1 (key 2) the grid is twice as wide.
        m.apply(Command::SetWarpKeyform {
            name: "W".into(),
            key: vec![2],
            points: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 1.0], [2.0, 1.0]],
            opacity: 1.0,
        })
        .unwrap();
        let w = crate::rig::DeformerRef::Warp(m.warps().find("W").unwrap());
        let p = m.parameters().find("P").unwrap();
        for (value, x) in [(0.0, 1.0), (1.0, 2.0), (0.5, 1.5)] {
            let space = Space::of_at(&m, Some(w), &|id| if id == p { value } else { 0.0 }).unwrap();
            let at = space.point_to_world([1.0, 0.0]);
            assert!((at[0] - x).abs() < 1e-9, "{value}: {at:?}");
        }
        // The default-valued space is the key at the default.
        let at_default = Space::of(&m, Some(w)).unwrap().point_to_world([1.0, 0.0]);
        assert!((at_default[0] - 1.0).abs() < 1e-9);
        m.apply(Command::AddRotationDeformer {
            name: "R".into(),
            part: "Part".into(),
            parent: None,
            base: crate::command::RotationBase {
                origin: [0.0, 0.0],
                angle_degrees: 0.0,
                scale: 1.0,
                reflect_x: false,
                reflect_y: false,
            },
        })
        .unwrap();
        bind(&mut m, vec![deformer_ref("R")]).unwrap();
        m.apply(Command::SetRotationKeyform {
            name: "R".into(),
            key: vec![0],
            base: crate::command::RotationBase {
                origin: [1.0, 0.0],
                angle_degrees: 90.0,
                scale: 1.0,
                reflect_x: false,
                reflect_y: false,
            },
            opacity: 0.5,
        })
        .unwrap();
        let r = m.rotations().get(m.rotations().find("R").unwrap()).unwrap();
        let kf = &r.keyforms.as_ref().unwrap().keyforms[0];
        assert_eq!(
            (kf.angle_degrees, kf.origin, kf.opacity),
            (90.0, [1.0, 0.0], 0.5)
        );
        let rid = crate::rig::DeformerRef::Rotation(m.rotations().find("R").unwrap());
        let space = Space::of_at(&m, Some(rid), &|_| -1.0).unwrap();
        let at = space.point_to_world([1.0, 0.0]);
        assert!(
            (at[0] - 1.0).abs() < 1e-6 && (at[1] - 1.0).abs() < 1e-6,
            "{at:?}"
        );
    }

    #[test]
    fn a_glue_binds_and_its_intensity_follows_the_keys() {
        let mut m = base();
        m.apply(Command::AddGlue {
            name: "G".into(),
            mesh_a: "A".into(),
            mesh_b: "B".into(),
            nodes: vec![crate::rig::GlueNode {
                index_a: 0,
                index_b: 1,
                weight: 0.5,
            }],
            intensity: 0.0,
        })
        .unwrap();
        let glue = ElementRef::Glue { name: "G".into() };
        let intensity_at = |m: &Model, v: f32| {
            let id = m.glues().find("G").unwrap();
            let g = m.glues().get(id).unwrap();
            let grid = g.keyforms.as_ref().unwrap();
            let binding = m.bindings().get(grid.binding).unwrap();
            sample(binding, &grid.keyforms, &[v])
        };
        // Bound: the base intensity is copied to every key, nothing changes.
        bind(&mut m, vec![glue.clone()]).unwrap();
        assert_eq!(intensity_at(&m, 1.0), 0.0);
        assert!(matches!(
            m.apply(Command::SetGlueIntensity {
                name: "G".into(),
                intensity: 1.0,
            }),
            Err(CommandError::HasKeyforms { .. })
        ));
        // Keys are [-1, 0, 1]: glue fully at +1, half way between 0 and +1.
        m.apply(Command::SetGlueKeyform {
            name: "G".into(),
            key: vec![2],
            intensity: 1.0,
        })
        .unwrap();
        assert_eq!(intensity_at(&m, 1.0), 1.0);
        assert_eq!(intensity_at(&m, 0.5), 0.5);
        assert!(
            m.apply(Command::SetGlueKeyform {
                name: "G".into(),
                key: vec![9],
                intensity: 1.0,
            })
            .is_err()
        );
        // A key added to the glue's binding keeps the shape; unbinding drops the grid.
        m.apply(Command::AddParameterKey {
            parameter: "P".into(),
            value: 0.5,
            element: glue.clone(),
        })
        .unwrap();
        assert_eq!(intensity_at(&m, 0.5), 0.5);
        m.apply(Command::UnbindParameter {
            parameter: "P".into(),
            elements: vec![glue],
        })
        .unwrap();
        let id = m.glues().find("G").unwrap();
        assert!(m.glues().get(id).unwrap().keyforms.is_none());
        assert!(m.undo());
        assert_eq!(intensity_at(&m, 0.5), 0.5);
    }

    fn parameter_names(m: &Model) -> Vec<String> {
        m.parameters()
            .iter()
            .map(|(_, n, _)| n.to_owned())
            .collect()
    }

    #[test]
    fn a_parameter_moves_and_the_move_undoes_and_redoes() {
        let mut m = base();
        for n in ["Q", "R"] {
            m.apply(Command::AddParameter {
                name: n.into(),
                min: 0.0,
                max: 1.0,
                default: 0.0,
                repeat: false,
            })
            .unwrap();
        }
        m.apply(Command::MoveParameter {
            name: "P".into(),
            to: 99,
        })
        .unwrap();
        assert_eq!(parameter_names(&m), ["Q", "R", "P"]);
        assert_eq!(m.compile().unwrap().parameter_name(2), "P");
        assert!(m.undo());
        assert_eq!(parameter_names(&m), ["P", "Q", "R"]);
        assert!(m.redo());
        assert_eq!(parameter_names(&m), ["Q", "R", "P"]);
        assert!(
            m.apply(Command::MoveParameter {
                name: "X".into(),
                to: 0
            })
            .is_err()
        );
    }

    #[test]
    fn a_parameter_group_is_set_cleared_and_undone() {
        let mut m = base();
        let group = |m: &Model| {
            let id = m.parameters().find("P").unwrap();
            m.parameters().get(id).unwrap().group.clone()
        };
        m.apply(Command::SetParameterGroup {
            name: "P".into(),
            group: Some("Head".into()),
        })
        .unwrap();
        assert_eq!(group(&m).as_deref(), Some("Head"));
        m.apply(Command::SetParameterGroup {
            name: "P".into(),
            group: None,
        })
        .unwrap();
        assert_eq!(group(&m), None);
        assert!(m.undo());
        assert_eq!(group(&m).as_deref(), Some("Head"));
    }

    fn add_q(m: &mut Model) {
        m.apply(Command::AddParameter {
            name: "Q".into(),
            min: 0.0,
            max: 10.0,
            default: 5.0,
            repeat: false,
        })
        .unwrap();
    }

    fn keys_of_axis(m: &Model, mesh: &str) -> (String, Vec<f32>) {
        let id = m.meshes().find(mesh).unwrap();
        let g = m.meshes().get(id).unwrap().keyforms.as_ref().unwrap();
        let a = &m.bindings().get(g.binding).unwrap().axes()[0];
        (
            m.parameters().name(a.parameter).unwrap().to_owned(),
            a.keys.clone(),
        )
    }

    #[test]
    fn keyforms_move_to_another_parameter_with_the_keys_scaled() {
        let mut m = base();
        add_q(&mut m);
        bind(&mut m, vec![mesh_ref("A"), mesh_ref("B")]).unwrap();
        m.apply(Command::SetMeshKeyform {
            name: "A".into(),
            key: vec![2],
            positions: vec![[0.0, 0.0], [3.0, 0.0], [0.0, 3.0]],
            opacity: 1.0,
            draw_order: 500.0,
        })
        .unwrap();
        let before = kf_of(&m, "A");
        m.apply(Command::MoveParameterKeyforms {
            from: "P".into(),
            to: "Q".into(),
        })
        .unwrap();
        assert_eq!(
            keys_of_axis(&m, "A"),
            ("Q".to_owned(), vec![0.0, 5.0, 10.0])
        );
        assert_eq!(keys_of_axis(&m, "B").0, "Q");
        assert_eq!(kf_of(&m, "A").keyforms, before.keyforms);
        assert!(m.compile().is_ok());
        assert!(m.undo());
        assert_eq!(
            keys_of_axis(&m, "A"),
            ("P".to_owned(), vec![-1.0, 0.0, 1.0])
        );
        assert_eq!(kf_of(&m, "A"), before);
    }

    #[test]
    fn moving_keyforms_needs_a_source_and_refuses_an_element_on_both() {
        let mut m = base();
        add_q(&mut m);
        let go = |m: &mut Model| {
            m.apply(Command::MoveParameterKeyforms {
                from: "P".into(),
                to: "Q".into(),
            })
        };
        assert!(go(&mut m).is_err());
        bind(&mut m, vec![mesh_ref("A")]).unwrap();
        m.apply(Command::BindParameter {
            parameter: "Q".into(),
            elements: vec![mesh_ref("A")],
        })
        .unwrap();
        assert!(go(&mut m).is_err());
    }
}
