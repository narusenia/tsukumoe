// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Local triangulation edits for art meshes: add and remove vertices, draw an
//! edge, flip an edge.
//!
//! Every function takes a [`Geometry`] and returns a new one, or an
//! [`EditError`] and nothing else, so the caller can send the result through
//! `Command::SetMeshGeometry` as one undo step. Nothing re-triangulates the
//! whole mesh: edges the user drew stay. Triangles keep their winding.

use std::collections::{HashMap, VecDeque};
use std::fmt;

use crate::elements::Point;

/// Barycentric weights at or below this count as "on the edge".
const ON_EDGE: f64 = 1e-6;

/// Vertices, UVs and triangle indices of one mesh.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    pub positions: Vec<Point>,
    pub uvs: Vec<Point>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    /// A vertex index is out of range.
    BadVertex,
    /// The point is not inside any triangle.
    OutsideMesh,
    /// The point is on an existing vertex.
    TooClose,
    /// The two vertices are not joined by an edge.
    NoSuchEdge,
    /// The edge belongs to one triangle only.
    BoundaryEdge,
    /// The two triangles do not form a convex quadrilateral.
    NotConvex,
    /// The polygon around the vertex cannot be triangulated again.
    CannotRemove,
    /// The edge cannot be drawn by flipping the edges in its way.
    CannotConnect,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BadVertex => "vertex index out of range",
            Self::OutsideMesh => "point is outside the mesh",
            Self::TooClose => "point is on an existing vertex",
            Self::NoSuchEdge => "no such edge",
            Self::BoundaryEdge => "edge is on the outline",
            Self::NotConvex => "the two triangles are not a convex quadrilateral",
            Self::CannotRemove => "the polygon around the vertex cannot be re-triangulated",
            Self::CannotConnect => "the edge cannot be drawn",
        })
    }
}

impl std::error::Error for EditError {}

fn p64(g: &Geometry, i: u32) -> [f64; 2] {
    let p = g.positions[i as usize];
    [f64::from(p[0]), f64::from(p[1])]
}

/// Twice the signed area of `a b c`.
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn tri(g: &Geometry, t: usize) -> [u32; 3] {
    [g.indices[t * 3], g.indices[t * 3 + 1], g.indices[t * 3 + 2]]
}

fn set_tri(g: &mut Geometry, t: usize, v: [u32; 3]) {
    g.indices[t * 3..t * 3 + 3].copy_from_slice(&v);
}

/// The vertex opposite the directed edge `u -> v` in a triangle.
fn apex(t: [u32; 3], u: u32, v: u32) -> Option<u32> {
    (0..3)
        .find(|&k| t[k] == u && t[(k + 1) % 3] == v)
        .map(|k| t[(k + 2) % 3])
}

/// Directed edge to the triangle that has it.
fn edge_map(g: &Geometry) -> HashMap<(u32, u32), usize> {
    let mut map = HashMap::new();
    for t in 0..g.indices.len() / 3 {
        let v = tri(g, t);
        for k in 0..3 {
            map.insert((v[k], v[(k + 1) % 3]), t);
        }
    }
    map
}

fn check_vertices(g: &Geometry, ids: &[u32]) -> Result<(), EditError> {
    if ids.iter().any(|&i| i as usize >= g.positions.len()) {
        return Err(EditError::BadVertex);
    }
    Ok(())
}

fn lerp(a: Point, b: Point, t: f32) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// Adds a vertex at `at`: inside a triangle it splits the triangle in three,
/// on an edge it splits the triangles on both sides in two. The UV comes from
/// the barycentric weights.
pub fn add_vertex(g: &Geometry, at: Point) -> Result<Geometry, EditError> {
    let p = [f64::from(at[0]), f64::from(at[1])];
    let mut best: Option<(usize, [f64; 3], f64)> = None;
    for t in 0..g.indices.len() / 3 {
        let v = tri(g, t);
        let (a, b, c) = (p64(g, v[0]), p64(g, v[1]), p64(g, v[2]));
        let area = cross(a, b, c);
        if area == 0.0 {
            continue;
        }
        let w = [
            cross(p, b, c) / area,
            cross(a, p, c) / area,
            cross(a, b, p) / area,
        ];
        let least = w[0].min(w[1]).min(w[2]);
        if least >= -ON_EDGE && best.is_none_or(|(_, _, m)| least > m) {
            best = Some((t, w, least));
        }
    }
    let (t, w, _) = best.ok_or(EditError::OutsideMesh)?;
    let v = tri(g, t);
    let on_edge: Vec<usize> = (0..3).filter(|&k| w[k] <= ON_EDGE).collect();
    match on_edge.as_slice() {
        [] => {}
        &[k] => {
            let (u, x) = (v[(k + 1) % 3], v[(k + 2) % 3]);
            let (wu, wx) = (w[(k + 1) % 3], w[(k + 2) % 3]);
            return split_edge(g, u, x, (wx / (wu + wx)) as f32);
        }
        _ => return Err(EditError::TooClose),
    }
    let mut out = g.clone();
    let n = g.positions.len() as u32;
    let uv = |axis: usize| {
        (0..3)
            .map(|k| w[k] * f64::from(g.uvs[v[k] as usize][axis]))
            .sum::<f64>() as f32
    };
    out.positions.push(at);
    out.uvs.push([uv(0), uv(1)]);
    set_tri(&mut out, t, [v[0], v[1], n]);
    out.indices.extend([v[1], v[2], n, v[2], v[0], n]);
    Ok(out)
}

/// Adds a vertex on the edge `u`-`v`, `t` of the way from `u` to `v`, and
/// splits the one or two triangles that share the edge.
pub fn split_edge(g: &Geometry, u: u32, v: u32, t: f32) -> Result<Geometry, EditError> {
    check_vertices(g, &[u, v])?;
    if !(1e-4..=1.0 - 1e-4).contains(&t) {
        return Err(EditError::TooClose);
    }
    let map = edge_map(g);
    if !map.contains_key(&(u, v)) && !map.contains_key(&(v, u)) {
        return Err(EditError::NoSuchEdge);
    }
    let n = g.positions.len() as u32;
    let mut out = g.clone();
    out.positions
        .push(lerp(g.positions[u as usize], g.positions[v as usize], t));
    out.uvs.push(lerp(g.uvs[u as usize], g.uvs[v as usize], t));
    for (from, to) in [(u, v), (v, u)] {
        let Some(&ti) = map.get(&(from, to)) else {
            continue;
        };
        let w = apex(tri(g, ti), from, to).expect("edge map entry has the edge");
        set_tri(&mut out, ti, [from, n, w]);
        out.indices.extend([n, to, w]);
    }
    Ok(out)
}

/// Flips the diagonal `u`-`v` of the two triangles that share it. Only a
/// strictly convex quadrilateral can be flipped.
pub fn flip_edge(g: &Geometry, u: u32, v: u32) -> Result<Geometry, EditError> {
    check_vertices(g, &[u, v])?;
    let mut out = g.clone();
    flip(&mut out, u, v).map(|_| out)
}

/// Flips in place; returns the new diagonal.
fn flip(g: &mut Geometry, u: u32, v: u32) -> Result<(u32, u32), EditError> {
    let mut map = edge_map(g);
    flip_mapped(g, &mut map, u, v)
}

/// [`flip`] with an edge map the caller keeps up to date.
fn flip_mapped(
    g: &mut Geometry,
    map: &mut HashMap<(u32, u32), usize>,
    u: u32,
    v: u32,
) -> Result<(u32, u32), EditError> {
    let (u, v) = if map.contains_key(&(u, v)) {
        (u, v)
    } else if map.contains_key(&(v, u)) {
        (v, u)
    } else {
        return Err(EditError::NoSuchEdge);
    };
    let (Some(&t1), Some(&t2)) = (map.get(&(u, v)), map.get(&(v, u))) else {
        return Err(EditError::BoundaryEdge);
    };
    let c = apex(tri(g, t1), u, v).expect("edge map entry has the edge");
    let d = apex(tri(g, t2), v, u).expect("edge map entry has the edge");
    let (pu, pv, pc, pd) = (p64(g, u), p64(g, v), p64(g, c), p64(g, d));
    let straddles = |a, b, x, y| cross(a, b, x) * cross(a, b, y) < 0.0;
    if !(straddles(pu, pv, pc, pd) && straddles(pc, pd, pu, pv)) {
        return Err(EditError::NotConvex);
    }
    set_tri(g, t1, [u, d, c]);
    set_tri(g, t2, [d, v, c]);
    for key in [(u, v), (v, c), (c, u), (v, u), (u, d), (d, v)] {
        map.remove(&key);
    }
    for (key, t) in [
        ((u, d), t1),
        ((d, c), t1),
        ((c, u), t1),
        ((d, v), t2),
        ((v, c), t2),
        ((c, d), t2),
    ] {
        map.insert(key, t);
    }
    Ok((c, d))
}

/// Positive when `d` is inside the circle through `a b c` (given counter-clockwise),
/// with the size of the terms to judge "zero" against.
fn in_circle(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> (f64, f64) {
    let (ax, ay, bx, by, cx, cy) = (
        a[0] - d[0],
        a[1] - d[1],
        b[0] - d[0],
        b[1] - d[1],
        c[0] - d[0],
        c[1] - d[1],
    );
    let (al, bl, cl) = (ax * ax + ay * ay, bx * bx + by * by, cx * cx + cy * cy);
    let terms = [
        al * (bx * cy - cx * by),
        -bl * (ax * cy - cx * ay),
        cl * (ax * by - bx * ay),
    ];
    (terms.iter().sum(), terms.iter().map(|t| t.abs()).sum())
}

/// Re-triangulates the whole mesh as a constrained Delaunay triangulation:
/// edges are flipped until every edge that may flip has an empty circle. The
/// outline never changes (its edges cannot flip) and the edges in `keep`
/// stay, so hand-drawn edges survive when they are listed. Triangles keep
/// their winding. Vertices are not moved, added or removed.
pub fn retriangulate(g: &Geometry, keep: &[(u32, u32)]) -> Result<Geometry, EditError> {
    let ids: Vec<u32> = keep.iter().flat_map(|&(a, b)| [a, b]).collect();
    check_vertices(g, &ids)?;
    let keep: std::collections::HashSet<(u32, u32)> =
        keep.iter().map(|&(a, b)| (a.min(b), a.max(b))).collect();
    let mut out = g.clone();
    let mut map = edge_map(&out);
    let mut queue: VecDeque<(u32, u32)> = {
        let mut edges: Vec<_> = map
            .keys()
            .filter(|&&(u, v)| u < v && map.contains_key(&(v, u)) && !keep.contains(&(u, v)))
            .copied()
            .collect();
        edges.sort_unstable();
        edges.into()
    };
    let (mut flips, limit) = (0, 64 + 32 * (g.indices.len() / 3));
    while let Some((u, v)) = queue.pop_front() {
        let (Some(&t1), Some(&t2)) = (map.get(&(u, v)), map.get(&(v, u))) else {
            continue;
        };
        let c = apex(tri(&out, t1), u, v).expect("edge map entry has the edge");
        let d = apex(tri(&out, t2), v, u).expect("edge map entry has the edge");
        let (pu, pv, pc, pd) = (p64(&out, u), p64(&out, v), p64(&out, c), p64(&out, d));
        // Triangle `u v c` is counter-clockwise or clockwise as drawn.
        let (det, size) = if cross(pu, pv, pc) > 0.0 {
            in_circle(pu, pv, pc, pd)
        } else {
            in_circle(pv, pu, pc, pd)
        };
        if det <= 1e-10 * size || flip_mapped(&mut out, &mut map, u, v).is_err() {
            continue;
        }
        flips += 1;
        if flips >= limit {
            break;
        }
        // The four edges around the quadrilateral may now violate.
        for (x, y) in [(c, u), (u, d), (d, v), (v, c)] {
            let edge = (x.min(y), x.max(y));
            if !keep.contains(&edge) {
                queue.push_back(edge);
            }
        }
    }
    Ok(out)
}

/// Draws an edge between `a` and `b` by flipping the edges that cross it, as
/// long as it takes. Does nothing if the edge exists. Fails when an edge on
/// the outline or a vertex is in the way, or no flip can make progress.
pub fn connect(g: &Geometry, a: u32, b: u32) -> Result<Geometry, EditError> {
    check_vertices(g, &[a, b])?;
    if a == b {
        return Err(EditError::NoSuchEdge);
    }
    let has_edge = |g: &Geometry| {
        let map = edge_map(g);
        map.contains_key(&(a, b)) || map.contains_key(&(b, a))
    };
    let mut out = g.clone();
    if has_edge(&out) {
        return Ok(out);
    }
    let (pa, pb) = (p64(g, a), p64(g, b));
    let crosses = |g: &Geometry, x: u32, y: u32| {
        let (px, py) = (p64(g, x), p64(g, y));
        x != a
            && x != b
            && y != a
            && y != b
            && cross(pa, pb, px) * cross(pa, pb, py) < 0.0
            && cross(px, py, pa) * cross(px, py, pb) < 0.0
    };
    // A vertex exactly on the segment cannot be passed by flips.
    let blocked = g.indices.iter().any(|&w| {
        let pw = p64(g, w);
        w != a
            && w != b
            && cross(pa, pb, pw) == 0.0
            && (pw[0] - pa[0]) * (pw[0] - pb[0]) + (pw[1] - pa[1]) * (pw[1] - pb[1]) < 0.0
    });
    if blocked {
        return Err(EditError::CannotConnect);
    }
    let crossing = |g: &Geometry| {
        let mut edges: Vec<(u32, u32)> = (0..g.indices.len() / 3)
            .flat_map(|t| {
                let v = tri(g, t);
                (0..3).map(move |k| (v[k], v[(k + 1) % 3]))
            })
            .filter(|&(x, y)| crosses(g, x, y))
            .map(|(x, y)| (x.min(y), x.max(y)))
            .collect();
        edges.sort_unstable();
        edges.dedup();
        edges
    };
    let mut queue: VecDeque<(u32, u32)> = crossing(&out).into();
    let limit = 16 + 8 * g.indices.len();
    let (mut steps, mut stuck) = (0, 0);
    while let Some((u, v)) = queue.pop_front() {
        steps += 1;
        if steps > limit {
            return Err(EditError::CannotConnect);
        }
        match flip(&mut out, u, v) {
            Ok((c, d)) => {
                stuck = 0;
                if crosses(&out, c, d) {
                    queue.push_back((c.min(d), c.max(d)));
                }
            }
            Err(EditError::NotConvex) => {
                queue.push_back((u, v));
                stuck += 1;
                if stuck > queue.len() {
                    return Err(EditError::CannotConnect);
                }
            }
            Err(_) => return Err(EditError::CannotConnect),
        }
    }
    if has_edge(&out) {
        Ok(out)
    } else {
        Err(EditError::CannotConnect)
    }
}

/// Removes vertices (all or nothing). The polygon around each one is
/// triangulated again by ear clipping. A vertex on the outline can go only if
/// its corner is convex, so the outline never grows. Vertices left over in
/// no triangle are dropped as well.
pub fn remove_vertices(g: &Geometry, ids: &[u32]) -> Result<Geometry, EditError> {
    check_vertices(g, ids)?;
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut out = g.clone();
    for &id in &ids {
        remove_one(&mut out, id)?;
    }
    let mut remap = vec![u32::MAX; g.positions.len()];
    let mut next = 0;
    for (i, slot) in remap.iter_mut().enumerate() {
        if ids.binary_search(&(i as u32)).is_err() {
            *slot = next;
            next += 1;
        }
    }
    let keep = |i: usize| remap[i] != u32::MAX;
    out.positions = (0..g.positions.len())
        .filter(|&i| keep(i))
        .map(|i| g.positions[i])
        .collect();
    out.uvs = (0..g.uvs.len())
        .filter(|&i| keep(i))
        .map(|i| g.uvs[i])
        .collect();
    for i in &mut out.indices {
        *i = remap[*i as usize];
    }
    Ok(out)
}

/// Quality of the ear `p q r`: 4 sqrt(3) area over the sum of squared edge lengths.
fn ear_quality(p: [f64; 2], q: [f64; 2], r: [f64; 2]) -> f64 {
    let sq = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    2.0 * 3f64.sqrt() * cross(p, q, r).abs() / (sq(p, q) + sq(q, r) + sq(r, p))
}

/// Replaces the triangles around `v` (kept as an unused vertex) by an ear-clipped polygon.
fn remove_one(g: &mut Geometry, v: u32) -> Result<(), EditError> {
    let star: Vec<usize> = (0..g.indices.len() / 3)
        .filter(|&t| tri(g, t).contains(&v))
        .collect();
    if star.is_empty() {
        return Ok(());
    }
    // Link edges a -> b of the triangles `v a b`.
    let mut next: HashMap<u32, u32> = HashMap::new();
    let mut sign = 0.0;
    for &t in &star {
        let w = tri(g, t);
        let k = w.iter().position(|&x| x == v).expect("star triangle has v");
        let (a, b) = (w[(k + 1) % 3], w[(k + 2) % 3]);
        if next.insert(a, b).is_some() {
            return Err(EditError::CannotRemove);
        }
        if sign == 0.0 {
            sign = cross(p64(g, w[0]), p64(g, w[1]), p64(g, w[2])).signum();
        }
    }
    let heads: Vec<u32> = next
        .keys()
        .copied()
        .filter(|a| !next.values().any(|b| b == a))
        .collect();
    let closed = heads.is_empty();
    let start = match heads.as_slice() {
        [] => *next.keys().next().expect("star is not empty"),
        &[h] => h,
        _ => return Err(EditError::CannotRemove),
    };
    let mut poly = vec![start];
    while let Some(&n) = next.get(poly.last().expect("poly has start")) {
        if n == start {
            break;
        }
        poly.push(n);
        if poly.len() > next.len() + 1 {
            return Err(EditError::CannotRemove);
        }
    }
    if poly.len() != next.len() + usize::from(!closed) || poly.len() < 3 {
        return Err(EditError::CannotRemove);
    }
    if !closed {
        let (first, last) = (poly[0], poly[poly.len() - 1]);
        if cross(p64(g, last), p64(g, v), p64(g, first)) * sign < 0.0 {
            return Err(EditError::CannotRemove);
        }
    }
    let mut made = Vec::new();
    while poly.len() > 2 {
        let n = poly.len();
        let ear = (0..n)
            .filter(|&i| {
                let ids = [poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]];
                let [p, q, r] = ids.map(|x| p64(g, x));
                cross(p, q, r) * sign > 0.0
                    && !poly.iter().any(|&w| {
                        let pw = p64(g, w);
                        !ids.contains(&w)
                            && cross(p, q, pw) * sign >= 0.0
                            && cross(q, r, pw) * sign >= 0.0
                            && cross(r, p, pw) * sign >= 0.0
                    })
            })
            .max_by(|&i, &j| {
                let q = |i: usize| {
                    let [p, q, r] = [(i + n - 1) % n, i, (i + 1) % n].map(|k| p64(g, poly[k]));
                    ear_quality(p, q, r)
                };
                q(i).total_cmp(&q(j))
            })
            .ok_or(EditError::CannotRemove)?;
        made.extend([poly[(ear + n - 1) % n], poly[ear], poly[(ear + 1) % n]]);
        poly.remove(ear);
    }
    let mut keep = Vec::with_capacity(g.indices.len());
    for t in 0..g.indices.len() / 3 {
        if !star.contains(&t) {
            keep.extend(tri(g, t));
        }
    }
    keep.extend(made);
    g.indices = keep;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` x `h` grid of unit cells with jittered inner vertices; UVs equal the unjittered grid.
    fn grid(w: u32, h: u32, flip_winding: bool, seed: &mut u64) -> Geometry {
        let mut g = Geometry::default();
        for y in 0..=h {
            for x in 0..=w {
                let inner = x > 0 && x < w && y > 0 && y < h;
                let j = |s: &mut u64| if inner { rand(s) * 0.3 - 0.15 } else { 0.0 };
                g.positions.push([x as f32 + j(seed), y as f32 + j(seed)]);
                g.uvs.push([x as f32 / w as f32, y as f32 / h as f32]);
            }
        }
        let at = |x, y| y * (w + 1) + x;
        for y in 0..h {
            for x in 0..w {
                let (a, b, c, d) = (at(x, y), at(x + 1, y), at(x + 1, y + 1), at(x, y + 1));
                let quad = if (x + y) % 2 == 0 {
                    [a, b, c, a, c, d]
                } else {
                    [a, b, d, b, c, d]
                };
                g.indices.extend(quad);
            }
        }
        if flip_winding {
            for t in g.indices.chunks_mut(3) {
                t.swap(1, 2);
            }
        }
        g
    }

    fn rand(s: &mut u64) -> f32 {
        *s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*s >> 40) as f32) / (1u64 << 24) as f32
    }

    fn area(g: &Geometry) -> f64 {
        (0..g.indices.len() / 3)
            .map(|t| {
                let v = tri(g, t);
                cross(p64(g, v[0]), p64(g, v[1]), p64(g, v[2])).abs() / 2.0
            })
            .sum()
    }

    /// Directed edges with no opposite edge, and their total length.
    fn outline(g: &Geometry) -> (Vec<(u32, u32)>, f64) {
        let map = edge_map(g);
        let edges: Vec<_> = map
            .keys()
            .copied()
            .filter(|&(a, b)| !map.contains_key(&(b, a)))
            .collect();
        let len = edges
            .iter()
            .map(|&(a, b)| {
                let (pa, pb) = (p64(g, a), p64(g, b));
                (pa[0] - pb[0]).hypot(pa[1] - pb[1])
            })
            .sum();
        (edges, len)
    }

    /// Winding sign of every triangle must equal `sign`; no triangle is degenerate.
    fn assert_sound(g: &Geometry, sign: f64) {
        assert_eq!(g.positions.len(), g.uvs.len());
        assert_eq!(g.indices.len() % 3, 0);
        assert!(g.indices.iter().all(|&i| (i as usize) < g.positions.len()));
        let mut seen = std::collections::HashSet::new();
        for t in 0..g.indices.len() / 3 {
            let v = tri(g, t);
            let a = cross(p64(g, v[0]), p64(g, v[1]), p64(g, v[2]));
            assert!(a * sign > 1e-9, "triangle {t} is degenerate or flipped");
            for k in 0..3 {
                assert!(seen.insert((v[k], v[(k + 1) % 3])), "edge used twice");
            }
        }
    }

    fn sign_of(g: &Geometry) -> f64 {
        let v = tri(g, 0);
        cross(p64(g, v[0]), p64(g, v[1]), p64(g, v[2])).signum()
    }

    #[test]
    fn add_inside_splits_in_three_and_keeps_area_and_winding() {
        let mut seed = 1;
        for flipped in [false, true] {
            let g = grid(2, 2, flipped, &mut seed);
            let out = add_vertex(&g, [0.4, 0.3]).unwrap();
            assert_eq!(out.positions.len(), g.positions.len() + 1);
            assert_eq!(out.indices.len(), g.indices.len() + 6);
            assert!((area(&out) - area(&g)).abs() < 1e-5);
            assert_sound(&out, sign_of(&g));
            let uv = out.uvs.last().unwrap();
            assert!((uv[0] - 0.2).abs() < 0.05 && (uv[1] - 0.15).abs() < 0.05);
        }
    }

    #[test]
    fn add_on_an_edge_splits_both_sides() {
        let mut seed = 2;
        let g = grid(2, 2, false, &mut seed);
        // The diagonal (0,0)-(1,1) of the first cell is an inner edge.
        let out = add_vertex(&g, [0.5, 0.5]).unwrap();
        assert_eq!(out.indices.len(), g.indices.len() + 6);
        assert!((area(&out) - area(&g)).abs() < 1e-5);
        assert_sound(&out, 1.0);
        // On the outline only one triangle is split.
        let edge = add_vertex(&g, [0.5, 0.0]).unwrap();
        assert_eq!(edge.indices.len(), g.indices.len() + 3);
        assert_eq!(outline(&edge).1, outline(&g).1);
        assert_sound(&edge, 1.0);
    }

    #[test]
    fn add_rejects_outside_and_on_a_vertex() {
        let mut seed = 3;
        let g = grid(2, 2, false, &mut seed);
        assert_eq!(add_vertex(&g, [5.0, 5.0]), Err(EditError::OutsideMesh));
        assert_eq!(add_vertex(&g, [0.0, 0.0]), Err(EditError::TooClose));
    }

    #[test]
    fn remove_inner_vertex_keeps_area() {
        let mut seed = 4;
        let g = grid(3, 3, false, &mut seed);
        let v = 5;
        let out = remove_vertices(&g, &[v]).unwrap();
        assert_eq!(out.positions.len(), g.positions.len() - 1);
        assert!((area(&out) - area(&g)).abs() < 1e-5);
        assert_eq!(outline(&out).1, outline(&g).1);
        assert_sound(&out, 1.0);
    }

    #[test]
    fn remove_rejects_a_reflex_outline_corner_and_keeps_convex_ones() {
        // An L shape: the inner corner (1,1) is reflex.
        let g = Geometry {
            positions: vec![
                [0.0, 0.0],
                [2.0, 0.0],
                [2.0, 1.0],
                [1.0, 1.0],
                [1.0, 2.0],
                [0.0, 2.0],
                [0.0, 1.0],
            ],
            uvs: vec![[0.0, 0.0]; 7],
            indices: vec![0, 1, 3, 1, 2, 3, 0, 3, 6, 6, 3, 4, 6, 4, 5],
        };
        assert_sound(&g, 1.0);
        assert_eq!(remove_vertices(&g, &[3]), Err(EditError::CannotRemove));
        // Corner (2,1) is convex: its star is one triangle (1,2,3), too small to re-triangulate.
        assert_eq!(remove_vertices(&g, &[2]), Err(EditError::CannotRemove));
        // A convex corner with a bigger star goes and the outline only shrinks.
        let mut seed = 5;
        let sq = grid(2, 2, false, &mut seed);
        let out = remove_vertices(&sq, &[0]);
        match out {
            Ok(out) => {
                assert!(area(&out) <= area(&sq) + 1e-6);
                assert_sound(&out, 1.0);
            }
            Err(e) => assert_eq!(e, EditError::CannotRemove),
        }
    }

    #[test]
    fn flip_only_a_convex_quad() {
        let g = Geometry {
            positions: vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]],
            uvs: vec![[0.0, 0.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        let out = flip_edge(&g, 0, 2).unwrap();
        assert!((area(&out) - area(&g)).abs() < 1e-9);
        assert!(edge_map(&out).contains_key(&(3, 1)) || edge_map(&out).contains_key(&(1, 3)));
        assert_sound(&out, 1.0);
        assert_eq!(flip_edge(&g, 0, 1), Err(EditError::BoundaryEdge));
        assert_eq!(flip_edge(&g, 1, 3), Err(EditError::NoSuchEdge));
        // A dart (concave quadrilateral) cannot flip.
        let dart = Geometry {
            positions: vec![[0.0, 0.0], [4.0, 0.0], [1.0, 1.0], [0.0, 4.0]],
            uvs: vec![[0.0, 0.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        assert_eq!(flip_edge(&dart, 0, 2), Err(EditError::NotConvex));
    }

    #[test]
    fn connect_makes_the_edge_and_keeps_the_outline() {
        let mut seed = 6;
        let g = grid(4, 4, false, &mut seed);
        let at = |x: u32, y: u32| y * 5 + x;
        let out = connect(&g, at(0, 1), at(3, 2)).unwrap();
        let map = edge_map(&out);
        assert!(map.contains_key(&(at(0, 1), at(3, 2))) || map.contains_key(&(at(3, 2), at(0, 1))));
        assert!((area(&out) - area(&g)).abs() < 1e-5);
        assert_eq!(outline(&out).0.len(), outline(&g).0.len());
        assert_sound(&out, 1.0);
        // Existing edge: no change.
        let again = connect(&out, at(0, 1), at(3, 2)).unwrap();
        assert_eq!(again, out);
    }

    #[test]
    fn connect_fails_across_a_notch() {
        // The segment from (2,0.5) to (0.5,2) leaves the L shape.
        let g = Geometry {
            positions: vec![
                [0.0, 0.0],
                [2.0, 0.0],
                [2.0, 1.0],
                [1.0, 1.0],
                [1.0, 2.0],
                [0.0, 2.0],
                [0.0, 1.0],
            ],
            uvs: vec![[0.0, 0.0]; 7],
            indices: vec![0, 1, 3, 1, 2, 3, 0, 3, 6, 6, 3, 4, 6, 4, 5],
        };
        assert_eq!(connect(&g, 2, 4), Err(EditError::CannotConnect));
    }

    #[test]
    fn bad_vertex_indices_are_errors() {
        let mut seed = 7;
        let g = grid(1, 1, false, &mut seed);
        assert_eq!(connect(&g, 0, 99), Err(EditError::BadVertex));
        assert_eq!(flip_edge(&g, 99, 0), Err(EditError::BadVertex));
        assert_eq!(remove_vertices(&g, &[99]), Err(EditError::BadVertex));
        assert_eq!(split_edge(&g, 0, 99, 0.5), Err(EditError::BadVertex));
    }

    /// No vertex of `g` is strictly inside the circle of a triangle.
    fn assert_empty_circles(g: &Geometry) {
        for t in 0..g.indices.len() / 3 {
            let v = tri(g, t);
            let (a, b, c) = (p64(g, v[0]), p64(g, v[1]), p64(g, v[2]));
            let ccw = cross(a, b, c) > 0.0;
            for w in 0..g.positions.len() as u32 {
                if v.contains(&w) || !g.indices.contains(&w) {
                    continue;
                }
                let (det, size) = if ccw {
                    in_circle(a, b, c, p64(g, w))
                } else {
                    in_circle(b, a, c, p64(g, w))
                };
                assert!(
                    det <= 1e-9 * size.max(1e-12),
                    "vertex {w} is inside triangle {t}"
                );
            }
        }
    }

    fn sorted_triangles(g: &Geometry) -> Vec<[u32; 3]> {
        let mut all: Vec<[u32; 3]> = (0..g.indices.len() / 3)
            .map(|t| {
                let mut v = tri(g, t);
                let k = (0..3).min_by_key(|&k| v[k]).unwrap();
                v.rotate_left(k);
                v
            })
            .collect();
        all.sort_unstable();
        all
    }

    /// A scrambled triangulation of random points in a square.
    fn cloud(seed: &mut u64, points: usize, flipped: bool) -> Geometry {
        let mut g = Geometry {
            positions: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        if flipped {
            g.indices.swap(1, 2);
            g.indices.swap(4, 5);
        }
        while g.positions.len() < points + 4 {
            if let Ok(out) = add_vertex(&g, [rand(seed) * 10.0, rand(seed) * 10.0]) {
                g = out;
            }
        }
        for _ in 0..points * 4 {
            let t = (rand(seed) * (g.indices.len() / 3) as f32) as usize % (g.indices.len() / 3);
            let k = (rand(seed) * 3.0) as usize % 3;
            let v = tri(&g, t);
            if let Ok(out) = flip_edge(&g, v[k], v[(k + 1) % 3]) {
                g = out;
            }
        }
        g
    }

    #[test]
    fn retriangulating_random_clouds_gives_empty_circles_and_keeps_the_shape() {
        for (seed0, flipped) in [(21, false), (22, true), (23, false), (24, true)] {
            let mut seed = seed0;
            let g = cloud(&mut seed, 60, flipped);
            let out = retriangulate(&g, &[]).unwrap();
            assert_sound(&out, sign_of(&g));
            assert_eq!(out.positions, g.positions);
            assert!((area(&out) - area(&g)).abs() < 1e-3);
            let mut a = outline(&g).0;
            let mut b = outline(&out).0;
            a.sort_unstable();
            b.sort_unstable();
            assert_eq!(a, b);
            assert_empty_circles(&out);
            // Running it again changes nothing.
            let again = retriangulate(&out, &[]).unwrap();
            assert_eq!(sorted_triangles(&again), sorted_triangles(&out));
        }
    }

    #[test]
    fn retriangulating_keeps_the_listed_edges() {
        let mut seed = 31;
        let g = cloud(&mut seed, 40, false);
        let map = edge_map(&g);
        // Keep a few inner edges of the scrambled mesh.
        let keep: Vec<(u32, u32)> = map
            .keys()
            .filter(|&&(a, b)| a < b && map.contains_key(&(b, a)))
            .copied()
            .step_by(7)
            .collect();
        assert!(keep.len() > 3);
        let out = retriangulate(&g, &keep).unwrap();
        let after = edge_map(&out);
        for &(a, b) in &keep {
            assert!(
                after.contains_key(&(a, b)) || after.contains_key(&(b, a)),
                "{a}-{b}"
            );
        }
        assert_sound(&out, 1.0);
        assert!((area(&out) - area(&g)).abs() < 1e-3);
        // Every other inner edge has an empty circle on both sides.
        let keep: std::collections::HashSet<_> = keep.into_iter().collect();
        for (&(u, v), &t1) in &after {
            if u > v || keep.contains(&(u, v)) || !after.contains_key(&(v, u)) {
                continue;
            }
            let c = apex(tri(&out, t1), u, v).unwrap();
            let d = apex(tri(&out, after[&(v, u)]), v, u).unwrap();
            let (det, size) = in_circle(p64(&out, u), p64(&out, v), p64(&out, c), p64(&out, d));
            assert!(det <= 1e-9 * size.max(1e-12), "edge {u}-{v}");
        }
    }

    #[test]
    fn retriangulating_a_concave_outline_keeps_it() {
        let g = Geometry {
            positions: vec![
                [0.0, 0.0],
                [4.0, 0.0],
                [4.0, 1.0],
                [1.0, 1.0],
                [1.0, 4.0],
                [0.0, 4.0],
                [0.0, 1.0],
            ],
            uvs: vec![[0.0, 0.0]; 7],
            indices: vec![0, 1, 3, 1, 2, 3, 0, 3, 6, 6, 3, 4, 6, 4, 5],
        };
        let out = retriangulate(&g, &[]).unwrap();
        assert_sound(&out, 1.0);
        assert!((area(&out) - area(&g)).abs() < 1e-9);
        assert_eq!(outline(&out).1, outline(&g).1);
        assert_eq!(retriangulate(&g, &[(0, 99)]), Err(EditError::BadVertex));
    }

    #[test]
    fn random_edit_sequences_keep_the_invariants() {
        let mut successes = [0; 4];
        for (seed0, flipped) in [(11, false), (12, true), (13, false), (14, true)] {
            let mut seed = seed0;
            let mut g = grid(5, 4, flipped, &mut seed);
            let sign = sign_of(&g);
            let mut expected_area = area(&g);
            let mut expected_outline = outline(&g).1;
            for step in 0..300 {
                let n = g.positions.len() as u32;
                let pick = |s: &mut u64| (rand(s) * n as f32) as u32 % n;
                let result = match step % 4 {
                    0 => add_vertex(&g, [rand(&mut seed) * 5.0, rand(&mut seed) * 4.0]),
                    1 => {
                        let v = pick(&mut seed);
                        let boundary = outline(&g).0.iter().any(|&(a, b)| a == v || b == v);
                        let result = remove_vertices(&g, &[v]);
                        if boundary && let Ok(out) = &result {
                            // The outline may shrink at a convex corner.
                            assert!(area(out) <= expected_area + 1e-5);
                            expected_area = area(out);
                            expected_outline = outline(out).1;
                        }
                        result
                    }
                    2 => connect(&g, pick(&mut seed), pick(&mut seed)),
                    _ => {
                        let t = (rand(&mut seed) * (g.indices.len() / 3) as f32) as usize;
                        let k = (rand(&mut seed) * 3.0) as usize % 3;
                        let v = tri(&g, t % (g.indices.len() / 3));
                        flip_edge(&g, v[k], v[(k + 1) % 3])
                    }
                };
                if let Ok(out) = result {
                    successes[step % 4] += 1;
                    g = out;
                }
                assert_sound(&g, sign);
                assert!(
                    (area(&g) - expected_area).abs() < 1e-3,
                    "area changed at step {step}"
                );
                assert!(
                    (outline(&g).1 - expected_outline).abs() < 1e-3,
                    "outline changed at step {step}"
                );
            }
        }
        assert!(successes.iter().all(|&n| n >= 20), "{successes:?}");
    }
}
