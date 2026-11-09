// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The commands that make and change bones and the bone weights of a mesh
//! (M5-05a). A bone is local to its parent; a root is local to the parent
//! deformer of the meshes it skins (ADR-0021).

use crate::model::Model;
use crate::rig::{ArtMesh, Bone, BoneId, VertexInfluences};

use super::keyform::refused;
use super::{Change, Command, CommandError};

/// Most bones that drive one vertex (REQ-CORE-003).
const MAX_BONES: usize = 4;

impl Model {
    fn bone_parent(&self, parent: Option<&str>) -> Result<Option<BoneId>, CommandError> {
        parent.map(|n| self.resolve::<Bone>(n)).transpose()
    }

    pub(super) fn run_bone(
        &mut self,
        command: &Command,
        changes: &mut Vec<Change>,
    ) -> Result<(), CommandError> {
        match command {
            Command::AddBone { name, parent, base } => {
                if !base.valid() {
                    return Err(refused(name, "bad-range"));
                }
                let parent = self.bone_parent(parent.as_deref())?;
                let bone = Bone {
                    parent,
                    base: base.transform(),
                    keyforms: None,
                };
                self.insert(changes, name, bone)?;
            }
            Command::RemoveBone { name } => {
                let id = self.resolve::<Bone>(name)?;
                self.remove(changes, id);
            }
            Command::RenameBone { name, new_name } => {
                let id = self.resolve::<Bone>(name)?;
                self.rename(changes, id, new_name)?;
            }
            Command::SetBoneParent { name, parent } => {
                let id = self.resolve::<Bone>(name)?;
                let parent = self.bone_parent(parent.as_deref())?;
                self.modify(changes, id, |b| b.parent = parent);
            }
            Command::SetBoneBase { name, base } => {
                let id = self.resolve::<Bone>(name)?;
                if !base.valid() {
                    return Err(refused(name, "bad-range"));
                }
                if self.bones.get(id).is_some_and(|b| b.keyforms.is_some()) {
                    return Err(CommandError::HasKeyforms {
                        kind: "bone",
                        name: name.clone(),
                    });
                }
                self.modify(changes, id, |b| b.base = base.transform());
            }
            Command::SetMeshInfluences { name, influences } => {
                let id = self.resolve::<ArtMesh>(name)?;
                let n = self.meshes.get(id).map_or(0, |m| m.positions.len());
                if !influences.is_empty() && influences.len() != n {
                    return Err(refused(name, "bad-count"));
                }
                let mut built = Vec::with_capacity(influences.len());
                for vertex in influences {
                    if vertex.len() > MAX_BONES {
                        return Err(refused(name, "too-many-bones"));
                    }
                    let mut inf = VertexInfluences::default();
                    for (slot, w) in vertex.iter().enumerate() {
                        if !w.weight.is_finite() || w.weight < 0.0 {
                            return Err(refused(name, "bad-range"));
                        }
                        let bone = self.resolve::<Bone>(&w.bone)?;
                        if inf.bones[..slot].contains(&Some(bone)) {
                            return Err(refused(name, "duplicate-bone"));
                        }
                        inf.bones[slot] = Some(bone);
                        inf.weights[slot] = w.weight;
                    }
                    built.push(inf);
                }
                self.modify(changes, id, |m| m.influences = built);
            }
            _ => unreachable!("run_bone gets bone commands only"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{BoneBase, BoneWeight, ElementRef};
    use crate::eval::Frame;

    fn base(x: f32, y: f32) -> BoneBase {
        BoneBase {
            translation: [x, y],
            rotation_degrees: 0.0,
            scale: [1.0, 1.0],
        }
    }

    fn add(name: &str, parent: Option<&str>, at: [f32; 2]) -> Command {
        Command::AddBone {
            name: name.into(),
            parent: parent.map(Into::into),
            base: base(at[0], at[1]),
        }
    }

    fn model() -> Model {
        let mut m = Model::new();
        let steps = [
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
            Command::AddMesh {
                name: "Skin".into(),
                part: "Part".into(),
                positions: vec![[1.0, 0.0], [2.0, 0.0], [1.0, 1.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                indices: vec![0, 1, 2],
            },
            add("Root", None, [0.0, 0.0]),
            add("Arm", Some("Root"), [1.0, 0.0]),
        ];
        for step in steps {
            m.apply(step).unwrap();
        }
        m
    }

    fn weights(bone: &str, weight: f32, vertices: usize) -> Vec<Vec<BoneWeight>> {
        vec![
            vec![BoneWeight {
                bone: bone.into(),
                weight
            }];
            vertices
        ]
    }

    #[test]
    fn bones_are_added_renamed_reparented_removed_and_each_step_undoes() {
        let mut m = model();
        assert_eq!(m.bones().len(), 2);
        m.apply(Command::RenameBone {
            name: "Arm".into(),
            new_name: "Hand".into(),
        })
        .unwrap();
        m.apply(Command::SetBoneParent {
            name: "Hand".into(),
            parent: None,
        })
        .unwrap();
        let hand = m.bones().find("Hand").unwrap();
        assert_eq!(m.bones().get(hand).unwrap().parent, None);
        m.apply(Command::SetBoneBase {
            name: "Hand".into(),
            base: BoneBase {
                translation: [3.0, 1.0],
                rotation_degrees: 90.0,
                scale: [2.0, 2.0],
            },
        })
        .unwrap();
        m.apply(Command::RemoveBone {
            name: "Hand".into(),
        })
        .unwrap();
        assert_eq!(m.bones().len(), 1);
        for _ in 0..4 {
            assert!(m.undo());
        }
        assert_eq!(m.bones().len(), 2);
        assert!(m.bones().find("Arm").is_some());
    }

    #[test]
    fn bad_bones_are_refused() {
        let mut m = model();
        assert!(m.apply(add("Root", None, [0.0, 0.0])).is_err(), "duplicate");
        assert!(m.apply(add("", None, [0.0, 0.0])).is_err(), "empty");
        assert!(m.apply(add("X", Some("Nope"), [0.0, 0.0])).is_err());
        assert!(m.apply(add("X", None, [f32::NAN, 0.0])).is_err());
        let flat = Command::AddBone {
            name: "Flat".into(),
            parent: None,
            base: BoneBase {
                translation: [0.0, 0.0],
                rotation_degrees: 0.0,
                scale: [0.0, 1.0],
            },
        };
        assert!(m.apply(flat).is_err(), "a scale that cannot be inverted");
        // A loop of parents.
        assert!(
            m.apply(Command::SetBoneParent {
                name: "Root".into(),
                parent: Some("Arm".into())
            })
            .is_err()
        );
        // A bone with a child cannot go.
        assert!(
            m.apply(Command::RemoveBone {
                name: "Root".into()
            })
            .is_err()
        );
    }

    #[test]
    fn weights_are_set_checked_and_hold_the_bones_and_the_mesh_in_place() {
        let mut m = model();
        let set = |bone: &str, w: f32, n: usize| Command::SetMeshInfluences {
            name: "Skin".into(),
            influences: weights(bone, w, n),
        };
        assert!(m.apply(set("Root", 1.0, 2)).is_err(), "one per vertex");
        assert!(m.apply(set("Nope", 1.0, 3)).is_err());
        assert!(m.apply(set("Root", -1.0, 3)).is_err());
        assert!(m.apply(set("Root", f32::NAN, 3)).is_err());
        let five = vec![
            (0..5)
                .map(|i| BoneWeight {
                    bone: if i % 2 == 0 {
                        "Root".into()
                    } else {
                        "Arm".into()
                    },
                    weight: 0.2,
                })
                .collect::<Vec<_>>();
            3
        ];
        assert!(
            m.apply(Command::SetMeshInfluences {
                name: "Skin".into(),
                influences: five
            })
            .is_err(),
            "at most four bones"
        );
        let twice = vec![
            vec![
                BoneWeight {
                    bone: "Root".into(),
                    weight: 0.5
                },
                BoneWeight {
                    bone: "Root".into(),
                    weight: 0.5
                },
            ];
            3
        ];
        assert!(
            m.apply(Command::SetMeshInfluences {
                name: "Skin".into(),
                influences: twice
            })
            .is_err(),
            "one share per bone"
        );
        m.apply(set("Arm", 1.0, 3)).unwrap();
        assert!(m.apply(Command::RemoveBone { name: "Arm".into() }).is_err());
        assert!(m.undo());
        m.apply(set("Arm", 1.0, 3)).unwrap();
        m.apply(Command::SetMeshInfluences {
            name: "Skin".into(),
            influences: vec![],
        })
        .unwrap();
        m.apply(Command::RemoveBone { name: "Arm".into() }).unwrap();
    }

    #[test]
    fn a_bone_tree_skins_meshes_of_one_parent_only() {
        let mut m = model();
        m.apply(Command::AddWarpDeformer {
            name: "Warp".into(),
            part: "Part".into(),
            parent: None,
            rows: 1,
            columns: 1,
            base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        })
        .unwrap();
        m.apply(Command::AddMesh {
            name: "Other".into(),
            part: "Part".into(),
            positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
        })
        .unwrap();
        m.apply(Command::SetMeshParent {
            name: "Other".into(),
            parent: Some("Warp".into()),
        })
        .unwrap();
        m.apply(Command::SetMeshInfluences {
            name: "Skin".into(),
            influences: weights("Root", 1.0, 3),
        })
        .unwrap();
        assert!(
            m.apply(Command::SetMeshInfluences {
                name: "Other".into(),
                influences: weights("Arm", 1.0, 3),
            })
            .is_err(),
            "the same tree under a different parent"
        );
    }

    #[test]
    fn a_driven_bone_moves_the_skinned_vertices() {
        let mut m = model();
        let bone = ElementRef::Bone {
            name: "Root".into(),
        };
        m.apply(Command::SetMeshInfluences {
            name: "Skin".into(),
            influences: weights("Root", 1.0, 3),
        })
        .unwrap();
        m.apply(Command::BindParameter {
            parameter: "P".into(),
            elements: vec![bone],
        })
        .unwrap();
        // Keys are [-1, 0, 1]: at +1 the root moves 2 to the right.
        m.apply(Command::SetBoneKeyform {
            name: "Root".into(),
            key: vec![2],
            base: base(2.0, 0.0),
        })
        .unwrap();
        assert!(
            m.apply(Command::SetBoneBase {
                name: "Root".into(),
                base: base(1.0, 1.0)
            })
            .is_err(),
            "a bone with keyforms is edited at its keys"
        );
        assert!(
            m.apply(Command::SetBoneKeyform {
                name: "Root".into(),
                key: vec![9],
                base: base(0.0, 0.0)
            })
            .is_err()
        );
        let c = m.compile().unwrap();
        let mut f = Frame::new(&c);
        let x0 = |f: &Frame| f.positions()[0];
        f.set_parameter(0, 0.0);
        f.evaluate(&c);
        let at_rest = x0(&f);
        f.set_parameter(0, 1.0);
        f.evaluate(&c);
        assert!((x0(&f) - at_rest - 2.0).abs() < 1e-4);
        // Unbinding puts the shape at the default back as the base.
        m.apply(Command::UnbindParameter {
            parameter: "P".into(),
            elements: vec![ElementRef::Bone {
                name: "Root".into(),
            }],
        })
        .unwrap();
        let id = m.bones().find("Root").unwrap();
        assert!(m.bones().get(id).unwrap().keyforms.is_none());
    }
}
