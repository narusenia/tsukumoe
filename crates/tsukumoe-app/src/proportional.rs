// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Proportional edit as the editor holds it: the option values, the radius
//! the wheel changes, and the snapshot a drag measures distances on. The
//! weights themselves come from `tsukumoe_core::proportional`.

use tsukumoe_core::elements::Point;
use tsukumoe_core::proportional::{Distance, Falloff, Influence};

use crate::mesh_tool::Mapper;

/// The radius is in screen points so it feels the same at any zoom.
pub const RADIUS_RANGE: (f32, f32) = (4.0, 800.0);
const DEFAULT_RADIUS: f32 = 80.0;
/// One wheel point changes the radius by this fraction (as in the zoom).
const WHEEL_SENSITIVITY: f32 = 0.002;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Proportional {
    pub on: bool,
    /// Reach in screen points (while `fixed` is off).
    pub radius: f32,
    /// The reach is a length in the model instead, so zooming does not change
    /// what a drag moves. `model_radius` is set from the screen radius the
    /// first time the view is known (`settle`).
    pub fixed: bool,
    pub model_radius: Option<f32>,
    pub falloff: Falloff,
    pub distance: Distance,
}

impl Default for Proportional {
    fn default() -> Self {
        Self {
            on: false,
            radius: DEFAULT_RADIUS,
            fixed: false,
            model_radius: None,
            falloff: Falloff::default(),
            distance: Distance::default(),
        }
    }
}

impl Proportional {
    /// Model length of one screen point in the view `m` shows.
    fn per_point(m: &Mapper) -> f32 {
        (m.view[2] - m.view[0]) / m.size[0].max(1e-6)
    }

    /// The radius as a length in model space, for the view `m` shows.
    pub fn world_radius(&self, m: &Mapper) -> f32 {
        match (self.fixed, self.model_radius) {
            (true, Some(r)) => r,
            _ => Self::per_point(m) * self.radius,
        }
    }

    /// The radius in screen points in the view `m` shows (for drawing it).
    pub fn screen_radius(&self, m: &Mapper) -> f32 {
        match (self.fixed, self.model_radius) {
            (true, Some(r)) => r / Self::per_point(m).max(1e-9),
            _ => self.radius,
        }
    }

    /// Takes the model radius from what is on screen when the fixed mode has
    /// just been turned on, so the circle does not jump.
    pub fn settle(&mut self, m: &Mapper) {
        if self.fixed && self.model_radius.is_none() {
            self.model_radius = Some(Self::per_point(m) * self.radius);
        }
    }

    pub fn influence(&self, m: &Mapper) -> Influence {
        Influence {
            radius: self.world_radius(m),
            falloff: self.falloff,
            distance: self.distance,
        }
    }

    /// The wheel turned by `delta` points: up widens, down narrows.
    pub fn scrolled(&mut self, delta: f32) {
        if let (true, Some(r)) = (self.fixed, self.model_radius) {
            // Model lengths have no screen range: only keep it positive and finite.
            let wider = r * (delta * WHEEL_SENSITIVITY).exp();
            self.model_radius = Some(wider.clamp(1e-4, 1e6));
            return;
        }
        self.radius =
            (self.radius * (delta * WHEEL_SENSITIVITY).exp()).clamp(RADIUS_RANGE.0, RADIUS_RANGE.1);
    }

    pub fn set_radius(&mut self, radius: f32) {
        self.radius = radius.clamp(RADIUS_RANGE.0, RADIUS_RANGE.1);
    }
}

/// The points as drawn when a press began, with their edges: a drag measures
/// distances here, so the weights do not change as the drag moves the shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub positions: Vec<Point>,
    pub edges: Vec<(u32, u32)>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_fixed_radius_stays_the_same_in_the_model_whatever_the_zoom() {
        let at = |scale: f32| Mapper {
            origin: [0.0, 0.0],
            size: [100.0, 100.0],
            view: [0.0, 0.0, scale * 100.0, scale * 100.0],
        };
        let mut p = Proportional {
            fixed: true,
            ..Proportional::default()
        };
        // Settled at a zoom where one point is 0.1 model units: 80 points = 8.
        p.settle(&at(0.1));
        assert!((p.world_radius(&at(0.1)) - 8.0).abs() < 1e-5);
        // Zoomed in or out, the model radius holds and the circle on screen scales.
        assert!((p.world_radius(&at(0.05)) - 8.0).abs() < 1e-5);
        assert!((p.screen_radius(&at(0.05)) - 160.0).abs() < 1e-3);
        // Off, the radius is in points as before.
        p.fixed = false;
        assert!((p.world_radius(&at(0.05)) - 4.0).abs() < 1e-5);
        // The wheel changes the model radius while it is fixed.
        p.fixed = true;
        p.scrolled(100.0);
        assert!(p.model_radius.unwrap() > 8.0);
    }

    use super::*;

    #[test]
    fn the_radius_follows_the_zoom_and_the_wheel_stays_in_range() {
        let p = Proportional::default();
        let at = |w: f32| Mapper {
            origin: [0.0, 0.0],
            size: [100.0, 100.0],
            view: [0.0, 0.0, w, w],
        };
        // 80 points of 100 across a 10 wide view: 8 units; a wider view, more.
        assert!((p.world_radius(&at(10.0)) - 8.0).abs() < 1e-5);
        assert!((p.world_radius(&at(20.0)) - 16.0).abs() < 1e-5);
        let mut q = p;
        q.scrolled(100.0);
        assert!(q.radius > p.radius);
        q.scrolled(-1.0e6);
        assert_eq!(q.radius, RADIUS_RANGE.0);
        q.scrolled(1.0e6);
        assert_eq!(q.radius, RADIUS_RANGE.1);
        q.set_radius(f32::MAX);
        assert_eq!(q.radius, RADIUS_RANGE.1);
    }
}
