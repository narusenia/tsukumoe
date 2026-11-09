// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The view aids (M5-02e): a grid, rulers, guides, snapping, a reference
//! image and an info box. They are view data kept with the project, never in
//! the model (ADR-0019): `editor/view.json` holds the settings and
//! `editor/reference` the image (any format the image loaders read). This file
//! is only the data and its arithmetic; `panels::view_overlay` draws it.

use serde::{Deserialize, Serialize};
use tsukumoe_core::elements::Point;

use crate::mesh_tool::Mapper;

/// Where the settings are kept in a project.
pub const VIEW_PATH: &str = "editor/view.json";
/// Where the reference image is kept in a project.
pub const REFERENCE_PATH: &str = "editor/reference";

/// A ruler tick is at least this many points from the next.
const MIN_TICK_POINTS: f32 = 60.0;
/// A guide is grabbed within this many points.
pub const GUIDE_GRAB: f32 = 4.0;
/// A point snaps to a guide within this many points.
const GUIDE_SNAP: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// A guide at a fixed x: a vertical line, made from the left ruler.
    X,
    /// A guide at a fixed y: a horizontal line, made from the top ruler.
    Y,
}

/// A picture behind the model to trace or compare. It starts fitted to the
/// model's bounds; `scale` and `offset` move it from there.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reference {
    /// 1 fits the picture to the width of the model.
    pub scale: f32,
    /// In model units, from where the fit puts the picture's centre.
    pub offset: Point,
    pub opacity: f32,
    pub visible: bool,
}

impl Default for Reference {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset: [0.0, 0.0],
            opacity: 0.5,
            visible: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewAids {
    pub grid: bool,
    /// In model units; 0 takes the step of the rulers at the current zoom.
    pub grid_step: f32,
    /// Moves in the editing tools go by grid steps, and new points land on them.
    pub snap: bool,
    pub rulers: bool,
    /// The info box: counts, zoom, the pointer.
    pub stats: bool,
    pub guides_x: Vec<f32>,
    pub guides_y: Vec<f32>,
    pub reference: Option<Reference>,
}

impl Default for ViewAids {
    fn default() -> Self {
        Self {
            grid: false,
            grid_step: 0.0,
            snap: false,
            rulers: false,
            stats: false,
            guides_x: Vec::new(),
            guides_y: Vec::new(),
            reference: None,
        }
    }
}

/// A round step (1, 2 or 5 times a power of ten) near `span / count`, so that
/// `count` steps cover `span`.
pub fn nice_step(span: f32, count: f32) -> f32 {
    let raw = (span / count.max(1.0)).abs();
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let power = 10f32.powf(raw.log10().floor());
    let unit = raw / power;
    let nice = if unit <= 1.0 {
        1.0
    } else if unit <= 2.0 {
        2.0
    } else if unit <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * power
}

/// The multiples of `step` in `[lo, hi]`, at most `limit` of them.
pub fn ticks(lo: f32, hi: f32, step: f32, limit: usize) -> Vec<f32> {
    if !(step.is_finite() && step > 0.0 && lo.is_finite() && hi.is_finite()) || hi < lo {
        return Vec::new();
    }
    let first = (lo / step).ceil() as i64;
    (first..)
        .map(|i| i as f32 * step)
        .take_while(|&v| v <= hi)
        .take(limit)
        .collect()
}

impl ViewAids {
    /// The grid step as the view shows it now: the set one, or the rulers'
    /// step for a viewport `width` points wide that shows `span` model units.
    pub fn step(&self, span: f32, width: f32) -> f32 {
        if self.grid_step > 0.0 {
            self.grid_step
        } else {
            nice_step(span, width / MIN_TICK_POINTS)
        }
    }

    pub fn guides(&self, axis: Axis) -> &[f32] {
        match axis {
            Axis::X => &self.guides_x,
            Axis::Y => &self.guides_y,
        }
    }

    pub fn guides_mut(&mut self, axis: Axis) -> &mut Vec<f32> {
        match axis {
            Axis::X => &mut self.guides_x,
            Axis::Y => &mut self.guides_y,
        }
    }

    pub fn clear_guides(&mut self) {
        self.guides_x.clear();
        self.guides_y.clear();
    }

    /// The step moves snap to in `mapper`'s view, if snapping is on.
    pub fn snap_step(&self, mapper: &Mapper) -> Option<f32> {
        self.snap
            .then(|| self.step(mapper.view[2] - mapper.view[0], mapper.size[0]))
    }

    /// A point after snapping: to a guide within a few points of it, else to
    /// the grid. Unchanged when snapping is off.
    pub fn snap_point(&self, mapper: &Mapper, p: Point) -> Point {
        let Some(step) = self.snap_step(mapper) else {
            return p;
        };
        let per_point = [
            (mapper.view[2] - mapper.view[0]) / mapper.size[0].max(1.0),
            (mapper.view[3] - mapper.view[1]) / mapper.size[1].max(1.0),
        ];
        let one = |v: f32, guides: &[f32], tolerance: f32| {
            guides
                .iter()
                .copied()
                .filter(|g| (g - v).abs() <= tolerance)
                .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
                .unwrap_or_else(|| (v / step).round() * step)
        };
        [
            one(p[0], &self.guides_x, GUIDE_SNAP * per_point[0]),
            one(p[1], &self.guides_y, GUIDE_SNAP * per_point[1]),
        ]
    }

    /// The screen position of a drag from `from` to `to` whose distance has
    /// been rounded to whole grid steps. Unchanged when snapping is off.
    pub fn snap_drag(&self, mapper: &Mapper, from: [f32; 2], to: [f32; 2]) -> [f32; 2] {
        let Some(step) = self.snap_step(mapper) else {
            return to;
        };
        let (a, b) = (mapper.to_model(from), mapper.to_model(to));
        let d = [
            ((b[0] - a[0]) / step).round() * step,
            ((b[1] - a[1]) / step).round() * step,
        ];
        mapper.to_screen([a[0] + d[0], a[1] + d[1]])
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).unwrap_or_default()
    }

    /// Settings from a project's file; anything unreadable gives the defaults.
    pub fn from_json(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).unwrap_or_default()
    }

    /// Whether there is anything to keep: all defaults are not written.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAPPER: Mapper = Mapper {
        origin: [0.0, 0.0],
        size: [100.0, 100.0],
        view: [0.0, 0.0, 100.0, 100.0],
    };

    #[test]
    fn steps_are_round_and_cover_the_span() {
        assert_eq!(nice_step(100.0, 10.0), 10.0);
        assert_eq!(nice_step(100.0, 7.0), 20.0);
        assert_eq!(nice_step(3.0, 10.0), 0.5);
        assert_eq!(nice_step(0.0, 10.0), 1.0);
        assert_eq!(nice_step(f32::NAN, 10.0), 1.0);
    }

    #[test]
    fn ticks_are_the_multiples_inside_the_range() {
        assert_eq!(ticks(-5.0, 12.0, 5.0, 100), [-5.0, 0.0, 5.0, 10.0]);
        assert_eq!(ticks(0.0, 100.0, 10.0, 3), [0.0, 10.0, 20.0]);
        assert!(ticks(0.0, 1.0, 0.0, 10).is_empty());
        assert!(ticks(5.0, 1.0, 1.0, 10).is_empty());
    }

    #[test]
    fn the_grid_step_is_the_set_one_or_the_rulers() {
        let mut aids = ViewAids::default();
        assert_eq!(aids.step(600.0, 600.0), 100.0);
        aids.grid_step = 25.0;
        assert_eq!(aids.step(600.0, 600.0), 25.0);
    }

    #[test]
    fn snapping_is_off_by_default_and_rounds_to_the_grid_or_a_guide() {
        let mut aids = ViewAids {
            grid_step: 10.0,
            ..ViewAids::default()
        };
        assert_eq!(aids.snap_point(&MAPPER, [13.0, 27.0]), [13.0, 27.0]);
        aids.snap = true;
        assert_eq!(aids.snap_point(&MAPPER, [13.0, 27.0]), [10.0, 30.0]);
        aids.guides_x.push(14.0);
        assert_eq!(aids.snap_point(&MAPPER, [13.0, 27.0]), [14.0, 30.0]);
        // A guide out of reach does nothing.
        aids.guides_x = vec![40.0];
        assert_eq!(aids.snap_point(&MAPPER, [13.0, 27.0]), [10.0, 30.0]);
    }

    #[test]
    fn a_snapped_drag_moves_by_whole_steps() {
        let aids = ViewAids {
            grid_step: 10.0,
            snap: true,
            ..ViewAids::default()
        };
        let from = MAPPER.to_screen([33.0, 33.0]);
        let to = MAPPER.to_screen([48.0, 41.0]);
        let snapped = MAPPER.to_model(aids.snap_drag(&MAPPER, from, to));
        assert!((snapped[0] - 53.0).abs() < 1e-3 && (snapped[1] - 43.0).abs() < 1e-3);
        let off = ViewAids::default();
        assert_eq!(off.snap_drag(&MAPPER, from, to), to);
    }

    #[test]
    fn settings_round_trip_and_unreadable_ones_give_the_defaults() {
        let mut aids = ViewAids {
            grid: true,
            guides_y: vec![1.5],
            reference: Some(Reference::default()),
            ..ViewAids::default()
        };
        aids.rulers = true;
        assert_eq!(ViewAids::from_json(&aids.to_json()), aids);
        assert!(ViewAids::from_json(b"{ nope").is_default());
        // A file with fewer fields (an older writer) keeps the defaults for the rest.
        assert!(ViewAids::from_json(br#"{"grid": true}"#).grid);
        assert!(ViewAids::default().is_default() && !aids.is_default());
    }
}
