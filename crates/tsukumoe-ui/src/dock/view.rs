// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Draws a [`Layout`]: split areas with draggable dividers, tab groups whose
//! tabs can be dragged to another group, an edge of a group or a side strip,
//! and the side strips with their pinned or floating panels. Every change is
//! collected as an [`Action`] while drawing and applied afterwards.

use egui::{
    Align2, Color32, Context, CursorIcon, DragAndDrop, FontId, Id, Order, Rect, Sense, Stroke, Ui,
    UiBuilder, pos2, vec2,
};

use super::model::{Edge, Layout, Node, Side, Target};
use crate::icons::Icon;
use crate::lang::tr;
use crate::theme::Theme;

/// What the dock needs from the app: names, optional icons, and the content.
pub trait PanelHost {
    fn title(&self, ctx: &Context, id: &str) -> String;
    fn icon(&self, _id: &str) -> Option<Icon> {
        None
    }
    fn ui(&mut self, ui: &mut Ui, id: &str);
}

#[derive(Clone, Debug)]
struct Dragged(String);

#[derive(Debug)]
enum Action {
    Ratio(Vec<bool>, f32),
    Activate(String),
    Move(String, Target),
    Toggle(Side, String),
    Pin(Side, bool),
    Width(Side, f32),
    CloseOverlays,
}

struct Zone {
    rect: Rect,
    kind: ZoneKind,
}

enum ZoneKind {
    Group(Vec<String>),
    Strip(Side),
}

#[derive(Default)]
struct Frame {
    actions: Vec<Action>,
    zones: Vec<Zone>,
}

const GAP: f32 = 4.0;

/// The edge of `rect` the point is near (within a quarter), or `None` for the
/// middle, which means "as a tab".
pub fn drop_edge(rect: Rect, p: egui::Pos2) -> Option<Edge> {
    let f = pos2(
        (p.x - rect.left()) / rect.width().max(1.0),
        (p.y - rect.top()) / rect.height().max(1.0),
    );
    // The nearest edge wins when the point is within a quarter of two.
    let candidates = [
        (f.x, Edge::Left),
        (1.0 - f.x, Edge::Right),
        (f.y, Edge::Top),
        (1.0 - f.y, Edge::Bottom),
    ];
    candidates
        .into_iter()
        .filter(|(d, _)| *d < 0.25)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, e)| e)
}

fn edge_rect(rect: Rect, edge: Edge) -> Rect {
    match edge {
        Edge::Left => Rect::from_min_size(rect.min, vec2(rect.width() / 2.0, rect.height())),
        Edge::Right => Rect::from_min_max(pos2(rect.center().x, rect.top()), rect.max),
        Edge::Top => Rect::from_min_size(rect.min, vec2(rect.width(), rect.height() / 2.0)),
        Edge::Bottom => Rect::from_min_max(pos2(rect.left(), rect.center().y), rect.max),
    }
}

/// Draws the dock into the free space of `ui` and applies what the user did.
pub fn show(ui: &mut Ui, layout: &mut Layout, host: &mut impl PanelHost) {
    let ctx = ui.ctx().clone();
    let t = Theme::get(&ctx);
    let whole = ui.available_rect_before_wrap();
    ui.allocate_rect(whole, Sense::hover());
    let dragging = DragAndDrop::payload::<Dragged>(&ctx).is_some();
    let strip_w = t.metrics.control + 6.0;
    let mut frame = Frame::default();
    let mut center = whole;
    let mut overlay: Option<(Side, Rect)> = None;

    for side in [Side::Left, Side::Right] {
        let strip = layout.strip(side);
        if strip.panels.is_empty() && !dragging {
            continue;
        }
        let rect = match side {
            Side::Left => Rect::from_min_size(whole.min, vec2(strip_w, whole.height())),
            Side::Right => Rect::from_min_size(
                pos2(whole.right() - strip_w, whole.top()),
                vec2(strip_w, whole.height()),
            ),
        };
        match side {
            Side::Left => center.min.x += strip_w,
            Side::Right => center.max.x -= strip_w,
        }
        draw_strip(ui, layout, host, side, rect, &mut frame);
        frame.zones.push(Zone {
            rect,
            kind: ZoneKind::Strip(side),
        });
    }
    for side in [Side::Left, Side::Right] {
        let strip = layout.strip(side);
        if strip.open.is_none() {
            continue;
        }
        let w = strip.width.min(center.width() * 0.8);
        let rect = match side {
            Side::Left => Rect::from_min_size(center.min, vec2(w, center.height())),
            Side::Right => Rect::from_min_size(
                pos2(center.right() - w, center.top()),
                vec2(w, center.height()),
            ),
        };
        if strip.pinned {
            match side {
                Side::Left => center.min.x += w + GAP,
                Side::Right => center.max.x -= w + GAP,
            }
            pinned_panel(ui, layout, host, side, rect, &mut frame);
        } else {
            overlay = Some((side, rect));
        }
    }

    draw_node(ui, host, &layout.root, center, &mut Vec::new(), &mut frame);

    if let Some((side, rect)) = overlay {
        let open = layout.strip(side).open.clone().expect("overlay is open");
        floating_panel(&ctx, layout, host, side, rect, &open, &mut frame);
    }
    if dragging {
        handle_drop(&ctx, &mut frame);
    }
    for action in frame.actions {
        match action {
            Action::Ratio(path, r) => drop(layout.set_ratio(&path, r)),
            Action::Activate(id) => layout.activate(&id),
            Action::Move(id, target) => drop(layout.move_panel(&id, target)),
            Action::Toggle(side, id) => layout.toggle_strip_panel(side, &id),
            Action::Pin(side, p) => layout.set_pinned(side, p),
            Action::Width(side, w) => layout.set_strip_width(side, w),
            Action::CloseOverlays => layout.close_overlays(),
        }
    }
}

fn handle_drop(ctx: &Context, frame: &mut Frame) {
    let Some(dragged) = DragAndDrop::payload::<Dragged>(ctx) else {
        return;
    };
    ctx.set_cursor_icon(CursorIcon::Grabbing);
    let Some(p) = ctx.pointer_latest_pos() else {
        return;
    };
    let Some(zone) = frame.zones.iter().rev().find(|z| z.rect.contains(p)) else {
        return;
    };
    let t = Theme::get(ctx);
    let (shade, target) = match &zone.kind {
        ZoneKind::Strip(side) => (zone.rect, Target::Strip(*side)),
        ZoneKind::Group(tabs) => {
            // Prefer an anchor other than the dragged panel itself.
            let anchor = tabs
                .iter()
                .find(|x| **x != dragged.0)
                .or(tabs.first())
                .cloned()
                .unwrap_or_default();
            match drop_edge(zone.rect, p) {
                Some(edge) => (edge_rect(zone.rect, edge), Target::Split { anchor, edge }),
                None => (zone.rect, Target::Tabs { anchor }),
            }
        }
    };
    let [r, g, b, _] = t.colors.accent.to_array();
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::unique("dock-drop")));
    painter.rect_filled(
        shade,
        t.metrics.radius,
        Color32::from_rgba_unmultiplied(r, g, b, 60),
    );
    painter.rect_stroke(
        shade,
        t.metrics.radius,
        Stroke::new(1.0, t.colors.accent),
        egui::StrokeKind::Inside,
    );
    if ctx.input(|i| i.pointer.any_released()) {
        frame.actions.push(Action::Move(dragged.0.clone(), target));
    }
}

fn paint_label(ui: &Ui, rect: Rect, text: &str, color: Color32, size: f32) {
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(size),
        color,
    );
}

fn draw_strip(
    ui: &mut Ui,
    layout: &Layout,
    host: &impl PanelHost,
    side: Side,
    rect: Rect,
    frame: &mut Frame,
) {
    let ctx = ui.ctx().clone();
    let t = Theme::get(&ctx);
    ui.painter().rect_filled(rect, 0.0, t.colors.background);
    let strip = layout.strip(side);
    let size = t.metrics.control + 2.0;
    for (i, id) in strip.panels.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(
                rect.left() + 2.0,
                rect.top() + 4.0 + i as f32 * (size + 2.0),
            ),
            vec2(rect.width() - 4.0, size),
        );
        let open = strip.open.as_deref() == Some(id.as_str());
        let resp = ui.interact(
            r,
            Id::unique(("dock-strip", side as u8, id)),
            Sense::click_and_drag(),
        );
        let title = host.title(&ctx, id);
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Button, true, open, title.as_str())
        });
        if open || resp.hovered() {
            let fill = if open {
                t.colors.control_hover
            } else {
                t.colors.surface
            };
            ui.painter().rect_filled(r, t.metrics.radius, fill);
        }
        let color = if open {
            t.colors.text
        } else {
            t.colors.text_muted
        };
        match host.icon(id) {
            Some(icon) => {
                let at = Rect::from_center_size(r.center(), vec2(t.metrics.icon, t.metrics.icon));
                icon.image(color, t.metrics.icon).paint_at(ui, at);
            }
            None => paint_label(
                ui,
                r,
                &title.chars().take(1).collect::<String>(),
                color,
                t.metrics.text,
            ),
        }
        let resp = resp.on_hover_text(&title);
        if resp.clicked() {
            frame.actions.push(Action::Toggle(side, id.clone()));
        }
        if resp.drag_started() {
            DragAndDrop::set_payload(&ctx, Dragged(id.clone()));
        }
        resp.context_menu(|ui| tab_menu(ui, id, Some(side), frame));
    }
}

fn tab_menu(ui: &mut Ui, id: &str, in_strip: Option<Side>, frame: &mut Frame) {
    let ctx = ui.ctx().clone();
    let mut item = |ui: &mut Ui, key: &str, action: Action| {
        if crate::components::menu_item(ui, &tr(&ctx, key), None).clicked() {
            frame.actions.push(action);
            ui.close();
        }
    };
    if in_strip != Some(Side::Left) {
        item(
            ui,
            "dock-move-left",
            Action::Move(id.to_owned(), Target::Strip(Side::Left)),
        );
    }
    if in_strip != Some(Side::Right) {
        item(
            ui,
            "dock-move-right",
            Action::Move(id.to_owned(), Target::Strip(Side::Right)),
        );
    }
}

fn panel_frame(ui: &mut Ui, rect: Rect, fill: Color32, add: impl FnOnce(&mut Ui)) {
    let t = Theme::get(ui.ctx());
    ui.painter().rect_filled(rect, t.metrics.radius, fill);
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink(t.metrics.gap)));
    child.set_clip_rect(rect);
    add(&mut child);
}

/// A strip panel's header: its title, a pin toggle and a "back to dock" menu.
fn strip_header(
    ui: &mut Ui,
    host: &impl PanelHost,
    side: Side,
    id: &str,
    pinned: bool,
    frame: &mut Frame,
) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        ui.label(host.title(&ctx, id));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon, key) = if pinned {
                (Icon::Lock, "dock-unpin")
            } else {
                (Icon::Unlock, "dock-pin")
            };
            if crate::components::icon_button(ui, icon, &tr(&ctx, key), crate::Variant::Ghost)
                .clicked()
            {
                frame.actions.push(Action::Pin(side, !pinned));
            }
        });
    });
    ui.separator();
}

fn pinned_panel(
    ui: &mut Ui,
    layout: &Layout,
    host: &mut impl PanelHost,
    side: Side,
    rect: Rect,
    frame: &mut Frame,
) {
    let t = Theme::get(ui.ctx());
    let id = layout.strip(side).open.clone().expect("open");
    panel_frame(ui, rect, t.colors.surface, |ui| {
        strip_header(ui, host, side, &id, true, frame);
        host.ui(ui, &id);
    });
    // The resize handle sits on the inner edge.
    let x = match side {
        Side::Left => rect.right(),
        Side::Right => rect.left() - GAP,
    };
    let handle = Rect::from_min_size(pos2(x, rect.top()), vec2(GAP, rect.height()));
    let resp = ui.interact(
        handle,
        Id::unique(("dock-width", side as u8)),
        Sense::drag(),
    );
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let w = match side {
            Side::Left => p.x - rect.left(),
            Side::Right => rect.right() - p.x,
        };
        frame.actions.push(Action::Width(side, w));
    }
}

fn floating_panel(
    ctx: &Context,
    layout: &Layout,
    host: &mut impl PanelHost,
    side: Side,
    rect: Rect,
    id: &str,
    frame: &mut Frame,
) {
    let t = Theme::get(ctx);
    let pinned = layout.strip(side).pinned;
    let area = egui::Area::new(Id::unique(("dock-overlay", side as u8)))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect);
            ui.allocate_rect(rect, Sense::click());
            panel_frame(ui, rect, t.colors.surface_raised, |ui| {
                strip_header(ui, host, side, id, pinned, frame);
                host.ui(ui, id);
            });
        });
    // A press elsewhere (not on this panel, not on a strip button) closes it.
    let strip_w = t.metrics.control + 6.0;
    let strip_rect = match side {
        Side::Left => Rect::from_min_max(
            pos2(rect.left() - strip_w, rect.top()),
            pos2(rect.left(), rect.bottom()),
        ),
        Side::Right => Rect::from_min_max(
            pos2(rect.right(), rect.top()),
            pos2(rect.right() + strip_w, rect.bottom()),
        ),
    };
    let pressed = ctx.input(|i| i.pointer.any_pressed());
    if pressed
        && let Some(p) = ctx.pointer_interact_pos()
        && !area.response.rect.contains(p)
        && !strip_rect.contains(p)
        && !ctx.any_popup_open()
    {
        frame.actions.push(Action::CloseOverlays);
    }
}

fn draw_node(
    ui: &mut Ui,
    host: &mut impl PanelHost,
    node: &Node,
    rect: Rect,
    path: &mut Vec<bool>,
    frame: &mut Frame,
) {
    match node {
        Node::Split { axis, ratio, a, b } => {
            let horizontal = *axis == super::model::Axis::Horizontal;
            let total = if horizontal {
                rect.width()
            } else {
                rect.height()
            };
            let first = ((total - GAP) * ratio).max(0.0);
            let (ra, rb, bar) = if horizontal {
                (
                    Rect::from_min_size(rect.min, vec2(first, rect.height())),
                    Rect::from_min_max(pos2(rect.left() + first + GAP, rect.top()), rect.max),
                    Rect::from_min_size(
                        pos2(rect.left() + first, rect.top()),
                        vec2(GAP, rect.height()),
                    ),
                )
            } else {
                (
                    Rect::from_min_size(rect.min, vec2(rect.width(), first)),
                    Rect::from_min_max(pos2(rect.left(), rect.top() + first + GAP), rect.max),
                    Rect::from_min_size(
                        pos2(rect.left(), rect.top() + first),
                        vec2(rect.width(), GAP),
                    ),
                )
            };
            let id = Id::unique(("dock-split", path.clone()));
            let resp = ui.interact(bar, id, Sense::drag());
            let t = Theme::get(ui.ctx());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(if horizontal {
                    CursorIcon::ResizeHorizontal
                } else {
                    CursorIcon::ResizeVertical
                });
                ui.painter().rect_filled(bar, 0.0, t.colors.border_strong);
            }
            if resp.dragged()
                && let Some(p) = resp.interact_pointer_pos()
            {
                let (pos, start, len) = if horizontal {
                    (p.x, rect.left(), total - GAP)
                } else {
                    (p.y, rect.top(), total - GAP)
                };
                frame.actions.push(Action::Ratio(
                    path.clone(),
                    (pos - start - GAP / 2.0) / len.max(1.0),
                ));
            }
            path.push(false);
            draw_node(ui, host, a, ra, path, frame);
            path.pop();
            path.push(true);
            draw_node(ui, host, b, rb, path, frame);
            path.pop();
        }
        Node::Tabs { tabs, active } => draw_tabs(ui, host, tabs, *active, rect, path, frame),
    }
}

fn draw_tabs(
    ui: &mut Ui,
    host: &mut impl PanelHost,
    tabs: &[String],
    active: usize,
    rect: Rect,
    path: &[bool],
    frame: &mut Frame,
) {
    let ctx = ui.ctx().clone();
    let t = Theme::get(&ctx);
    let m = t.metrics;
    ui.painter().rect_filled(rect, m.radius, t.colors.surface);
    frame.zones.push(Zone {
        rect,
        kind: ZoneKind::Group(tabs.to_vec()),
    });
    let bar_h = m.control + 2.0;
    let mut x = rect.left();
    for (i, id) in tabs.iter().enumerate() {
        let title = host.title(&ctx, id);
        let font = FontId::proportional(m.text);
        let w = ui
            .painter()
            .layout_no_wrap(title.clone(), font.clone(), Color32::PLACEHOLDER)
            .size()
            .x
            + 2.0 * m.pad;
        let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w, bar_h));
        x += w;
        let resp = ui.interact(
            r,
            Id::unique(("dock-tab", path.to_vec(), id)),
            Sense::click_and_drag(),
        );
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Button, true, active == i, title.as_str())
        });
        let on = active == i;
        if resp.hovered() && !on {
            ui.painter()
                .rect_filled(r.shrink2(vec2(2.0, 3.0)), m.radius, t.colors.control_hover);
        }
        let color = if on || resp.hovered() {
            t.colors.text
        } else {
            t.colors.text_muted
        };
        paint_label(ui, r, &title, color, m.text);
        if on {
            let bar = Rect::from_min_max(
                pos2(r.left() + m.gap, r.bottom() - 2.0),
                pos2(r.right() - m.gap, r.bottom()),
            );
            ui.painter().rect_filled(bar, 1.0, t.colors.accent);
        }
        if resp.clicked() || resp.drag_started() {
            frame.actions.push(Action::Activate(id.clone()));
        }
        if resp.drag_started() {
            DragAndDrop::set_payload(&ctx, Dragged(id.clone()));
        }
        resp.context_menu(|ui| tab_menu(ui, id, None, frame));
    }
    ui.painter().hline(
        rect.x_range(),
        rect.top() + bar_h,
        Stroke::new(1.0, t.colors.border),
    );
    let body = Rect::from_min_max(pos2(rect.left(), rect.top() + bar_h + 1.0), rect.max);
    if let Some(id) = tabs.get(active) {
        panel_frame(ui, body, Color32::TRANSPARENT, |ui| host.ui(ui, id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::model::Axis;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    struct Host;
    impl PanelHost for Host {
        fn title(&self, _: &Context, id: &str) -> String {
            format!("title-{id}")
        }
        fn ui(&mut self, ui: &mut Ui, id: &str) {
            ui.label(format!("body-{id}"));
        }
    }

    #[test]
    fn edges_are_picked_within_a_quarter() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        assert_eq!(drop_edge(r, pos2(5.0, 50.0)), Some(Edge::Left));
        assert_eq!(drop_edge(r, pos2(95.0, 50.0)), Some(Edge::Right));
        assert_eq!(drop_edge(r, pos2(50.0, 5.0)), Some(Edge::Top));
        assert_eq!(drop_edge(r, pos2(50.0, 96.0)), Some(Edge::Bottom));
        assert_eq!(drop_edge(r, pos2(50.0, 50.0)), None);
    }

    #[test]
    fn shows_tabs_strips_and_switches_tabs() {
        let mut layout = Layout::new(Node::split(
            Axis::Horizontal,
            0.5,
            Node::tabs(&["a", "b"]),
            Node::tabs(&["c"]),
        ));
        layout.left.panels.push("d".into());
        layout.toggle_strip_panel(Side::Left, "d");
        layout.set_pinned(Side::Left, true);
        let mut harness = Harness::builder()
            .with_size(vec2(600.0, 300.0))
            .build_ui_state(
                |ui, layout: &mut Layout| {
                    crate::init(ui.ctx());
                    show(ui, layout, &mut Host);
                },
                layout,
            );
        harness.run();
        harness.get_by_label("body-a");
        harness.get_by_label("body-c");
        harness.get_by_label("body-d");
        assert!(harness.query_by_label("body-b").is_none());
        harness.get_by_label("title-b").click();
        harness.run();
        harness.get_by_label("body-b");
    }
}
