// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The pendulum diagram: the particles of a group as a chain on a canvas. A
//! particle drags to its place, and its radius (the distance to the particle
//! before it) follows. The drag works on a copy and the result goes out once,
//! when the pointer lets go.

use egui::{Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use tsukumoe_core::physics::Particle;
use tsukumoe_ui::Theme;

const HEIGHT: f32 = 180.0;
const MARGIN: f32 = 20.0;
const HIT: f32 = 10.0;
const DOT: f32 = 6.0;
/// The smallest extent (in model units) the canvas fits, so a short chain is
/// not blown up.
const MIN_EXTENT: f32 = 5.0;

/// Where the model's origin sits on screen, and how many points a unit is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    origin: Pos2,
    scale: f32,
}

/// A drag in progress.
#[derive(Clone, Debug)]
pub struct Work {
    particles: Vec<Particle>,
    /// Kept for the whole drag so the picture does not rescale under the pointer.
    view: View,
    dragging: usize,
}

fn view_of(rect: Rect, particles: &[Particle]) -> View {
    let (mut max_x, mut min_y, mut max_y) = (MIN_EXTENT, 0.0f32, MIN_EXTENT);
    for p in particles {
        max_x = max_x.max(p.position[0].abs());
        min_y = min_y.min(p.position[1]);
        max_y = max_y.max(p.position[1]);
    }
    let scale = ((rect.width() / 2.0 - MARGIN) / max_x)
        .min((rect.height() - 2.0 * MARGIN) / (max_y - min_y))
        .max(f32::EPSILON);
    View {
        origin: pos2(rect.center().x, rect.top() + MARGIN - min_y * scale),
        scale,
    }
}

fn to_screen(v: &View, p: [f32; 2]) -> Pos2 {
    pos2(v.origin.x + p[0] * v.scale, v.origin.y + p[1] * v.scale)
}

fn to_model(v: &View, p: Pos2) -> [f32; 2] {
    [(p.x - v.origin.x) / v.scale, (p.y - v.origin.y) / v.scale]
}

/// The particles with particle `index` moved to `to`; its radius becomes the
/// distance to the particle before it. The root stays where it is.
pub fn moved(particles: &[Particle], index: usize, to: [f32; 2]) -> Vec<Particle> {
    let mut out = particles.to_vec();
    if index == 0 || index >= out.len() {
        return out;
    }
    let parent = out[index - 1].position;
    out[index].position = to;
    out[index].radius = (to[0] - parent[0]).hypot(to[1] - parent[1]);
    out
}

/// Draws the chain and handles the drag; the particles once a drag that
/// changed them ends.
pub fn show(ui: &mut Ui, particles: &[Particle], work: &mut Option<Work>) -> Option<Vec<Particle>> {
    let theme = Theme::get(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::click_and_drag());
    let view = work
        .as_ref()
        .map_or_else(|| view_of(rect, particles), |w| w.view);
    let shown: &[Particle] = work.as_ref().map_or(particles, |w| &w.particles);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme.metrics.radius, theme.colors.surface);
    let at: Vec<Pos2> = shown.iter().map(|p| to_screen(&view, p.position)).collect();
    for pair in at.windows(2) {
        painter.line_segment(
            [pair[0], pair[1]],
            Stroke::new(2.0, theme.colors.border_strong),
        );
    }
    let dragging = work.as_ref().map(|w| w.dragging);
    let hover = response
        .hover_pos()
        .and_then(|h| (1..at.len()).find(|&i| (at[i] - h).length() <= HIT));
    for (i, p) in at.iter().enumerate() {
        let lit = dragging == Some(i) || (dragging.is_none() && hover == Some(i));
        let fill = if i == 0 {
            theme.colors.text_muted
        } else if lit {
            theme.colors.accent
        } else {
            theme.colors.text
        };
        painter.add(Shape::circle_filled(*p, DOT, fill));
    }
    if dragging.is_none() && hover.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    let pointer = response.interact_pointer_pos();
    if response.drag_started()
        && let Some(p) = pointer
        && let Some(i) = (1..at.len()).find(|&i| (at[i] - p).length() <= HIT)
    {
        *work = Some(Work {
            particles: particles.to_vec(),
            view,
            dragging: i,
        });
    }
    if response.dragged()
        && let (Some(w), Some(p)) = (work.as_mut(), pointer)
    {
        w.particles = moved(&w.particles, w.dragging, to_model(&w.view, p));
    }
    if response.drag_stopped() {
        return work
            .take()
            .map(|w| w.particles)
            .filter(|ps| ps != particles);
    }
    if !response.dragged() {
        *work = None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> Vec<Particle> {
        let p = |y: f32, radius: f32| Particle {
            position: [0.0, y],
            mobility: 0.9,
            delay: 0.9,
            acceleration: 1.5,
            radius,
        };
        vec![p(0.0, 0.0), p(10.0, 10.0), p(20.0, 10.0)]
    }

    #[test]
    fn a_moved_particle_takes_its_distance_to_the_one_before_as_radius() {
        let out = moved(&chain(), 2, [6.0, 18.0]);
        assert_eq!(out[2].position, [6.0, 18.0]);
        assert!((out[2].radius - 10.0).abs() < 1e-5, "{}", out[2].radius);
        // The others are as they were, and the root never moves.
        assert_eq!(out[1], chain()[1]);
        assert_eq!(moved(&chain(), 0, [3.0, 3.0]), chain());
        assert_eq!(moved(&chain(), 9, [3.0, 3.0]), chain());
    }

    #[test]
    fn the_view_fits_the_chain_and_maps_both_ways() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, HEIGHT));
        let mut ps = chain();
        ps[2].position = [-12.0, -4.0];
        let v = view_of(rect, &ps);
        for p in &ps {
            let s = to_screen(&v, p.position);
            assert!(rect.shrink(1.0).contains(s), "{s:?} inside {rect:?}");
            let back = to_model(&v, s);
            assert!(
                (back[0] - p.position[0]).abs() < 1e-3 && (back[1] - p.position[1]).abs() < 1e-3
            );
        }
    }
}
