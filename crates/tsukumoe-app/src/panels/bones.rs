// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The bones panel: the bones nested by parent, and the transform and parent
//! of the picked one. A finished edit of a field is one command. A bone with
//! keyforms is edited at the key the sliders are on.

use std::collections::HashMap;

use egui::Ui;
use tsukumoe_core::command::{BoneBase, Command};
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::BoneId;
use tsukumoe_ui::{
    Icon, Variant, button, icon_button, label, number_edit, section_header, select, selectable_row,
    tr,
};

use crate::bone_tool;
use crate::view::{Selection, ViewState};

use super::model_settings::text_field;

/// What the panel remembers between frames. Not saved, not in the history.
#[derive(Clone, Debug, Default)]
pub struct State {
    drafts: HashMap<String, String>,
}

pub enum Out {
    Command(Command, String),
    /// A refused edit; the translation key says why.
    Error(&'static str),
}

/// The bones with how deep each is, parents first, in the order the model keeps.
fn rows(model: &Model) -> Vec<(BoneId, String, usize)> {
    fn walk(
        model: &Model,
        parent: Option<BoneId>,
        depth: usize,
        out: &mut Vec<(BoneId, String, usize)>,
    ) {
        for (id, name, bone) in model.bones().iter() {
            if bone.parent == parent {
                out.push((id, name.to_owned(), depth));
                walk(model, Some(id), depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(model, None, 0, &mut out);
    out
}

/// The bones `name` may take as its parent: not itself, not below it.
fn parent_options(model: &Model, name: &str) -> Vec<String> {
    let Some(id) = model.bones().find(name) else {
        return Vec::new();
    };
    let below = |mut b: BoneId| {
        while let Some(parent) = model.bones().get(b).and_then(|bone| bone.parent) {
            if parent == id {
                return true;
            }
            b = parent;
        }
        false
    };
    model
        .bones()
        .iter()
        .filter(|(b, _, _)| *b != id && !below(*b))
        .map(|(_, n, _)| n.to_owned())
        .collect()
}

fn edit(model: &Model, view: &ViewState, name: &str, base: BoneBase) -> Out {
    let at = crate::keyed::values(model, view);
    match bone_tool::set_command(model, name, base, &at) {
        Ok((command, label)) => Out::Command(command, label),
        Err(e) => Out::Error(e.key()),
    }
}

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Vec<Out> {
    let mut outs = Vec::new();
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let picked = match view.selection.clone() {
        Some(Selection::Bone(n)) if model.bones().find(&n).is_some() => Some(n),
        _ => None,
    };
    ui.horizontal(|ui| {
        if button(ui, &t("bones-add"), Variant::Secondary).clicked() {
            let name = bone_tool::unique_name(model);
            outs.push(Out::Command(
                Command::AddBone {
                    name: name.clone(),
                    parent: picked.clone(),
                    base: BoneBase {
                        translation: [0.0, 0.0],
                        rotation_degrees: 0.0,
                        scale: [1.0, 1.0],
                    },
                },
                format!("bone_add: {name}"),
            ));
            view.select_only(Selection::Bone(name));
        }
        if let Some(name) = &picked
            && icon_button(ui, Icon::Trash, &t("bones-delete"), Variant::Ghost).clicked()
        {
            outs.push(Out::Command(
                Command::RemoveBone { name: name.clone() },
                format!("bone_remove: {name}"),
            ));
        }
    });
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .id_salt("bones-scroll")
        .show(ui, |ui| {
            for (_, name, depth) in rows(model) {
                ui.horizontal(|ui| {
                    ui.add_space(12.0 * depth as f32);
                    let on = picked.as_deref() == Some(name.as_str());
                    if selectable_row(ui, on, None, &name).clicked() {
                        view.select_only(Selection::Bone(name.clone()));
                    }
                });
            }
            let Some(name) = picked else {
                return;
            };
            fields(ui, model, view, &name, &mut outs);
        });
    outs
}

fn fields(ui: &mut Ui, model: &Model, view: &mut ViewState, name: &str, outs: &mut Vec<Out>) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let Some(id) = model.bones().find(name) else {
        return;
    };
    let at = crate::keyed::values(model, view);
    let Some(local) = bone_tool::local_at(model, id, &at) else {
        return;
    };
    section_header(ui, name);
    let parent = model
        .bones()
        .get(id)
        .and_then(|b| b.parent)
        .and_then(|p| model.bones().name(p))
        .map(str::to_owned);
    egui::Grid::new("bones-fields")
        .num_columns(2)
        .show(ui, |ui| {
            label(ui, t("bones-name"));
            let drafts = &mut view.bones.drafts;
            if let Some(new_name) = text_field(ui, drafts, "bone.name", name, &t("bones-name")) {
                outs.push(Out::Command(
                    Command::RenameBone {
                        name: name.to_owned(),
                        new_name: new_name.clone(),
                    },
                    format!("bone_rename: {name}"),
                ));
                view.select_only(Selection::Bone(new_name));
            }
            ui.end_row();

            label(ui, t("bones-parent"));
            let none = t("bones-no-parent");
            let mut options: Vec<(Option<String>, &str)> = vec![(None, none.as_str())];
            let names = parent_options(model, name);
            options.extend(names.iter().map(|n| (Some(n.clone()), n.as_str())));
            let mut value = parent.clone();
            let response = select(ui, "bones-parent", &t("bones-parent"), &mut value, &options);
            if response.changed() && value != parent {
                outs.push(Out::Command(
                    Command::SetBoneParent {
                        name: name.to_owned(),
                        parent: value,
                    },
                    format!("bone_parent: {name}"),
                ));
            }
            ui.end_row();

            let mut base = BoneBase::of(&local);
            let mut changed = false;
            let mut row =
                |ui: &mut Ui, key: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>| {
                    label(ui, t(key));
                    if let Some(v) = number_edit(ui, *value, range, 0.05, &t(key)) {
                        *value = v;
                        changed = true;
                    }
                    ui.end_row();
                };
            row(ui, "bones-x", &mut base.translation[0], -1.0e6..=1.0e6);
            row(ui, "bones-y", &mut base.translation[1], -1.0e6..=1.0e6);
            row(
                ui,
                "bones-rotation",
                &mut base.rotation_degrees,
                -360.0..=360.0,
            );
            row(ui, "bones-scale-x", &mut base.scale[0], -100.0..=100.0);
            row(ui, "bones-scale-y", &mut base.scale[1], -100.0..=100.0);
            if changed {
                outs.push(edit(model, view, name, base));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn base(x: f32) -> BoneBase {
        BoneBase {
            translation: [x, 0.0],
            rotation_degrees: 0.0,
            scale: [1.0, 1.0],
        }
    }

    fn model() -> Model {
        let mut m = Model::new();
        for c in [
            Command::AddBone {
                name: "Root".into(),
                parent: None,
                base: base(1.0),
            },
            Command::AddBone {
                name: "Arm".into(),
                parent: Some("Root".into()),
                base: base(2.0),
            },
        ] {
            m.apply(c).unwrap();
        }
        m
    }

    struct Panel {
        model: Model,
        view: ViewState,
        outs: Vec<Out>,
    }

    fn harness(select: Option<&str>) -> Harness<'static, Panel> {
        let mut view = ViewState::default();
        if let Some(n) = select {
            view.select_only(Selection::Bone(n.into()));
        }
        Harness::builder()
            .with_size(egui::vec2(420.0, 520.0))
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

    #[test]
    fn the_rows_nest_by_parent() {
        let m = model();
        let r: Vec<(String, usize)> = rows(&m).into_iter().map(|(_, n, d)| (n, d)).collect();
        assert_eq!(r, [("Root".into(), 0), ("Arm".into(), 1)]);
    }

    #[test]
    fn a_bone_cannot_take_itself_or_a_descendant_as_its_parent() {
        let m = model();
        assert_eq!(parent_options(&m, "Root"), Vec::<String>::new());
        assert_eq!(parent_options(&m, "Arm"), ["Root"]);
    }

    #[test]
    fn adding_a_bone_goes_under_the_picked_one_and_picks_the_new_one() {
        let mut h = harness(Some("Root"));
        h.run();
        h.get_by_label("Add bone").click();
        h.run();
        let Some(Out::Command(Command::AddBone { name, parent, .. }, _)) = h.state().outs.last()
        else {
            panic!("an AddBone");
        };
        assert_eq!((name.as_str(), parent.as_deref()), ("Bone1", Some("Root")));
        assert_eq!(
            h.state().view.selection,
            Some(Selection::Bone("Bone1".into()))
        );
    }

    #[test]
    fn clicking_a_row_picks_the_bone_and_delete_removes_it() {
        let mut h = harness(None);
        h.run();
        h.get_by_label("Arm").click();
        h.run();
        assert_eq!(
            h.state().view.selection,
            Some(Selection::Bone("Arm".into()))
        );
        h.get_by_label("Delete bone").click();
        h.run();
        let Some(Out::Command(Command::RemoveBone { name }, _)) = h.state().outs.last() else {
            panic!("a RemoveBone");
        };
        assert_eq!(name, "Arm");
    }

    #[test]
    fn typing_a_translation_sends_the_base() {
        let mut h = harness(Some("Arm"));
        h.run();
        h.get_by_role_and_label(Role::SpinButton, "X").click();
        h.run();
        h.get_by_role_and_label(Role::SpinButton, "X")
            .type_text("7");
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        let Some(Out::Command(Command::SetBoneBase { name, base }, _)) = h.state().outs.last()
        else {
            panic!("a SetBoneBase");
        };
        assert_eq!(name, "Arm");
        assert!(
            (base.translation[0] - 7.0).abs() < 1e-4,
            "{:?}",
            base.translation
        );
    }

    #[test]
    fn an_edit_between_keys_is_reported_not_sent() {
        let mut m = model();
        m.apply(Command::BindParameter {
            parameter: "P".into(),
            elements: vec![tsukumoe_core::command::ElementRef::Bone { name: "Arm".into() }],
        })
        .unwrap_err();
        m.apply(Command::AddParameter {
            name: "P".into(),
            min: -1.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        m.apply(Command::BindParameter {
            parameter: "P".into(),
            elements: vec![tsukumoe_core::command::ElementRef::Bone { name: "Arm".into() }],
        })
        .unwrap();
        let mut view = ViewState::default();
        let id = m.parameters().find("P").unwrap();
        let p = m.parameters().get(id).unwrap().clone();
        view.set_param("P", &p, 0.37);
        let out = edit(&m, &view, "Arm", base(5.0));
        assert!(matches!(out, Out::Error("param-err-not-on-key")));
    }
}
