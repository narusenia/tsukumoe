// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The editor's own options that have no place of their own elsewhere
//! (M5-02f): auto-save, the undo limit, the thresholds of the edit tools, the
//! viewport background and the grab radii. Saved per user in `options.json`;
//! a missing, broken or out-of-range value falls back to its default or is
//! clamped, so the editor always runs on usable numbers.

use std::cell::Cell;

use serde::{Deserialize, Serialize};

/// What the viewport is cleared to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    /// The theme's own surface colour.
    #[default]
    Theme,
    White,
    Black,
    Checker,
}

pub const AUTOSAVE_MINUTES: (u32, u32) = (1, 60);
pub const HISTORY_LIMIT: (u32, u32) = (10, 1000);
pub const WELD_RATIO: (f32, f32) = (0.001, 0.1);
pub const MIRROR_TOLERANCE: (f32, f32) = (0.001, 0.1);
pub const RADIUS: (f32, f32) = (4.0, 16.0);

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Options {
    pub autosave: bool,
    /// Minutes between auto-saves.
    pub autosave_minutes: u32,
    /// Undo steps kept; 0 keeps all.
    pub history_limit: u32,
    /// Vertices closer than this fraction of the mesh diagonal merge in `mesh.weld-near`.
    pub weld_ratio: f32,
    /// How far a mirrored vertex may be from its partner, as a fraction of the diagonal.
    pub mirror_tolerance: f32,
    pub background: Background,
    /// Screen points within which a vertex, or an edge, is grabbed.
    pub vertex_radius: f32,
    pub edge_radius: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            autosave: false,
            autosave_minutes: 5,
            history_limit: 200,
            weld_ratio: 0.01,
            mirror_tolerance: 0.02,
            background: Background::Theme,
            vertex_radius: 8.0,
            edge_radius: 5.0,
        }
    }
}

fn clamp_f(v: f32, (lo, hi): (f32, f32), default: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        default
    }
}

impl Options {
    /// Puts every value in its range (a non-number goes back to its default;
    /// a history limit of 0, "keep all", stays).
    pub fn clamped(mut self) -> Self {
        let d = Self::default();
        self.autosave_minutes = self
            .autosave_minutes
            .clamp(AUTOSAVE_MINUTES.0, AUTOSAVE_MINUTES.1);
        if self.history_limit != 0 {
            self.history_limit = self.history_limit.clamp(HISTORY_LIMIT.0, HISTORY_LIMIT.1);
        }
        self.weld_ratio = clamp_f(self.weld_ratio, WELD_RATIO, d.weld_ratio);
        self.mirror_tolerance =
            clamp_f(self.mirror_tolerance, MIRROR_TOLERANCE, d.mirror_tolerance);
        self.vertex_radius = clamp_f(self.vertex_radius, RADIUS, d.vertex_radius);
        self.edge_radius = clamp_f(self.edge_radius, RADIUS, d.edge_radius);
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("options serialize")
    }

    /// The options in `json`, clamped; defaults for a broken file.
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str::<Self>(json)
            .unwrap_or_default()
            .clamped()
    }
}

/// The thresholds the edit tools read, and the viewport background. Set by the editor once a frame on the
/// UI thread; a thread-local so that the many tool functions that read them
/// need no extra parameter, and a test thread always sees the defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub weld_ratio: f32,
    pub mirror_tolerance: f32,
    pub vertex_radius: f32,
    pub edge_radius: f32,
    pub background: Background,
}

impl From<&Options> for Tuning {
    fn from(o: &Options) -> Self {
        Self {
            weld_ratio: o.weld_ratio,
            mirror_tolerance: o.mirror_tolerance,
            vertex_radius: o.vertex_radius,
            edge_radius: o.edge_radius,
            background: o.background,
        }
    }
}

thread_local! {
    static TUNING: Cell<Option<Tuning>> = const { Cell::new(None) };
}

/// The thresholds in force on this thread: the options' last set, else the defaults.
pub fn tuning() -> Tuning {
    TUNING
        .with(Cell::get)
        .unwrap_or_else(|| (&Options::default()).into())
}

pub fn set_tuning(options: &Options) {
    TUNING.with(|t| t.set(Some(options.into())));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_broken_file_gives_the_defaults() {
        assert_eq!(Options::from_json("{"), Options::default());
        assert_eq!(Options::from_json("{}"), Options::default());
        let some = Options::from_json(r#"{"autosave":true}"#);
        assert!(some.autosave);
        assert_eq!(some.history_limit, 200);
    }

    #[test]
    fn values_are_clamped_and_the_round_trip_keeps_them() {
        let wild = Options::from_json(
            r#"{"autosave_minutes":999,"history_limit":3,"weld_ratio":5.0,
                "mirror_tolerance":0.0,"vertex_radius":1.0,"edge_radius":99.0}"#,
        );
        assert_eq!(wild.autosave_minutes, 60);
        assert_eq!(wild.history_limit, 10);
        assert_eq!(wild.weld_ratio, 0.1);
        assert_eq!(wild.mirror_tolerance, 0.001);
        assert_eq!((wild.vertex_radius, wild.edge_radius), (4.0, 16.0));
        assert_eq!(Options::from_json(&wild.to_json()), wild);
        // 0 means "keep all" and is not raised to the lower bound.
        assert_eq!(
            Options::from_json(r#"{"history_limit":0}"#).history_limit,
            0
        );
    }

    #[test]
    fn the_tuning_follows_the_options_set_on_this_thread() {
        assert_eq!(tuning(), (&Options::default()).into());
        let wide = Options {
            vertex_radius: 12.0,
            ..Options::default()
        };
        set_tuning(&wide);
        assert_eq!(tuning().vertex_radius, 12.0);
    }

    #[test]
    fn a_non_number_falls_back_to_its_default() {
        let o = Options {
            weld_ratio: f32::NAN,
            vertex_radius: f32::INFINITY,
            ..Options::default()
        }
        .clamped();
        assert_eq!(o.weld_ratio, 0.01);
        assert_eq!(o.vertex_radius, 8.0);
    }
}
