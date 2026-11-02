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
    /// Reach in screen points.
    pub radius: f32,
    pub falloff: Falloff,
    pub distance: Distance,
}

impl Default for Proportional {
    fn default() -> Self {
        Self {
            on: false,
            radius: DEFAULT_RADIUS,
            falloff: Falloff::default(),
            distance: Distance::default(),
        }
    }
}

impl Proportional {
    /// The radius as a length in model space, for the view `m` shows.
    pub fn world_radius(&self, m: &Mapper) -> f32 {
        (m.view[2] - m.view[0]) / m.size[0].max(1e-6) * self.radius
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
