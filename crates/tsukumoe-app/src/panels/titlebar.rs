// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The window's own title bar and resize edges (the OS frame is off). The
//! drawing code returns a [`WindowOp`] and the editor turns it into a
//! `ViewportCommand`, so what is sent can be tested without a window.

use egui::{
    Align, Context, CursorIcon, Layout, Pos2, Rect, ResizeDirection, Sense, Stroke, Ui,
    ViewportCommand, pos2, vec2,
};
use tsukumoe_ui::{Theme, tr};

/// Thickness of the resize edges, and the length of the corners, in points.
const EDGE: f32 = 5.0;
const CORNER: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowOp {
    Minimize,
    /// Maximize, or restore when already maximized.
    ToggleMaximize,
    Close,
    /// Press on empty bar: move the window.
    Drag,
    Resize(ResizeDirection),
}

/// The command for `op`; `maximized` is the window's state now.
pub fn command(op: WindowOp, maximized: bool) -> ViewportCommand {
    match op {
        WindowOp::Minimize => ViewportCommand::Minimized(true),
        WindowOp::ToggleMaximize => ViewportCommand::Maximized(!maximized),
        WindowOp::Close => ViewportCommand::Close,
        WindowOp::Drag => ViewportCommand::StartDrag,
        WindowOp::Resize(d) => ViewportCommand::BeginResize(d),
    }
}

/// Who draws the window frame. macOS keeps its own (traffic lights, rounded
/// corners, shadow, resize, full screen) and the content reaches under the
/// title bar; elsewhere the OS frame is off and the app draws buttons and edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chrome {
    Native,
    Custom,
}

pub const CHROME: Chrome = if cfg!(target_os = "macos") {
    Chrome::Native
} else {
    Chrome::Custom
};

/// Room kept free on the left for the macOS traffic lights, in points.
const TRAFFIC_LIGHTS: f32 = 76.0;
/// Height of the bar with the native frame: the traffic lights' centre line.
const NATIVE_HEIGHT: f32 = 33.0;

/// The window's maximized or full-screen state, as the platform reports it.
pub fn is_maximized(ctx: &Context) -> bool {
    ctx.input(|i| {
        let v = i.viewport();
        v.maximized.unwrap_or(false) || v.fullscreen.unwrap_or(false)
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn button(ui: &mut Ui, glyph: Glyph, name: &str) -> bool {
    let theme = Theme::get(ui.ctx());
    let size = vec2(theme.metrics.control * 1.4, theme.metrics.control);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    let c = &theme.colors;
    let close = glyph == Glyph::Close;
    if response.hovered() {
        let fill = if close { c.error } else { c.control_hover };
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    let color = if response.hovered() && close {
        c.accent_text
    } else {
        c.text
    };
    let stroke = Stroke::new(1.2, color);
    let r = 5.0;
    let at = rect.center();
    let p = ui.painter();
    match glyph {
        Glyph::Minimize => {
            p.line_segment([at + vec2(-r, 0.0), at + vec2(r, 0.0)], stroke);
        }
        Glyph::Maximize => {
            p.rect_stroke(
                Rect::from_center_size(at, vec2(2.0 * r, 2.0 * r)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Glyph::Restore => {
            let back =
                Rect::from_center_size(at + vec2(1.5, -1.5), vec2(2.0 * r - 2.0, 2.0 * r - 2.0));
            let front =
                Rect::from_center_size(at + vec2(-1.0, 1.0), vec2(2.0 * r - 2.0, 2.0 * r - 2.0));
            p.rect_stroke(back, 0.0, stroke, egui::StrokeKind::Inside);
            p.rect_filled(
                front,
                0.0,
                if response.hovered() {
                    c.control_hover
                } else {
                    c.surface
                },
            );
            p.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Inside);
        }
        Glyph::Close => {
            p.line_segment([at + vec2(-r, -r), at + vec2(r, r)], stroke);
            p.line_segment([at + vec2(-r, r), at + vec2(r, -r)], stroke);
        }
    }
    response.on_hover_text(name).clicked()
}

/// The three window buttons: minimize, maximize, close.
pub fn window_buttons(ui: &mut Ui, maximized: bool) -> Option<WindowOp> {
    let ctx = ui.ctx().clone();
    let max_glyph = if maximized {
        Glyph::Restore
    } else {
        Glyph::Maximize
    };
    let max_name = tr(
        &ctx,
        if maximized {
            "window-restore"
        } else {
            "window-maximize"
        },
    );
    let items = [
        (
            Glyph::Minimize,
            tr(&ctx, "window-minimize"),
            WindowOp::Minimize,
        ),
        (max_glyph, max_name, WindowOp::ToggleMaximize),
        (Glyph::Close, tr(&ctx, "window-close"), WindowOp::Close),
    ];
    let mut chosen = None;
    ui.spacing_mut().item_spacing.x = 0.0;
    for (glyph, name, op) in items {
        if button(ui, glyph, &name) {
            chosen = Some(op);
        }
    }
    chosen
}

/// What a press or double click on the empty bar does.
pub fn drag_op(response: &egui::Response) -> Option<WindowOp> {
    if response.double_clicked() {
        Some(WindowOp::ToggleMaximize)
    } else if response.drag_started_by(egui::PointerButton::Primary) {
        Some(WindowOp::Drag)
    } else {
        None
    }
}

/// The title bar row. `left` draws the names, menu and tabs; with the custom
/// frame the window buttons go to the right, with the native one the traffic
/// lights' room is left free. Returns the window op, if any.
pub fn show(
    ui: &mut Ui,
    chrome: Chrome,
    maximized: bool,
    left: impl FnOnce(&mut Ui),
) -> Option<WindowOp> {
    let rect = ui.max_rect();
    // Under everything: widgets added later take the click first.
    let bar = ui.interact(rect, ui.scope_id().with("bar"), Sense::click_and_drag());
    let mut op = drag_op(&bar);
    if chrome == Chrome::Native {
        ui.set_min_height(NATIVE_HEIGHT);
    }
    ui.horizontal(|ui| {
        if chrome == Chrome::Native {
            ui.add_space(TRAFFIC_LIGHTS);
        }
        left(ui);
        if chrome == Chrome::Custom {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Right to left: reverse the reading order.
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut inner = None;
                ui.scope(|ui| inner = window_buttons_rtl(ui, maximized));
                op = inner.or(op);
            });
        }
    });
    op
}

fn window_buttons_rtl(ui: &mut Ui, maximized: bool) -> Option<WindowOp> {
    // `window_buttons` lays out left to right inside its own row, so keep the
    // group together and let the layout place the group at the right edge.
    ui.horizontal(|ui| window_buttons(ui, maximized)).inner
}

/// The eight edges and corners as rectangles inside `screen`, corners first
/// so they win where they overlap an edge.
pub fn edges(screen: Rect) -> [(ResizeDirection, Rect, CursorIcon); 8] {
    let (l, t, r, b) = (screen.left(), screen.top(), screen.right(), screen.bottom());
    let rect = |a: Pos2, c: Pos2| Rect::from_min_max(a, c);
    [
        (
            ResizeDirection::NorthWest,
            rect(pos2(l, t), pos2(l + CORNER, t + CORNER)),
            CursorIcon::ResizeNorthWest,
        ),
        (
            ResizeDirection::NorthEast,
            rect(pos2(r - CORNER, t), pos2(r, t + CORNER)),
            CursorIcon::ResizeNorthEast,
        ),
        (
            ResizeDirection::SouthWest,
            rect(pos2(l, b - CORNER), pos2(l + CORNER, b)),
            CursorIcon::ResizeSouthWest,
        ),
        (
            ResizeDirection::SouthEast,
            rect(pos2(r - CORNER, b - CORNER), pos2(r, b)),
            CursorIcon::ResizeSouthEast,
        ),
        (
            ResizeDirection::North,
            rect(pos2(l + CORNER, t), pos2(r - CORNER, t + EDGE)),
            CursorIcon::ResizeNorth,
        ),
        (
            ResizeDirection::South,
            rect(pos2(l + CORNER, b - EDGE), pos2(r - CORNER, b)),
            CursorIcon::ResizeSouth,
        ),
        (
            ResizeDirection::West,
            rect(pos2(l, t + CORNER), pos2(l + EDGE, b - CORNER)),
            CursorIcon::ResizeWest,
        ),
        (
            ResizeDirection::East,
            rect(pos2(r - EDGE, t + CORNER), pos2(r, b - CORNER)),
            CursorIcon::ResizeEast,
        ),
    ]
}

/// The thin outline of the window and, unless maximized or full screen, the
/// resize edges. Returns the resize to begin.
pub fn frame(ctx: &Context, chrome: Chrome, maximized: bool) -> Option<WindowOp> {
    if chrome == Chrome::Native {
        return None;
    }
    let theme = Theme::get(ctx);
    let screen = ctx.content_rect();
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("tsukumoe-window-frame"),
    ))
    .rect_stroke(
        screen,
        0.0,
        Stroke::new(1.0, theme.colors.border_strong),
        egui::StrokeKind::Inside,
    );
    if maximized {
        return None;
    }
    let mut op = None;
    for (i, (direction, rect, cursor)) in edges(screen).into_iter().enumerate() {
        egui::Area::new(egui::Id::unique("tsukumoe-resize").with(i))
            .fixed_pos(rect.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(rect, Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(cursor);
                }
                if response.drag_started_by(egui::PointerButton::Primary) {
                    op = Some(WindowOp::Resize(direction));
                }
            });
    }
    op
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn every_op_is_its_viewport_command() {
        assert_eq!(
            command(WindowOp::Minimize, false),
            ViewportCommand::Minimized(true)
        );
        assert_eq!(
            command(WindowOp::ToggleMaximize, false),
            ViewportCommand::Maximized(true)
        );
        assert_eq!(
            command(WindowOp::ToggleMaximize, true),
            ViewportCommand::Maximized(false)
        );
        assert_eq!(command(WindowOp::Close, false), ViewportCommand::Close);
        assert_eq!(command(WindowOp::Drag, false), ViewportCommand::StartDrag);
        assert_eq!(
            command(WindowOp::Resize(ResizeDirection::East), false),
            ViewportCommand::BeginResize(ResizeDirection::East)
        );
    }

    #[test]
    fn the_edges_cover_the_sides_and_corners_without_overlap() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        let e = edges(screen);
        let mut seen: Vec<ResizeDirection> = e.iter().map(|(d, ..)| *d).collect();
        seen.dedup();
        assert_eq!(seen.len(), 8);
        for (i, (_, a, _)) in e.iter().enumerate() {
            assert!(screen.contains_rect(*a));
            for (_, b, _) in &e[i + 1..] {
                assert!(!a.intersects(b.shrink(0.01)), "{a:?} {b:?}");
            }
        }
        let at = |d: ResizeDirection| e.iter().find(|(x, ..)| *x == d).unwrap().1;
        assert!(at(ResizeDirection::East).contains(pos2(399.0, 150.0)));
        assert!(at(ResizeDirection::North).contains(pos2(200.0, 1.0)));
        assert!(at(ResizeDirection::SouthWest).contains(pos2(2.0, 298.0)));
    }

    fn harness(maximized: bool) -> Harness<'static, Option<WindowOp>> {
        Harness::builder()
            .with_size(vec2(500.0, 60.0))
            .build_ui_state(
                move |ui, op: &mut Option<WindowOp>| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    if let Some(o) = show(ui, Chrome::Custom, maximized, |ui| {
                        ui.label("Tsukumoe");
                    }) {
                        *op = Some(o);
                    }
                },
                None,
            )
    }

    #[test]
    fn the_native_frame_has_no_buttons_or_edges_and_leaves_room_for_the_traffic_lights() {
        let mut h = Harness::builder()
            .with_size(vec2(500.0, 60.0))
            .build_ui_state(
                |ui, left: &mut f32| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    show(ui, Chrome::Native, false, |ui| {
                        *left = ui.cursor().left();
                    });
                    assert!(frame(ui.ctx(), Chrome::Native, false).is_none());
                },
                0.0,
            );
        h.run();
        assert!(h.query_by_label("Close").is_none());
        assert!(h.query_by_label("Minimize").is_none());
        assert!(*h.state() >= TRAFFIC_LIGHTS);
    }

    #[test]
    fn each_window_button_sends_its_op_and_the_maximize_button_names_its_state() {
        let mut h = harness(false);
        h.run();
        for (label, op) in [
            ("Minimize", WindowOp::Minimize),
            ("Maximize", WindowOp::ToggleMaximize),
            ("Close", WindowOp::Close),
        ] {
            *h.state_mut() = None;
            h.get_by_label(label).click();
            h.run();
            assert_eq!(*h.state(), Some(op), "{label}");
        }
        let mut h = harness(true);
        h.run();
        h.get_by_label("Restore").click();
        h.run();
        assert_eq!(*h.state(), Some(WindowOp::ToggleMaximize));
    }
}
