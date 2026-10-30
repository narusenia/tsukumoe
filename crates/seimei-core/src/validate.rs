//! Integrity checks for a [`Model`]: dangling references, key ranges, grid
//! sizes, mesh geometry and parent cycles.

use crate::keyform::BindingId;
use crate::model::Model;
use crate::physics::{PhysicsInput, PhysicsOutput};
use crate::rig::{BlendTarget, DeformerRef};

#[derive(Debug, Clone, PartialEq)]
pub enum ModelError {
    /// `field` of `owner` refers to an element that no longer exists.
    DanglingReference {
        owner: String,
        field: &'static str,
    },
    InvalidParameterRange {
        owner: String,
    },
    KeyOutOfParameterRange {
        owner: String,
    },
    KeyformCountMismatch {
        owner: String,
        expected: usize,
        actual: usize,
    },
    GeometryMismatch {
        owner: String,
        field: &'static str,
    },
    Cycle {
        owner: String,
    },
}

#[derive(Default)]
struct Report(Vec<ModelError>);

impl Report {
    fn dangling(&mut self, owner: &str, field: &'static str) {
        self.0.push(ModelError::DanglingReference {
            owner: owner.to_owned(),
            field,
        });
    }

    fn geometry(&mut self, owner: &str, field: &'static str) {
        self.0.push(ModelError::GeometryMismatch {
            owner: owner.to_owned(),
            field,
        });
    }
}

/// Follows `parent` links from `start`; true if they loop.
fn has_cycle<K: Copy + PartialEq>(start: K, limit: usize, parent: impl Fn(K) -> Option<K>) -> bool {
    let mut current = start;
    for _ in 0..=limit {
        match parent(current) {
            None => return false,
            Some(next) if next == start => return true,
            Some(next) => current = next,
        }
    }
    false
}

impl Model {
    /// Every problem found; empty when the model is consistent.
    pub fn validate(&self) -> Vec<ModelError> {
        let mut r = Report::default();
        self.check_parameters(&mut r);
        self.check_bindings(&mut r);
        self.check_parts(&mut r);
        self.check_meshes(&mut r);
        self.check_deformers(&mut r);
        self.check_glues(&mut r);
        self.check_bones(&mut r);
        self.check_blend_shapes(&mut r);
        self.check_drivers(&mut r);
        self.check_physics(&mut r);
        self.check_playback(&mut r);
        r.0
    }

    fn deformer_exists(&self, deformer: DeformerRef) -> bool {
        match deformer {
            DeformerRef::Warp(id) => self.warps.contains(id),
            DeformerRef::Rotation(id) => self.rotations.contains(id),
        }
    }

    fn deformer_parent(&self, deformer: DeformerRef) -> Option<DeformerRef> {
        match deformer {
            DeformerRef::Warp(id) => self.warps.get(id).and_then(|d| d.parent),
            DeformerRef::Rotation(id) => self.rotations.get(id).and_then(|d| d.parent),
        }
    }

    fn check_grid_size(&self, r: &mut Report, owner: &str, binding: BindingId, actual: usize) {
        match self.bindings.get(binding) {
            None => r.dangling(owner, "keyforms.binding"),
            Some(b) if b.keyform_count() != actual => {
                r.0.push(ModelError::KeyformCountMismatch {
                    owner: owner.to_owned(),
                    expected: b.keyform_count(),
                    actual,
                });
            }
            Some(_) => {}
        }
    }

    fn check_parameters(&self, r: &mut Report) {
        for (_, name, p) in self.parameters.iter() {
            let finite = p.min.is_finite() && p.max.is_finite() && p.default.is_finite();
            if !(finite && p.min < p.max && p.min <= p.default && p.default <= p.max) {
                r.0.push(ModelError::InvalidParameterRange {
                    owner: format!("parameter '{name}'"),
                });
            }
        }
    }

    fn check_bindings(&self, r: &mut Report) {
        for (id, binding) in self.bindings.iter() {
            let owner = format!("binding {id:?}");
            for axis in binding.axes() {
                match self.parameters.get(axis.parameter) {
                    None => r.dangling(&owner, "axis.parameter"),
                    Some(p) => {
                        if axis.keys.iter().any(|&k| k < p.min || k > p.max) {
                            r.0.push(ModelError::KeyOutOfParameterRange {
                                owner: owner.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    fn check_parts(&self, r: &mut Report) {
        for (id, name, part) in self.parts.iter() {
            let owner = format!("part '{name}'");
            if let Some(parent) = part.parent
                && !self.parts.contains(parent)
            {
                r.dangling(&owner, "parent");
            }
            let parent_of = |p| self.parts.get(p).and_then(|part| part.parent);
            if has_cycle(id, self.parts.len(), parent_of) {
                r.0.push(ModelError::Cycle { owner });
            }
        }
    }

    fn check_meshes(&self, r: &mut Report) {
        for (_, name, mesh) in self.meshes.iter() {
            let owner = format!("mesh '{name}'");
            if !self.parts.contains(mesh.part) {
                r.dangling(&owner, "part");
            }
            if let Some(parent) = mesh.parent
                && !self.deformer_exists(parent)
            {
                r.dangling(&owner, "parent");
            }
            if mesh.masks.iter().any(|&m| !self.meshes.contains(m)) {
                r.dangling(&owner, "masks");
            }
            let n = mesh.positions.len();
            if mesh.uvs.len() != n {
                r.geometry(&owner, "uvs");
            }
            if mesh.indices.len() % 3 != 0 || mesh.indices.iter().any(|&i| i as usize >= n) {
                r.geometry(&owner, "indices");
            }
            if !mesh.influences.is_empty() {
                if mesh.influences.len() != n {
                    r.geometry(&owner, "influences");
                }
                let missing = mesh
                    .influences
                    .iter()
                    .flat_map(|inf| inf.bones.iter().flatten())
                    .any(|&b| !self.bones.contains(b));
                if missing {
                    r.dangling(&owner, "influences");
                }
            }
            if let Some(grid) = &mesh.keyforms {
                self.check_grid_size(r, &owner, grid.binding, grid.keyforms.len());
                if grid.keyforms.iter().any(|k| k.positions.len() != n) {
                    r.geometry(&owner, "keyforms.positions");
                }
            }
        }
    }

    fn check_deformers(&self, r: &mut Report) {
        let total = self.warps.len() + self.rotations.len();
        let parent_of = |d| self.deformer_parent(d);

        for (id, name, warp) in self.warps.iter() {
            let owner = format!("warp deformer '{name}'");
            if !self.parts.contains(warp.part) {
                r.dangling(&owner, "part");
            }
            if let Some(parent) = warp.parent
                && !self.deformer_exists(parent)
            {
                r.dangling(&owner, "parent");
            }
            if has_cycle(DeformerRef::Warp(id), total, parent_of) {
                r.0.push(ModelError::Cycle {
                    owner: owner.clone(),
                });
            }
            let points = (warp.rows as usize + 1) * (warp.columns as usize + 1);
            if warp.rows == 0 || warp.columns == 0 || warp.base_points.len() != points {
                r.geometry(&owner, "base_points");
            }
            if let Some(grid) = &warp.keyforms {
                self.check_grid_size(r, &owner, grid.binding, grid.keyforms.len());
                if grid.keyforms.iter().any(|k| k.points.len() != points) {
                    r.geometry(&owner, "keyforms.points");
                }
            }
        }

        for (id, name, rotation) in self.rotations.iter() {
            let owner = format!("rotation deformer '{name}'");
            if !self.parts.contains(rotation.part) {
                r.dangling(&owner, "part");
            }
            if let Some(parent) = rotation.parent
                && !self.deformer_exists(parent)
            {
                r.dangling(&owner, "parent");
            }
            if has_cycle(DeformerRef::Rotation(id), total, parent_of) {
                r.0.push(ModelError::Cycle {
                    owner: owner.clone(),
                });
            }
            if let Some(grid) = &rotation.keyforms {
                self.check_grid_size(r, &owner, grid.binding, grid.keyforms.len());
            }
        }
    }

    fn check_glues(&self, r: &mut Report) {
        for (_, name, glue) in self.glues.iter() {
            let owner = format!("glue '{name}'");
            let (a, b) = (self.meshes.get(glue.mesh_a), self.meshes.get(glue.mesh_b));
            if a.is_none() {
                r.dangling(&owner, "mesh_a");
            }
            if b.is_none() {
                r.dangling(&owner, "mesh_b");
            }
            if let (Some(a), Some(b)) = (a, b) {
                let in_range = glue.nodes.iter().all(|n| {
                    (n.index_a as usize) < a.positions.len()
                        && (n.index_b as usize) < b.positions.len()
                        && (0.0..=1.0).contains(&n.weight)
                });
                if !in_range {
                    r.geometry(&owner, "nodes");
                }
            }
            if let Some(grid) = &glue.keyforms {
                self.check_grid_size(r, &owner, grid.binding, grid.keyforms.len());
            }
        }
    }

    fn check_bones(&self, r: &mut Report) {
        for (id, name, bone) in self.bones.iter() {
            let owner = format!("bone '{name}'");
            if let Some(parent) = bone.parent
                && !self.bones.contains(parent)
            {
                r.dangling(&owner, "parent");
            }
            let parent_of = |b| self.bones.get(b).and_then(|bone| bone.parent);
            if has_cycle(id, self.bones.len(), parent_of) {
                r.0.push(ModelError::Cycle {
                    owner: owner.clone(),
                });
            }
            if let Some(grid) = &bone.keyforms {
                self.check_grid_size(r, &owner, grid.binding, grid.keyforms.len());
            }
        }
    }

    fn check_blend_shapes(&self, r: &mut Report) {
        for (_, name, shape) in self.blend_shapes.iter() {
            let owner = format!("blend shape '{name}'");
            if !self.parameters.contains(shape.parameter) {
                r.dangling(&owner, "parameter");
            }
            if shape.keys.len() != shape.weights.len() {
                r.geometry(&owner, "keys");
            }
            let target_len = match shape.target {
                BlendTarget::Mesh(id) => self.meshes.get(id).map(|m| m.positions.len()),
                BlendTarget::Warp(id) => self.warps.get(id).map(|w| w.base_points.len()),
            };
            match target_len {
                None => r.dangling(&owner, "target"),
                Some(n) if n != shape.deltas.len() => r.geometry(&owner, "deltas"),
                Some(_) => {}
            }
        }
    }

    fn check_drivers(&self, r: &mut Report) {
        for (_, name, driver) in self.drivers.iter() {
            if !self.parameters.contains(driver.output) {
                r.dangling(&format!("driver '{name}'"), "output");
            }
        }
    }

    fn check_physics_io(
        &self,
        r: &mut Report,
        owner: &str,
        inputs: &[PhysicsInput],
        outputs: &[PhysicsOutput],
        particles: usize,
    ) {
        if inputs
            .iter()
            .any(|i| !self.parameters.contains(i.parameter))
        {
            r.dangling(owner, "inputs");
        }
        if outputs
            .iter()
            .any(|o| !self.parameters.contains(o.parameter))
        {
            r.dangling(owner, "outputs");
        }
        if outputs.iter().any(|o| o.particle >= particles) {
            r.geometry(owner, "outputs.particle");
        }
    }

    fn check_physics(&self, r: &mut Report) {
        for group in &self.physics.pendulums {
            let owner = format!("pendulum '{}'", group.name);
            self.check_physics_io(
                r,
                &owner,
                &group.inputs,
                &group.outputs,
                group.particles.len(),
            );
        }
        for chain in &self.physics.chains {
            let owner = format!("physics chain '{}'", chain.name);
            self.check_physics_io(r, &owner, &chain.inputs, &chain.outputs, chain.joints.len());
        }
    }

    fn check_playback(&self, r: &mut Report) {
        use crate::playback::CurveTarget;

        for (_, name, motion) in self.motions.iter() {
            let owner = format!("motion '{name}'");
            let missing = motion.curves.iter().any(|c| match c.target {
                CurveTarget::Parameter(id) => !self.parameters.contains(id),
                CurveTarget::PartOpacity(id) => !self.parts.contains(id),
            });
            if missing {
                r.dangling(&owner, "curves");
            }
        }
        for (_, name, expression) in self.expressions.iter() {
            if expression
                .parameters
                .iter()
                .any(|p| !self.parameters.contains(p.parameter))
            {
                r.dangling(&format!("expression '{name}'"), "parameters");
            }
        }
        if let Some(pose) = &self.pose {
            let missing = pose.groups.iter().flatten().any(|e| {
                !self.parts.contains(e.part) || e.linked.iter().any(|&p| !self.parts.contains(p))
            });
            if missing {
                r.dangling("pose", "groups");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::Id;
    use crate::driver::Driver;
    use crate::elements::{Parameter, ParameterId, Part, PartId};
    use crate::keyform::{Axis, Binding, KeyformGrid};
    use crate::rig::{
        ArtMesh, BlendShape, BlendTarget, Bone, BoneTransform, Glue, GlueNode, MeshKeyform,
        RotationDeformer, RotationKeyform, WarpDeformer,
    };

    fn base() -> (Model, ParameterId, PartId) {
        let mut m = Model::new();
        let p = m
            .parameters
            .insert("ParamAngleX", Parameter::new(-30.0, 30.0, 0.0))
            .unwrap();
        let part = m.parts.insert("Face", Part::default()).unwrap();
        (m, p, part)
    }

    fn triangle(part: PartId) -> ArtMesh {
        let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        ArtMesh::new(part, pts.clone(), pts, vec![0, 1, 2])
    }

    fn mesh_keyform() -> MeshKeyform {
        MeshKeyform {
            positions: vec![[0.0; 2]; 3],
            opacity: 1.0,
            draw_order: 500.0,
            multiply_color: [1.0; 3],
            screen_color: [0.0; 3],
        }
    }

    fn three_keys(m: &mut Model, p: ParameterId) -> crate::keyform::BindingId {
        let axis = Axis {
            parameter: p,
            keys: vec![-30.0, 0.0, 30.0],
        };
        m.bindings.insert(Binding::new(vec![axis]).unwrap())
    }

    fn has(errors: &[ModelError], expected: ModelError) -> bool {
        errors.contains(&expected)
    }

    #[test]
    fn consistent_model_has_no_errors_and_shares_a_binding() {
        let (mut m, p, part) = base();
        let binding = three_keys(&mut m, p);

        let mut left = triangle(part);
        left.keyforms = Some(KeyformGrid::new(binding, vec![mesh_keyform(); 3]));
        let mut right = triangle(part);
        right.keyforms = Some(KeyformGrid::new(binding, vec![mesh_keyform(); 3]));
        let left = m.meshes.insert("Left", left).unwrap();
        let right = m.meshes.insert("Right", right).unwrap();

        let rot = m
            .rotations
            .insert(
                "Rot",
                RotationDeformer {
                    part,
                    parent: None,
                    base: RotationKeyform::default(),
                    keyforms: None,
                },
            )
            .unwrap();
        let warp = m
            .warps
            .insert(
                "Warp",
                WarpDeformer {
                    part,
                    parent: Some(DeformerRef::Rotation(rot)),
                    rows: 1,
                    columns: 1,
                    bezier: false,
                    base_points: vec![[0.0; 2]; 4],
                    keyforms: None,
                },
            )
            .unwrap();
        m.meshes.get_mut(left).unwrap().parent = Some(DeformerRef::Warp(warp));
        m.meshes.get_mut(right).unwrap().masks.push(left);
        let bone = m
            .bones
            .insert(
                "Root",
                Bone {
                    parent: None,
                    base: BoneTransform::default(),
                    keyforms: None,
                },
            )
            .unwrap();
        m.bones
            .insert(
                "Child",
                Bone {
                    parent: Some(bone),
                    base: BoneTransform::default(),
                    keyforms: None,
                },
            )
            .unwrap();
        m.glues
            .insert(
                "Glue",
                Glue {
                    mesh_a: left,
                    mesh_b: right,
                    nodes: vec![GlueNode {
                        index_a: 0,
                        index_b: 1,
                        weight: 0.5,
                    }],
                    intensity: 1.0,
                    keyforms: None,
                },
            )
            .unwrap();
        m.blend_shapes
            .insert(
                "Shape",
                BlendShape {
                    target: BlendTarget::Mesh(left),
                    parameter: p,
                    keys: vec![0.0, 30.0],
                    weights: vec![0.0, 1.0],
                    deltas: vec![[0.1, 0.0]; 3],
                },
            )
            .unwrap();
        m.drivers
            .insert(
                "Drive",
                Driver {
                    output: p,
                    expression: "0".into(),
                },
            )
            .unwrap();

        assert_eq!(m.validate(), vec![]);
    }

    #[test]
    fn removed_part_is_reported_as_dangling() {
        let (mut m, _, part) = base();
        m.meshes.insert("Mesh", triangle(part)).unwrap();
        m.parts.remove(part);
        assert!(has(
            &m.validate(),
            ModelError::DanglingReference {
                owner: "mesh 'Mesh'".into(),
                field: "part"
            }
        ));
    }

    #[test]
    fn removed_parameter_breaks_bindings_and_drivers() {
        let (mut m, p, _) = base();
        three_keys(&mut m, p);
        m.drivers
            .insert(
                "Drive",
                Driver {
                    output: p,
                    expression: String::new(),
                },
            )
            .unwrap();
        m.parameters.remove(p);
        let errors = m.validate();
        assert!(errors.iter().any(|e| matches!(
            e,
            ModelError::DanglingReference {
                field: "axis.parameter",
                ..
            }
        )));
        assert!(has(
            &errors,
            ModelError::DanglingReference {
                owner: "driver 'Drive'".into(),
                field: "output"
            }
        ));
    }

    #[test]
    fn keyform_count_must_match_the_binding() {
        let (mut m, p, part) = base();
        let binding = three_keys(&mut m, p);
        let mut mesh = triangle(part);
        mesh.keyforms = Some(KeyformGrid::new(binding, vec![mesh_keyform(); 2]));
        m.meshes.insert("Mesh", mesh).unwrap();
        assert!(has(
            &m.validate(),
            ModelError::KeyformCountMismatch {
                owner: "mesh 'Mesh'".into(),
                expected: 3,
                actual: 2
            }
        ));
    }

    #[test]
    fn keys_must_lie_within_the_parameter_range() {
        let (mut m, p, _) = base();
        let axis = Axis {
            parameter: p,
            keys: vec![0.0, 60.0],
        };
        m.bindings.insert(Binding::new(vec![axis]).unwrap());
        assert!(
            m.validate()
                .iter()
                .any(|e| matches!(e, ModelError::KeyOutOfParameterRange { .. }))
        );
    }

    #[test]
    fn invalid_parameter_range_is_reported() {
        let (mut m, _, _) = base();
        m.parameters
            .insert("Bad", Parameter::new(1.0, 0.0, 0.5))
            .unwrap();
        assert!(has(
            &m.validate(),
            ModelError::InvalidParameterRange {
                owner: "parameter 'Bad'".into()
            }
        ));
    }

    #[test]
    fn mesh_geometry_is_checked() {
        let (mut m, _, part) = base();
        let mut mesh = triangle(part);
        mesh.uvs.pop();
        mesh.indices = vec![0, 1, 3];
        m.meshes.insert("Mesh", mesh).unwrap();
        let errors = m.validate();
        for field in ["uvs", "indices"] {
            assert!(has(
                &errors,
                ModelError::GeometryMismatch {
                    owner: "mesh 'Mesh'".into(),
                    field
                }
            ));
        }
    }

    #[test]
    fn parent_cycles_are_reported() {
        let (mut m, _, part) = base();
        let rot = |parent| RotationDeformer {
            part,
            parent,
            base: RotationKeyform::default(),
            keyforms: None,
        };
        let a = m.rotations.insert("A", rot(None)).unwrap();
        let b = m
            .rotations
            .insert("B", rot(Some(DeformerRef::Rotation(a))))
            .unwrap();
        m.rotations.get_mut(a).unwrap().parent = Some(DeformerRef::Rotation(b));
        assert!(has(
            &m.validate(),
            ModelError::Cycle {
                owner: "rotation deformer 'A'".into()
            }
        ));

        let (mut m, _, _) = base();
        let bone = |parent| Bone {
            parent,
            base: BoneTransform::default(),
            keyforms: None,
        };
        let root = m.bones.insert("Root", bone(None)).unwrap();
        m.bones.get_mut(root).unwrap().parent = Some(root);
        assert!(has(
            &m.validate(),
            ModelError::Cycle {
                owner: "bone 'Root'".into()
            }
        ));
    }

    #[test]
    fn warp_and_glue_and_blend_shape_sizes_are_checked() {
        let (mut m, p, part) = base();
        m.warps
            .insert(
                "Warp",
                WarpDeformer {
                    part,
                    parent: None,
                    rows: 2,
                    columns: 2,
                    bezier: true,
                    base_points: vec![[0.0; 2]; 4],
                    keyforms: None,
                },
            )
            .unwrap();
        let a = m.meshes.insert("A", triangle(part)).unwrap();
        let b = m.meshes.insert("B", triangle(part)).unwrap();
        m.glues
            .insert(
                "Glue",
                Glue {
                    mesh_a: a,
                    mesh_b: b,
                    nodes: vec![GlueNode {
                        index_a: 5,
                        index_b: 0,
                        weight: 0.5,
                    }],
                    intensity: 1.0,
                    keyforms: None,
                },
            )
            .unwrap();
        m.blend_shapes
            .insert(
                "Shape",
                BlendShape {
                    target: BlendTarget::Mesh(a),
                    parameter: p,
                    keys: vec![0.0],
                    weights: vec![],
                    deltas: vec![[0.0; 2]; 2],
                },
            )
            .unwrap();
        let errors = m.validate();
        for (owner, field) in [
            ("warp deformer 'Warp'", "base_points"),
            ("glue 'Glue'", "nodes"),
            ("blend shape 'Shape'", "keys"),
            ("blend shape 'Shape'", "deltas"),
        ] {
            assert!(
                has(
                    &errors,
                    ModelError::GeometryMismatch {
                        owner: owner.into(),
                        field
                    }
                ),
                "{owner} {field}"
            );
        }
    }

    #[test]
    fn removed_mask_mesh_is_reported_as_dangling() {
        let (mut m, _, part) = base();
        let ghost: Id<ArtMesh> = m.meshes.insert("Tmp", triangle(part)).unwrap();
        m.meshes.remove(ghost);
        let mut mesh = triangle(part);
        mesh.masks.push(ghost);
        m.meshes.insert("Mesh", mesh).unwrap();
        assert!(has(
            &m.validate(),
            ModelError::DanglingReference {
                owner: "mesh 'Mesh'".into(),
                field: "masks"
            }
        ));
    }
}
