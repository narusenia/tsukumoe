// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The motion player: layers, fades, crossfades, loops and priorities.

use tsukumoe_core::tmod::PlaybackData;
use tsukumoe_core::tmod::playback::{
    CURVE_FADE_IN, CURVE_FADE_OUT, CurveDef, MotionDef, TARGET_PART_OPACITY,
};

use crate::curve;

/// Most layers a runtime has.
pub const MAX_LAYERS: usize = 16;

/// What a motion may interrupt. A motion does not start while a motion of a
/// higher priority is playing, so an idle motion that restarts by itself
/// never disturbs a normal one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Idle,
    Normal,
    /// Starts whatever else plays, and cuts what the same layer plays.
    Force,
}

const PRIORITIES: [Priority; 3] = [Priority::Idle, Priority::Normal, Priority::Force];

struct Playing {
    motion: usize,
    priority: Priority,
    /// Seconds since the motion started.
    elapsed: f64,
    /// `elapsed` when it was told to fade out (stopped or crossfaded away).
    stopped_at: Option<f64>,
}

#[derive(Default)]
struct Layer {
    current: Option<Playing>,
    /// Older motions fading out under `current`, oldest first.
    fading: Vec<Playing>,
}

#[derive(Default)]
pub struct MotionPlayer {
    layers: Vec<Layer>,
}

/// 0 to 1 over `span` seconds; 1 when there is no span.
fn ramp(x: f64, span: f32) -> f64 {
    if span > 0.0 {
        (x / f64::from(span)).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

fn fade_in_of(m: &MotionDef, c: &CurveDef) -> f32 {
    if c.flags & CURVE_FADE_IN != 0 {
        c.fade_in
    } else {
        m.fade_in
    }
}

fn fade_out_of(m: &MotionDef, c: &CurveDef) -> f32 {
    if c.flags & CURVE_FADE_OUT != 0 {
        c.fade_out
    } else {
        m.fade_out
    }
}

impl Playing {
    fn is_active(&self, m: &MotionDef) -> bool {
        self.stopped_at.is_none() && (m.looped != 0 || self.elapsed < f64::from(m.duration))
    }

    fn finished(&self, pb: &PlaybackData, m: &MotionDef) -> bool {
        let ended = m.looped == 0 && self.elapsed >= f64::from(m.duration);
        let faded = self.stopped_at.is_some_and(|s| {
            let span = pb
                .motion_curves(m)
                .iter()
                .map(|c| fade_out_of(m, c))
                .fold(m.fade_out, f32::max);
            self.elapsed - s >= f64::from(span)
        });
        ended || faded
    }

    /// The weight of one curve: its fade in, the fade out at the end of a
    /// motion that does not loop, and the fade out after a stop.
    fn weight(&self, m: &MotionDef, c: &CurveDef) -> f64 {
        let mut w = ramp(self.elapsed, fade_in_of(m, c));
        let fade_out = fade_out_of(m, c);
        if m.looped == 0 {
            w *= ramp(f64::from(m.duration) - self.elapsed, fade_out);
        }
        if let Some(s) = self.stopped_at {
            w *= if fade_out > 0.0 {
                (1.0 - (self.elapsed - s) / f64::from(fade_out)).clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        w
    }

    fn apply(&self, pb: &PlaybackData, work: &mut [f32], parts: &mut [f32], driven: &mut [bool]) {
        let m = &pb.motions[self.motion];
        let time = if m.looped == 0 {
            self.elapsed
        } else if m.duration > 0.0 {
            self.elapsed % f64::from(m.duration)
        } else {
            0.0
        } as f32;
        for c in pb.motion_curves(m) {
            let w = self.weight(m, c);
            let value = curve::sample([c.start_time, c.start_value], pb.curve_segments(c), time);
            let slot = if c.target_kind == TARGET_PART_OPACITY {
                driven[c.target as usize] |= w > 0.0;
                &mut parts[c.target as usize]
            } else {
                &mut work[c.target as usize]
            };
            *slot += (value - *slot) * w as f32;
        }
    }
}

impl MotionPlayer {
    /// Starts motion `motion` on `layer`. `false` when a motion of a higher
    /// priority is playing, in which case nothing changes.
    ///
    /// The motion that the layer was playing fades out as the new one fades
    /// in (a crossfade); a [`Priority::Force`] motion cuts it instead.
    pub fn play(
        &mut self,
        pb: &PlaybackData,
        motion: usize,
        layer: usize,
        priority: Priority,
    ) -> bool {
        let blocked = self.layers.iter().any(|l| {
            l.current
                .as_ref()
                .is_some_and(|p| p.priority > priority && p.is_active(&pb.motions[p.motion]))
        });
        if blocked {
            return false;
        }
        if self.layers.len() <= layer {
            self.layers.resize_with(layer + 1, Layer::default);
        }
        let l = &mut self.layers[layer];
        if priority == Priority::Force {
            l.current = None;
            l.fading.clear();
        } else if let Some(mut old) = l.current.take() {
            old.stopped_at.get_or_insert(old.elapsed);
            l.fading.push(old);
        }
        l.current = Some(Playing {
            motion,
            priority,
            elapsed: 0.0,
            stopped_at: None,
        });
        true
    }

    /// Fades out what `layer` plays, over each motion's fade out.
    pub fn stop(&mut self, layer: usize) {
        if let Some(p) = self.layers.get_mut(layer).and_then(|l| l.current.as_mut()) {
            p.stopped_at.get_or_insert(p.elapsed);
        }
    }

    /// Whether `layer` has a motion that has not been told to stop or ended.
    pub fn is_playing(&self, pb: &PlaybackData, layer: usize) -> bool {
        self.layers
            .get(layer)
            .and_then(|l| l.current.as_ref())
            .is_some_and(|p| p.is_active(&pb.motions[p.motion]))
    }

    /// Advances time by `dt` seconds and blends every motion into `work`
    /// (parameters) and `parts` (opacities, marking `driven`). Lower
    /// priorities go first, then lower layers, then older motions: the later
    /// one has the last word.
    pub fn update(
        &mut self,
        pb: &PlaybackData,
        dt: f64,
        work: &mut [f32],
        parts: &mut [f32],
        driven: &mut [bool],
    ) {
        for l in &mut self.layers {
            for p in l.fading.iter_mut().chain(l.current.iter_mut()) {
                p.elapsed += dt;
            }
        }
        for priority in PRIORITIES {
            for l in &self.layers {
                for p in l.fading.iter().chain(l.current.iter()) {
                    if p.priority == priority {
                        p.apply(pb, work, parts, driven);
                    }
                }
            }
        }
        for l in &mut self.layers {
            l.fading.retain(|p| !p.finished(pb, &pb.motions[p.motion]));
            if l.current
                .as_ref()
                .is_some_and(|p| p.finished(pb, &pb.motions[p.motion]))
            {
                l.current = None;
            }
        }
    }
}
