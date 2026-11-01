// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What proportional edit shows in the viewport: a small options box (on or
//! off, radius, falloff curve, distance kind) and, over the shape, the circle
//! of influence around what is moved and a dot per point sized by its weight.

use egui::{Painter, Pos2, Sense, Shape, Stroke, pos2, vec2};
use tsukumoe_core::proportional::{Distance, Falloff};
use tsukumoe_ui::{Icon, Theme, Variant, icon_button, tr};

use crate::proportional::{Proportional, RADIUS_RANGE};

/// Circles are drawn around at most this many moved points.
const MAX_CIRCLES: usize = 16;

fn falloff_key(f: Falloff) -> &'static str {
    match f {
        Falloff::Smooth => "proportional-falloff-smooth",
        Falloff::Linear => "proportional-falloff-linear",
        Falloff::Sharp => "proportional-falloff-sharp",
        Falloff::Constant => "proportional-falloff-constant",
    }
}

fn next_falloff(f: Falloff) -> Falloff {
    match f {
        Falloff::Smooth => Falloff::Linear,
        Falloff::Linear => Falloff::Sharp,
        Falloff::Sharp => Falloff::Constant,
        Falloff::Constant => Falloff::Smooth,
    }
}

/// A button that shows the curve itself and cycles to the next one.
fn falloff_button(ui: &mut egui::Ui, falloff: &mut Falloff, name: &str) {
    let theme = Theme::get(ui.ctx());
    let size = theme.metrics.control;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    if response.clicked() {
        *falloff = next_falloff(*falloff);
    }
    let c = &theme.colors;
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, theme.metrics.radius, c.control_hover);
    }
    let inner = rect.shrink(size * 0.25);
    let curve: Vec<Pos2> = (0..=12)
        .map(|i| {
            let t = i as f32 / 12.0;
            let w = falloff.weight(t, 1.0);
            pos2(
                inner.left() + t * inner.width(),
                inner.bottom() - w * inner.height(),
            )
        })
        .collect();
    ui.painter()
        .add(Shape::line(curve, Stroke::new(1.5, c.text)));
    response.on_hover_text(name);
}

/// The options box at the top left, under the help line. Returns true when
/// anything changed.
pub fn options(ctx: &egui::Context, rect: egui::Rect, p: &mut Proportional) -> bool {
    let theme = Theme::get(ctx);
    let m = theme.metrics;
    let before = *p;
    egui::Area::new(egui::Id::unique("tsukumoe-proportional-options"))
        .fixed_pos(rect.left_top() + vec2(8.0, 32.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme.colors.surface_raised)
                .stroke(Stroke::new(1.0, theme.colors.border))
                .corner_radius(m.radius)
                .inner_margin(egui::Margin::symmetric(m.pad as i8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let variant = if p.on {
                            Variant::Primary
                        } else {
                            Variant::Ghost
                        };
                        if icon_button(ui, Icon::Locate, &tr(ctx, "proportional-on"), variant)
                            .clicked()
                        {
                            p.on = !p.on;
                        }
                        if !p.on {
                            return;
                        }
                        ui.add(
                            egui::DragValue::new(&mut p.radius)
                                .range(RADIUS_RANGE.0..=RADIUS_RANGE.1)
                                .speed(1.0)
                                .max_decimals(0)
                                .suffix(" pt"),
                        )
                        .accessible_name(tr(ctx, "proportional-radius"))
                        .on_hover_text(tr(ctx, "proportional-radius"));
                        let curve = format!(
                            "{}: {}",
                            tr(ctx, "proportional-falloff"),
                            tr(ctx, falloff_key(p.falloff))
                        );
                        falloff_button(ui, &mut p.falloff, &curve);
                        let (icon, key) = match p.distance {
                            Distance::Path => (Icon::Grid, "proportional-distance-path"),
                            Distance::Straight => (Icon::Minus, "proportional-distance-straight"),
                        };
                        let name =
                            format!("{}: {}", tr(ctx, "proportional-distance"), tr(ctx, key));
                        if icon_button(ui, icon, &name, Variant::Ghost).clicked() {
                            p.distance = match p.distance {
                                Distance::Path => Distance::Straight,
                                Distance::Straight => Distance::Path,
                            };
                        }
                    });
                });
        });
    *p != before
}

/// The circle of influence around each moved point (`moved`, screen points)
/// and, while a drag is on (`weights` given with the `points` they belong
/// to), a dot per weighted point: bigger and stronger the more it follows.
pub fn paint(
    painter: &Painter,
    theme: &Theme,
    p: &Proportional,
    moved: &[Pos2],
    weighted: Option<(&[Pos2], &[f32])>,
) {
    let c = &theme.colors;
    if moved.len() <= MAX_CIRCLES {
        for &at in moved {
            painter.circle_stroke(at, p.radius, Stroke::new(1.0, c.accent.gamma_multiply(0.6)));
        }
    }
    if let Some((points, weights)) = weighted {
        let clip = painter.clip_rect().expand(8.0);
        for (&at, &w) in points.iter().zip(weights) {
            if w > 0.0 && w < 1.0 && clip.contains(at) {
                painter.circle_filled(at, 4.5 + 4.0 * w, c.accent.gamma_multiply(0.3 + 0.6 * w));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn harness(p: Proportional) -> Harness<'static, Proportional> {
        Harness::builder()
            .with_size(vec2(420.0, 160.0))
            .build_ui_state(
                |ui, p: &mut Proportional| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    options(ui.ctx(), ui.max_rect(), p);
                },
                p,
            )
    }

    #[test]
    fn the_box_shows_the_other_controls_only_while_on_and_each_button_changes_its_option() {
        let mut h = harness(Proportional::default());
        h.run();
        assert!(h.query_by_label("Influence radius").is_none());
        h.get_by_label("Proportional edit").click();
        h.run();
        assert!(h.state().on);
        h.get_by_label("Influence radius");
        h.get_by_label("Falloff: Smooth").click();
        h.run();
        assert_eq!(h.state().falloff, Falloff::Linear);
        h.get_by_label("Falloff: Linear").click();
        h.run();
        assert_eq!(h.state().falloff, Falloff::Sharp);
        h.get_by_label("Distance: Along the edges").click();
        h.run();
        assert_eq!(h.state().distance, Distance::Straight);
        h.get_by_label("Distance: Straight line").click();
        h.run();
        assert_eq!(h.state().distance, Distance::Path);
        h.get_by_label("Proportional edit").click();
        h.run();
        assert!(!h.state().on);
    }

    #[test]
    fn the_falloff_cycles_through_every_curve_and_back() {
        let mut f = Falloff::Smooth;
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(f);
            f = next_falloff(f);
        }
        assert_eq!(f, Falloff::Smooth);
        seen.dedup();
        assert_eq!(seen.len(), 4);
    }
}
