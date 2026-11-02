// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Proportional editing: how much each point follows when some points are
//! moved. Given the positions, the edges between points and the points being
//! moved, [`weights`] gives every point a weight from 0 to 1 that falls off
//! with the distance to the nearest moved point. The editors move a point by
//! the move of the anchor times its weight; the keyframe editor can use the
//! same function. Pure and deterministic.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::elements::Point;

/// How the weight falls from 1 at the anchor to 0 at the radius.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Falloff {
    /// `1 - smoothstep(d / radius)`: flat at both ends.
    #[default]
    Smooth,
    /// `1 - d / radius`.
    Linear,
    /// `(1 - d / radius)^2`: falls fast, then slowly.
    Sharp,
    /// 1 everywhere inside the radius.
    Constant,
}

/// What distance is measured along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Distance {
    /// The shortest way along the edges: points not joined to an anchor are not reached.
    #[default]
    Path,
    /// The straight line: separate islands inside the radius follow too.
    Straight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Influence {
    /// Reach, in the units of the positions; 0 moves the anchors only.
    pub radius: f32,
    pub falloff: Falloff,
    pub distance: Distance,
}

impl Falloff {
    /// The weight at distance `d` of `radius`: 1 at the anchor, 0 at and
    /// beyond the radius (`Constant` keeps 1 up to the radius).
    pub fn weight(self, d: f32, radius: f32) -> f32 {
        if d <= 0.0 {
            return 1.0;
        }
        if radius <= 0.0 || d > radius {
            return 0.0;
        }
        let t = (d / radius).clamp(0.0, 1.0);
        match self {
            Falloff::Smooth => 1.0 - t * t * (3.0 - 2.0 * t),
            Falloff::Linear => 1.0 - t,
            Falloff::Sharp => (1.0 - t) * (1.0 - t),
            Falloff::Constant => 1.0,
        }
    }
}

/// A distance in the heap, smallest first.
#[derive(PartialEq)]
struct Entry(f32, u32);

impl Eq for Entry {}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed for a min-heap; ties by index so the order is fixed.
        other.0.total_cmp(&self.0).then(other.1.cmp(&self.1))
    }
}

fn dist(a: Point, b: Point) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The weight of every point (0 to 1). `anchors` are the indices of the
/// points being moved: they weigh 1, the others by their distance to the
/// nearest anchor. Indices out of range are ignored.
pub fn weights(
    positions: &[Point],
    edges: &[(u32, u32)],
    anchors: &[u32],
    influence: &Influence,
) -> Vec<f32> {
    let n = positions.len();
    let anchors: Vec<u32> = anchors
        .iter()
        .copied()
        .filter(|&a| (a as usize) < n)
        .collect();
    let radius = if influence.radius.is_finite() {
        influence.radius.max(0.0)
    } else {
        0.0
    };
    let reach = match influence.distance {
        Distance::Path => path_distances(positions, edges, &anchors, radius),
        Distance::Straight => straight_distances(positions, &anchors, radius),
    };
    let mut out: Vec<f32> = reach
        .iter()
        .map(|&d| influence.falloff.weight(d, radius))
        .collect();
    for &a in &anchors {
        out[a as usize] = 1.0;
    }
    out
}

/// Shortest distance along the edges to the nearest anchor, `INFINITY` for
/// points further than `radius` (not explored) or not joined to one.
fn path_distances(
    positions: &[Point],
    edges: &[(u32, u32)],
    anchors: &[u32],
    radius: f32,
) -> Vec<f32> {
    let n = positions.len();
    let mut dist_to = vec![f32::INFINITY; n];
    let mut heap = BinaryHeap::new();
    for &a in anchors {
        dist_to[a as usize] = 0.0;
        heap.push(Entry(0.0, a));
    }
    if radius <= 0.0 {
        return dist_to;
    }
    // Adjacency in a flat form: neighbours of v are next[start[v]..start[v + 1]].
    let mut start = vec![0usize; n + 1];
    let valid = |&&(a, b): &&(u32, u32)| (a as usize) < n && (b as usize) < n && a != b;
    for &(a, b) in edges.iter().filter(valid) {
        start[a as usize + 1] += 1;
        start[b as usize + 1] += 1;
    }
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut fill = start.clone();
    let mut next = vec![0u32; start[n]];
    for &(a, b) in edges.iter().filter(valid) {
        next[fill[a as usize]] = b;
        fill[a as usize] += 1;
        next[fill[b as usize]] = a;
        fill[b as usize] += 1;
    }
    while let Some(Entry(d, v)) = heap.pop() {
        if d > dist_to[v as usize] || d > radius {
            continue;
        }
        for &w in &next[start[v as usize]..start[v as usize + 1]] {
            let nd = d + dist(positions[v as usize], positions[w as usize]);
            if nd < dist_to[w as usize] && nd <= radius {
                dist_to[w as usize] = nd;
                heap.push(Entry(nd, w));
            }
        }
    }
    dist_to
}

/// Distance to the nearest anchor in a straight line, `INFINITY` beyond
/// `radius`. Anchors are put in a grid of cells one radius wide, so a point
/// looks only at its own cell and the eight around it.
fn straight_distances(positions: &[Point], anchors: &[u32], radius: f32) -> Vec<f32> {
    let mut out = vec![f32::INFINITY; positions.len()];
    for &a in anchors {
        out[a as usize] = 0.0;
    }
    if radius <= 0.0 || anchors.is_empty() {
        return out;
    }
    let cell = |p: Point| {
        (
            (p[0] / radius).floor() as i64,
            (p[1] / radius).floor() as i64,
        )
    };
    let mut grid: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
    for &a in anchors {
        grid.entry(cell(positions[a as usize])).or_default().push(a);
    }
    for (i, &p) in positions.iter().enumerate() {
        let (cx, cy) = cell(p);
        let mut best = out[i];
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &a in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    best = best.min(dist(p, positions[a as usize]));
                }
            }
        }
        if best <= radius {
            out[i] = best;
        }
    }
    out
}

/// The edges of a grid of `rows` x `columns` cells: each point with the one
/// to its right and the one below, points numbered row by row.
pub fn grid_edges(rows: u32, columns: u32) -> Vec<(u32, u32)> {
    let at = |r: u32, c: u32| r * (columns + 1) + c;
    let mut edges = Vec::new();
    for r in 0..=rows {
        for c in 0..=columns {
            if c < columns {
                edges.push((at(r, c), at(r, c + 1)));
            }
            if r < rows {
                edges.push((at(r, c), at(r + 1, c)));
            }
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize) -> (Vec<Point>, Vec<(u32, u32)>) {
        let p = (0..n).map(|i| [i as f32, 0.0]).collect();
        let e = (0..n as u32 - 1).map(|i| (i, i + 1)).collect();
        (p, e)
    }

    fn infl(radius: f32, falloff: Falloff, distance: Distance) -> Influence {
        Influence {
            radius,
            falloff,
            distance,
        }
    }

    #[test]
    fn the_anchor_weighs_one_the_outside_zero_and_each_curve_falls_monotonically() {
        let (p, e) = line(12);
        for falloff in [
            Falloff::Smooth,
            Falloff::Linear,
            Falloff::Sharp,
            Falloff::Constant,
        ] {
            let w = weights(&p, &e, &[0], &infl(6.0, falloff, Distance::Path));
            assert_eq!(w[0], 1.0, "{falloff:?}");
            // At the radius and beyond: nothing (a constant curve holds 1 up to the radius).
            assert!(w[7..].iter().all(|&x| x == 0.0), "{falloff:?} {w:?}");
            assert!(
                w.windows(2).all(|p| p[1] <= p[0] + 1e-6),
                "{falloff:?} {w:?}"
            );
            assert!(w.iter().all(|x| (0.0..=1.0).contains(x)));
            if falloff == Falloff::Constant {
                assert!(w[..=6].iter().all(|&x| x == 1.0));
            } else {
                assert!(w[1] < 1.0 && w[5] > 0.0 && w[6] == 0.0, "{falloff:?} {w:?}");
            }
        }
        // The curves differ where they should: at the middle linear 0.5, smooth 0.5, sharp 0.25.
        let mid = |f| {
            weights(
                &line(12).0,
                &line(12).1,
                &[0],
                &infl(6.0, f, Distance::Path),
            )[3]
        };
        assert!((mid(Falloff::Linear) - 0.5).abs() < 1e-6);
        assert!((mid(Falloff::Smooth) - 0.5).abs() < 1e-6);
        assert!((mid(Falloff::Sharp) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_path_does_not_reach_another_island_but_a_straight_line_does() {
        // Two pieces of line, a gap of one unit between them.
        let positions: Vec<Point> =
            vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0], [4.0, 0.0]];
        let edges = [(0, 1), (1, 2), (3, 4)];
        let path = weights(
            &positions,
            &edges,
            &[0],
            &infl(4.0, Falloff::Linear, Distance::Path),
        );
        assert!(
            path[2] > 0.0 && path[3] == 0.0 && path[4] == 0.0,
            "{path:?}"
        );
        let straight = weights(
            &positions,
            &edges,
            &[0],
            &infl(4.0, Falloff::Linear, Distance::Straight),
        );
        assert!(
            straight[3] > 0.0 && straight[3] < straight[2],
            "{straight:?}"
        );
        // Beyond the radius even a straight line stops.
        assert_eq!(straight[4], 0.0);
    }

    #[test]
    fn a_path_is_the_shortest_way_and_bends_around() {
        // A U: 0 and 4 are close in a straight line but far along the path.
        let positions: Vec<Point> =
            vec![[0.0, 0.0], [0.0, 3.0], [1.0, 3.0], [2.0, 3.0], [2.0, 0.0]];
        let edges = [(0, 1), (1, 2), (2, 3), (3, 4)];
        let path = weights(
            &positions,
            &edges,
            &[0],
            &infl(5.0, Falloff::Linear, Distance::Path),
        );
        assert_eq!(path[4], 0.0, "9 units along the path");
        let straight = weights(
            &positions,
            &edges,
            &[0],
            &infl(5.0, Falloff::Linear, Distance::Straight),
        );
        assert!(straight[4] > 0.0);
        assert!((path[1] - (1.0 - 3.0 / 5.0)).abs() < 1e-6);
    }

    #[test]
    fn several_anchors_are_measured_to_the_nearest() {
        let (p, e) = line(11);
        let both = weights(
            &p,
            &e,
            &[0, 10],
            &infl(4.0, Falloff::Linear, Distance::Path),
        );
        assert_eq!(both[0], 1.0);
        assert_eq!(both[10], 1.0);
        assert!((both[2] - 0.5).abs() < 1e-6 && (both[8] - 0.5).abs() < 1e-6);
        assert_eq!(both[5], 0.0);
        let straight = weights(
            &p,
            &e,
            &[0, 10],
            &infl(4.0, Falloff::Linear, Distance::Straight),
        );
        assert_eq!(both, straight);
    }

    #[test]
    fn a_zero_radius_moves_the_anchors_alone_and_bad_input_is_ignored() {
        let (p, e) = line(5);
        for d in [Distance::Path, Distance::Straight] {
            for f in [Falloff::Smooth, Falloff::Constant] {
                let w = weights(&p, &e, &[2], &infl(0.0, f, d));
                assert_eq!(w, [0.0, 0.0, 1.0, 0.0, 0.0], "{f:?} {d:?}");
            }
        }
        // Out-of-range anchors and edges, no anchors, a negative or NaN radius.
        let w = weights(
            &p,
            &[(0, 9), (3, 3)],
            &[99],
            &infl(3.0, Falloff::Linear, Distance::Path),
        );
        assert!(w.iter().all(|&x| x == 0.0));
        assert!(
            weights(
                &p,
                &e,
                &[1],
                &infl(f32::NAN, Falloff::Linear, Distance::Path)
            )[2] == 0.0
        );
        assert!(weights(&p, &e, &[1], &infl(-3.0, Falloff::Linear, Distance::Path))[2] == 0.0);
        assert!(weights(&[], &[], &[0], &infl(1.0, Falloff::Smooth, Distance::Path)).is_empty());
    }

    #[test]
    fn the_same_input_gives_the_same_weights_and_random_meshes_stay_in_range() {
        let mut seed = 7u64;
        let mut rand = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 40) as f32) / (1u32 << 24) as f32
        };
        for case in 0..20 {
            let n = 30 + case * 7;
            let positions: Vec<Point> = (0..n).map(|_| [rand() * 20.0, rand() * 20.0]).collect();
            let edges: Vec<(u32, u32)> = (0..n * 2)
                .map(|_| {
                    (
                        (rand() * n as f32) as u32 % n as u32,
                        (rand() * n as f32) as u32 % n as u32,
                    )
                })
                .collect();
            let anchors: Vec<u32> = (0..3)
                .map(|_| (rand() * n as f32) as u32 % n as u32)
                .collect();
            for d in [Distance::Path, Distance::Straight] {
                for f in [
                    Falloff::Smooth,
                    Falloff::Linear,
                    Falloff::Sharp,
                    Falloff::Constant,
                ] {
                    let i = infl(rand() * 12.0, f, d);
                    let a = weights(&positions, &edges, &anchors, &i);
                    assert_eq!(a, weights(&positions, &edges, &anchors, &i));
                    assert!(a.iter().all(|x| (0.0..=1.0).contains(x)), "{f:?} {d:?}");
                    assert!(anchors.iter().all(|&k| a[k as usize] == 1.0));
                }
            }
        }
    }

    #[test]
    fn a_grid_has_its_horizontal_and_vertical_neighbours() {
        let e = grid_edges(2, 3);
        // 3 rows of 3 horizontal edges and 4 columns of 2 vertical ones.
        assert_eq!(e.len(), 3 * 3 + 4 * 2);
        assert!(e.contains(&(0, 1)) && e.contains(&(0, 4)) && !e.contains(&(0, 5)));
        // Weights spread over it by the grid.
        let positions: Vec<Point> = (0..3)
            .flat_map(|r| (0..4).map(move |c| [c as f32, r as f32]))
            .collect();
        let w = weights(
            &positions,
            &e,
            &[5],
            &infl(1.5, Falloff::Linear, Distance::Path),
        );
        assert!(w[5] == 1.0 && w[4] > 0.0 && w[6] > 0.0 && w[1] > 0.0 && w[9] > 0.0);
        assert_eq!(w[0], 0.0);
        assert!(grid_edges(0, 0).is_empty());
    }

    #[test]
    fn a_big_mesh_with_many_anchors_is_quick() {
        // 10 000 points on a grid, every other one an anchor.
        let side = 100u32;
        let positions: Vec<Point> = (0..side * side)
            .map(|i| [(i % side) as f32, (i / side) as f32])
            .collect();
        let edges = grid_edges(side - 1, side - 1);
        let anchors: Vec<u32> = (0..side * side).step_by(2).collect();
        let start = std::time::Instant::now();
        for d in [Distance::Path, Distance::Straight] {
            let w = weights(&positions, &edges, &anchors, &infl(6.0, Falloff::Smooth, d));
            assert_eq!(w.len(), positions.len());
        }
        assert!(start.elapsed().as_secs_f32() < 5.0);
    }
}
