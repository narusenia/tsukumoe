// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Mesh from the alpha of a texture: the outline of each opaque island
//! (marching squares), thinned (Douglas-Peucker) and pushed outward, then a
//! triangulation with the outline as its outer edge and a lattice of points
//! inside. Pure functions on an alpha array, so no image file is needed to
//! test them. Everything is deterministic.
//!
//! Pixel coordinates have the origin at the top left, `y` down, and a pixel
//! `(x, y)` covers `[x, x + 1] x [y, y + 1]`. A texture coordinate is the
//! pixel coordinate over the image size (`v = 0` is the top row), so a point
//! found in pixels has an exact UV and only its position has to be derived,
//! from the correspondence of the original mesh's positions and UVs.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use crate::elements::Point;
use crate::mesh_edit::{self, Geometry, in_circle};

/// The most points (outline and lattice together) one island may have.
pub const MAX_POINTS: usize = 3000;

/// An 8-bit alpha image, row by row from the top.
#[derive(Clone, Copy, Debug)]
pub struct Alpha<'a> {
    pub width: u32,
    pub height: u32,
    pub data: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContourParams {
    /// Pixels with alpha at or above this are opaque.
    pub threshold: u8,
    /// Islands smaller than this fraction of the image area are dropped.
    pub min_island: f32,
    /// Douglas-Peucker tolerance in pixels; 0 keeps every vertex.
    pub simplify: f32,
    /// How far the outline is pushed outward, in pixels.
    pub offset: f32,
    /// Spacing of the inner lattice in pixels; 0 adds no inner points.
    pub spacing: f32,
}

impl Default for ContourParams {
    fn default() -> Self {
        Self {
            threshold: 16,
            min_island: 0.001,
            simplify: 2.0,
            offset: 2.0,
            spacing: 64.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourError {
    /// The alpha array does not match the size, or the image is empty.
    BadImage,
    /// A parameter is out of range (negative, not finite, or a spacing under 4).
    BadParams,
    /// No island is left: the image is transparent, or every island is small.
    Empty,
    /// The original mesh has no triangle with a usable UV area.
    NoUvMapping,
    /// More than [`MAX_POINTS`] points in one island: raise the spacing.
    TooManyPoints,
    /// The outline could not be made the outer edge of a triangulation.
    CannotTriangulate,
}

impl fmt::Display for ContourError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BadImage => "the alpha image is empty or its size does not match",
            Self::BadParams => "a contour parameter is out of range",
            Self::Empty => "no opaque island was found",
            Self::NoUvMapping => "the mesh has no triangle with a UV area",
            Self::TooManyPoints => "too many points: raise the spacing",
            Self::CannotTriangulate => "the outline cannot be triangulated",
        })
    }
}

impl std::error::Error for ContourError {}

type P = [f64; 2];

fn cross(o: P, a: P, b: P) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Signed area; positive when the ring runs counter-clockwise in the plane.
fn area(ring: &[P]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

/// Even-odd test.
fn inside(ring: &[P], p: P) -> bool {
    let n = ring.len();
    let mut odd = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            odd = !odd;
        }
    }
    odd
}

fn segment_distance(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

fn ring_distance(ring: &[P], p: P) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| segment_distance(p, ring[i], ring[(i + 1) % n]))
        .fold(f64::MAX, f64::min)
}

/// A crossing point on the pixel lattice: the edge between two neighbouring
/// pixel centres, horizontal (`h`) or vertical.
type Edge = (i32, i32, bool);

/// Directed outlines of the opaque region, the opaque side on the left (so
/// outer outlines have positive area and holes negative). The image is
/// padded with transparent pixels, so every outline is closed.
fn trace(alpha: &Alpha, threshold: u8) -> Vec<Vec<P>> {
    let (w, h) = (alpha.width as i32, alpha.height as i32);
    let opaque = |x: i32, y: i32| {
        (0..w).contains(&x)
            && (0..h).contains(&y)
            && alpha.data[(y as usize) * w as usize + x as usize] >= threshold
    };
    let mut next: BTreeMap<Edge, Edge> = BTreeMap::new();
    for cy in -1..h {
        for cx in -1..w {
            let case = u8::from(opaque(cx, cy))
                | u8::from(opaque(cx + 1, cy)) << 1
                | u8::from(opaque(cx + 1, cy + 1)) << 2
                | u8::from(opaque(cx, cy + 1)) << 3;
            if case == 0 || case == 15 {
                continue;
            }
            let top = (cx, cy, true);
            let bottom = (cx, cy + 1, true);
            let left = (cx, cy, false);
            let right = (cx + 1, cy, false);
            // Local midpoints and corners for orienting: (edge, midpoint).
            let mid = |e: Edge| -> P {
                if e == top {
                    [0.5, 0.0]
                } else if e == bottom {
                    [0.5, 1.0]
                } else if e == left {
                    [0.0, 0.5]
                } else {
                    [1.0, 0.5]
                }
            };
            const TL: P = [0.0, 0.0];
            const TR: P = [1.0, 0.0];
            const BR: P = [1.0, 1.0];
            const BL: P = [0.0, 1.0];
            // (from, to, a corner on one side, whether that corner is opaque)
            let segments: &[(Edge, Edge, P, bool)] = match case {
                1 => &[(left, top, TL, true)],
                2 => &[(top, right, TR, true)],
                4 => &[(right, bottom, BR, true)],
                8 => &[(bottom, left, BL, true)],
                14 => &[(left, top, TL, false)],
                13 => &[(top, right, TR, false)],
                11 => &[(right, bottom, BR, false)],
                7 => &[(bottom, left, BL, false)],
                3 => &[(left, right, TL, true)],
                6 => &[(top, bottom, TR, true)],
                12 => &[(left, right, BR, true)],
                9 => &[(top, bottom, TL, true)],
                5 => &[(top, right, TR, false), (bottom, left, BL, false)],
                _ => &[(left, top, TL, false), (right, bottom, BR, false)],
            };
            for &(a, b, corner, corner_opaque) in segments {
                let (pa, pb) = (mid(a), mid(b));
                let side = cross(pa, pb, corner);
                // The opaque side must be on the left (cross > 0).
                let forward = (side > 0.0) == corner_opaque;
                let (from, to) = if forward { (a, b) } else { (b, a) };
                next.insert(from, to);
            }
        }
    }
    let point = |e: Edge| -> P {
        let (x, y) = (f64::from(e.0), f64::from(e.1));
        if e.2 {
            [x + 1.0, y + 0.5]
        } else {
            [x + 0.5, y + 1.0]
        }
    };
    let mut loops = Vec::new();
    while let Some((&start, _)) = next.iter().next() {
        let mut ring = vec![point(start)];
        let mut at = start;
        while let Some(to) = next.remove(&at) {
            if to == start {
                break;
            }
            ring.push(point(to));
            at = to;
        }
        loops.push(ring);
    }
    loops
}

/// Douglas-Peucker on a closed ring; keeps the first vertex and the one
/// farthest from it.
fn simplify(ring: &[P], tolerance: f64) -> Vec<P> {
    let n = ring.len();
    if tolerance <= 0.0 || n < 4 {
        return ring.to_vec();
    }
    let far = (1..n)
        .max_by(|&a, &b| {
            let d = |i: usize| (ring[i][0] - ring[0][0]).hypot(ring[i][1] - ring[0][1]);
            d(a).total_cmp(&d(b))
        })
        .expect("ring has vertices");
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    let mut stack = vec![(0, far), (far, n)];
    while let Some((lo, hi)) = stack.pop() {
        let (a, b) = (ring[lo % n], ring[hi % n]);
        let worst = (lo + 1..hi)
            .map(|i| (i, segment_distance(ring[i % n], a, b)))
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((i, d)) = worst
            && d > tolerance
        {
            keep[i % n] = true;
            stack.push((lo, i));
            stack.push((i, hi));
        }
    }
    (0..n).filter(|&i| keep[i]).map(|i| ring[i]).collect()
}

fn properly_cross(a: P, b: P, c: P, d: P) -> bool {
    cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0
}

fn self_intersects(ring: &[P]) -> bool {
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (c, d) = (ring[j], ring[(j + 1) % n]);
            if properly_cross(a, b, c, d) {
                return true;
            }
        }
    }
    false
}

/// The ring pushed outward by `d` at its vertices (miter, limited to twice `d`).
fn push_out(ring: &[P], d: f64) -> Vec<P> {
    let n = ring.len();
    let normal = |a: P, b: P| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy).max(1e-12);
        [dy / len, -dx / len]
    };
    (0..n)
        .map(|i| {
            let (prev, v, next) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            let (n1, n2) = (normal(prev, v), normal(v, next));
            let sum = [n1[0] + n2[0], n1[1] + n2[1]];
            let len = sum[0].hypot(sum[1]);
            let m = if len < 1e-9 {
                n1
            } else {
                [sum[0] / len, sum[1] / len]
            };
            let cos_half = (m[0] * n1[0] + m[1] * n1[1]).max(0.5);
            [v[0] + m[0] * d / cos_half, v[1] + m[1] * d / cos_half]
        })
        .collect()
}

/// Pushes the ring out by `offset`, or by less if that makes it cross itself;
/// `None` when even the unmoved ring crosses itself.
fn offset_ring(ring: &[P], offset: f64) -> Option<Vec<P>> {
    if self_intersects(ring) {
        return None;
    }
    let mut d = offset;
    for _ in 0..6 {
        if d <= 0.0 {
            break;
        }
        let moved = push_out(ring, d);
        if !self_intersects(&moved) && area(&moved) > area(ring) {
            return Some(moved);
        }
        d /= 2.0;
    }
    Some(ring.to_vec())
}

/// One island: its outline and the lattice points inside, in pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    pub outline: Vec<Point>,
    pub inner: Vec<Point>,
}

fn to_point(p: P) -> Point {
    [p[0] as f32, p[1] as f32]
}

fn check(alpha: &Alpha, params: &ContourParams) -> Result<(), ContourError> {
    let ok = |v: f32| v.is_finite() && v >= 0.0;
    if alpha.width == 0
        || alpha.height == 0
        || alpha.data.len() != alpha.width as usize * alpha.height as usize
    {
        return Err(ContourError::BadImage);
    }
    if !(ok(params.min_island) && ok(params.simplify) && ok(params.offset) && ok(params.spacing))
        || (params.spacing != 0.0 && params.spacing < 4.0)
    {
        return Err(ContourError::BadParams);
    }
    Ok(())
}

/// The outer outline of every island of the opaque region, thinned and pushed
/// outward, with the lattice points inside each. Holes are ignored (filled).
/// Fails with `Empty` when no island is left.
pub fn islands(alpha: &Alpha, params: &ContourParams) -> Result<Vec<Island>, ContourError> {
    check(alpha, params)?;
    let min_area = f64::from(params.min_island) * f64::from(alpha.width) * f64::from(alpha.height);
    let mut outer: Vec<Vec<P>> = trace(alpha, params.threshold)
        .into_iter()
        .filter(|r| area(r) > 0.0 && area(r) >= min_area)
        .collect();
    // An island inside the hole of a larger one is filled with it: drop it.
    outer.sort_by(|a, b| area(b).total_cmp(&area(a)));
    let mut kept: Vec<Vec<P>> = Vec::new();
    for ring in outer {
        if !kept.iter().any(|big| inside(big, ring[0])) {
            kept.push(ring);
        }
    }
    let mut out = Vec::new();
    for ring in kept {
        let thin = simplify(&ring, f64::from(params.simplify));
        if thin.len() < 3 || area(&thin) <= 0.0 {
            continue;
        }
        let Some(ring) = offset_ring(&thin, f64::from(params.offset)) else {
            continue;
        };
        let inner = lattice(&ring, f64::from(params.spacing));
        if ring.len() + inner.len() > MAX_POINTS {
            return Err(ContourError::TooManyPoints);
        }
        out.push(Island {
            outline: ring.iter().map(|&p| to_point(p)).collect(),
            inner: inner.into_iter().map(to_point).collect(),
        });
    }
    if out.is_empty() {
        return Err(ContourError::Empty);
    }
    Ok(out)
}

/// Lattice points inside `ring`, at least a quarter of the spacing from it.
fn lattice(ring: &[P], spacing: f64) -> Vec<P> {
    if spacing <= 0.0 {
        return Vec::new();
    }
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in ring {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let first = |v: f64| ((v / spacing).floor() + 0.5) * spacing;
    let mut points = Vec::new();
    let mut y = first(lo[1]);
    while y <= hi[1] {
        let mut x = first(lo[0]);
        while x <= hi[0] {
            let p = [x, y];
            if inside(ring, p) && ring_distance(ring, p) >= spacing / 4.0 {
                points.push(p);
            }
            x += spacing;
        }
        y += spacing;
    }
    points
}

/// Bowyer-Watson Delaunay triangulation, counter-clockwise triangles.
fn delaunay(points: &[P]) -> Vec<[u32; 3]> {
    let n = points.len();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in points {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let r = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0) * 1e3;
    let mut all = points.to_vec();
    all.extend([
        [c[0] - 2.0 * r, c[1] - r],
        [c[0] + 2.0 * r, c[1] - r],
        [c[0], c[1] + 2.0 * r],
    ]);
    let mut triangles: Vec<[u32; 3]> = vec![[n as u32, n as u32 + 1, n as u32 + 2]];
    for p in 0..n {
        let q = all[p];
        let bad: Vec<bool> = triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| all[i as usize]);
                let (det, size) = in_circle(a, b, c, q);
                det > 1e-10 * size
            })
            .collect();
        let mut directed: HashSet<(u32, u32)> = HashSet::new();
        for (t, _) in triangles.iter().zip(&bad).filter(|(_, b)| **b) {
            for k in 0..3 {
                directed.insert((t[k], t[(k + 1) % 3]));
            }
        }
        let mut next: Vec<[u32; 3]> = triangles
            .iter()
            .zip(&bad)
            .filter(|(_, b)| !**b)
            .map(|(t, _)| *t)
            .collect();
        let mut border: Vec<(u32, u32)> = directed
            .iter()
            .filter(|&&(a, b)| !directed.contains(&(b, a)))
            .copied()
            .collect();
        border.sort_unstable();
        next.extend(border.into_iter().map(|(a, b)| [a, b, p as u32]));
        triangles = next;
    }
    triangles.retain(|t| t.iter().all(|&i| (i as usize) < n));
    triangles
}

/// Triangulates the polygon `outline` with the points `inner` inside it: the
/// polygon is the outer edge of the result. Returns the points (outline
/// first, then `inner`) and counter-clockwise triangle indices. Works as the
/// plan says: Delaunay of all points, the outline edges drawn with
/// `mesh_edit::connect`, the triangles outside removed, and the rest
/// re-triangulated with only the outline kept.
pub fn triangulate_polygon(
    outline: &[Point],
    inner: &[Point],
) -> Result<(Vec<Point>, Vec<u32>), ContourError> {
    let points: Vec<Point> = outline.iter().chain(inner).copied().collect();
    if outline.len() < 3 {
        return Err(ContourError::CannotTriangulate);
    }
    let p64: Vec<P> = points
        .iter()
        .map(|p| [f64::from(p[0]), f64::from(p[1])])
        .collect();
    let mut seen = HashSet::new();
    if !points
        .iter()
        .all(|p| seen.insert((p[0].to_bits(), p[1].to_bits())))
    {
        return Err(ContourError::CannotTriangulate);
    }
    let mut g = Geometry {
        positions: points.clone(),
        uvs: vec![[0.0, 0.0]; points.len()],
        indices: delaunay(&p64).into_iter().flatten().collect(),
    };
    let n = outline.len() as u32;
    for i in 0..n {
        g = mesh_edit::connect(&g, i, (i + 1) % n).map_err(|_| ContourError::CannotTriangulate)?;
    }
    let ring = &p64[..outline.len()];
    let kept: Vec<u32> = g
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|t| {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| p64[i as usize]);
            inside(
                ring,
                [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0],
            )
        })
        .flatten()
        .copied()
        .collect();
    if kept.is_empty() {
        return Err(ContourError::CannotTriangulate);
    }
    g.indices = kept;
    let g = mesh_edit::retriangulate(&g, &[]).map_err(|_| ContourError::CannotTriangulate)?;
    Ok((g.positions, g.indices))
}

/// Where a texture coordinate lands on the original mesh: the barycentric
/// position in the triangle holding it, or, outside the UV area, the affine
/// extension of the nearest triangle.
struct UvMap {
    triangles: Vec<([P; 3], [P; 3])>,
}

impl UvMap {
    fn new(g: &Geometry) -> Result<Self, ContourError> {
        let at = |v: &[Point], i: u32| [f64::from(v[i as usize][0]), f64::from(v[i as usize][1])];
        let triangles: Vec<_> = g
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|t| t.iter().all(|&i| (i as usize) < g.positions.len()))
            .map(|t| {
                (
                    [at(&g.uvs, t[0]), at(&g.uvs, t[1]), at(&g.uvs, t[2])],
                    [
                        at(&g.positions, t[0]),
                        at(&g.positions, t[1]),
                        at(&g.positions, t[2]),
                    ],
                )
            })
            .filter(|(uv, _)| cross(uv[0], uv[1], uv[2]).abs() > 1e-12)
            .collect();
        if triangles.is_empty() {
            return Err(ContourError::NoUvMapping);
        }
        Ok(Self { triangles })
    }

    fn position(&self, uv: P) -> P {
        let weights = |t: &[P; 3]| {
            let a = cross(t[0], t[1], t[2]);
            [
                cross(uv, t[1], t[2]) / a,
                cross(t[0], uv, t[2]) / a,
                cross(t[0], t[1], uv) / a,
            ]
        };
        let distance = |t: &[P; 3]| {
            if weights(t).iter().all(|&w| w >= 0.0) {
                0.0
            } else {
                (0..3)
                    .map(|k| segment_distance(uv, t[k], t[(k + 1) % 3]))
                    .fold(f64::MAX, f64::min)
            }
        };
        let (uvs, pos) = self
            .triangles
            .iter()
            .min_by(|a, b| distance(&a.0).total_cmp(&distance(&b.0)))
            .expect("triangles are not empty");
        let w = weights(uvs);
        [
            w[0] * pos[0][0] + w[1] * pos[1][0] + w[2] * pos[2][0],
            w[0] * pos[0][1] + w[1] * pos[1][1] + w[2] * pos[2][1],
        ]
    }
}

/// A new mesh for `original` from the alpha of its texture: one outline per
/// island plus inner lattice points, triangulated. The triangles keep the
/// winding of the original. Positions come from the original's
/// position-UV correspondence; UVs are the pixel coordinates over the image
/// size and are not clamped.
pub fn contour_mesh(
    original: &Geometry,
    alpha: &Alpha,
    params: &ContourParams,
) -> Result<Geometry, ContourError> {
    let islands = islands(alpha, params)?;
    let map = UvMap::new(original)?;
    let winding: f64 = original
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|t| t.iter().all(|&i| (i as usize) < original.positions.len()))
        .map(|t| {
            let p = |i: u32| {
                let v = original.positions[i as usize];
                [f64::from(v[0]), f64::from(v[1])]
            };
            cross(p(t[0]), p(t[1]), p(t[2]))
        })
        .sum();
    let (w, h) = (f64::from(alpha.width), f64::from(alpha.height));
    let mut out = Geometry::default();
    for island in islands {
        let pixels: Vec<Point> = island
            .outline
            .iter()
            .chain(&island.inner)
            .copied()
            .collect();
        let uvs: Vec<P> = pixels
            .iter()
            .map(|p| [f64::from(p[0]) / w, f64::from(p[1]) / h])
            .collect();
        let positions: Vec<Point> = uvs.iter().map(|&uv| to_point(map.position(uv))).collect();
        let outline = island.outline.len();
        let (positions, indices) =
            triangulate_polygon(&positions[..outline], &positions[outline..])?;
        let base = out.positions.len() as u32;
        out.positions.extend(positions);
        out.uvs.extend(uvs.iter().map(|&uv| to_point(uv)));
        for t in indices.as_chunks::<3>().0 {
            let (b, c) = if winding < 0.0 {
                (t[2], t[1])
            } else {
                (t[1], t[2])
            };
            out.indices.extend([base + t[0], base + b, base + c]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32, f: impl Fn(f64, f64) -> bool) -> Vec<u8> {
        (0..w * h)
            .map(|i| {
                let (x, y) = (f64::from(i % w) + 0.5, f64::from(i / w) + 0.5);
                if f(x, y) { 255 } else { 0 }
            })
            .collect()
    }

    fn alpha(w: u32, h: u32, data: &[u8]) -> Alpha<'_> {
        Alpha {
            width: w,
            height: h,
            data,
        }
    }

    fn quad(w: f32, h: f32) -> Geometry {
        Geometry {
            positions: vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    fn ring64(r: &[Point]) -> Vec<P> {
        r.iter()
            .map(|p| [f64::from(p[0]), f64::from(p[1])])
            .collect()
    }

    fn circle(w: u32, h: u32) -> Vec<u8> {
        image(w, h, |x, y| (x - 128.0).hypot(y - 128.0) < 80.0)
    }

    fn l_shape() -> Vec<u8> {
        image(256, 256, |x, y| {
            (30.0..90.0).contains(&x) && (30.0..220.0).contains(&y)
                || (30.0..220.0).contains(&x) && (160.0..220.0).contains(&y)
        })
    }

    #[test]
    fn a_circle_gives_one_outline_of_about_its_area() {
        let data = circle(256, 256);
        let out = islands(&alpha(256, 256, &data), &ContourParams::default()).unwrap();
        assert_eq!(out.len(), 1);
        let ring = ring64(&out[0].outline);
        assert!((8..100).contains(&ring.len()), "{}", ring.len());
        // Radius 80 plus the offset 2 (the miter makes it a little larger).
        let want = std::f64::consts::PI * 82.0 * 82.0;
        assert!((area(&ring) - want).abs() / want < 0.04, "{}", area(&ring));
        assert!(!self_intersects(&ring));
        assert!(!out[0].inner.is_empty());
    }

    #[test]
    fn an_l_shape_is_one_concave_outline() {
        let data = l_shape();
        let out = islands(&alpha(256, 256, &data), &ContourParams::default()).unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            (6..=12).contains(&out[0].outline.len()),
            "{}",
            out[0].outline.len()
        );
        let ring = ring64(&out[0].outline);
        // Inside the notch of the L there is no point.
        assert!(
            out[0]
                .inner
                .iter()
                .all(|p| inside(&ring, [f64::from(p[0]), f64::from(p[1])]))
        );
    }

    #[test]
    fn two_islands_give_two_outlines_and_a_small_one_is_dropped() {
        let data = image(256, 256, |x, y| {
            (x - 60.0).hypot(y - 60.0) < 40.0
                || (x - 190.0).hypot(y - 190.0) < 40.0
                || (x - 200.0).hypot(y - 40.0) < 3.0
        });
        let a = alpha(256, 256, &data);
        let two = islands(&a, &ContourParams::default()).unwrap();
        assert_eq!(two.len(), 2);
        let all = islands(
            &a,
            &ContourParams {
                min_island: 0.0,
                ..ContourParams::default()
            },
        )
        .unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn a_ring_part_is_filled_and_an_island_in_its_hole_is_dropped() {
        let data = image(256, 256, |x, y| {
            let d = (x - 128.0).hypot(y - 128.0);
            (60.0..100.0).contains(&d) || d < 20.0
        });
        let out = islands(&alpha(256, 256, &data), &ContourParams::default()).unwrap();
        assert_eq!(out.len(), 1);
        assert!(area(&ring64(&out[0].outline)) > 0.9 * std::f64::consts::PI * 100.0 * 100.0);
    }

    #[test]
    fn a_thin_line_does_not_break_anything() {
        let data = image(256, 256, |x, y| {
            (x - 128.0).abs() < 0.6 && (20.0..230.0).contains(&y)
        });
        let a = alpha(256, 256, &data);
        match islands(&a, &ContourParams::default()) {
            Ok(out) => assert!(out.len() <= 1),
            Err(e) => assert_eq!(e, ContourError::Empty),
        }
        if let Ok(g) = contour_mesh(&quad(256.0, 256.0), &a, &ContourParams::default()) {
            assert!(g.indices.len() % 3 == 0);
        }
    }

    #[test]
    fn transparent_is_empty_and_opaque_is_the_whole_image() {
        let clear = vec![0u8; 64 * 64];
        assert_eq!(
            islands(&alpha(64, 64, &clear), &ContourParams::default()),
            Err(ContourError::Empty)
        );
        let full = vec![255u8; 64 * 64];
        let params = ContourParams {
            offset: 0.0,
            spacing: 0.0,
            ..ContourParams::default()
        };
        let out = islands(&alpha(64, 64, &full), &params).unwrap();
        assert_eq!(out.len(), 1);
        assert!(
            (4..=6).contains(&out[0].outline.len()),
            "{}",
            out[0].outline.len()
        );
        assert!((area(&ring64(&out[0].outline)) - 64.0 * 64.0).abs() < 82.0);
        assert!(out[0].inner.is_empty());
    }

    #[test]
    fn bad_input_is_an_error() {
        let data = vec![255u8; 10];
        assert_eq!(
            islands(&alpha(4, 4, &data), &ContourParams::default()),
            Err(ContourError::BadImage)
        );
        let full = vec![255u8; 16];
        for bad in [
            ContourParams {
                simplify: -1.0,
                ..ContourParams::default()
            },
            ContourParams {
                spacing: 2.0,
                ..ContourParams::default()
            },
            ContourParams {
                offset: f32::NAN,
                ..ContourParams::default()
            },
        ] {
            assert_eq!(
                islands(&alpha(4, 4, &full), &bad),
                Err(ContourError::BadParams)
            );
        }
    }

    #[test]
    fn the_result_is_the_same_every_time() {
        let data = l_shape();
        let a = alpha(256, 256, &data);
        let p = ContourParams::default();
        assert_eq!(islands(&a, &p), islands(&a, &p));
        let q = quad(100.0, 100.0);
        assert_eq!(contour_mesh(&q, &a, &p), contour_mesh(&q, &a, &p));
    }

    #[test]
    fn simplifying_stays_within_the_tolerance_and_zero_keeps_every_vertex() {
        let data = circle(256, 256);
        let raw = trace(&alpha(256, 256, &data), 16)
            .into_iter()
            .find(|r| area(r) > 0.0)
            .unwrap();
        assert!(raw.len() > 100);
        assert_eq!(simplify(&raw, 0.0).len(), raw.len());
        for tol in [0.5, 2.0, 6.0] {
            let thin = simplify(&raw, tol);
            assert!(thin.len() < raw.len());
            for p in &raw {
                assert!(ring_distance(&thin, *p) <= tol + 1e-9, "tol {tol}");
            }
        }
    }

    #[test]
    fn the_offset_grows_the_outline_and_gives_up_on_a_self_crossing_one() {
        let square: Vec<P> = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let out = offset_ring(&square, 2.0).unwrap();
        assert!(area(&out) > area(&square));
        assert!(!self_intersects(&out));
        // A thin spike: the full offset would cross itself, so it shrinks.
        let spike: Vec<P> = vec![[0.0, 0.0], [20.0, 0.5], [0.0, 1.0], [-5.0, 0.5]];
        let spike = if area(&spike) > 0.0 {
            spike
        } else {
            spike.into_iter().rev().collect()
        };
        let out = offset_ring(&spike, 4.0).unwrap();
        assert!(!self_intersects(&out));
        // A bow tie crosses itself already.
        let bow: Vec<P> = vec![[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
        assert_eq!(offset_ring(&bow, 1.0), None);
    }

    /// A random star-shaped (so simple) polygon, convex or not.
    fn star(seed: &mut u64, n: usize, jag: f64) -> Vec<Point> {
        let mut rand = || {
            *seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((*seed >> 40) as f64) / f64::from(1u32 << 24)
        };
        (0..n)
            .map(|i| {
                let a = (i as f64 + rand() * 0.6) / n as f64 * std::f64::consts::TAU;
                let r = 50.0 * (1.0 - jag * rand());
                [(100.0 + r * a.cos()) as f32, (100.0 + r * a.sin()) as f32]
            })
            .collect()
    }

    #[test]
    fn a_polygon_is_triangulated_with_itself_as_the_outline() {
        let mut seed = 5;
        for case in 0..30 {
            let n = 5 + case % 20;
            let jag = if case % 2 == 0 { 0.0 } else { 0.6 };
            let outline = star(&mut seed, n, jag);
            let ring = ring64(&outline);
            let inner: Vec<Point> = (0..40)
                .filter_map(|k| {
                    let p = [60.0 + (k % 7) as f32 * 13.7, 60.0 + (k / 7) as f32 * 13.1];
                    (inside(&ring, [f64::from(p[0]), f64::from(p[1])])
                        && ring_distance(&ring, [f64::from(p[0]), f64::from(p[1])]) > 4.0)
                        .then_some(p)
                })
                .collect();
            let (positions, indices) = triangulate_polygon(&outline, &inner).unwrap();
            let g = Geometry {
                uvs: vec![[0.0; 2]; positions.len()],
                positions,
                indices,
            };
            assert!(g.indices.iter().all(|&i| (i as usize) < g.positions.len()));
            let mut total = 0.0;
            let mut directed = HashSet::new();
            for t in g.indices.as_chunks::<3>().0 {
                let p = |i: u32| {
                    [
                        f64::from(g.positions[i as usize][0]),
                        f64::from(g.positions[i as usize][1]),
                    ]
                };
                let a = cross(p(t[0]), p(t[1]), p(t[2]));
                assert!(a > 1e-9, "case {case}: flipped or degenerate triangle");
                total += a / 2.0;
                for k in 0..3 {
                    assert!(directed.insert((t[k], t[(k + 1) % 3])));
                }
            }
            assert!(
                (total - area(&ring).abs()).abs() < 1e-3 * total,
                "case {case}"
            );
            // The outline edges are the only edges without a twin.
            let n = outline.len() as u32;
            let boundary: HashSet<(u32, u32)> = directed
                .iter()
                .filter(|&&(a, b)| !directed.contains(&(b, a)))
                .map(|&(a, b)| (a.min(b), a.max(b)))
                .collect();
            let want: HashSet<(u32, u32)> = (0..n)
                .map(|i| (i.min((i + 1) % n), i.max((i + 1) % n)))
                .collect();
            assert_eq!(boundary, want, "case {case}");
        }
    }

    #[test]
    fn the_contour_mesh_follows_the_shape_and_keeps_uvs_and_winding() {
        let data = l_shape();
        let a = alpha(256, 256, &data);
        for flipped in [false, true] {
            let mut original = quad(512.0, 512.0);
            if flipped {
                original.indices = vec![0, 2, 1, 0, 3, 2];
            }
            let out = contour_mesh(&original, &a, &ContourParams::default()).unwrap();
            assert!(out.positions.len() > 4);
            let sign = if flipped { -1.0 } else { 1.0 };
            let mut total = 0.0;
            for t in out.indices.as_chunks::<3>().0 {
                let p = |i: u32| {
                    [
                        f64::from(out.positions[i as usize][0]),
                        f64::from(out.positions[i as usize][1]),
                    ]
                };
                let a = cross(p(t[0]), p(t[1]), p(t[2]));
                assert!(a * sign > 1e-9);
                total += a.abs() / 2.0;
            }
            // The L covers 60x190 + 130x60 = 19200 px of 65536, scaled by 4.
            let want = 19200.0 * 4.0;
            assert!((total - want).abs() / want < 0.12, "{total}");
            // UVs follow the position-UV correspondence of the original quad.
            for (p, uv) in out.positions.iter().zip(&out.uvs) {
                assert!((f64::from(uv[0]) - f64::from(p[0]) / 512.0).abs() < 1e-4);
                assert!((f64::from(uv[1]) - f64::from(p[1]) / 512.0).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn two_islands_make_two_separate_pieces() {
        let data = image(256, 256, |x, y| {
            (x - 60.0).hypot(y - 60.0) < 40.0 || (x - 190.0).hypot(y - 190.0) < 40.0
        });
        let out = contour_mesh(
            &quad(256.0, 256.0),
            &alpha(256, 256, &data),
            &ContourParams::default(),
        )
        .unwrap();
        // No triangle joins the two: each has all its vertices on one side.
        for t in out.indices.as_chunks::<3>().0 {
            let left = |i: u32| out.positions[i as usize][0] < 128.0;
            assert!(left(t[0]) == left(t[1]) && left(t[1]) == left(t[2]));
        }
    }

    #[test]
    fn a_point_outside_the_uv_area_is_extrapolated_continuously() {
        let q = quad(100.0, 200.0);
        let map = UvMap::new(&q).unwrap();
        for uv in [[0.5, 0.5], [0.0, 0.0], [1.0, 1.0]] {
            let p = map.position(uv);
            assert!((p[0] - uv[0] * 100.0).abs() < 1e-9 && (p[1] - uv[1] * 200.0).abs() < 1e-9);
        }
        let out = map.position([1.1, -0.05]);
        assert!((out[0] - 110.0).abs() < 1e-9 && (out[1] + 10.0).abs() < 1e-9);
        assert!(UvMap::new(&Geometry::default()).is_err());
    }

    #[test]
    fn too_many_points_are_refused() {
        let data = vec![255u8; 512 * 512];
        let p = ContourParams {
            spacing: 4.0,
            ..ContourParams::default()
        };
        assert_eq!(
            islands(&alpha(512, 512, &data), &p),
            Err(ContourError::TooManyPoints)
        );
    }
}
