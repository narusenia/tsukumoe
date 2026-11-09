// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The model settings panel: the standard parameters, the hit areas and the
//! model's information (M5-10). A finished edit of one field is one command.

use std::collections::HashMap;

use egui::Ui;
use tsukumoe_core::command::Command;
use tsukumoe_core::model::Model;
use tsukumoe_core::settings::StandardRole;
use tsukumoe_ui::{
    Icon, Variant, button, form_row, icon_button, section_header, select, text_input, tr,
};

use crate::view::ViewState;

/// What the panel remembers between frames. Not saved, not in the history.
#[derive(Clone, Debug, Default)]
pub struct State {
    /// Text being typed, by field. A field takes the model's value again when
    /// it is not being edited.
    drafts: HashMap<String, String>,
}

pub enum Out {
    Command(Command, String),
}

/// The key of the text that names `role` in the language files.
fn role_key(role: StandardRole) -> String {
    format!("model-settings-role-{}", role.key())
}

/// A name for a new hit area that no hit area has.
pub fn unique_area_name(model: &Model) -> String {
    (1..)
        .map(|n| format!("Area{n}"))
        .find(|name| model.settings().hit_area(name).is_none())
        .expect("endless range")
}

/// The commands that give each role the parameter with its standard name, for
/// the roles whose standard-named parameter exists and is not assigned yet.
pub fn assign_by_name(model: &Model) -> Vec<Command> {
    StandardRole::ALL
        .into_iter()
        .filter_map(|role| {
            let id = model.parameters().find(role.standard_name())?;
            (model.settings().parameter(role) != Some(id)).then(|| Command::SetStandardParameter {
                role,
                parameter: Some(role.standard_name().to_owned()),
            })
        })
        .collect()
}

/// A text field that gives back its text once the edit ends with a change.
/// Escape drops the edit.
pub(super) fn text_field(
    ui: &mut Ui,
    drafts: &mut HashMap<String, String>,
    key: &str,
    current: &str,
    name: &str,
) -> Option<String> {
    let draft = drafts
        .entry(key.to_owned())
        .or_insert_with(|| current.to_owned());
    let response = text_input(ui, draft, name, "");
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let done = response.lost_focus() && !escaped && draft != current;
    let out = done.then(|| draft.clone());
    if !response.has_focus() {
        drafts.remove(key);
    }
    out
}

fn standard_section(ui: &mut Ui, model: &Model, outs: &mut Vec<Out>) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    section_header(ui, &t("model-settings-standard"));
    let names: Vec<&str> = model.parameters().iter().map(|(_, n, _)| n).collect();
    let none = t("model-settings-none");
    let mut options: Vec<(Option<String>, &str)> = vec![(None, none.as_str())];
    options.extend(names.iter().map(|n| (Some((*n).to_owned()), *n)));
    {
        {
            for role in StandardRole::ALL {
                let current = model
                    .settings()
                    .parameter(role)
                    .and_then(|id| model.parameters().name(id))
                    .map(str::to_owned);
                let mut value = current.clone();
                let name = t(&role_key(role));
                let response = form_row(ui, &name, |ui| {
                    select(
                        ui,
                        ("model-settings-role", role),
                        &name,
                        &mut value,
                        &options,
                    )
                });
                if response.changed() && value != current {
                    outs.push(Out::Command(
                        Command::SetStandardParameter {
                            role,
                            parameter: value.clone(),
                        },
                        format!("settings_standard: {role:?}"),
                    ));
                }
            }
        }
    }
    ui.add_space(2.0);
    let assign = assign_by_name(model);
    ui.add_enabled_ui(!assign.is_empty(), |ui| {
        if button(ui, &t("model-settings-assign"), Variant::Secondary).clicked() {
            outs.push(Out::Command(
                Command::Batch { commands: assign },
                "settings_assign_standard".to_owned(),
            ));
        }
    });
}

fn hit_area_section(
    ui: &mut Ui,
    model: &Model,
    drafts: &mut HashMap<String, String>,
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    section_header(ui, &t("model-settings-hit-areas"));
    let meshes: Vec<&str> = model.meshes().iter().map(|(_, n, _)| n).collect();
    let options: Vec<(String, &str)> = meshes.iter().map(|n| ((*n).to_owned(), *n)).collect();
    for area in &model.settings().hit_areas {
        ui.push_id(("hit-area", &area.name), |ui| {
            ui.horizontal(|ui| {
                let name_width = (ui.available_width() * 0.38).clamp(64.0, 160.0);
                ui.spacing_mut().combo_width = (ui.available_width() - name_width - 40.0).max(48.0);
                let key = format!("area.{}", area.name);
                let renamed = ui
                    .allocate_ui(egui::vec2(name_width, ui.spacing().interact_size.y), |ui| {
                        text_field(ui, drafts, &key, &area.name, &t("model-settings-area-name"))
                    })
                    .inner;
                if let Some(new_name) = renamed {
                    outs.push(Out::Command(
                        Command::RenameHitArea {
                            name: area.name.clone(),
                            new_name,
                        },
                        format!("hit_area_rename: {}", area.name),
                    ));
                }
                let current = model.meshes().name(area.mesh).unwrap_or_default();
                let mut value = current.to_owned();
                let response = select(
                    ui,
                    ("model-settings-mesh", &area.name),
                    &t("model-settings-area-mesh"),
                    &mut value,
                    &options,
                );
                if response.changed() && value != current {
                    outs.push(Out::Command(
                        Command::SetHitAreaMesh {
                            name: area.name.clone(),
                            mesh: value,
                        },
                        format!("hit_area_mesh: {}", area.name),
                    ));
                }
                if icon_button(
                    ui,
                    Icon::Trash,
                    &t("model-settings-area-delete"),
                    Variant::Ghost,
                )
                .clicked()
                {
                    outs.push(Out::Command(
                        Command::RemoveHitArea {
                            name: area.name.clone(),
                        },
                        format!("hit_area_remove: {}", area.name),
                    ));
                }
            });
        });
    }
    ui.add_enabled_ui(!meshes.is_empty(), |ui| {
        if button(ui, &t("model-settings-area-add"), Variant::Secondary).clicked() {
            let name = unique_area_name(model);
            outs.push(Out::Command(
                Command::AddHitArea {
                    name: name.clone(),
                    mesh: meshes[0].to_owned(),
                },
                format!("hit_area_add: {name}"),
            ));
        }
    });
}

fn info_section(
    ui: &mut Ui,
    model: &Model,
    drafts: &mut HashMap<String, String>,
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    section_header(ui, &t("model-settings-info"));
    let info = &model.settings().info;
    let fields = [
        ("title", &info.title),
        ("author", &info.author),
        ("license", &info.license),
        ("notes", &info.notes),
    ];
    {
        {
            for (field, current) in fields {
                let name = t(&format!("model-settings-{field}"));
                let key = format!("info.{field}");
                let edited = form_row(ui, &name, |ui| text_field(ui, drafts, &key, current, &name));
                if let Some(text) = edited {
                    let mut next = info.clone();
                    match field {
                        "title" => next.title = text,
                        "author" => next.author = text,
                        "license" => next.license = text,
                        _ => next.notes = text,
                    }
                    outs.push(Out::Command(
                        Command::SetModelInfo {
                            title: next.title,
                            author: next.author,
                            license: next.license,
                            notes: next.notes,
                        },
                        format!("settings_info: {field}"),
                    ));
                }
            }
        }
    }
}

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Vec<Out> {
    let mut outs = Vec::new();
    let drafts = &mut view.model_settings.drafts;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .id_salt("model-settings-scroll")
        .show(ui, |ui| {
            standard_section(ui, model, &mut outs);
            hit_area_section(ui, model, drafts, &mut outs);
            info_section(ui, model, drafts, &mut outs);
        });
    outs
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn model() -> Model {
        let mut m = Model::new();
        let steps = [
            Command::AddParameter {
                name: "ParamAngleX".into(),
                min: -30.0,
                max: 30.0,
                default: 0.0,
                repeat: false,
            },
            Command::AddParameter {
                name: "Smile".into(),
                min: 0.0,
                max: 1.0,
                default: 0.0,
                repeat: false,
            },
            Command::AddPart {
                name: "Part".into(),
                parent: None,
            },
        ];
        for step in steps {
            m.apply(step).unwrap();
        }
        for name in ["Head", "Body"] {
            m.apply(Command::AddMesh {
                name: name.into(),
                part: "Part".into(),
                positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                indices: vec![0, 1, 2],
            })
            .unwrap();
        }
        m
    }

    struct Panel {
        model: Model,
        view: ViewState,
        outs: Vec<Out>,
    }

    fn harness(model: Model) -> Harness<'static, Panel> {
        Harness::builder()
            .with_size(egui::vec2(560.0, 700.0))
            .build_ui_state(
                |ui, p: &mut Panel| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    let outs = show(ui, &p.model, &mut p.view);
                    p.outs.extend(outs);
                },
                Panel {
                    model,
                    view: ViewState::default(),
                    outs: Vec::new(),
                },
            )
    }

    #[test]
    fn assigning_by_name_gives_a_role_its_standard_parameter() {
        let mut h = harness(model());
        h.run();
        h.get_by_label("Assign by name").click();
        h.run();
        let Some(Out::Command(Command::Batch { commands }, _)) = h.state().outs.last() else {
            panic!("a Batch");
        };
        assert_eq!(
            commands,
            &[Command::SetStandardParameter {
                role: StandardRole::AngleX,
                parameter: Some("ParamAngleX".into()),
            }]
        );
    }

    #[test]
    fn nothing_is_left_to_assign_once_the_standard_parameter_is_set() {
        let mut m = model();
        for c in assign_by_name(&m) {
            m.apply(c).unwrap();
        }
        assert!(assign_by_name(&m).is_empty());
    }

    #[test]
    fn choosing_a_parameter_for_a_role_sends_the_command() {
        let mut h = harness(model());
        h.run();
        h.get_by_role_and_label(Role::ComboBox, "Mouth open")
            .click();
        h.run();
        h.get_by_label("Smile").click();
        h.run();
        let Some(Out::Command(command, _)) = h.state().outs.last() else {
            panic!("a command");
        };
        assert_eq!(
            command,
            &Command::SetStandardParameter {
                role: StandardRole::MouthOpen,
                parameter: Some("Smile".into()),
            }
        );
    }

    #[test]
    fn a_hit_area_is_added_on_the_first_mesh_and_deleted() {
        let mut h = harness(model());
        h.run();
        h.get_by_label("Add hit area").click();
        h.run();
        let Some(Out::Command(command, _)) = h.state().outs.last() else {
            panic!("a command");
        };
        assert_eq!(
            command,
            &Command::AddHitArea {
                name: "Area1".into(),
                mesh: "Head".into(),
            }
        );
        h.state_mut()
            .model
            .apply(Command::AddHitArea {
                name: "Area1".into(),
                mesh: "Head".into(),
            })
            .unwrap();
        h.state_mut().outs.clear();
        h.run();
        h.get_by_label("Delete hit area").click();
        h.run();
        let Some(Out::Command(command, _)) = h.state().outs.last() else {
            panic!("a command");
        };
        assert_eq!(
            command,
            &Command::RemoveHitArea {
                name: "Area1".into()
            }
        );
    }

    #[test]
    fn there_is_no_hit_area_to_add_without_a_mesh() {
        let mut h = harness(Model::new());
        h.run();
        h.get_by_label("Add hit area").click();
        h.run();
        assert!(h.state().outs.is_empty());
    }

    #[test]
    fn typing_in_a_info_field_and_leaving_it_sends_the_whole_info() {
        let mut m = model();
        m.apply(Command::SetModelInfo {
            title: "T".into(),
            author: "A".into(),
            license: String::new(),
            notes: String::new(),
        })
        .unwrap();
        let mut h = harness(m);
        h.run();
        let author = h.get_by_role_and_label(Role::TextInput, "Author");
        author.focus();
        author.type_text("bc");
        h.run();
        assert!(h.state().outs.is_empty(), "nothing is sent while typing");
        h.get_by_role_and_label(Role::TextInput, "Title").focus();
        h.run();
        let Some(Out::Command(command, _)) = h.state().outs.last() else {
            panic!("a command");
        };
        assert_eq!(
            command,
            &Command::SetModelInfo {
                title: "T".into(),
                author: "Abc".into(),
                license: String::new(),
                notes: String::new(),
            }
        );
    }
}
