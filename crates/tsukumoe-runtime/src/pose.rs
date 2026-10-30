// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The pose: parts in a group are mutually exclusive, and the one that is
//! not shown fades away.

use tsukumoe_core::tmod::PlaybackData;

/// Below this a part counts as hidden by a motion.
const VISIBLE: f32 = 0.001;

pub struct PosePlayer {
    /// One opacity per pose entry (the entry's position in `pose_entries`).
    opacity: Vec<f64>,
    /// The shown entry of each group, as a position inside the group.
    selected: Vec<usize>,
}

impl PosePlayer {
    /// Each group starts with its first part shown and the rest hidden.
    pub fn new(pb: &PlaybackData) -> Self {
        let mut opacity = vec![0.0; pb.pose_entries.len()];
        let mut selected = Vec::new();
        for pose in pb.pose.iter() {
            for g in pb.pose_groups_of(pose) {
                if g.entry_count > 0 {
                    opacity[g.entry_start as usize] = 1.0;
                }
                selected.push(0);
            }
        }
        Self { opacity, selected }
    }

    /// Writes the opacity of every part of the pose (and its linked parts)
    /// into `parts`, after the motions have been applied.
    ///
    /// A motion that drives a part's opacity above 0.001 shows that part (the
    /// most visible one when several do); a group with no such part keeps
    /// what it shows. The shown part fades in and the others fade out, both
    /// over the pose's fade time.
    pub fn update(&mut self, pb: &PlaybackData, dt: f64, parts: &mut [f32], driven: &[bool]) {
        let Some(pose) = pb.pose.first() else { return };
        let step = if pose.fade_in > 0.0 {
            dt / f64::from(pose.fade_in)
        } else {
            1.0
        };
        for (gi, g) in pb.pose_groups_of(pose).iter().enumerate() {
            let entries = pb.group_entries(g);
            let mut best: Option<(usize, f32)> = None;
            for (k, e) in entries.iter().enumerate() {
                let v = parts[e.part as usize];
                if driven[e.part as usize] && v > VISIBLE && best.is_none_or(|(_, b)| v > b) {
                    best = Some((k, v));
                }
            }
            if let Some((k, _)) = best {
                self.selected[gi] = k;
            }
            for (k, e) in entries.iter().enumerate() {
                let o = &mut self.opacity[g.entry_start as usize + k];
                *o = if k == self.selected[gi] {
                    (*o + step).min(1.0)
                } else {
                    (*o - step).max(0.0)
                };
                let v = *o as f32;
                parts[e.part as usize] = v;
                for &linked in pb.entry_links(e) {
                    parts[linked as usize] = v;
                }
            }
        }
    }
}
