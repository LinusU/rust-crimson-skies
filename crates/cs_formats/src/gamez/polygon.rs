//! Validated triangulation of polygon outlines with more than three corners.
//!
//! Non-negotiable #2 of `specs/F10-gamez-mesh-topology-and-material-
//! records.md`: n-gons need validated triangulation, not a triangle fan. A
//! fan from corner 0 draws triangles outside the outline as soon as the
//! polygon is concave, so [`triangulate_polygon`] clips ears instead and
//! first proves the outline can be triangulated at all:
//!
//! 1. No two corners share a location ([`NgonIssue::CoincidentCorners`]).
//! 2. The outline encloses area ([`NgonIssue::ZeroArea`]). Its normal is
//!    the Newell normal, and the outline is projected onto the coordinate
//!    plane that normal is most aligned with.
//! 3. The projected outline is simple: no two edges touch except adjacent
//!    edges at their shared corner ([`NgonIssue::SelfIntersecting`]).
//!
//! Every triangle then has the winding of the outline as stored and lies
//! inside it. Nothing is snapped, merged, reordered or guessed: an outline
//! that fails a check is reported with the reason and no triangles.
//!
//! Tests are exact (no tolerance): the values are the stored `f32`s widened
//! to `f64`. Whether the original renderer accepted any outline this module
//! rejects, and how it triangulated n-gons, is not established.

use std::fmt;

/// Why an n-gon outline was not triangulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NgonIssue {
    /// Two corners are at the same location.
    CoincidentCorners {
        /// Corner in stored order.
        first: usize,
        /// Later corner in stored order.
        second: usize,
    },
    /// The outline encloses no net area: its corners are collinear, or
    /// lobes of opposite winding cancel out, as in a symmetric bow tie.
    ZeroArea,
    /// Two edges cross or touch, or an edge folds back along the previous
    /// one. Edge `i` runs from corner `i` to corner `i + 1` (wrapping).
    SelfIntersecting {
        /// Edge in stored order.
        first_edge: usize,
        /// Later edge in stored order.
        second_edge: usize,
    },
    /// Ear clipping found no ear among the corners still to be clipped.
    /// Only reachable through rounding on an outline that passed the checks.
    NoEar {
        /// Corners left when clipping stopped.
        remaining: usize,
    },
}

impl NgonIssue {
    /// Stable machine-matchable identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::CoincidentCorners { .. } => "coincident_corners",
            Self::ZeroArea => "zero_area",
            Self::SelfIntersecting { .. } => "self_intersecting",
            Self::NoEar { .. } => "no_ear",
        }
    }
}

impl fmt::Display for NgonIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::CoincidentCorners { first, second } => {
                write!(f, "{code}: corners {first} and {second}")
            }
            Self::ZeroArea => write!(f, "{code}: the outline encloses no net area"),
            Self::SelfIntersecting {
                first_edge,
                second_edge,
            } => write!(f, "{code}: edges {first_edge} and {second_edge}"),
            Self::NoEar { remaining } => {
                write!(f, "{code}: {remaining} corners left unclipped")
            }
        }
    }
}

/// Triangulates the outline through `points` (corners in stored order).
///
/// Returns `points.len() - 2` triangles as corner indices in drawing order,
/// each with the winding of the stored outline. Outlines with fewer than
/// three corners are the caller's to reject; they yield no triangles here.
pub fn triangulate_polygon(points: &[[f32; 3]]) -> Result<Vec<[usize; 3]>, NgonIssue> {
    let n = points.len();
    if n < 3 {
        return Ok(Vec::new());
    }
    for first in 0..n {
        for second in first + 1..n {
            if points[first] == points[second] {
                return Err(NgonIssue::CoincidentCorners { first, second });
            }
        }
    }

    let flat = project(points)?;
    check_simple(&flat)?;

    // Every candidate ear is tested against the outline's own orientation,
    // so clockwise and counter-clockwise outlines keep their winding.
    let mut remaining: Vec<usize> = (0..n).collect();
    let mut triangles = Vec::with_capacity(n - 2);
    while remaining.len() > 3 {
        let len = remaining.len();
        let ear = (0..len).find(|&at| {
            let [a, b, c] = [
                remaining[(at + len - 1) % len],
                remaining[at],
                remaining[(at + 1) % len],
            ];
            cross(flat[a], flat[b], flat[c]) > 0.0
                && remaining
                    .iter()
                    .filter(|&&other| other != a && other != b && other != c)
                    .all(|&other| !in_closed_triangle(flat[other], flat[a], flat[b], flat[c]))
        });
        let Some(at) = ear else {
            return Err(NgonIssue::NoEar { remaining: len });
        };
        triangles.push([
            remaining[(at + len - 1) % len],
            remaining[at],
            remaining[(at + 1) % len],
        ]);
        remaining.remove(at);
    }
    let [a, b, c] = [remaining[0], remaining[1], remaining[2]];
    if cross(flat[a], flat[b], flat[c]) <= 0.0 {
        return Err(NgonIssue::NoEar { remaining: 3 });
    }
    triangles.push([a, b, c]);
    Ok(triangles)
}

/// Projects the outline onto the coordinate plane its Newell normal is most
/// aligned with, mirrored so the outline is counter-clockwise there.
fn project(points: &[[f32; 3]]) -> Result<Vec<[f64; 2]>, NgonIssue> {
    let widened: Vec<[f64; 3]> = points.iter().map(|p| p.map(f64::from)).collect();
    let mut normal = [0.0f64; 3];
    for (i, p) in widened.iter().enumerate() {
        let q = widened[(i + 1) % widened.len()];
        normal[0] += (p[1] - q[1]) * (p[2] + q[2]);
        normal[1] += (p[2] - q[2]) * (p[0] + q[0]);
        normal[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    let axis = (0..3)
        .max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs()))
        .expect("three axes");
    if normal[axis] == 0.0 || !normal[axis].is_finite() {
        return Err(NgonIssue::ZeroArea);
    }
    // (u, v) in cyclic order after the dropped axis keeps the sign of that
    // normal component; swapping u and v mirrors a clockwise outline.
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let (u, v) = if normal[axis] > 0.0 { (u, v) } else { (v, u) };
    Ok(widened.iter().map(|p| [p[u], p[v]]).collect())
}

fn check_simple(flat: &[[f64; 2]]) -> Result<(), NgonIssue> {
    let n = flat.len();
    let edge = |i: usize| (flat[i], flat[(i + 1) % n]);
    for first_edge in 0..n {
        // Adjacent edges share one corner; they only overlap when the second
        // folds back along the first.
        let (a, b) = edge(first_edge);
        let (_, c) = edge((first_edge + 1) % n);
        let back = (b[0] - a[0]) * (c[0] - b[0]) + (b[1] - a[1]) * (c[1] - b[1]);
        if cross(a, b, c) == 0.0 && back < 0.0 {
            let pair = [first_edge, (first_edge + 1) % n];
            return Err(NgonIssue::SelfIntersecting {
                first_edge: pair[0].min(pair[1]),
                second_edge: pair[0].max(pair[1]),
            });
        }
        for second_edge in first_edge + 2..n {
            if first_edge == 0 && second_edge == n - 1 {
                continue;
            }
            let (p, q) = edge(first_edge);
            let (r, s) = edge(second_edge);
            if segments_touch(p, q, r, s) {
                return Err(NgonIssue::SelfIntersecting {
                    first_edge,
                    second_edge,
                });
            }
        }
    }
    Ok(())
}

/// Twice the signed area of `(a, b, c)`: positive when counter-clockwise.
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// `p` is inside or on the boundary of the counter-clockwise `(a, b, c)`.
fn in_closed_triangle(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
}

/// Closed segments `pq` and `rs` share at least one point.
fn segments_touch(p: [f64; 2], q: [f64; 2], r: [f64; 2], s: [f64; 2]) -> bool {
    let d1 = cross(r, s, p);
    let d2 = cross(r, s, q);
    let d3 = cross(p, q, r);
    let d4 = cross(p, q, s);
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    (d1 == 0.0 && on_segment(r, s, p))
        || (d2 == 0.0 && on_segment(r, s, q))
        || (d3 == 0.0 && on_segment(p, q, r))
        || (d4 == 0.0 && on_segment(p, q, s))
}

/// `p`, known to be collinear with `ab`, lies within its bounding box.
fn on_segment(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> bool {
    p[0] >= a[0].min(b[0])
        && p[0] <= a[0].max(b[0])
        && p[1] >= a[1].min(b[1])
        && p[1] <= a[1].max(b[1])
}
