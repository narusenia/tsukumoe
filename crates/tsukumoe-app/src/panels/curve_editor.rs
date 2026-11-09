// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The curve editor: the curve picked in the timeline on a time × value plane,
//! under the timeline's lanes and sharing its time axis. Keys and Bezier
//! controls drag on a working copy of the curve (like the lanes); the kind of
//! a segment switches with one command.

use egui::{Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use tsukumoe_core::model::Model;
use tsukumoe_core::playback::{Curve, CurveTarget, Segment, SegmentKind};
use tsukumoe_ui::{Theme, Variant, button, radio, tr};

use super::timeline::{LABEL_WIDTH, Out, set_curve, target_name};
use crate::view::ViewState;

const HANDLE_HIT: f32 = 8.0;
const KEY_RADIUS: f32 = 5.0;
const KINDS: [(SegmentKind, &str); 4] = [
    (SegmentKind::Linear, "curve-kind-linear"),
    (SegmentKind::Bezier, "curve-kind-bezier"),
    (SegmentKind::Stepped, "curve-kind-stepped"),
    (SegmentKind::InverseStepped, "curve-kind-inverse-stepped"),
];

/// What a drag on the plane moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drag {
    Keys,
    /// Control `.1` of the Bezier segment `.0`.
    Control(usize, usize),
}

/// Where the plane puts times and values.
struct Plane {
    rect: Rect,
    lo: f32,
    span: f32,
}

impl Plane {
    fn y_of(&self, value: f32) -> f32 {
        self.rect.bottom() - 6.0 - (value - self.lo) / self.span * (self.rect.height() - 12.0)
    }

    fn value_at(&self, y: f32) -> f32 {
        self.lo + (self.rect.bottom() - 6.0 - y) / (self.rect.height() - 12.0) * self.span
    }
}

/// The value range to show: the parameter's range, grown to hold every point.
fn value_range(model: &Model, curve: &Curve) -> (f32, f32) {
    let (mut lo, mut hi) = match curve.target {
        CurveTarget::Parameter(id) => model
            .parameters()
            .get(id)
            .map_or((0.0, 1.0), |p| (p.min, p.max)),
        CurveTarget::PartOpacity(_) => (0.0, 1.0),
    };
    let mut grow = |p: [f32; 2]| {
        lo = lo.min(p[1]);
        hi = hi.max(p[1]);
    };
    grow(curve.start);
    for s in &curve.segments {
        if let Segment::Bezier([a, b, _]) = s {
            grow(*a);
            grow(*b);
        }
        grow(s.end());
    }
    let pad = ((hi - lo) * 0.08).max(1e-3);
    (lo - pad, hi + pad)
}

/// Moves key `key` to `(time, value)`, the time held between its neighbors'.
fn move_clamped(curve: &mut Curve, key: usize, time: f32, value: f32) {
    let lo = key
        .checked_sub(1)
        .and_then(|b| curve.key(b))
        .map_or(0.0, |b| b[0]);
    let hi = curve.key(key + 1).map_or(f32::MAX, |a| a[0]);
    let _ = curve.move_key(key, time.clamp(lo, hi), value);
}

/// Moves every picked key by `(dt, dv)`, the ones ahead of the motion first.
fn move_picked(curve: &mut Curve, picked: &[usize], dt: f32, dv: f32) {
    let mut keys = picked.to_vec();
    keys.sort_unstable();
    if dt > 0.0 {
        keys.reverse();
    }
    for k in keys {
        if let Some([t, v]) = curve.key(k) {
            move_clamped(curve, k, t + dt, v + dv);
        }
    }
}

/// The segment the kind buttons act on: the picked one, else the one that
/// starts at the picked key.
fn segment_of(view: &ViewState, curve: &Curve) -> Option<usize> {
    let last = curve.segments.len().checked_sub(1)?;
    view.timeline
        .segment
        .filter(|s| *s <= last)
        .or(view.timeline.key.map(|k| k.min(last)))
}

pub fn show(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    name: &str,
    ci: usize,
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let theme = Theme::get(&ctx);
    let curve = match &view.timeline.work {
        Some((w, c)) if *w == ci => c.clone(),
        _ => match model
            .motions()
            .find(name)
            .and_then(|id| model.motions().get(id))
            .and_then(|m| m.curves.get(ci))
        {
            Some(c) => c.clone(),
            None => return,
        },
    };
    let st = &mut view.timeline;
    st.picked.retain(|k| *k < curve.key_count());
    match st.key {
        Some(k) if !st.picked.contains(&k) => st.picked = vec![k],
        None => st.picked.clear(),
        _ => {}
    }

    ui.horizontal_wrapped(|ui| {
        ui.label(target_name(model, curve.target));
        let seg = segment_of(view, &curve);
        let on = seg.map(|s| curve.segments[s].kind());
        for (kind, key) in KINDS {
            let r = radio(ui, on == Some(kind), &t(key));
            if r.clicked()
                && let Some(s) = seg
                && on != Some(kind)
                && view.timeline.work.is_none()
            {
                let mut next = curve.clone();
                if next.set_kind(s, kind).is_ok() {
                    view.timeline.segment = Some(s);
                    outs.push(Out::Command(
                        set_curve(name, ci, &next),
                        format!("motion_segment_kind: {name}"),
                    ));
                }
            }
        }
        let many = view.timeline.picked.len() > 1;
        ui.add_enabled_ui(many, |ui| {
            if button(ui, &t("curve-align"), Variant::Secondary).clicked()
                && let Some([_, v]) = view.timeline.key.and_then(|k| curve.key(k))
            {
                let mut next = curve.clone();
                for k in view.timeline.picked.clone() {
                    if let Some([t, _]) = next.key(k) {
                        move_clamped(&mut next, k, t, v);
                    }
                }
                view.timeline.work = Some((ci, next));
            }
        });
    });

    let size = vec2(ui.available_width(), ui.available_height().max(60.0));
    let (outer, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let rect = Rect::from_min_max(pos2(outer.left() + LABEL_WIDTH, outer.top()), outer.max);
    let (lo, hi) = value_range(model, &curve);
    let plane = Plane {
        rect,
        lo,
        span: hi - lo,
    };
    let x_of = |time: f32| rect.left() + view.timeline.x_of(time);
    let painter = ui.painter_at(outer);
    painter.rect_filled(outer, 0.0, theme.colors.surface);
    for v in [hi - (hi - lo) * 0.08, lo + (hi - lo) * 0.08] {
        let y = plane.y_of(v);
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.0, theme.colors.border),
        );
        painter.text(
            pos2(outer.left() + 8.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{v:.2}"),
            egui::FontId::proportional(10.0),
            theme.colors.text_muted,
        );
    }
    let mut points: Vec<Pos2> = Vec::new();
    let mut x = rect.left();
    while x <= rect.right() {
        let time = view.timeline.time_at(x - rect.left());
        points.push(pos2(x, plane.y_of(curve.value_at(time))));
        x += 2.0;
    }
    painter.add(Shape::line(points, Stroke::new(1.5, theme.colors.accent)));
    let px = x_of(view.timeline.time);
    if px >= rect.left() && px <= rect.right() {
        painter.line_segment(
            [pos2(px, rect.top()), pos2(px, rect.bottom())],
            Stroke::new(1.0, theme.colors.text_muted),
        );
    }
    // Keys, and the controls of each Bezier segment.
    let key_pos = |k: usize| {
        let [t, v] = curve.key(k).expect("in range");
        pos2(x_of(t), plane.y_of(v))
    };
    let mut controls: Vec<(usize, usize, Pos2)> = Vec::new();
    for (i, s) in curve.segments.iter().enumerate() {
        if let Segment::Bezier([a, b, _]) = s {
            for (c, p, owner) in [(0, a, i), (1, b, i + 1)] {
                let at = pos2(x_of(p[0]), plane.y_of(p[1]));
                painter.line_segment(
                    [key_pos(owner), at],
                    Stroke::new(1.0, theme.colors.border_strong),
                );
                painter.rect_filled(
                    Rect::from_center_size(at, vec2(7.0, 7.0)),
                    1.0,
                    theme.colors.text_muted,
                );
                controls.push((i, c, at));
            }
        }
    }
    for k in 0..curve.key_count() {
        let on = view.timeline.picked.contains(&k);
        painter.add(Shape::circle_filled(
            key_pos(k),
            KEY_RADIUS,
            if on {
                theme.colors.accent
            } else {
                theme.colors.text
            },
        ));
    }

    let Some(p) = response.interact_pointer_pos() else {
        return;
    };
    let (time, value) = (
        view.timeline.time_at(p.x - rect.left()).max(0.0),
        plane.value_at(p.y),
    );
    let near = |q: Pos2| (q - p).length() <= HANDLE_HIT;
    if response.drag_started() {
        let key = (0..curve.key_count()).find(|&k| near(key_pos(k)));
        let control = controls.iter().find(|c| near(c.2));
        if let Some(k) = key {
            let st = &mut view.timeline;
            if ui.input(|i| i.modifiers.shift) {
                if let Some(at) = st.picked.iter().position(|x| *x == k) {
                    st.picked.remove(at);
                    st.key = st.picked.last().copied();
                } else {
                    st.picked.push(k);
                    st.key = Some(k);
                }
            } else if !st.picked.contains(&k) {
                st.picked = vec![k];
                st.key = Some(k);
            } else {
                st.key = Some(k);
            }
            st.segment = Some(k.min(curve.segments.len().saturating_sub(1)));
            st.work = Some((ci, curve.clone()));
            st.drag = Some(Drag::Keys);
        } else if let Some(&(s, c, _)) = control {
            let st = &mut view.timeline;
            st.segment = Some(s);
            st.work = Some((ci, curve.clone()));
            st.drag = Some(Drag::Control(s, c));
        }
    }
    if response.dragged() {
        let st = &mut view.timeline;
        match (st.drag, st.work.as_mut()) {
            (Some(Drag::Keys), Some((_, work))) => {
                if let Some([kt, kv]) = st.key.and_then(|k| work.key(k)) {
                    let picked = st.picked.clone();
                    move_picked(work, &picked, time - kt, value - kv);
                }
            }
            (Some(Drag::Control(s, c)), Some((_, work))) => {
                let _ = work.move_control(s, c, [time, value]);
            }
            _ => {}
        }
    }
    if response.drag_stopped() {
        view.timeline.drag = None;
    }
    if response.clicked() {
        // A click on empty plane moves the playhead and picks the segment under it.
        let st = &mut view.timeline;
        st.seek(time);
        st.playing = false;
        st.segment = (0..curve.segments.len())
            .find(|&s| time < curve.segments[s].end()[0])
            .or(curve.segments.len().checked_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> Curve {
        let mut m = Model::new();
        m.apply(tsukumoe_core::command::Command::AddParameter {
            name: "P".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        Curve {
            target: CurveTarget::Parameter(m.parameters().find("P").unwrap()),
            start: [0.0, 0.0],
            segments: vec![
                Segment::Linear([1.0, 1.0]),
                Segment::Linear([2.0, 2.0]),
                Segment::Linear([3.0, 3.0]),
            ],
            fade_in: None,
            fade_out: None,
        }
    }

    #[test]
    fn picked_keys_move_together_and_stop_at_the_neighbors() {
        let mut c = curve();
        move_picked(&mut c, &[1, 2], 0.5, 1.0);
        assert_eq!((c.key(1), c.key(2)), (Some([1.5, 2.0]), Some([2.5, 3.0])));
        // Key 2 meets key 3; key 1 still goes the whole way.
        move_picked(&mut c, &[1, 2], 5.0, 0.0);
        assert_eq!((c.key(1), c.key(2)), (Some([3.0, 2.0]), Some([3.0, 3.0])));
        assert!(c.is_ordered());
    }

    #[test]
    fn the_plane_maps_values_both_ways() {
        let p = Plane {
            rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 112.0)),
            lo: 0.0,
            span: 10.0,
        };
        for v in [0.0, 2.5, 10.0] {
            assert!((p.value_at(p.y_of(v)) - v).abs() < 1e-4);
        }
    }
}
