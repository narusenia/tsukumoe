// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Expressions: parameter offsets that fade in and out and stack in the
//! order they were started.

use tsukumoe_core::tmod::PlaybackData;
use tsukumoe_core::tmod::playback::{BLEND_ADD, BLEND_MULTIPLY};

struct Active {
    expression: usize,
    /// 0 to 1.
    weight: f64,
    fading_in: bool,
}

#[derive(Default)]
pub struct ExpressionPlayer {
    /// In the order of starting: the last one has the last word.
    active: Vec<Active>,
}

impl ExpressionPlayer {
    /// Starts `expression`, after the ones already active. Starting one that
    /// is already active fades it in again from where it is and moves it last.
    pub fn play(&mut self, expression: usize) {
        let weight = match self.active.iter().position(|a| a.expression == expression) {
            Some(i) => self.active.remove(i).weight,
            None => 0.0,
        };
        self.active.push(Active {
            expression,
            weight,
            fading_in: true,
        });
    }

    /// Fades `expression` out; it is dropped when the fade ends.
    pub fn stop(&mut self, expression: usize) {
        for a in self
            .active
            .iter_mut()
            .filter(|a| a.expression == expression)
        {
            a.fading_in = false;
        }
    }

    pub fn is_active(&self, expression: usize) -> bool {
        self.active.iter().any(|a| a.expression == expression)
    }

    /// The weight of `expression`, 0 when it is not active.
    pub fn weight(&self, expression: usize) -> f64 {
        self.active
            .iter()
            .find(|a| a.expression == expression)
            .map_or(0.0, |a| a.weight)
    }

    /// Moves the fades by `dt` seconds and applies every expression to `work`.
    ///
    /// With weight `w`: add gives `v + value * w`, multiply `v * (1 + (value - 1) * w)`,
    /// overwrite `v * (1 - w) + value * w`.
    pub fn update(&mut self, pb: &PlaybackData, dt: f64, work: &mut [f32]) {
        for a in &mut self.active {
            let def = &pb.expressions[a.expression];
            let (span, sign) = if a.fading_in {
                (def.fade_in, 1.0)
            } else {
                (def.fade_out, -1.0)
            };
            let step = if span > 0.0 {
                dt / f64::from(span)
            } else {
                1.0
            };
            a.weight = (a.weight + sign * step).clamp(0.0, 1.0);
        }
        self.active.retain(|a| a.fading_in || a.weight > 0.0);
        for a in &self.active {
            let w = a.weight as f32;
            let def = &pb.expressions[a.expression];
            for p in pb.expression_params_of(def) {
                let v = &mut work[p.parameter as usize];
                *v = match p.blend {
                    BLEND_ADD => *v + p.value * w,
                    BLEND_MULTIPLY => *v * (1.0 + (p.value - 1.0) * w),
                    _ => *v * (1.0 - w) + p.value * w,
                };
            }
        }
    }
}
