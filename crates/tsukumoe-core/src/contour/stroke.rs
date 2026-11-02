// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Line-art mode: for shapes that are strokes (hair strands, eyebrows, line
//! drawings) the outline of the opaque area is two thin edges close together
//! and a lattice has nowhere to go. Here the strokes are thinned to a
//! one-pixel skeleton, cut into paths at the branch points, and each path
//! becomes a ribbon (an outline polygon along it, as wide as the stroke is,
//! with points on its centre line), which the triangulation turns into a strip
//! of triangles that bends along the line.

use super::{
    Alpha, ContourError, ContourParams, Island, MAX_POINTS, P, area, self_intersects, simplify,
    to_point,
};

/// The most thinning passes: a stroke wider than twice this stays a band.
const MAX_PASSES: usize = 64;
/// How far to look to each side of a stroke's end for its width: at a branch the
/// normal would otherwise run along the other line.
const END_REACH: f64 = 12.0;
/// The most the centre line may be thinned by, in pixels.
const MAX_TOLERANCE: f64 = 0.75;
/// The cosine of the sharpest turn a ribbon may make (about 70 degrees); a
/// bend beyond it starts a new ribbon.
const MAX_TURN_COS: f64 = 0.34;
/// How many pixels either side of a point the turn there is measured over.
const WINDOW: usize = 6;
/// Paths shorter than this (pixels) are specks and are dropped.
const MIN_LENGTH: f64 = 3.0;

const NEIGHBOURS: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

struct Grid {
    w: i32,
    h: i32,
    on: Vec<bool>,
}

impl Grid {
    fn get(&self, x: i32, y: i32) -> bool {
        (0..self.w).contains(&x) && (0..self.h).contains(&y) && self.on[(y * self.w + x) as usize]
    }

    /// How many separate runs of set pixels the 8 neighbours make: 1 at an end,
    /// 2 along a line (also on a staircase), 3 or more at a branch.
    fn runs(&self, x: i32, y: i32) -> usize {
        let n: Vec<bool> = NEIGHBOURS
            .iter()
            .map(|(dx, dy)| self.get(x + dx, y + dy))
            .collect();
        (0..8).filter(|&i| !n[i] && n[(i + 1) % 8]).count()
    }
}

/// Zhang-Suen thinning: peels the border off until a line one pixel wide is left.
fn thin(grid: &mut Grid) {
    for _ in 0..MAX_PASSES {
        let mut changed = false;
        for step in 0..2 {
            let mut remove = Vec::new();
            for y in 0..grid.h {
                for x in 0..grid.w {
                    if !grid.get(x, y) {
                        continue;
                    }
                    let n: Vec<bool> = NEIGHBOURS
                        .iter()
                        .map(|(dx, dy)| grid.get(x + dx, y + dy))
                        .collect();
                    let count = n.iter().filter(|&&b| b).count();
                    if !(2..=6).contains(&count) {
                        continue;
                    }
                    let transitions = (0..8).filter(|&i| !n[i] && n[(i + 1) % 8]).count();
                    if transitions != 1 {
                        continue;
                    }
                    // n[0] north, n[2] east, n[4] south, n[6] west.
                    let (a, b) = if step == 0 {
                        (!(n[0] && n[2] && n[4]), !(n[2] && n[4] && n[6]))
                    } else {
                        (!(n[0] && n[2] && n[6]), !(n[0] && n[4] && n[6]))
                    };
                    if a && b {
                        remove.push((x, y));
                    }
                }
            }
            changed |= !remove.is_empty();
            for (x, y) in remove {
                grid.on[(y * grid.w + x) as usize] = false;
            }
        }
        if !changed {
            break;
        }
    }
}

/// The skeleton cut into paths of pixels, each in walking order. A pixel with
/// three or more neighbours is a branch point; paths end next to it.
fn paths(skeleton: &Grid) -> Vec<Vec<(i32, i32)>> {
    let (w, h) = (skeleton.w, skeleton.h);
    let branch = |x: i32, y: i32| skeleton.get(x, y) && skeleton.runs(x, y) >= 3;
    let mut seen = vec![false; (w * h) as usize];
    let mut out = Vec::new();
    let line = |x: i32, y: i32| skeleton.get(x, y) && !branch(x, y);
    // Starts at the pixels with one neighbour on the line, then whatever is
    // left (closed loops).
    let mut starts: Vec<(i32, i32)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if line(x, y) {
                let on_line = NEIGHBOURS
                    .iter()
                    .filter(|(dx, dy)| line(x + dx, y + dy))
                    .count();
                if on_line <= 1 {
                    starts.push((x, y));
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            if line(x, y) {
                starts.push((x, y));
            }
        }
    }
    for (sx, sy) in starts {
        if seen[(sy * w + sx) as usize] {
            continue;
        }
        let mut path = vec![(sx, sy)];
        seen[(sy * w + sx) as usize] = true;
        let mut at = (sx, sy);
        // Prefer the neighbour that keeps going straight (4-neighbours first).
        while let Some(next) = [2, 4, 6, 0, 1, 3, 5, 7]
            .into_iter()
            .map(|i| (at.0 + NEIGHBOURS[i].0, at.1 + NEIGHBOURS[i].1))
            .find(|&(x, y)| line(x, y) && !seen[(y * w + x) as usize])
        {
            seen[(next.1 * w + next.0) as usize] = true;
            path.push(next);
            at = next;
        }
        // Reach the branch point each end touches, so paths meet.
        for end in [0, path.len() - 1] {
            let (x, y) = path[end];
            if let Some(b) = NEIGHBOURS
                .iter()
                .map(|(dx, dy)| (x + dx, y + dy))
                .find(|&(bx, by)| branch(bx, by))
            {
                if end == 0 {
                    path.insert(0, b);
                } else {
                    path.push(b);
                }
            }
        }
        out.push(path);
    }
    out
}

/// A moving average of three, ends kept: removes the staircase of a pixel path.
fn smooth(points: &[P], rounds: usize) -> Vec<P> {
    let mut p = points.to_vec();
    for _ in 0..rounds {
        let before = p.clone();
        for i in 1..p.len().saturating_sub(1) {
            for k in 0..2 {
                p[i][k] = (before[i - 1][k] + before[i][k] + before[i + 1][k]) / 3.0;
            }
        }
    }
    p
}

/// Points on the polyline every `spacing`, not within a quarter of it of the ends.
fn resample(line: &[P], spacing: f64) -> Vec<P> {
    if spacing <= 0.0 {
        return Vec::new();
    }
    let total: f64 = line
        .windows(2)
        .map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]))
        .sum();
    let mut out = Vec::new();
    let mut at = spacing;
    let mut walked = 0.0;
    for s in line.windows(2) {
        let len = (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]);
        while at <= walked + len && at < total - spacing / 4.0 {
            let t = (at - walked) / len.max(1e-12);
            out.push([
                s[0][0] + (s[1][0] - s[0][0]) * t,
                s[0][1] + (s[1][1] - s[0][1]) * t,
            ]);
            at += spacing;
        }
        walked += len;
    }
    out
}

/// A ribbon around the polyline `line`: `left[i]` / `right[i]` to each side at point `i`.
fn ribbon(line: &[P], left: &[f64], right: &[f64]) -> Vec<P> {
    let n = line.len();
    let normal = |i: usize| {
        let (a, b) = (line[i.saturating_sub(1)], line[(i + 1).min(n - 1)]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy).max(1e-12);
        [dy / len, -dx / len]
    };
    let l: Vec<P> = (0..n)
        .map(|i| {
            let m = normal(i);
            [line[i][0] + m[0] * left[i], line[i][1] + m[1] * left[i]]
        })
        .collect();
    let r: Vec<P> = (0..n)
        .rev()
        .map(|i| {
            let m = normal(i);
            [line[i][0] - m[0] * right[i], line[i][1] - m[1] * right[i]]
        })
        .collect();
    let mut ring: Vec<P> = l.into_iter().chain(r).collect();
    if area(&ring) < 0.0 {
        ring.reverse();
    }
    ring
}

/// How far from `from` along `dir` the opaque pixels go (marched in quarter pixels).
fn reach(alpha: &Alpha, threshold: u8, from: P, dir: P, limit: f64) -> f64 {
    let opaque = |p: P| {
        let (x, y) = (p[0].floor() as i32, p[1].floor() as i32);
        (0..alpha.width as i32).contains(&x)
            && (0..alpha.height as i32).contains(&y)
            && alpha.data[(y as usize) * alpha.width as usize + x as usize] >= threshold
    };
    let step = 0.25;
    let mut d = 0.0;
    while d < limit && opaque([from[0] + dir[0] * (d + step), from[1] + dir[1] * (d + step)]) {
        d += step;
    }
    d
}

/// The pixel path cut into pieces where it turns by more than `MAX_TURN_COS`,
/// seen over `WINDOW` pixels either side (so a staircase does not count); a
/// piece starts at the pixel the one before ended on.
fn split_at_corners(line: &[P]) -> Vec<Vec<P>> {
    let n = line.len();
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut i = WINDOW;
    while i + WINDOW < n {
        let (a, b, c) = (line[i - WINDOW], line[i], line[i + WINDOW]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
        let cos = (u[0] * v[0] + u[1] * v[1]) / (u[0].hypot(u[1]) * v[0].hypot(v[1])).max(1e-12);
        if cos < MAX_TURN_COS {
            // The sharpest point of this turn: the middle of the run that stays sharp.
            let mut j = i;
            while j + 1 + WINDOW < n && {
                let (a, b, c) = (line[j + 1 - WINDOW], line[j + 1], line[j + 1 + WINDOW]);
                let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
                (u[0] * v[0] + u[1] * v[1]) / (u[0].hypot(u[1]) * v[0].hypot(v[1])).max(1e-12)
                    < MAX_TURN_COS
            } {
                j += 1;
            }
            let corner = (i + j) / 2;
            pieces.push(line[start..=corner].to_vec());
            start = corner;
            i = corner + WINDOW;
        } else {
            i += 1;
        }
    }
    pieces.push(line[start..].to_vec());
    pieces
}

fn unit(a: P, b: P) -> P {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy).max(1e-12);
    [dx / len, dy / len]
}

/// One ribbon along `kept`, or `None` when nothing sensible fits (too short, or
/// it folds over itself however narrow it is made).
fn piece(alpha: &Alpha, params: &ContourParams, mut kept: Vec<P>) -> Option<Island> {
    let length: f64 = kept
        .windows(2)
        .map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]))
        .sum();
    if kept.len() < 2 || length < MIN_LENGTH {
        return None;
    }
    // Thinning eats the ends of a stroke: walk each end on to where the stroke stops.
    let last = kept.len() - 1;
    for (end, next) in [(0, 1), (last, last - 1)] {
        let dir = unit(kept[next], kept[end]);
        // No farther than half the stroke is wide: that is how much thinning eats,
        // and at a branch it must not run on into the other line.
        let normal = [dir[1], -dir[0]];
        let half = (reach(alpha, params.threshold, kept[end], normal, END_REACH)
            + reach(
                alpha,
                params.threshold,
                kept[end],
                [-normal[0], -normal[1]],
                END_REACH,
            ))
            / 2.0;
        let more = reach(alpha, params.threshold, kept[end], dir, half + 0.5);
        kept[end] = [kept[end][0] + dir[0] * more, kept[end][1] + dir[1] * more];
    }
    // How far the stroke goes to each side of the centre line, measured on the
    // image (so even and odd widths come out right), averaged with the
    // neighbours.
    let n = kept.len();
    let sides: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let t = unit(kept[i.saturating_sub(1)], kept[(i + 1).min(n - 1)]);
            let normal = [t[1], -t[0]];
            (
                reach(alpha, params.threshold, kept[i], normal, 64.0),
                reach(
                    alpha,
                    params.threshold,
                    kept[i],
                    [-normal[0], -normal[1]],
                    64.0,
                ),
            )
        })
        .collect();
    let average = |pick: fn(&(f64, f64)) -> f64| -> Vec<f64> {
        (0..n)
            .map(|i| {
                let (lo, hi) = (i.saturating_sub(1), (i + 1).min(n - 1));
                (lo..=hi).map(|j| pick(&sides[j])).sum::<f64>() / (hi - lo + 1) as f64
            })
            .collect()
    };
    // Near a branch the normal runs into the other line: no side is wider than
    // half as much again as the stroke is along most of its length.
    let limit = |v: Vec<f64>| {
        let mut sorted = v.clone();
        sorted.sort_by(f64::total_cmp);
        let cap = sorted[sorted.len() / 2] * 1.5 + 0.5;
        v.into_iter().map(|x| x.min(cap)).collect::<Vec<_>>()
    };
    let (left, right) = (limit(average(|s| s.0)), limit(average(|s| s.1)));
    let fixed = f64::from(params.stroke_width) / 2.0;
    // A tight bend can fold the ribbon over itself: narrow it until it does not.
    let mut scale = 1.0;
    let ring = loop {
        let grow = |h: &f64| {
            let half = if fixed > 0.0 { fixed } else { *h };
            (half + f64::from(params.offset)) * scale
        };
        let (l, r): (Vec<f64>, Vec<f64>) = (
            left.iter().map(grow).collect(),
            right.iter().map(grow).collect(),
        );
        let ring = ribbon(&kept, &l, &r);
        if area(&ring) > 0.0 && !self_intersects(&ring) {
            break ring;
        }
        scale *= 0.7;
        if scale < 0.3 {
            return None;
        }
    };
    let inner = resample(&kept, f64::from(params.spacing));
    Some(Island {
        outline: ring.iter().map(|&p| to_point(p)).collect(),
        inner: inner.into_iter().map(to_point).collect(),
    })
}

pub(super) fn islands(alpha: &Alpha, params: &ContourParams) -> Result<Vec<Island>, ContourError> {
    let (w, h) = (alpha.width as i32, alpha.height as i32);
    let mut skeleton = Grid {
        w,
        h,
        on: alpha.data.iter().map(|&a| a >= params.threshold).collect(),
    };
    thin(&mut skeleton);
    // A closed loop has no ends for a ribbon to start and stop at: cut it in two.
    let mut all = Vec::new();
    for path in paths(&skeleton) {
        let (first, last) = (path[0], path[path.len() - 1]);
        let closed = (first.0 - last.0).abs() <= 1 && (first.1 - last.1).abs() <= 1;
        if closed && path.len() > 8 {
            let mid = path.len() / 2;
            let mut second = path[mid..].to_vec();
            second.push(first);
            all.push(path[..=mid].to_vec());
            all.push(second);
        } else {
            all.push(path);
        }
    }
    let mut out = Vec::new();
    for path in all {
        let centre: Vec<P> = path
            .iter()
            .map(|&(x, y)| [f64::from(x) + 0.5, f64::from(y) + 0.5])
            .collect();
        for part in split_at_corners(&centre) {
            let line = smooth(&part, 2);
            // A stroke is only a few pixels wide: a coarse tolerance would cut across its bends.
            let tolerance = f64::from(params.simplify).clamp(0.25, MAX_TOLERANCE);
            let kept = simplify_open(&line, tolerance);
            let Some(island) = piece(alpha, params, kept) else {
                continue;
            };
            if island.outline.len() + island.inner.len() > MAX_POINTS {
                return Err(ContourError::TooManyPoints);
            }
            out.push(island);
        }
    }
    if out.is_empty() {
        return Err(ContourError::Empty);
    }
    Ok(out)
}

/// Douglas-Peucker on an open polyline (the ends stay).
fn simplify_open(line: &[P], tolerance: f64) -> Vec<P> {
    if line.len() < 3 {
        return line.to_vec();
    }
    // `simplify` works on rings: close the line by doubling it back, which keeps
    // both ends because they are the farthest apart.
    let mut ring = line.to_vec();
    ring.extend(line[1..line.len() - 1].iter().rev());
    let thin = simplify(&ring, tolerance);
    // Take the way out again: the points up to the far end, in line order.
    let far = line[line.len() - 1];
    let end = thin
        .iter()
        .position(|p| p[0] == far[0] && p[1] == far[1])
        .unwrap_or(thin.len() - 1);
    thin[..=end].to_vec()
}
