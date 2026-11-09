// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The timeline: the motions of the model, the curves of one motion as lanes of
//! keys on a time axis, and a playhead. Playing sets the parameter values of
//! the view (never the model); every edit goes out as a command. A drag works
//! on a copy of the curve and sends one `SetMotionCurve` when it ends.

use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use tsukumoe_core::command::{Command, MotionTarget};
use tsukumoe_core::model::Model;
use tsukumoe_core::playback::{Curve, CurveTarget, Motion};
use tsukumoe_ui::{Icon, Theme, Variant, button, checkbox, hint, icon_button, number, tr};

use crate::view::ViewState;

pub(super) const LABEL_WIDTH: f32 = 150.0;
const RULER_HEIGHT: f32 = 22.0;
const LANE_HEIGHT: f32 = 26.0;
const KEY_RADIUS: f32 = 5.0;
/// A press within this many points of a key picks it.
const KEY_HIT: f32 = 8.0;
pub const DEFAULT_ZOOM: f32 = 120.0;
const ZOOM_RANGE: (f32, f32) = (10.0, 2000.0);

/// What the timeline remembers between frames. Not saved, not in the history.
#[derive(Clone, Debug)]
pub struct State {
    pub motion: Option<String>,
    /// The playhead, in seconds.
    pub time: f32,
    pub playing: bool,
    /// Points per second.
    pub zoom: f32,
    /// The time at the left edge, in seconds.
    pub scroll: f32,
    pub curve: Option<usize>,
    pub key: Option<usize>,
    /// The curve being dragged or typed into, until the edit ends.
    pub(super) work: Option<(usize, Curve)>,
    /// The segment the curve editor's kind buttons act on.
    pub segment: Option<usize>,
    /// The keys picked in the curve editor (`key` is the main one).
    pub picked: Vec<usize>,
    pub(super) drag: Option<super::curve_editor::Drag>,
    /// The playhead moved or the motion changed: the parameters follow.
    apply: bool,
    scrubbing: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            motion: None,
            time: 0.0,
            playing: false,
            zoom: DEFAULT_ZOOM,
            scroll: 0.0,
            curve: None,
            key: None,
            work: None,
            segment: None,
            picked: Vec::new(),
            drag: None,
            apply: false,
            scrubbing: false,
        }
    }
}

pub enum Out {
    Command(Command, String),
    Error(&'static str),
    /// Ask for a motion file to import.
    Import,
}

impl State {
    /// The motion chosen, if the model still has it.
    pub fn current<'m>(&self, model: &'m Model) -> Option<(&str, &'m Motion)> {
        let name = self.motion.as_deref()?;
        let id = model.motions().find(name)?;
        Some((name, model.motions().get(id)?))
    }

    pub fn select_motion(&mut self, name: Option<String>) {
        self.motion = name;
        self.curve = None;
        self.key = None;
        self.work = None;
        self.segment = None;
        self.picked.clear();
        self.drag = None;
        self.playing = false;
        self.time = 0.0;
        self.apply = true;
    }

    pub fn x_of(&self, time: f32) -> f32 {
        (time - self.scroll) * self.zoom
    }

    pub fn time_at(&self, x: f32) -> f32 {
        x / self.zoom + self.scroll
    }

    /// Moves the playhead; the parameters follow at the next frame.
    pub fn seek(&mut self, time: f32) {
        self.time = time.max(0.0);
        self.apply = true;
    }

    /// Plays, or pauses a playing motion. From the end it plays from the start.
    pub fn toggle_play(&mut self, duration: f32) {
        if self.playing {
            self.playing = false;
        } else {
            if self.time >= duration {
                self.seek(0.0);
            }
            self.playing = true;
        }
    }

    /// Advances a playing motion by `dt` seconds.
    pub fn advance(&mut self, motion: &Motion, dt: f32) {
        if !self.playing {
            return;
        }
        self.time += dt;
        if self.time >= motion.duration {
            if motion.looped && motion.duration > 0.0 {
                self.time %= motion.duration;
            } else {
                self.time = motion.duration;
                self.playing = false;
            }
        }
        self.apply = true;
    }
}

/// A name for a new motion that no motion has.
pub fn unique_motion_name(model: &Model) -> String {
    (1..)
        .map(|n| format!("Motion{n}"))
        .find(|name| model.motions().find(name).is_none())
        .expect("endless range")
}

/// The name a curve's target goes by.
pub fn target_name(model: &Model, target: CurveTarget) -> String {
    match target {
        CurveTarget::Parameter(id) => model.parameters().name(id),
        CurveTarget::PartOpacity(id) => model.parts().name(id),
    }
    .unwrap_or("?")
    .to_owned()
}

/// The parameter values a motion gives at `time`: one per parameter curve.
pub fn values_at(model: &Model, motion: &Motion, time: f32) -> Vec<(String, f32)> {
    motion
        .curves
        .iter()
        .filter_map(|c| match c.target {
            CurveTarget::Parameter(id) => {
                Some((model.parameters().name(id)?.to_owned(), c.value_at(time)))
            }
            CurveTarget::PartOpacity(_) => None,
        })
        .collect()
}

pub(super) fn set_curve(motion: &str, index: usize, curve: &Curve) -> Command {
    Command::SetMotionCurve {
        motion: motion.to_owned(),
        curve: index,
        start: curve.start,
        segments: curve.segments.clone(),
    }
}

/// The commands that make a new curve for `target` and put a key at `time`
/// with `value` (replacing the key already there).
pub fn key_here(
    model: &Model,
    motion: &str,
    target: &MotionTarget,
    time: f32,
    value: f32,
) -> Result<Command, &'static str> {
    let (_, m) = model
        .motions()
        .find(motion)
        .and_then(|id| model.motions().get(id).map(|m| (id, m)))
        .ok_or("timeline-error-no-motion")?;
    let target_id = match target {
        MotionTarget::Parameter { name } => {
            model.parameters().find(name).map(CurveTarget::Parameter)
        }
        MotionTarget::PartOpacity { name } => {
            model.parts().find(name).map(CurveTarget::PartOpacity)
        }
    }
    .ok_or("timeline-error-no-target")?;
    let existing = m.curves.iter().position(|c| c.target == target_id);
    let mut curve = existing.map_or_else(
        || Curve {
            target: target_id,
            start: [0.0, value],
            segments: Vec::new(),
            fade_in: None,
            fade_out: None,
        },
        |i| m.curves[i].clone(),
    );
    match curve.key_at(time) {
        Some(k) => {
            let [t, _] = curve.key(k).expect("key_at gives a key");
            curve
                .move_key(k, t, value)
                .map_err(|_| "timeline-error-bad-key")?;
        }
        None => {
            let k = curve
                .insert_key(time)
                .map_err(|_| "timeline-error-bad-key")?;
            let [t, _] = curve.key(k).expect("insert_key gives a key");
            curve
                .move_key(k, t, value)
                .map_err(|_| "timeline-error-bad-key")?;
        }
    }
    let mut commands = Vec::new();
    let index = match existing {
        Some(i) => i,
        None => {
            commands.push(Command::AddMotionCurve {
                motion: motion.to_owned(),
                target: target.clone(),
            });
            m.curves.len()
        }
    };
    commands.push(set_curve(motion, index, &curve));
    Ok(Command::Batch { commands })
}

pub fn show(ui: &mut Ui, model: &Model, view: &mut ViewState) -> Vec<Out> {
    let mut outs = Vec::new();
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    // The motion may be gone (undo, another document).
    if view.timeline.motion.is_some() && view.timeline.current(model).is_none() {
        view.timeline.select_motion(None);
    }
    toolbar(ui, model, view, &mut outs);
    let Some((name, motion)) = view
        .timeline
        .current(model)
        .map(|(n, m)| (n.to_owned(), m.clone()))
    else {
        hint(ui, t("timeline-empty"));
        return outs;
    };
    view.timeline.advance(&motion, ui.input(|i| i.stable_dt));
    if view.timeline.playing {
        ui.ctx().request_repaint();
    }
    actions_row(ui, model, view, &name, &motion, &mut outs);
    // The curve editor takes the lower part when a curve is picked.
    let editor = view.timeline.curve.filter(|i| *i < motion.curves.len());
    let avail = ui.available_height();
    let editor_height = editor.map_or(0.0, |_| (avail * 0.5).clamp(120.0, 340.0).min(avail * 0.7));
    ui.scope(|ui| {
        ui.set_max_height((avail - editor_height).max(40.0));
        body(ui, model, view, &name, &motion, &mut outs);
    });
    if let Some(ci) = editor {
        ui.allocate_ui(vec2(ui.available_width(), editor_height), |ui| {
            super::curve_editor::show(ui, model, view, &name, ci, &mut outs);
        });
    }
    key_fields(ui, view, &motion);
    commit_work(ui, view, &name, &motion, &mut outs);
    if std::mem::take(&mut view.timeline.apply) {
        let time = view.timeline.time;
        for (param, value) in values_at(model, &motion, time) {
            if let Some(id) = model.parameters().find(&param)
                && let Some(p) = model.parameters().get(id)
            {
                view.set_param(&param, p, value);
            }
        }
    }
    outs
}

fn toolbar(ui: &mut Ui, model: &Model, view: &mut ViewState, outs: &mut Vec<Out>) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    ui.horizontal(|ui| {
        let names: Vec<String> = model
            .motions()
            .iter()
            .map(|(_, n, _)| n.to_owned())
            .collect();
        let shown = view
            .timeline
            .motion
            .clone()
            .unwrap_or_else(|| t("timeline-pick"));
        egui::ComboBox::from_id_salt("timeline-motion")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for n in &names {
                    let on = view.timeline.motion.as_deref() == Some(n.as_str());
                    if ui.selectable_label(on, n).clicked() && !on {
                        view.timeline.select_motion(Some(n.clone()));
                    }
                }
            })
            .response
            .accessible_name(t("timeline-pick"));
        if icon_button(ui, Icon::Plus, &t("timeline-new"), Variant::Ghost).clicked() {
            let name = unique_motion_name(model);
            outs.push(Out::Command(
                Command::AddMotion {
                    name: name.clone(),
                    duration: 2.0,
                    fps: 30.0,
                    looped: false,
                },
                format!("motion_add: {name}"),
            ));
            view.timeline.select_motion(Some(name));
        }
        if let Some(name) = view.timeline.motion.clone()
            && icon_button(ui, Icon::Trash, &t("timeline-delete"), Variant::Ghost).clicked()
        {
            outs.push(Out::Command(
                Command::RemoveMotion { name: name.clone() },
                format!("motion_remove: {name}"),
            ));
            view.timeline.select_motion(None);
        }
        if icon_button(ui, Icon::Upload, &t("timeline-import"), Variant::Ghost).clicked() {
            outs.push(Out::Import);
        }
    });
}

fn actions_row(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    name: &str,
    motion: &Motion,
    outs: &mut Vec<Out>,
) {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    ui.horizontal_wrapped(|ui| {
        let (icon, key) = if view.timeline.playing {
            (Icon::Pause, "timeline-pause")
        } else {
            (Icon::Play, "timeline-play")
        };
        if icon_button(ui, icon, &t(key), Variant::Secondary).clicked() {
            view.timeline.toggle_play(motion.duration);
        }
        if icon_button(ui, Icon::RotateCcw, &t("timeline-rewind"), Variant::Ghost).clicked() {
            view.timeline.seek(0.0);
        }
        ui.label(format!(
            "{:.2} / {:.2} s",
            view.timeline.time, motion.duration
        ));
        // Length, rate and loop: one command per finished edit.
        let mut next = (motion.duration, motion.fps, motion.looped);
        let mut changed = false;
        let r = number(ui, &mut next.0, 0.0..=3600.0, 0.05, &t("timeline-duration"));
        changed |= r.drag_stopped() || (r.changed() && !r.dragged() && !r.has_focus());
        let r = number(ui, &mut next.1, 1.0..=240.0, 0.5, &t("timeline-fps"));
        changed |= r.drag_stopped() || (r.changed() && !r.dragged() && !r.has_focus());
        changed |= checkbox(ui, &mut next.2, &t("timeline-loop")).changed();
        if changed {
            outs.push(Out::Command(
                Command::SetMotionTiming {
                    name: name.to_owned(),
                    duration: next.0,
                    fps: next.1,
                    looped: next.2,
                    fade_in: motion.fade_in,
                    fade_out: motion.fade_out,
                },
                format!("motion_timing: {name}"),
            ));
        }
    });
    ui.horizontal_wrapped(|ui| {
        let focus = view
            .param_focus
            .clone()
            .filter(|f| model.parameters().find(f).is_some());
        let param = focus.as_deref().and_then(|f| {
            let id = model.parameters().find(f)?;
            Some((f, model.parameters().get(id)?))
        });
        let has = param.is_some_and(|(f, _)| {
            let id = model.parameters().find(f);
            motion
                .curves
                .iter()
                .any(|c| id.is_some_and(|id| c.target == CurveTarget::Parameter(id)))
        });
        ui.add_enabled_ui(param.is_some(), |ui| {
            if button(ui, &t("timeline-add-curve"), Variant::Secondary).clicked()
                && let Some((f, _)) = param
                && !has
            {
                outs.push(Out::Command(
                    Command::AddMotionCurve {
                        motion: name.to_owned(),
                        target: MotionTarget::Parameter { name: f.to_owned() },
                    },
                    format!("motion_curve_add: {f}"),
                ));
            }
            if button(ui, &t("timeline-key-here"), Variant::Secondary)
                .on_hover_text(t("timeline-key-here-hint"))
                .clicked()
                && let Some((f, p)) = param
            {
                let value = view.param(f, p);
                let target = MotionTarget::Parameter { name: f.to_owned() };
                match key_here(model, name, &target, view.timeline.time, value) {
                    Ok(command) => outs.push(Out::Command(command, format!("motion_key: {f}"))),
                    Err(key) => outs.push(Out::Error(key)),
                }
            }
        });
        if param.is_none() {
            hint(ui, t("timeline-focus-hint"));
        }
        let selected = view.timeline.curve.zip(view.timeline.key);
        ui.add_enabled_ui(selected.is_some(), |ui| {
            if button(ui, &t("timeline-key-delete"), Variant::Secondary).clicked()
                && let Some((ci, ki)) = selected
                && let Some(curve) = motion.curves.get(ci)
            {
                let mut curve = curve.clone();
                match curve.remove_key(ki) {
                    Ok(()) => {
                        outs.push(Out::Command(
                            set_curve(name, ci, &curve),
                            format!("motion_key_remove: {name}"),
                        ));
                        view.timeline.key = None;
                    }
                    Err(_) => outs.push(Out::Error("timeline-error-last-key")),
                }
            }
        });
    });
}

fn lane_color(theme: &Theme, selected: bool) -> Color32 {
    if selected {
        theme.colors.accent
    } else {
        theme.colors.text_muted
    }
}

fn body(
    ui: &mut Ui,
    model: &Model,
    view: &mut ViewState,
    name: &str,
    motion: &Motion,
    outs: &mut Vec<Out>,
) {
    let theme = Theme::get(ui.ctx());
    let curves: Vec<Curve> = motion
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| match &view.timeline.work {
            Some((w, curve)) if *w == i => curve.clone(),
            _ => c.clone(),
        })
        .collect();
    let height = (RULER_HEIGHT + LANE_HEIGHT * curves.len() as f32).max(RULER_HEIGHT + LANE_HEIGHT);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .id_salt("timeline-scroll")
        .show(ui, |ui| {
            let width = ui.available_width();
            let (rect, response) = ui.allocate_exact_size(
                vec2(width, height.max(ui.available_height())),
                Sense::click_and_drag(),
            );
            let track = Rect::from_min_max(pos2(rect.left() + LABEL_WIDTH, rect.top()), rect.max);
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme.colors.surface);
            // Ruler: a tick per second, finer when zoomed in.
            let step = if view.timeline.zoom >= 400.0 {
                0.1
            } else if view.timeline.zoom >= 60.0 {
                1.0
            } else {
                5.0
            };
            let (t0, t1) = (
                view.timeline.time_at(0.0).max(0.0),
                view.timeline.time_at(track.width()),
            );
            let mut tick = (t0 / step).floor() * step;
            while tick <= t1 {
                let x = track.left() + view.timeline.x_of(tick);
                if x >= track.left() {
                    painter.line_segment(
                        [
                            pos2(x, rect.top() + RULER_HEIGHT - 6.0),
                            pos2(x, rect.top() + RULER_HEIGHT),
                        ],
                        Stroke::new(1.0, theme.colors.border_strong),
                    );
                    painter.text(
                        pos2(x + 3.0, rect.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        format!("{tick:.1}"),
                        egui::FontId::proportional(10.0),
                        theme.colors.text_muted,
                    );
                }
                tick += step;
            }
            // The motion's length.
            let end_x = track.left() + view.timeline.x_of(motion.duration);
            if end_x >= track.left() && end_x <= track.right() {
                painter.line_segment(
                    [pos2(end_x, rect.top()), pos2(end_x, rect.bottom())],
                    Stroke::new(1.0, theme.colors.border_strong),
                );
            }
            // Lanes.
            let lane_rect = |i: usize| {
                let top = rect.top() + RULER_HEIGHT + LANE_HEIGHT * i as f32;
                Rect::from_min_size(pos2(track.left(), top), vec2(track.width(), LANE_HEIGHT))
            };
            let mut label_click = None;
            for (i, curve) in curves.iter().enumerate() {
                let lane = lane_rect(i);
                let on = view.timeline.curve == Some(i);
                let label = Rect::from_min_size(
                    pos2(rect.left(), lane.top()),
                    vec2(LABEL_WIDTH, LANE_HEIGHT),
                );
                if on {
                    painter.rect_filled(lane.union(label), 0.0, theme.colors.control);
                }
                painter.text(
                    pos2(label.left() + 8.0, label.center().y),
                    egui::Align2::LEFT_CENTER,
                    target_name(model, curve.target),
                    egui::FontId::proportional(12.0),
                    theme.colors.text,
                );
                painter.line_segment(
                    [lane.left_bottom(), lane.right_bottom()],
                    Stroke::new(1.0, theme.colors.border),
                );
                draw_curve(&painter, &theme, model, view, curve, lane, on);
                for k in 0..curve.key_count() {
                    let [kt, _] = curve.key(k).expect("in range");
                    let x = lane.left() + view.timeline.x_of(kt);
                    if x < lane.left() - KEY_RADIUS || x > lane.right() + KEY_RADIUS {
                        continue;
                    }
                    let picked = on && view.timeline.key == Some(k);
                    painter.add(Shape::circle_filled(
                        pos2(x, lane.center().y),
                        KEY_RADIUS,
                        if picked {
                            theme.colors.accent
                        } else {
                            theme.colors.text
                        },
                    ));
                }
                if let Some(p) = response.interact_pointer_pos()
                    && response.clicked()
                    && p.x < track.left()
                    && label.contains(p)
                {
                    label_click = Some(i);
                }
            }
            // The playhead.
            let px = track.left() + view.timeline.x_of(view.timeline.time);
            if px >= track.left() && px <= track.right() {
                painter.line_segment(
                    [pos2(px, rect.top()), pos2(px, rect.bottom())],
                    Stroke::new(1.5, theme.colors.accent),
                );
            }
            if let Some(i) = label_click {
                view.timeline.curve = Some(i);
                view.timeline.key = None;
                if let CurveTarget::Parameter(_) = curves[i].target {
                    view.param_focus = Some(target_name(model, curves[i].target));
                }
            }
            pointer(view, name, &curves, &response, track, rect, outs);
            wheel(ui, view, &response, track);
        });
}

/// A faint outline of the curve, scaled to the parameter's range.
fn draw_curve(
    painter: &egui::Painter,
    theme: &Theme,
    model: &Model,
    view: &ViewState,
    curve: &Curve,
    lane: Rect,
    selected: bool,
) {
    let (lo, hi) = match curve.target {
        CurveTarget::Parameter(id) => model
            .parameters()
            .get(id)
            .map_or((0.0, 1.0), |p| (p.min, p.max)),
        CurveTarget::PartOpacity(_) => (0.0, 1.0),
    };
    let span = (hi - lo).max(f32::EPSILON);
    let mut points: Vec<Pos2> = Vec::new();
    let mut x = lane.left();
    while x <= lane.right() {
        let v = curve.value_at(view.timeline.time_at(x - lane.left()));
        let y = lane.bottom() - 3.0 - ((v - lo) / span).clamp(0.0, 1.0) * (LANE_HEIGHT - 6.0);
        points.push(pos2(x, y));
        x += 3.0;
    }
    painter.add(Shape::line(
        points,
        Stroke::new(1.0, lane_color(theme, selected)),
    ));
}

/// Press, drag and double click on the track: scrub, move a key, add a key.
fn pointer(
    view: &mut ViewState,
    name: &str,
    curves: &[Curve],
    response: &egui::Response,
    track: Rect,
    rect: Rect,
    outs: &mut Vec<Out>,
) {
    let st = &mut view.timeline;
    let Some(p) = response.interact_pointer_pos() else {
        return;
    };
    let lane_of = |y: f32| {
        let i = ((y - rect.top() - RULER_HEIGHT) / LANE_HEIGHT).floor();
        (i >= 0.0 && (i as usize) < curves.len()).then_some(i as usize)
    };
    if response.drag_started() && p.x >= track.left() {
        // A key under the pointer is picked; anything else moves the playhead.
        let hit = lane_of(p.y).and_then(|i| {
            let lane_y = rect.top() + RULER_HEIGHT + LANE_HEIGHT * (i as f32 + 0.5);
            (0..curves[i].key_count())
                .find(|&k| {
                    let x = track.left() + st.x_of(curves[i].key(k).expect("in range")[0]);
                    (x - p.x).abs() <= KEY_HIT && (lane_y - p.y).abs() <= LANE_HEIGHT / 2.0
                })
                .map(|k| (i, k))
        });
        match hit {
            Some((i, k)) => {
                st.curve = Some(i);
                st.key = Some(k);
                st.work = Some((i, curves[i].clone()));
                st.scrubbing = false;
            }
            None => st.scrubbing = true,
        }
    }
    if response.dragged() && p.x >= track.left() - 1.0 {
        let time = st.time_at(p.x - track.left()).max(0.0);
        if st.scrubbing {
            st.seek(time);
            st.playing = false;
        } else if let (Some((_, curve)), Some(k)) = (st.work.as_mut(), st.key) {
            let lo = k
                .checked_sub(1)
                .and_then(|b| curve.key(b))
                .map_or(0.0, |b| b[0]);
            let hi = curve.key(k + 1).map_or(f32::MAX, |a| a[0]);
            let value = curve.key(k).map_or(0.0, |v| v[1]);
            let _ = curve.move_key(k, time.clamp(lo, hi), value);
        }
    }
    if response.drag_stopped() {
        st.scrubbing = false;
    }
    if response.clicked() && p.x >= track.left() {
        // A click on the ruler or a lane moves the playhead and picks a lane.
        let time = st.time_at(p.x - track.left()).max(0.0);
        st.seek(time);
        st.playing = false;
        if let Some(i) = lane_of(p.y) {
            st.curve = Some(i);
            st.key = None;
        }
    }
    if response.double_clicked()
        && p.x >= track.left()
        && let Some(i) = lane_of(p.y)
    {
        let time = st.time_at(p.x - track.left()).max(0.0);
        let mut curve = curves[i].clone();
        if let Ok(k) = curve.insert_key(time) {
            outs.push(Out::Command(
                set_curve(name, i, &curve),
                format!("motion_key_add: {name}"),
            ));
            st.curve = Some(i);
            st.key = Some(k);
        }
    }
}

/// Ctrl / Cmd and the wheel zoom the time axis; the wheel alone scrolls it.
fn wheel(ui: &Ui, view: &mut ViewState, response: &egui::Response, track: Rect) {
    if !response.hovered() {
        return;
    }
    let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
    let st = &mut view.timeline;
    if (zoom - 1.0).abs() > f32::EPSILON
        && let Some(p) = response.hover_pos()
    {
        let anchor = st.time_at(p.x - track.left());
        st.zoom = (st.zoom * zoom).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        st.scroll = anchor - (p.x - track.left()) / st.zoom;
    } else if scroll.x != 0.0 {
        st.scroll -= scroll.x / st.zoom;
    }
    st.scroll = st.scroll.max(0.0);
}

/// Time and value of the picked key as fields; a change goes into the working copy.
fn key_fields(ui: &mut Ui, view: &mut ViewState, motion: &Motion) {
    let ctx = ui.ctx().clone();
    let (Some(ci), Some(ki)) = (view.timeline.curve, view.timeline.key) else {
        return;
    };
    let mut curve = match &view.timeline.work {
        Some((wi, c)) if *wi == ci => c.clone(),
        _ => match motion.curves.get(ci) {
            Some(c) => c.clone(),
            None => return,
        },
    };
    let Some([mut time, mut value]) = curve.key(ki) else {
        return;
    };
    let lo = ki
        .checked_sub(1)
        .and_then(|b| curve.key(b))
        .map_or(0.0, |b| b[0]);
    let hi = curve.key(ki + 1).map_or(3600.0, |a| a[0]);
    let before = (time, value);
    ui.horizontal(|ui| {
        ui.label(tr(&ctx, "timeline-time"));
        number(ui, &mut time, lo..=hi, 0.01, &tr(&ctx, "timeline-time"));
        ui.label(tr(&ctx, "timeline-value"));
        number(
            ui,
            &mut value,
            -1.0e6..=1.0e6,
            0.01,
            &tr(&ctx, "timeline-value"),
        );
    });
    if (time, value) != before && curve.move_key(ki, time.clamp(lo, hi), value).is_ok() {
        view.timeline.work = Some((ci, curve));
    }
}

/// Sends the working copy of a curve once the pointer and the keyboard let go,
/// if it differs from the model's.
fn commit_work(
    ui: &mut Ui,
    view: &mut ViewState,
    name: &str,
    motion: &Motion,
    outs: &mut Vec<Out>,
) {
    if view.timeline.work.is_none() {
        return;
    }
    if ui.input(|i| i.pointer.any_down()) || ui.memory(|m| m.focused().is_some()) {
        return;
    }
    let Some((i, curve)) = view.timeline.work.take() else {
        return;
    };
    if motion.curves.get(i) != Some(&curve) {
        outs.push(Out::Command(
            set_curve(name, i, &curve),
            format!("motion_key_move: {name}"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use tsukumoe_core::playback::Segment;

    fn model() -> Model {
        let mut m = Model::new();
        for c in [
            Command::AddParameter {
                name: "P".into(),
                min: 0.0,
                max: 10.0,
                default: 0.0,
                repeat: false,
            },
            Command::AddMotion {
                name: "M".into(),
                duration: 2.0,
                fps: 30.0,
                looped: false,
            },
            Command::AddMotionCurve {
                motion: "M".into(),
                target: MotionTarget::Parameter { name: "P".into() },
            },
            Command::SetMotionCurve {
                motion: "M".into(),
                curve: 0,
                start: [0.0, 0.0],
                segments: vec![Segment::Linear([2.0, 10.0])],
            },
        ] {
            m.apply(c).unwrap();
        }
        m
    }

    fn motion(m: &Model) -> Motion {
        m.motions()
            .get(m.motions().find("M").unwrap())
            .unwrap()
            .clone()
    }

    #[test]
    fn values_follow_the_playhead() {
        let m = model();
        let values = values_at(&m, &motion(&m), 1.0);
        assert_eq!(values, vec![("P".to_owned(), 5.0)]);
    }

    #[test]
    fn playing_stops_at_the_end_or_wraps() {
        let m = model();
        let mut mo = motion(&m);
        let mut st = State {
            playing: true,
            time: 1.5,
            ..State::default()
        };
        st.advance(&mo, 1.0);
        assert!(!st.playing && st.time == 2.0);
        mo.looped = true;
        st.playing = true;
        st.time = 1.5;
        st.advance(&mo, 1.0);
        assert!(st.playing && (st.time - 0.5).abs() < 1e-6);
    }

    #[test]
    fn toggling_play_pauses_and_restarts_from_the_end() {
        let mut st = State::default();
        st.toggle_play(2.0);
        assert!(st.playing);
        st.toggle_play(2.0);
        assert!(!st.playing);
        st.time = 2.0;
        st.toggle_play(2.0);
        assert!(st.playing && st.time == 0.0);
    }

    #[test]
    fn a_key_at_the_playhead_adds_a_curve_or_edits_the_key() {
        let mut m = model();
        let target = MotionTarget::Parameter { name: "P".into() };
        // Inside the segment: a key splits it, then holds the given value.
        let command = key_here(&m, "M", &target, 1.0, 2.0).unwrap();
        m.apply(command).unwrap();
        let c = &motion(&m).curves[0];
        assert_eq!(c.key_count(), 3);
        assert_eq!(c.key(1), Some([1.0, 2.0]));
        // On the key: its value changes, no key is added.
        let command = key_here(&m, "M", &target, 1.0, 7.0).unwrap();
        m.apply(command).unwrap();
        assert_eq!(motion(&m).curves[0].key(1), Some([1.0, 7.0]));
        assert_eq!(motion(&m).curves[0].key_count(), 3);
        // A parameter without a curve gets one.
        m.apply(Command::AddParameter {
            name: "Q".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        })
        .unwrap();
        let q = MotionTarget::Parameter { name: "Q".into() };
        m.apply(key_here(&m, "M", &q, 0.5, 1.0).unwrap()).unwrap();
        assert_eq!(motion(&m).curves.len(), 2);
        assert!(
            key_here(
                &m,
                "M",
                &MotionTarget::Parameter {
                    name: "Nope".into()
                },
                0.0,
                0.0
            )
            .is_err()
        );
    }

    struct Panel {
        model: Model,
        view: ViewState,
        outs: Vec<Out>,
    }

    fn harness(model: Model) -> Harness<'static, Panel> {
        let mut view = ViewState::default();
        view.timeline.select_motion(Some("M".into()));
        Harness::builder()
            .with_size(egui::vec2(700.0, 320.0))
            .build_ui_state(
                |ui, p: &mut Panel| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    let outs = show(ui, &p.model, &mut p.view);
                    p.outs.extend(outs);
                },
                Panel {
                    model,
                    view,
                    outs: Vec::new(),
                },
            )
    }

    #[test]
    fn the_new_button_makes_a_motion() {
        let mut h = harness(Model::new());
        h.state_mut().view.timeline.select_motion(None);
        h.run();
        h.get_by_label("Add motion").click();
        h.run();
        let Some(Out::Command(Command::AddMotion { name, .. }, _)) = h.state().outs.last() else {
            panic!("an AddMotion");
        };
        assert_eq!(name, "Motion1");
    }

    #[test]
    fn play_moves_the_playhead_and_the_parameter() {
        let mut h = harness(model());
        h.run();
        h.get_by_label("Play").click();
        h.run_steps(6);
        let st = &h.state().view.timeline;
        assert!(st.time > 0.0, "the playhead moved");
        let p = h.state().model.parameters().find("P").unwrap();
        let value = h
            .state()
            .view
            .param("P", h.state().model.parameters().get(p).unwrap());
        assert!(value > 0.0, "the parameter follows: {value}");
    }

    #[test]
    fn the_end_of_a_motion_that_does_not_loop_stops_playing() {
        let mut h = harness(model());
        h.state_mut().view.timeline.playing = true;
        h.state_mut().view.timeline.time = 1.99;
        h.run();
        h.run();
        assert!(!h.state().view.timeline.playing);
    }

    #[test]
    fn the_curve_editor_switches_the_kind_of_the_picked_segment() {
        let mut h = harness(model());
        h.state_mut().view.timeline.curve = Some(0);
        h.state_mut().view.timeline.key = Some(0);
        h.run();
        h.get_by_label("Bezier").click();
        h.run();
        let Some(Out::Command(Command::SetMotionCurve { segments, .. }, _)) = h.state().outs.last()
        else {
            panic!("a SetMotionCurve, got {}", h.state().outs.len());
        };
        assert!(matches!(segments[0], Segment::Bezier(_)));
        assert_eq!(segments[0].end(), [2.0, 10.0]);
    }
}
