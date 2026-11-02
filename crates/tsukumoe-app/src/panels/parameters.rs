// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The parameter list: a name filter and, per parameter, a slider and a
//! number field you can type into or drag. The values are view state, not
//! model data: moving one never creates an undo step.

use egui::{Ui, Vec2};
use tsukumoe_core::command::Command;
use tsukumoe_core::elements::Parameter;
use tsukumoe_core::model::Model;
use tsukumoe_ui::{
    Arg, Icon, Variant, button, icon_button, number, slider, text_input, tr, tr_args,
};

use crate::confirm::Kind;
use crate::mesh_tool::Op;
use crate::param_ops;
use crate::tree_model::matches;
use crate::view::{Selection, ViewState};

/// Names of the parameters matching the filter (name or display name).
pub fn names(model: &Model, filter: &str) -> Vec<String> {
    model
        .parameters()
        .iter()
        .filter(|(_, name, p)| {
            matches(name, filter) || matches(&p.display_name, filter) && !p.display_name.is_empty()
        })
        .map(|(_, name, _)| name.to_owned())
        .collect()
}

const LABEL_WIDTH: f32 = 68.0;
/// The width of the drag handle at the start of a row.
const GRIP_WIDTH: f32 = 16.0;
/// The width of the number field next to a slider or a pad.
const NUMBER_WIDTH: f32 = 64.0;
/// The width of the link button at the start of a row.
const LINK_WIDTH: f32 = 24.0;
/// The width of the reset button at the end of a row.
const RESET_WIDTH: f32 = 24.0;
/// A slider snaps to a key within this many points.
const SNAP_POINTS: f32 = 6.0;

/// What the panel asks the editor to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// A command and its history name.
    Op(Op),
    /// A deletion the editor may ask about first: its kind, the parameter it is about, the op.
    Confirm(Kind, String, Op),
    /// The new list of linked parameter pairs (kept with the project).
    Links(Vec<(String, String)>),
    /// A parameter rename; the editor moves the view state along with it.
    Rename { from: String, to: String },
    /// Something that cannot be done, as a translation key for a toast.
    Error(&'static str),
}

/// Draws a tick at each key over the slider `rect`.
fn paint_keys(ui: &Ui, rect: egui::Rect, keys: &[f32], min: f32, max: f32, value: f32) {
    let theme = tsukumoe_ui::Theme::get(ui.ctx());
    let r = rect.height() / 2.5;
    let span = (max - min).max(1e-6);
    for &k in keys {
        let t = ((k - min) / span).clamp(0.0, 1.0);
        let x = rect.left() + r + t * (rect.width() - 2.0 * r);
        let on = (k - value).abs() <= span * 1e-4;
        let color = if on {
            theme.colors.warning
        } else {
            theme.colors.text_muted
        };
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.bottom() - 1.0),
                egui::pos2(x, rect.bottom() + 3.0),
            ],
            egui::Stroke::new(if on { 2.0 } else { 1.0 }, color),
        );
    }
}

/// The width of the settings window of a parameter.
const SETTINGS_WIDTH: f32 = 380.0;

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Vec<Out> {
    let ctx = ui.ctx().clone();
    let mut outs = Vec::new();
    let selection = view.selected();
    let name = tr(&ctx, "filter-name");
    ui.horizontal(|ui| {
        if button(ui, &tr(&ctx, "parameters-reset"), Variant::Secondary).clicked() {
            view.reset_params();
        }
        if icon_button(ui, Icon::Plus, &tr(&ctx, "param-new"), Variant::Ghost).clicked() {
            outs.push(Out::Op(param_ops::create_parameter(model)));
        }
    });
    text_input(ui, &mut view.param_filter, &name, &name);
    let shown = names(model, &view.param_filter);
    let shift = ui.input(|i| i.modifiers.shift);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let links: Vec<&(String, String)> = view
                .param_links
                .iter()
                .filter(|(a, b)| {
                    model.parameters().find(a).is_some() && model.parameters().find(b).is_some()
                })
                .collect();
            let links: Vec<(String, String)> = links.into_iter().cloned().collect();
            let ctx = &ctx;
            let draw = |ui: &mut Ui, view: &mut ViewState, outs: &mut Vec<Out>, name: &str| {
                if links.iter().any(|(_, b)| b == name) {
                    return;
                }
                // A linked pair is one pad: it is not a drag source or a drop target.
                let rect = if let Some((_, b)) = links.iter().find(|(a, _)| a == name) {
                    link_row(ui, model, view, name, b, &selection, outs);
                    None
                } else {
                    row(ui, model, view, name, &selection, shift, outs)
                };
                if let Some(rect) = rect {
                    drop_on_row(ctx, ui, rect, model, name, outs);
                }
            };
            for (group, rows) in sections(model, &shown) {
                let Some(group) = group else {
                    for name in &rows {
                        draw(ui, view, &mut outs, name);
                    }
                    continue;
                };
                let r = egui::CollapsingHeader::new(&group)
                    .id_salt(("param-group", &group))
                    .default_open(true)
                    .show(ui, |ui| {
                        for name in &rows {
                            draw(ui, view, &mut outs, name);
                        }
                    });
                drop_on_header(ctx, &r.header_response, model, &group, &mut outs);
            }
        });
    // The settings window of one parameter (name, range, repeat, delete).
    if let Some(name) = view.param_dialog.clone() {
        let mut open = true;
        let target = model
            .parameters()
            .find(&name)
            .and_then(|id| model.parameters().get(id));
        match target {
            Some(p) => {
                egui::Window::new(format!("{}: {name}", tr(&ctx, "param-settings")))
                    .id(ui.make_persistent_id(("param-settings", &name)))
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .show(&ctx, |ui| {
                        // A fixed width: the name field fills what is left, so an
                        // auto-sized window would grow on every frame.
                        ui.set_width(SETTINGS_WIDTH);
                        settings_section(ui, model, &name, p, &mut outs);
                    });
            }
            None => open = false,
        }
        if !open {
            view.param_dialog = None;
        }
    }
    outs
}

/// The parameters to show, by group: those without one first, then each group
/// in the order it first appears. Rows keep the model order inside a group.
fn sections(model: &Model, shown: &[String]) -> Vec<(Option<String>, Vec<String>)> {
    let mut out: Vec<(Option<String>, Vec<String>)> = vec![(None, Vec::new())];
    for name in shown {
        let group = model
            .parameters()
            .find(name)
            .and_then(|id| model.parameters().get(id))
            .and_then(|p| p.group.clone());
        match out.iter_mut().find(|(g, _)| *g == group) {
            Some((_, rows)) => rows.push(name.clone()),
            None => out.push((group, vec![name.clone()])),
        }
    }
    out.retain(|(g, rows)| g.is_some() || !rows.is_empty());
    out
}

/// One parameter: grip, link button, name, slider, number and reset. Returns the row's rectangle.
fn row(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    name: &str,
    selection: &[Selection],
    shift: bool,
    outs: &mut Vec<Out>,
) -> Option<egui::Rect> {
    let m = tsukumoe_ui::Theme::get(ui.ctx()).metrics;
    let id = model.parameters().find(name)?;
    let p = model.parameters().get(id)?;
    let keys = param_ops::primary(model, name, selection).map(|(_, k)| k);
    let mut value = view.param(name, p);
    let mut changed = false;
    let bg = ui.painter().add(egui::Shape::Noop);
    let row = ui.horizontal(|ui| {
        grip(ui, name);
        link_button(ui, model, view, name, outs);
        ui.allocate_ui_with_layout(
            Vec2::new(LABEL_WIDTH, m.control),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                // The slider starts at the same x on every row, whatever the name.
                ui.set_min_width(LABEL_WIDTH);
                name_label(ui, model, view, name, outs);
            },
        );
        let width = (ui.available_width() - 72.0 - RESET_WIDTH).max(40.0);
        let slider_rect = ui
            .allocate_ui(Vec2::new(width, m.control), |ui| {
                let r = slider(ui, &mut value, p.min..=p.max, name);
                changed |= r.changed();
                r.rect
            })
            .inner;
        if let Some(keys) = &keys {
            if changed && !shift {
                let threshold = (p.max - p.min) * SNAP_POINTS / slider_rect.width().max(1.0);
                value = param_ops::snap(keys, value, threshold);
            }
            paint_keys(ui, slider_rect, keys, p.min, p.max, value);
        }
        let speed = f64::from((p.max - p.min).abs()) / 200.0;
        changed |= number(ui, &mut value, p.min..=p.max, speed.max(1e-4), name).changed();
        if reset_button(ui, value == p.default) {
            value = p.default;
            changed = true;
        }
    });
    highlight(
        ui,
        bg,
        row.response.rect,
        view.param_focus.as_deref() == Some(name),
    );
    if changed {
        view.set_param(name, p, value);
    }
    Some(row.response.rect)
}

/// The handle at the start of a row: dragging it carries the parameter's name.
fn grip(ui: &mut Ui, name: &str) {
    let color = tsukumoe_ui::Theme::get(ui.ctx()).colors.text_muted;
    let r = tsukumoe_ui::icon(ui, Icon::Grip, color);
    let r = ui.interact(
        r.rect,
        ui.make_persistent_id(("param-grip", name)),
        egui::Sense::drag(),
    );
    if r.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), name.to_owned());
    }
    r.on_hover_cursor(egui::CursorIcon::Grab);
}

/// The parameter being dragged, taken when the pointer is released over `rect`.
fn dropped(ctx: &egui::Context, rect: egui::Rect) -> Option<String> {
    if !egui::DragAndDrop::has_payload_of_type::<String>(ctx)
        || !ctx.input(|i| i.pointer.any_released())
        || !ctx.pointer_interact_pos().is_some_and(|p| rect.contains(p))
    {
        return None;
    }
    egui::DragAndDrop::take_payload::<String>(ctx).map(|s| (*s).clone())
}

/// A drag over a row shows a line above or below it; the release moves the
/// dragged parameter there.
fn drop_on_row(
    ctx: &egui::Context,
    ui: &Ui,
    rect: egui::Rect,
    model: &Model,
    name: &str,
    outs: &mut Vec<Out>,
) {
    let Some(pos) = ctx.pointer_interact_pos().filter(|p| rect.contains(*p)) else {
        return;
    };
    if !egui::DragAndDrop::has_payload_of_type::<String>(ctx) {
        return;
    }
    let after = pos.y > rect.center().y;
    let y = if after { rect.bottom() } else { rect.top() };
    let color = tsukumoe_ui::Theme::get(ctx).colors.accent;
    ui.painter()
        .hline(rect.x_range(), y, egui::Stroke::new(2.0, color));
    if let Some(dragged) = dropped(ctx, rect) {
        outs.extend(param_ops::move_beside(model, &dragged, name, after).map(Out::Op));
    }
}

/// Dropping on a group header puts the parameter into the group.
fn drop_on_header(
    ctx: &egui::Context,
    header: &egui::Response,
    model: &Model,
    group: &str,
    outs: &mut Vec<Out>,
) {
    if egui::DragAndDrop::has_payload_of_type::<String>(ctx) && header.contains_pointer() {
        let color = tsukumoe_ui::Theme::get(ctx).colors.accent;
        ctx.layer_painter(header.layer_id).rect_stroke(
            header.rect,
            3.0,
            egui::Stroke::new(2.0, color),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(dragged) = dropped(ctx, header.rect) {
        outs.extend(param_ops::move_to_group(model, &dragged, group).map(Out::Op));
    }
}

/// The button that puts a value back to its default; dimmed, and inert, when
/// it is there already. True when it was clicked.
fn reset_button(ui: &mut Ui, at_default: bool) -> bool {
    let name = tr(ui.ctx(), "param-reset");
    ui.add_enabled_ui(!at_default, |ui| {
        icon_button(ui, Icon::RotateCcw, &name, Variant::Ghost).clicked()
    })
    .inner
}

/// A tint behind the row of the parameter the keys panel works on.
fn highlight(ui: &Ui, bg: egui::layers::ShapeIdx, rect: egui::Rect, on: bool) {
    if !on {
        return;
    }
    let theme = tsukumoe_ui::Theme::get(ui.ctx());
    ui.painter().set(
        bg,
        egui::epaint::RectShape::filled(
            rect.expand2(Vec2::new(3.0, 1.0)),
            3.0,
            theme.colors.accent.gamma_multiply(0.22),
        ),
    );
}

/// The link button at the start of a row: for a free parameter it opens a
/// list of the others to link with; for a linked one it unlinks the pair.
fn link_button(ui: &mut Ui, model: &Model, view: &ViewState, name: &str, outs: &mut Vec<Out>) {
    let ctx = ui.ctx().clone();
    let linked = view.param_links.iter().any(|(a, b)| a == name || b == name);
    if linked {
        if icon_button(ui, Icon::Unlink, &tr(&ctx, "param-unlink"), Variant::Ghost).clicked() {
            let mut links = view.param_links.clone();
            links.retain(|(a, b)| a != name && b != name);
            outs.push(Out::Links(links));
        }
        return;
    }
    let r = icon_button(ui, Icon::Link, &tr(&ctx, "param-link"), Variant::Ghost);
    egui::Popup::from_toggle_button_response(&r).show(|ui| {
        let free: Vec<String> = model
            .parameters()
            .iter()
            .map(|(_, n, _)| n.to_owned())
            .filter(|n| n != name && !view.param_links.iter().any(|(a, b)| a == n || b == n))
            .collect();
        for other in free {
            if ui.button(&other).clicked() {
                let mut links = view.param_links.clone();
                links.push((name.to_owned(), other));
                outs.push(Out::Links(links));
                ui.close();
            }
        }
    });
}

/// The parameter's name: a click focuses it (the keys panel works on it), a
/// double click opens its settings; the menu also links it with another
/// parameter or unlinks it.
fn name_label(ui: &mut Ui, model: &Model, view: &mut ViewState, name: &str, outs: &mut Vec<Out>) {
    let ctx = ui.ctx().clone();
    let text = if view.param_focus.as_deref() == Some(name) {
        egui::RichText::new(name).strong()
    } else {
        egui::RichText::new(name)
    };
    // A long name is cut to the column; the tooltip shows all of it.
    let r = ui
        .add(
            egui::Label::new(text)
                .truncate()
                .sense(egui::Sense::click()),
        )
        .on_hover_text(name);
    if r.clicked() {
        view.param_focus = Some(name.to_owned());
    }
    if r.double_clicked() {
        view.param_dialog = Some(name.to_owned());
    }
    r.context_menu(|ui| {
        if ui.button(tr(&ctx, "param-settings")).clicked() {
            view.param_dialog = Some(name.to_owned());
            ui.close();
        }
        if ui.button(tr(&ctx, "param-keyforms-pick")).clicked() {
            view.param_source = Some(name.to_owned());
            ui.close();
        }
        // The source is checked again here: it may have been renamed or deleted.
        let source = view
            .param_source
            .clone()
            .filter(|s| s != name && model.parameters().find(s).is_some());
        if let Some(source) = source {
            let label = tr_args(
                &ctx,
                "param-keyforms-put",
                &[("name", Arg::from(source.as_str()))],
            );
            if ui.button(label).clicked() {
                outs.push(Out::Op((
                    Command::MoveParameterKeyforms {
                        from: source.clone(),
                        to: name.to_owned(),
                    },
                    format!("param_keyforms: {source} -> {name}"),
                )));
                view.param_source = None;
                ui.close();
            }
        }
        let linked = view.param_links.iter().any(|(a, b)| a == name || b == name);
        if linked {
            if ui.button(tr(&ctx, "param-unlink")).clicked() {
                let mut links = view.param_links.clone();
                links.retain(|(a, b)| a != name && b != name);
                outs.push(Out::Links(links));
                ui.close();
            }
        } else {
            ui.menu_button(tr(&ctx, "param-link"), |ui| {
                let free: Vec<String> = model
                    .parameters()
                    .iter()
                    .map(|(_, n, _)| n.to_owned())
                    .filter(|n| {
                        n != name && !view.param_links.iter().any(|(a, b)| a == n || b == n)
                    })
                    .collect();
                for other in free {
                    if ui.button(&other).clicked() {
                        let mut links = view.param_links.clone();
                        links.push((name.to_owned(), other));
                        outs.push(Out::Links(links));
                        ui.close();
                    }
                }
            });
        }
    });
}

/// Keys to draw for `name`: the selected element's, else the range ends and the default.
fn keys_for(model: &Model, name: &str, p: &Parameter, selection: &[Selection]) -> Vec<f32> {
    param_ops::primary(model, name, selection)
        .map(|(_, k)| k)
        .unwrap_or_else(|| {
            let mut k = vec![p.min, p.default, p.max];
            k.sort_by(f32::total_cmp);
            k.dedup();
            k
        })
}

/// Two linked parameters as one pad (the way Live2D shows a pair): white
/// circles at the key combinations of the selected element, a dot at the
/// values, two number fields on the right. Click or drag moves both values,
/// snapping to a circle (Shift: no snap).
fn link_row(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    a: &str,
    b: &str,
    selection: &[Selection],
    outs: &mut Vec<Out>,
) {
    let m = tsukumoe_ui::Theme::get(ui.ctx()).metrics;
    let (Some(ia), Some(ib)) = (model.parameters().find(a), model.parameters().find(b)) else {
        return;
    };
    let (Some(pa), Some(pb)) = (model.parameters().get(ia), model.parameters().get(ib)) else {
        return;
    };
    let (xkeys, ykeys) = (
        keys_for(model, a, pa, selection),
        keys_for(model, b, pb, selection),
    );
    let (mut va, mut vb) = (view.param(a, pa), view.param(b, pb));
    let mut changed = false;
    let shift = ui.input(|i| i.modifiers.shift);
    // The names column covers the grip too, so the pad lines up with the sliders.
    let row = Vec2::new(LABEL_WIDTH + GRIP_WIDTH, m.control);
    let gap = ui.spacing().item_spacing.y;
    let focused = [a, b].contains(&view.param_focus.as_deref().unwrap_or_default());
    let bg = ui.painter().add(egui::Shape::Noop);
    let whole = ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(LINK_WIDTH, 3.0 * m.control + 2.0 * gap),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space(m.control + gap);
                link_button(ui, model, view, a, outs);
            },
        );
        // The names and the number fields sit at the top and the bottom of the
        // pad, each as high as a slider row.
        ui.vertical(|ui| {
            for (i, name) in [a, b].into_iter().enumerate() {
                if i == 1 {
                    ui.add_space(m.control);
                }
                ui.allocate_ui_with_layout(
                    row,
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(LABEL_WIDTH + GRIP_WIDTH);
                        name_label(ui, model, view, name, outs);
                    },
                );
            }
        });
        let width =
            (ui.available_width() - NUMBER_WIDTH - RESET_WIDTH - 2.0 * ui.spacing().item_spacing.x)
                .max(60.0);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(width, 3.0 * m.control + 2.0 * gap),
            egui::Sense::click_and_drag(),
        );
        let inner = rect.shrink2(Vec2::new(8.0, m.control / 2.0));
        let to_screen = |x: f32, y: f32| {
            egui::pos2(
                inner.left() + (x - pa.min) / (pa.max - pa.min).max(1e-6) * inner.width(),
                inner.bottom() - (y - pb.min) / (pb.max - pb.min).max(1e-6) * inner.height(),
            )
        };
        let spots: Vec<(egui::Pos2, f32, f32)> = ykeys
            .iter()
            .flat_map(|&y| xkeys.iter().map(move |&x| (x, y)))
            .map(|(x, y)| (to_screen(x, y), x, y))
            .collect();
        if (response.clicked() || response.dragged())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            va = pa.min
                + ((pointer.x - inner.left()) / inner.width()).clamp(0.0, 1.0) * (pa.max - pa.min);
            vb = pb.min
                + ((inner.bottom() - pointer.y) / inner.height()).clamp(0.0, 1.0)
                    * (pb.max - pb.min);
            if !shift
                && let Some(&(_, x, y)) = spots
                    .iter()
                    .filter(|(pos, ..)| pos.distance(pointer) <= SNAP_POINTS * 1.5)
                    .min_by(|s, t| s.0.distance(pointer).total_cmp(&t.0.distance(pointer)))
            {
                (va, vb) = (x, y);
            }
            changed = true;
        }
        let theme = tsukumoe_ui::Theme::get(ui.ctx());
        let painter = ui.painter_at(rect);
        let dotted = egui::Stroke::new(1.0, theme.colors.border);
        for &x in &xkeys {
            painter.line_segment([to_screen(x, pb.max), to_screen(x, pb.min)], dotted);
        }
        for &y in &ykeys {
            painter.line_segment([to_screen(pa.min, y), to_screen(pa.max, y)], dotted);
        }
        for (pos, ..) in &spots {
            painter.circle(
                *pos,
                4.0,
                theme.colors.surface_raised,
                egui::Stroke::new(1.0, theme.colors.text_muted),
            );
        }
        let now = to_screen(va.clamp(pa.min, pa.max), vb.clamp(pb.min, pb.max));
        painter.circle_filled(now, 4.5, theme.colors.accent);
        ui.vertical(|ui| {
            let speed = |p: &Parameter| (f64::from(p.max - p.min) / 200.0).max(1e-4);
            for (i, (v, p, name)) in [(&mut va, pa, a), (&mut vb, pb, b)].into_iter().enumerate() {
                if i == 1 {
                    ui.add_space(m.control);
                }
                ui.allocate_ui_with_layout(
                    Vec2::new(NUMBER_WIDTH, m.control),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| changed |= number(ui, v, p.min..=p.max, speed(p), name).changed(),
                );
            }
        });
        // One button puts both back.
        ui.allocate_ui_with_layout(
            Vec2::new(RESET_WIDTH, 3.0 * m.control + 2.0 * gap),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_space(m.control + gap);
                if reset_button(ui, va == pa.default && vb == pb.default) {
                    (va, vb) = (pa.default, pb.default);
                    changed = true;
                }
            },
        );
    });
    highlight(ui, bg, whole.response.rect, focused);
    if changed {
        view.set_param(a, pa, va);
        view.set_param(b, pb, vb);
    }
}

/// The settings of one parameter: its name and range, repeat and delete.
pub(super) fn settings_section(
    ui: &mut Ui,
    model: &Model,
    name: &str,
    p: &Parameter,
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    // The name: edited in a buffer, sent when the field is left.
    let buffer_id = ui.make_persistent_id(("param-name", name));
    let mut text: String = ui
        .data_mut(|d| d.get_temp(buffer_id))
        .unwrap_or_else(|| name.to_owned());
    ui.horizontal(|ui| {
        // The delete button first: the name field takes the width that is left.
        if icon_button(ui, Icon::Trash, &t("param-delete"), Variant::Danger).clicked() {
            match param_ops::delete_parameter(model, name) {
                Some(op) => outs.push(Out::Confirm(Kind::Parameter, name.to_owned(), op)),
                None => outs.push(Out::Error("param-err-not-bound")),
            }
        }
        let response = text_input(ui, &mut text, &t("param-name"), &t("param-name"));
        if response.lost_focus() {
            if text != name && !text.is_empty() {
                outs.push(Out::Rename {
                    from: name.to_owned(),
                    to: text.clone(),
                });
            }
            text = name.to_owned();
        }
    });
    ui.data_mut(|d| d.insert_temp(buffer_id, text));
    // The group: edited in a buffer like the name; empty takes the parameter out of it.
    let group_id = ui.make_persistent_id(("param-group-name", name));
    let current = p.group.clone().unwrap_or_default();
    let mut group: String = ui
        .data_mut(|d| d.get_temp(group_id))
        .unwrap_or_else(|| current.clone());
    let response = text_input(ui, &mut group, &t("param-group"), &t("param-group"));
    if response.lost_focus() {
        if group != current {
            outs.push(Out::Op((
                Command::SetParameterGroup {
                    name: name.to_owned(),
                    group: (!group.is_empty()).then(|| group.clone()),
                },
                format!("param_group: {name}"),
            )));
        }
        group = current;
    }
    ui.data_mut(|d| d.insert_temp(group_id, group));
    // The range and the default: edited in a buffer, sent when a drag or an entry ends.
    let range_id = ui.make_persistent_id(("param-range", name));
    let mut vals: [f32; 3] = ui
        .data_mut(|d| d.get_temp(range_id))
        .unwrap_or([p.min, p.default, p.max]);
    let mut done = false;
    let mut touched = false;
    let mut repeat = p.repeat;
    ui.horizontal(|ui| {
        for (i, key) in ["param-min", "param-default", "param-max"]
            .into_iter()
            .enumerate()
        {
            ui.label(t(key));
            let r = ui
                .add(
                    egui::DragValue::new(&mut vals[i])
                        .speed(0.01)
                        .max_decimals(3),
                )
                .accessible_name(t(key));
            touched |= r.changed();
            done |= r.drag_stopped() || r.lost_focus();
        }
        if ui.checkbox(&mut repeat, t("param-repeat")).changed() {
            outs.push(Out::Op((
                Command::SetParameterRepeat {
                    name: name.to_owned(),
                    repeat,
                },
                format!("param_repeat: {name}"),
            )));
        }
    });
    if done && vals != [p.min, p.default, p.max] {
        outs.push(Out::Op((
            Command::SetParameterRange {
                name: name.to_owned(),
                min: vals[0],
                max: vals[2],
                default: vals[1],
            },
            format!("param_range: {name}"),
        )));
    }
    ui.data_mut(|d| {
        if done || !touched && vals == [p.min, p.default, p.max] {
            d.remove::<[f32; 3]>(range_id);
        } else {
            d.insert_temp(range_id, vals);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
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

    #[test]
    fn the_filter_matches_names_case_insensitively() {
        let m = model();
        assert_eq!(names(&m, "").len(), 3);
        assert_eq!(names(&m, "angle"), ["ParamAngleX", "ParamAngleY"]);
        assert_eq!(names(&m, "EYE"), ["ParamEyeOpen"]);
        assert!(names(&m, "nope").is_empty());
    }

    #[test]
    fn changing_a_value_does_not_touch_the_model_or_history() {
        let m = model();
        let mut view = ViewState::default();
        let id = m.parameters().find("ParamAngleX").unwrap();
        let p = m.parameters().get(id).unwrap();
        view.set_param("ParamAngleX", p, 0.5);
        assert_eq!(view.param("ParamAngleX", p), 0.5);
        assert_eq!(m.history().commands().count(), 3, "only the three adds");
        assert_eq!(p.default, 0.0);
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
    fn the_settings_window_keeps_its_width_from_frame_to_frame() {
        use egui_kittest::kittest::Queryable;
        let mut p = panel();
        p.view.param_dialog = Some("ParamAngleX".into());
        let mut h = harness(p);
        for _ in 0..3 {
            h.run();
        }
        let width = |h: &Harness<'static, Panel>| {
            h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Name")
                .rect()
                .right()
        };
        let before = width(&h);
        for _ in 0..30 {
            h.step();
        }
        assert!((width(&h) - before).abs() < 0.5, "the window grew");
    }

    #[test]
    fn the_new_button_sends_an_add_parameter() {
        let mut h = harness(panel());
        h.run();
        h.get_by_label("New parameter").click();
        h.run();
        assert!(matches!(
            h.state().outs.last(),
            Some(Out::Op((Command::AddParameter { .. }, _)))
        ));
    }

    #[test]
    fn the_reset_button_puts_one_parameter_back_and_is_inert_at_the_default() {
        let mut p = panel();
        let id = p.model.parameters().find("ParamAngleX").unwrap();
        let param = p.model.parameters().get(id).unwrap().clone();
        p.view.set_param("ParamAngleX", &param, 0.5);
        let mut h = harness(p);
        h.run();
        let enabled: Vec<_> = h
            .get_all_by_label("Reset to the default")
            .filter(|n| !n.accesskit_node().is_disabled())
            .collect();
        assert_eq!(enabled.len(), 1, "only the moved parameter can be reset");
        enabled[0].click();
        h.run();
        assert_eq!(h.state().view.param("ParamAngleX", &param), param.default);
    }

    #[test]
    fn parameters_are_listed_by_group_with_the_ungrouped_first() {
        let mut m = model();
        let names: Vec<String> = m
            .parameters()
            .iter()
            .map(|(_, n, _)| n.to_owned())
            .collect();
        m.apply(Command::SetParameterGroup {
            name: names[0].clone(),
            group: Some("Head".into()),
        })
        .unwrap();
        let s = sections(&m, &names);
        assert_eq!(s[0].0, None);
        assert_eq!(s[1], (Some("Head".to_owned()), vec![names[0].clone()]));
        assert_eq!(s[0].1, names[1..]);
    }

    #[test]
    fn a_group_shows_as_a_header() {
        use egui_kittest::kittest::Queryable;
        let mut p = panel();
        let first = p.model.parameters().iter().next().unwrap().1.to_owned();
        p.model
            .apply(Command::SetParameterGroup {
                name: first,
                group: Some("Head".into()),
            })
            .unwrap();
        let mut h = harness(p);
        h.run();
        h.get_by_label("Head");
    }
}
