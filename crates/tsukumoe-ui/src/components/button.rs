// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use egui::epaint::StrokeKind;
use egui::{
    Color32, CursorIcon, Rect, Response, Role, Sense, Stroke, TextStyle, Ui, Vec2, WidgetInfo, vec2,
};

use crate::icons::Icon;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// The main action of a view.
    Primary,
    Secondary,
    /// No fill until hovered: toolbars and rows.
    Ghost,
    /// A destructive action.
    Danger,
}

/// A text button. Disable it with `ui.add_enabled_ui(false, ..)`.
pub fn button(ui: &mut Ui, text: &str, variant: Variant) -> Response {
    action(ui, None, Some(text), text, variant)
}

/// An icon-only button. `name` is what a screen reader announces and what the
/// tooltip shows.
pub fn icon_button(ui: &mut Ui, icon: Icon, name: &str, variant: Variant) -> Response {
    action(ui, Some(icon), None, name, variant).on_hover_text(name)
}

pub fn icon_text_button(ui: &mut Ui, icon: Icon, text: &str, variant: Variant) -> Response {
    action(ui, Some(icon), Some(text), text, variant)
}

fn fill_and_text(t: &Theme, variant: Variant, enabled: bool, r: &Response) -> (Color32, Color32) {
    let c = &t.colors;
    if !enabled {
        let bg = if variant == Variant::Ghost {
            Color32::TRANSPARENT
        } else {
            c.control
        };
        return (bg, c.disabled);
    }
    let (hover, down) = (r.hovered(), r.is_pointer_button_down_on());
    let solid = |base: Color32| {
        let bg = if down {
            Theme::mix(base, Color32::BLACK, 0.18)
        } else if hover {
            Theme::mix(base, Color32::WHITE, 0.12)
        } else {
            base
        };
        (bg, c.accent_text)
    };
    match variant {
        Variant::Primary => solid(c.accent),
        Variant::Danger => solid(c.error),
        Variant::Secondary => {
            let bg = if down {
                c.control_active
            } else if hover {
                c.control_hover
            } else {
                c.control
            };
            (bg, c.text)
        }
        Variant::Ghost => {
            let bg = if down {
                c.control_active
            } else if hover {
                c.control_hover
            } else {
                Color32::TRANSPARENT
            };
            (bg, if hover || down { c.text } else { c.text_muted })
        }
    }
}

fn action(
    ui: &mut Ui,
    icon: Option<Icon>,
    text: Option<&str>,
    name: &str,
    variant: Variant,
) -> Response {
    let t = Theme::get(ui.ctx());
    let m = t.metrics;
    let enabled = ui.is_enabled();
    let galley = text.map(|s| {
        let font = TextStyle::Button.resolve(ui.style());
        ui.painter()
            .layout_no_wrap(s.to_owned(), font, Color32::PLACEHOLDER)
    });
    let text_width = galley.as_ref().map_or(0.0, |g| g.size().x);
    let icon_width = icon.map_or(0.0, |_| m.icon);
    let gap = if icon.is_some() && galley.is_some() {
        m.gap
    } else {
        0.0
    };
    // An icon-only button is a square.
    let pad = if galley.is_some() {
        m.pad
    } else {
        (m.control - m.icon) / 2.0
    };
    let size = vec2(2.0 * pad + icon_width + gap + text_width, m.control);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, name));
    if enabled {
        response.clone().on_hover_cursor(CursorIcon::PointingHand);
    }
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let (bg, fg) = fill_and_text(&t, variant, enabled, &response);
    let radius = m.radius;
    let painter = ui.painter();
    painter.rect_filled(rect, radius, bg);
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(1.0),
            radius + 1.0,
            Stroke::new(1.5, t.colors.accent),
            StrokeKind::Outside,
        );
    }
    let mut x = rect.left() + pad;
    if let Some(icon) = icon {
        let at = Rect::from_min_size(
            egui::pos2(x, rect.center().y - m.icon / 2.0),
            Vec2::splat(m.icon),
        );
        icon.image(fg, m.icon).paint_at(ui, at);
        x += m.icon + gap;
    }
    if let Some(galley) = galley {
        let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, fg);
    }
    response
}
