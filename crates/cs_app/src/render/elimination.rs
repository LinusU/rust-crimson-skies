//! The original's per-polygon back-face elimination edge cases, decided and
//! counted (task #1408, `F17-F.3`; evidence `OWNER-STATIC-2026-10-05`, see
//! `docs/findings/2026-10-18-f17-f-3-back-face-elimination-edge-cases.md`).
//!
//! The original eliminated an outline polygon on the normal
//! `N = (P1−P0)×(P2−P0)` of its first three corners and `d = N·P0` in camera
//! space (eye at the origin, so `d` is 2·area·distance). With `tol = 0.00005`:
//! `d < −tol` is a front face, `d > tol` is a back face (drawn only when the
//! polygon is two-sided), and `|d| ≤ tol` — edge-on or a zero normal — is
//! eliminated even for two-sided polygons.
//!
//! # The decision
//!
//! * **Nowhere in the GPU pipeline.** The GPU keeps `cull_mode` from
//!   [`crate::render::bevy_mesh::CullingClass`]; the view-dependent `|d| ≤ tol`
//!   band is not replicated, because F17 forbids culling from removing
//!   gameplay geometry and the band is a per-frame, per-polygon decision on
//!   a stored polygon the reader has already triangulated.
//! * **A CPU classification, reported never silent.** [`classify`] is the
//!   original's rule as a pure function and [`EliminationReport`] counts each
//!   outcome, so a caller that has camera-space corners can state how many
//!   polygons the original would not have drawn.
//! * **Zero first-three-corner normal** ([`Elimination::ZeroNormal`]) is the
//!   one view-independent case; it is counted, not removed.
//!
//! Unknown, and left so: whether a polygon eliminated by its first three
//! corners but with later corners off that plane is visible in our
//! triangulation, and the visual effect of the band at retail scale.

/// The effective tolerance: `SetBFETolerance 0.00005`, run by
/// `support\init.gw` on every world load (the engine default `0.005` is
/// overridden).
pub const BFE_TOLERANCE: f32 = 0.00005;

/// What the original's test did with one polygon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elimination {
    /// `d < −tol`: drawn as a front face.
    Front,
    /// `d > tol` on a two-sided polygon: drawn with the normal negated.
    BackDrawn,
    /// `d > tol` on a one-sided polygon: culled — the case the GPU's
    /// `Face::Back` cull reproduces.
    BackCulled,
    /// `|d| ≤ tol` with a non-zero normal (edge-on): eliminated even if
    /// two-sided. View-dependent; not replicated on the GPU.
    EdgeOn,
    /// The first-three-corner normal is exactly zero: never drawn, from any
    /// view.
    ZeroNormal,
}

impl Elimination {
    /// Whether the original did not draw the polygon at all.
    pub const fn eliminated(self) -> bool {
        matches!(self, Self::BackCulled | Self::EdgeOn | Self::ZeroNormal)
    }
}

/// Classify one polygon by its first three camera-space corners.
///
/// `corners` are in stored order; fewer than three corners cannot form a
/// normal and are [`Elimination::ZeroNormal`].
pub fn classify(corners: &[[f32; 3]], two_sided: bool, tolerance: f32) -> Elimination {
    let [p0, p1, p2] = match corners {
        [a, b, c, ..] => [*a, *b, *c],
        _ => return Elimination::ZeroNormal,
    };
    let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let v = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    if n == [0.0; 3] {
        return Elimination::ZeroNormal;
    }
    let d = n[0] * p0[0] + n[1] * p0[1] + n[2] * p0[2];
    if d < -tolerance {
        Elimination::Front
    } else if d > tolerance {
        if two_sided {
            Elimination::BackDrawn
        } else {
            Elimination::BackCulled
        }
    } else {
        Elimination::EdgeOn
    }
}

/// Counts of each outcome over a set of polygons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EliminationReport {
    pub front: usize,
    pub back_drawn: usize,
    pub back_culled: usize,
    pub edge_on: usize,
    pub zero_normal: usize,
}

impl EliminationReport {
    /// Count one outcome.
    pub fn record(&mut self, outcome: Elimination) {
        match outcome {
            Elimination::Front => self.front += 1,
            Elimination::BackDrawn => self.back_drawn += 1,
            Elimination::BackCulled => self.back_culled += 1,
            Elimination::EdgeOn => self.edge_on += 1,
            Elimination::ZeroNormal => self.zero_normal += 1,
        }
    }

    /// Polygons the original would not have drawn.
    pub const fn eliminated(&self) -> usize {
        self.back_culled + self.edge_on + self.zero_normal
    }

    /// Polygons eliminated by an edge case the GPU cull does not reproduce
    /// (edge-on band and zero normal), reported rather than silent.
    pub const fn unreplicated(&self) -> usize {
        self.edge_on + self.zero_normal
    }
}
