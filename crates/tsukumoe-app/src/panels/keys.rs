// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The keys panel: the keys of the selected element on the focused parameter
//! (previous, next, add, remove, bind), and for an element with two or more
//! axes a pad that shows the key combinations of two axes as diamonds, where
//! a drag moves both sliders (like Live2D's two-dimensional parameter).
//! Mirror copy lives here too. All of it is view state or goes through `Out`.

use egui::{Pos2, Sense, Shape, Stroke, Ui, Vec2, pos2};
use tsukumoe_core::command::{Command, ElementRef};
use tsukumoe_core::model::Model;
use tsukumoe_ui::{Icon, Variant, button, hint, icon_button, text_input, tr};

use super::parameters::Out;
use crate::confirm::Kind;
use crate::mirror;
use crate::param_ops;
use crate::view::ViewState;

/// A drag snaps to a key diamond within this many points.
const PAD_SNAP: f32 = 9.0;
const PAD_INSET: f32 = 12.0;

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Vec<Out> {
    let mut outs = Vec::new();
    let selection = view.selected();
    let selection = selection.as_slice();
    let focus = view
        .param_focus
        .clone()
        .filter(|f| model.parameters().find(f).is_some());
    let Some(name) = focus else {
        hint(ui, tr(ui.ctx(), "keys-pick"));
        return outs;
    };
    let id = model.parameters().find(&name).expect("filtered above");
    let p = model.parameters().get(id).expect("found");
    let name = name.as_str();
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    ui.label(egui::RichText::new(name).strong());
    // Rename, range, repeat and delete of the parameter, where its keys are.
    egui::CollapsingHeader::new(t("param-settings"))
        .id_salt(("keys-settings", name))
        .show(ui, |ui| {
            super::parameters::settings_section(ui, model, name, p, &mut outs);
        });
    // The keys of the selected element.
    let value = view.param(name, p);
    let keys = param_ops::primary(model, name, selection).map(|(_, k)| k);
    let range = p.max - p.min;
    ui.horizontal(|ui| {
        let step = |dir: i32, view: &mut ViewState| {
            if let Some(k) = keys
                .as_deref()
                .and_then(|k| param_ops::neighbour(k, value, range, dir))
            {
                view.set_param(name, p, k);
            }
        };
        if icon_button(ui, Icon::ChevronLeft, &t("param-key-prev"), Variant::Ghost).clicked() {
            step(-1, view);
        }
        if icon_button(ui, Icon::ChevronRight, &t("param-key-next"), Variant::Ghost).clicked() {
            step(1, view);
        }
        if icon_button(ui, Icon::Plus, &t("param-key-add"), Variant::Ghost).clicked() {
            outs.push(match param_ops::add_key(model, name, value, selection) {
                Ok(op) => Out::Op(op),
                Err(e) => Out::Error(e.key()),
            });
        }
        if icon_button(ui, Icon::Minus, &t("param-key-remove"), Variant::Ghost).clicked() {
            outs.push(match param_ops::remove_key(model, name, value, selection) {
                Ok(op) => Out::Confirm(Kind::Key, name.to_owned(), op),
                Err(e) => Out::Error(e.key()),
            });
        }
        if button(ui, &t("param-bind"), Variant::Secondary).clicked() {
            outs.push(match param_ops::bind(model, name, selection) {
                Ok(op) => Out::Op(op),
                Err(e) => Out::Error(e.key()),
            });
        }
        if keys.is_some() && button(ui, &t("param-unbind"), Variant::Secondary).clicked() {
            outs.push(match param_ops::unbind(model, name, selection) {
                Ok(op) => Out::Confirm(Kind::Unbind, name.to_owned(), op),
                Err(e) => Out::Error(e.key()),
            });
        }
    });
    if let Some(keys) = &keys {
        let on = param_ops::on_key(keys, value, range).is_some();
        let text = keys
            .iter()
            .map(|k| format!("{k:.2}"))
            .collect::<Vec<_>>()
            .join("  ");
        hint(ui, format!("{}{}", if on { "\u{25c6} " } else { "" }, text));
    }
    if let Some(element) = param_ops::elements(selection).pop()
        && let Some(binding) = param_ops::binding_of(model, &element)
    {
        // With several axes: the pad (which also names the combination).
        if binding.axes().len() > 1 {
            pad_section(ui, model, view, &element, &mut outs);
        }
        mirror_row(ui, model, view, &element, &mut outs);
    }
    outs
}

/// The pad of an element with two or more axes: the key combinations of two
/// axes as diamonds, the sliders as a dot. A click or drag moves both sliders
/// (snapping to a diamond, Shift for none); a right click on a diamond mirror
/// copies from or onto it.
fn pad_section(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    element: &ElementRef,
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let Some(binding) = param_ops::binding_of(model, element) else {
        return;
    };
    let n = binding.axes().len();
    let (mut r, mut c) = view.grid_axes;
    if r >= n || c >= n || r == c {
        (r, c) = (0, 1);
    }
    let name_of = |i: usize| {
        model
            .parameters()
            .name(binding.axes()[i].parameter)
            .unwrap_or("?")
            .to_owned()
    };
    let next = |from: usize, other: usize| {
        (1..=n)
            .map(|d| (from + d) % n)
            .find(|&i| i != other)
            .unwrap_or(from)
    };
    ui.horizontal_wrapped(|ui| {
        if button(ui, &format!("\u{2195} {}", name_of(r)), Variant::Ghost).clicked() {
            r = next(r, c);
        }
        if button(ui, &format!("\u{2194} {}", name_of(c)), Variant::Ghost).clicked() {
            c = next(c, r);
        }
        if icon_button(ui, Icon::ArrowUpDown, &t("param-axes-swap"), Variant::Ghost).clicked() {
            let mut order: Vec<String> = (0..n).map(name_of).collect();
            order.swap(r, c);
            outs.push(Out::Op((
                Command::SetAxisOrder {
                    element: element.clone(),
                    parameters: order,
                },
                "param_axis_order".to_owned(),
            )));
        }
    });
    view.grid_axes = (r, c);
    let at = crate::keyed::values(model, view);
    let here = param_ops::nearest_key(binding, &at);
    let on = matches!(
        crate::keyed::element_place(model, element, &at),
        Some(crate::keyed::Place::Key { .. })
    );
    let edited = param_ops::edited(model, element).unwrap_or_default();
    let range = |i: usize| {
        model
            .parameters()
            .get(binding.axes()[i].parameter)
            .map_or((0.0, 1.0), |p| (p.min, p.max))
    };
    let (cmin, cmax) = range(c);
    let (rmin, rmax) = range(r);
    let side = ui.available_width().clamp(120.0, 260.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click_and_drag());
    let inner = rect.shrink(PAD_INSET);
    let to_screen = |vc: f32, vr: f32| -> Pos2 {
        pos2(
            inner.left() + (vc - cmin) / (cmax - cmin).max(1e-6) * inner.width(),
            inner.bottom() - (vr - rmin) / (rmax - rmin).max(1e-6) * inner.height(),
        )
    };
    let from_screen = |p: Pos2| -> (f32, f32) {
        (
            cmin + ((p.x - inner.left()) / inner.width()).clamp(0.0, 1.0) * (cmax - cmin),
            rmin + ((inner.bottom() - p.y) / inner.height()).clamp(0.0, 1.0) * (rmax - rmin),
        )
    };
    let cell_key = |ri: usize, ci: usize| {
        let mut key = here.clone();
        key[r] = ri;
        key[c] = ci;
        key
    };
    let (rkeys, ckeys) = (&binding.axes()[r].keys, &binding.axes()[c].keys);
    let diamonds: Vec<(Pos2, Vec<usize>)> = (0..rkeys.len())
        .flat_map(|ri| (0..ckeys.len()).map(move |ci| (ri, ci)))
        .map(|(ri, ci)| (to_screen(ckeys[ci], rkeys[ri]), cell_key(ri, ci)))
        .collect();
    let nearest = |p: Pos2| {
        diamonds
            .iter()
            .min_by(|a, b| a.0.distance(p).total_cmp(&b.0.distance(p)))
    };
    // Moving: set both sliders; snap to a diamond unless Shift is held.
    let shift = ui.input(|i| i.modifiers.shift);
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let (mut vc, mut vr) = from_screen(pointer);
        if !shift
            && let Some((pos, key)) = nearest(pointer)
            && pos.distance(pointer) <= PAD_SNAP
        {
            vc = binding.axes()[c].keys[key[c]];
            vr = binding.axes()[r].keys[key[r]];
        }
        for (i, v) in [(c, vc), (r, vr)] {
            let pid = binding.axes()[i].parameter;
            if let Some(p) = model.parameters().get(pid)
                && let Some(name) = model.parameters().name(pid)
            {
                view.set_param(name, p, v);
            }
        }
    }
    // Which diamond a right click is on: remembered while the pointer is over the pad.
    let target_id = ui.make_persistent_id("pad-target");
    if response.hovered()
        && let Some(hover) = response.hover_pos()
        && let Some((_, key)) = nearest(hover)
    {
        ui.data_mut(|d| d.insert_temp(target_id, key.clone()));
    }
    response.context_menu(|ui| {
        let Some(key) = ui.data(|d| d.get_temp::<Vec<usize>>(target_id)) else {
            return;
        };
        if ui.button(t("mirror-source")).clicked() {
            view.mirror_source = Some((element.clone(), key.clone()));
            ui.close();
        }
        if ui.button(t("mirror-paste")).clicked() {
            outs.push(paste_at(model, view, element, &key));
            ui.close();
        }
    });
    // Drawing.
    let theme = tsukumoe_ui::Theme::get(&ctx);
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        4.0,
        theme.colors.surface,
        Stroke::new(1.0, theme.colors.border),
        egui::StrokeKind::Inside,
    );
    let source = view
        .mirror_source
        .as_ref()
        .filter(|(e, _)| e == element)
        .map(|(_, k)| k.clone());
    for (pos, key) in &diamonds {
        let flat = binding.index(key).unwrap_or(0);
        let done = edited.get(flat).copied().unwrap_or(false);
        let current = on && *key == here;
        let radius = if current { 7.0 } else { 5.5 };
        let points = vec![
            *pos + Vec2::new(0.0, -radius),
            *pos + Vec2::new(radius, 0.0),
            *pos + Vec2::new(0.0, radius),
            *pos + Vec2::new(-radius, 0.0),
        ];
        let (fill, stroke) = (
            if done {
                theme.colors.text_muted
            } else {
                theme.colors.surface
            },
            Stroke::new(
                if current { 2.0 } else { 1.0 },
                if current {
                    theme.colors.accent
                } else if source.as_ref() == Some(key) {
                    theme.colors.warning
                } else {
                    theme.colors.text_muted
                },
            ),
        );
        painter.add(Shape::convex_polygon(points, fill, stroke));
    }
    let dot = to_screen(
        at(binding.axes()[c].parameter).clamp(cmin, cmax),
        at(binding.axes()[r].parameter).clamp(rmin, rmax),
    );
    painter.circle_filled(dot, 4.0, theme.colors.accent);
    // Not in a `horizontal`: a long parameter name wraps only in a vertical layout.
    let at_label = match crate::keyed::element_place(model, element, &at) {
        Some(crate::keyed::Place::Key { label, .. }) => format!("\u{25c6} {label}"),
        Some(crate::keyed::Place::Off { label }) => label,
        None => String::new(),
    };
    hint(ui, at_label);
}

/// A mirror paste onto the keyform `key` of `element` (a cell of the grid).
fn paste_at(model: &Model, view: &ViewState, element: &ElementRef, key: &[usize]) -> Out {
    let Some((source, from)) = &view.mirror_source else {
        return Out::Error("mirror-err-no-source");
    };
    let center = mirror::center(model, &view.mirror_center);
    match mirror::mirror_copy(model, source, from, element, key, center) {
        Ok(op) => Out::Op(op),
        Err(e) => Out::Error(e.key()),
    }
}

/// Mirror copy: pick the keyform under the sliders as the source, then paste
/// it, mirrored, onto the keyform of the selected element under the sliders.
fn mirror_row(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    element: &ElementRef,
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    ui.horizontal(|ui| {
        if button(ui, &t("mirror-source"), Variant::Secondary).clicked()
            && let Err(key) = mirror::set_source(model, view, element)
        {
            outs.push(Out::Error(key));
        }
        if button(ui, &t("mirror-paste"), Variant::Secondary).clicked() {
            outs.push(match mirror::paste(model, view, element) {
                Ok(op) => Out::Op(op),
                Err(key) => Out::Error(key),
            });
        }
    });
    let hint_text = t("mirror-center");
    text_input(ui, &mut view.mirror_center, &hint_text, &hint_text);
    if let Some((source, key)) = &view.mirror_source {
        let (ElementRef::Mesh { name } | ElementRef::Deformer { name } | ElementRef::Glue { name }) =
            source;
        hint(ui, format!("{}: {name} {key:?}", t("mirror-source")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Selection;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use tsukumoe_core::command::Command;

    fn model() -> Model {
        let mut m = Model::new();
        for n in ["ParamAngleX", "ParamAngleY", "ParamEyeOpen"] {
            m.apply(Command::AddParameter {
                name: n.into(),
                min: -1.0,
                max: 1.0,
                default: 0.0,
                repeat: false,
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

    fn panel() -> Panel {
        let mut model = model();
        model
            .apply(Command::AddPart {
                name: "Part".into(),
                parent: None,
            })
            .unwrap();
        let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        model
            .apply(Command::AddMesh {
                name: "Skin".into(),
                part: "Part".into(),
                positions: pts.clone(),
                uvs: pts,
                indices: vec![0, 1, 2],
            })
            .unwrap();
        let mut view = ViewState::default();
        view.select_only(Selection::Mesh("Skin".into()));
        view.param_focus = Some("ParamAngleX".into());
        Panel {
            model,
            view,
            outs: Vec::new(),
        }
    }

    fn harness(p: Panel) -> Harness<'static, Panel> {
        Harness::builder()
            .with_size(egui::vec2(420.0, 420.0))
            .build_ui_state(
                |ui, p: &mut Panel| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    let outs = show(ui, &p.model, &mut p.view);
                    p.outs.extend(outs);
                },
                p,
            )
    }

    #[test]
    fn the_buttons_send_bind_keys_and_a_new_parameter_through_the_param_ops() {
        let mut h = harness(panel());
        h.run();
        // Unbound: bind it.
        h.get_by_label("Bind").click();
        h.run();
        let Out::Op((command, label)) = h.state().outs.last().unwrap().clone() else {
            panic!("an op");
        };
        assert_eq!(label, "param_bind: ParamAngleX");
        h.state_mut().model.apply(command).unwrap();
        h.state_mut().outs.clear();
        h.run();
        // Bound: add a key at the slider (the default 0 is a key: refused with a message).
        h.get_by_label("Add a key here").click();
        h.run();
        assert_eq!(
            h.state().outs.last(),
            Some(&Out::Error("param-err-key-exists"))
        );
        // Move off the keys and add.
        let p = h
            .state()
            .model
            .parameters()
            .get(h.state().model.parameters().find("ParamAngleX").unwrap())
            .unwrap()
            .clone();
        h.state_mut().view.set_param("ParamAngleX", &p, 0.5);
        h.run();
        h.get_by_label("Add a key here").click();
        h.run();
        assert!(matches!(
            h.state().outs.last(),
            Some(Out::Op((Command::AddParameterKey { value, .. }, _))) if (*value - 0.5).abs() < 1e-6
        ));
    }

    #[test]
    fn previous_and_next_key_move_the_slider_to_the_neighbouring_key() {
        let mut h = harness(panel());
        h.run();
        h.get_by_label("Bind").click();
        h.run();
        let Out::Op((command, _)) = h.state().outs.last().unwrap().clone() else {
            panic!("an op");
        };
        h.state_mut().model.apply(command).unwrap();
        h.run();
        let value = |h: &Harness<Panel>| {
            let m = &h.state().model;
            let p = m
                .parameters()
                .get(m.parameters().find("ParamAngleX").unwrap())
                .unwrap();
            h.state().view.param("ParamAngleX", p)
        };
        h.get_by_label("Next key").click();
        h.run();
        assert_eq!(value(&h), 1.0);
        h.get_by_label("Previous key").click();
        h.run();
        assert_eq!(value(&h), 0.0);
    }

    #[test]
    fn two_axes_show_a_grid_and_the_swap_button_reorders_them() {
        let mut p = panel();
        for name in ["ParamAngleX", "ParamAngleY"] {
            let command = param_ops::bind(&p.model, name, &p.view.selected())
                .unwrap()
                .0;
            p.model.apply(command).unwrap();
        }
        let mut h = harness(p);
        h.run();
        h.get_by_label("Swap the two axes (changes the grid order)")
            .click();
        h.run();
        let Some(Out::Op((Command::SetAxisOrder { parameters, .. }, _))) = h.state().outs.last()
        else {
            panic!("an axis order");
        };
        assert_eq!(parameters, &["ParamAngleY", "ParamAngleX"]);
    }
}
