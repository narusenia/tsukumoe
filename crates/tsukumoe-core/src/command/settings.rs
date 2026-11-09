// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The commands that edit the model settings (M5-10). The history keeps the
//! whole settings before and after (`Change::Settings`).

use crate::elements::Parameter;
use crate::model::Model;
use crate::rig::ArtMesh;
use crate::settings::{HitArea, ModelInfo};

use super::{Change, Command, CommandError};

impl Model {
    pub(super) fn run_settings(
        &mut self,
        command: &Command,
        changes: &mut Vec<Change>,
    ) -> Result<(), CommandError> {
        let before = self.settings.clone();
        match command {
            Command::SetModelInfo {
                title,
                author,
                license,
                notes,
            } => {
                self.settings.info = ModelInfo {
                    title: title.clone(),
                    author: author.clone(),
                    license: license.clone(),
                    notes: notes.clone(),
                };
            }
            Command::AddHitArea { name, mesh } => {
                self.check_hit_area_name(name)?;
                let mesh = self.resolve::<ArtMesh>(mesh)?;
                self.settings.hit_areas.push(HitArea {
                    name: name.clone(),
                    mesh,
                });
            }
            Command::RemoveHitArea { name } => {
                let at = self.hit_area_at(name)?;
                self.settings.hit_areas.remove(at);
            }
            Command::RenameHitArea { name, new_name } => {
                let at = self.hit_area_at(name)?;
                if name != new_name {
                    self.check_hit_area_name(new_name)?;
                }
                self.settings.hit_areas[at].name = new_name.clone();
            }
            Command::SetHitAreaMesh { name, mesh } => {
                let at = self.hit_area_at(name)?;
                self.settings.hit_areas[at].mesh = self.resolve::<ArtMesh>(mesh)?;
            }
            Command::SetStandardParameter { role, parameter } => {
                let parameter = parameter
                    .as_deref()
                    .map(|n| self.resolve::<Parameter>(n))
                    .transpose()?;
                let standard = &mut self.settings.standard;
                standard.retain(|(r, _)| r != role);
                if let Some(parameter) = parameter {
                    standard.push((*role, parameter));
                }
            }
            _ => unreachable!("run_settings gets settings commands only"),
        }
        if self.settings != before {
            changes.push(Change::Settings(Box::new((before, self.settings.clone()))));
        }
        Ok(())
    }

    fn check_hit_area_name(&self, name: &str) -> Result<(), CommandError> {
        if name.is_empty() {
            return Err(CommandError::EmptyName { kind: "hit area" });
        }
        if self.settings.hit_area(name).is_some() {
            return Err(CommandError::AlreadyExists {
                kind: "hit area",
                name: name.to_owned(),
            });
        }
        Ok(())
    }

    fn hit_area_at(&self, name: &str) -> Result<usize, CommandError> {
        self.settings
            .hit_areas
            .iter()
            .position(|a| a.name == name)
            .ok_or_else(|| CommandError::NotFound {
                kind: "hit area",
                name: name.to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::StandardRole;

    fn model() -> Model {
        let mut m = Model::new();
        m.apply(Command::AddParameter {
            name: "ParamAngleX".into(),
            min: -30.0,
            max: 30.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        m.apply(Command::AddPart {
            name: "P".into(),
            parent: None,
        })
        .unwrap();
        for name in ["Face", "Body"] {
            m.apply(Command::AddMesh {
                name: name.into(),
                part: "P".into(),
                positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                indices: vec![0, 1, 2],
            })
            .unwrap();
        }
        m
    }

    fn area(name: &str, mesh: &str) -> Command {
        Command::AddHitArea {
            name: name.into(),
            mesh: mesh.into(),
        }
    }

    #[test]
    fn info_areas_and_roles_are_set_and_each_step_undoes() {
        let mut m = model();
        m.apply(Command::SetModelInfo {
            title: "T".into(),
            author: "A".into(),
            license: "L".into(),
            notes: "N".into(),
        })
        .unwrap();
        m.apply(area("Head", "Face")).unwrap();
        m.apply(Command::SetStandardParameter {
            role: StandardRole::AngleX,
            parameter: Some("ParamAngleX".into()),
        })
        .unwrap();
        let s = m.settings();
        assert_eq!(s.info.title, "T");
        assert_eq!(s.hit_areas.len(), 1);
        assert!(s.parameter(StandardRole::AngleX).is_some());
        m.apply(Command::SetHitAreaMesh {
            name: "Head".into(),
            mesh: "Body".into(),
        })
        .unwrap();
        m.apply(Command::RenameHitArea {
            name: "Head".into(),
            new_name: "Top".into(),
        })
        .unwrap();
        assert_eq!(m.settings().hit_areas[0].name, "Top");
        m.apply(Command::SetStandardParameter {
            role: StandardRole::AngleX,
            parameter: None,
        })
        .unwrap();
        assert!(m.settings().standard.is_empty());
        while m.undo() {}
        assert!(m.settings().is_empty());
        while m.redo() {}
        assert_eq!(m.settings().hit_areas[0].name, "Top");
        assert!(m.settings().standard.is_empty());
    }

    #[test]
    fn bad_names_and_targets_are_refused() {
        let mut m = model();
        m.apply(area("Head", "Face")).unwrap();
        assert!(m.apply(area("Head", "Body")).is_err(), "duplicate name");
        assert!(m.apply(area("", "Body")).is_err(), "empty name");
        assert!(m.apply(area("Other", "Nope")).is_err(), "no such mesh");
        assert!(
            m.apply(Command::RemoveHitArea { name: "X".into() })
                .is_err()
        );
        m.apply(area("Other", "Body")).unwrap();
        assert!(
            m.apply(Command::RenameHitArea {
                name: "Other".into(),
                new_name: "Head".into()
            })
            .is_err()
        );
        assert!(
            m.apply(Command::SetStandardParameter {
                role: StandardRole::AngleY,
                parameter: Some("Nope".into())
            })
            .is_err()
        );
    }

    #[test]
    fn a_parameter_or_mesh_the_settings_name_cannot_be_removed() {
        let mut m = model();
        m.apply(area("Head", "Face")).unwrap();
        m.apply(Command::SetStandardParameter {
            role: StandardRole::AngleX,
            parameter: Some("ParamAngleX".into()),
        })
        .unwrap();
        assert!(
            m.apply(Command::RemoveMesh {
                name: "Face".into()
            })
            .is_err()
        );
        assert!(
            m.apply(Command::RemoveParameter {
                name: "ParamAngleX".into()
            })
            .is_err()
        );
        m.apply(Command::RemoveHitArea {
            name: "Head".into(),
        })
        .unwrap();
        m.apply(Command::RemoveMesh {
            name: "Face".into(),
        })
        .unwrap();
    }

    #[test]
    fn setting_a_role_again_replaces_it() {
        let mut m = model();
        m.apply(Command::AddParameter {
            name: "Other".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        for p in ["ParamAngleX", "Other"] {
            m.apply(Command::SetStandardParameter {
                role: StandardRole::AngleX,
                parameter: Some(p.into()),
            })
            .unwrap();
        }
        assert_eq!(m.settings().standard.len(), 1);
        let id = m.parameters().find("Other").unwrap();
        assert_eq!(m.settings().parameter(StandardRole::AngleX), Some(id));
    }
}
