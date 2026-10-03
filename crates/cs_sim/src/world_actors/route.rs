//! Gate-aware route plans: the declared polyline a route-following world
//! actor drives at one authored cruise speed, holding before closed gates.
//!
//! Spec: `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`,
//! stage `### F34-B`, non-negotiable behavior 3. A route is **not** a
//! [`super::trajectory::Trajectory`]: a trajectory is one function of the
//! tick and can never wait, while a convoy stopped at a closed gate must
//! hold for exactly as long as the gate stands and then continue. Route
//! progress is therefore state the [`super::runtime::WorldActorSet`] advances
//! one tick at a time, and a gate is an explicit [`RouteGate`] edge to the
//! actor whose [`super::graph::Presence`] controls the passage — never a
//! name or a radius the caller has to re-derive.
//!
//! All values are canonical SI units (meters, meters per second) and are
//! newly authored engine contract: how the original encodes convoy paths and
//! gate passages is unmeasured
//! (`docs/findings/2026-10-01-f34-a-world-actor-motion-and-dependency.md`).

use cs_script::ir::ActorId;

use super::math::{add, norm, sub};

/// One passage on a route that a destroyable actor controls.
///
/// While `gate` is [`super::graph::Presence::Intact`] and closed, the
/// passage is shut: a follower may advance only as far as
/// `at_m - stop_before_m`. Destroying the gate opens the passage
/// permanently — presence is monotonic (intact to destroyed), so a passage
/// never re-closes through destruction and a follower that legitimately
/// crossed is never pulled back. A scripted
/// [`super::runtime::WorldActorSet::set_gate_open`] transition may open and
/// re-close a passage without destroying the gate (F34-C); a close lands
/// behind a follower that already crossed, capping its further progress
/// without dragging it back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteGate {
    /// The actor whose presence controls this passage.
    pub gate: ActorId,
    /// Where the gate stands along the route, as arc length from the first
    /// point: `0` is the route start, `length_m` its end.
    pub at_m: f64,
    /// How far before `at_m` the follower must hold while the gate is
    /// closed. `0` holds it at the gate line itself.
    pub stop_before_m: f64,
}

impl RouteGate {
    /// The arc length a closed gate caps progress at.
    #[must_use]
    pub fn stop_line_m(&self) -> f64 {
        self.at_m - self.stop_before_m
    }
}

/// Why a [`RoutePlan`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteError {
    /// A route needs at least a start and an end point.
    TooFewPoints {
        /// How many points were offered.
        points: usize,
    },
    /// A waypoint was NaN or infinite.
    NonFinitePoint {
        /// Which waypoint.
        index: usize,
    },
    /// Two consecutive waypoints coincide, so the travel direction through
    /// them is undefined.
    DegenerateSegment {
        /// The first waypoint of the zero-length segment.
        index: usize,
    },
    /// The cruise speed was non-finite or not positive.
    InvalidSpeed {
        /// The rejected speed, in m/s.
        speed_m_s: f64,
    },
    /// A gate's `at_m` or `stop_before_m` was NaN or infinite.
    NonFiniteGate {
        /// Which gate.
        index: usize,
    },
    /// Gates are not strictly ascending in `at_m`: a follower could not tell
    /// which passage comes first.
    GatesNotAscending {
        /// The first out-of-order gate.
        index: usize,
    },
    /// A gate stands outside the route's arc length.
    GateBeyondRoute {
        /// The offending gate's actor.
        gate: ActorId,
        /// Its claimed position along the route.
        at_m: f64,
        /// The route's total length.
        length_m: f64,
    },
    /// A stop distance was negative or placed the stop line before the route
    /// start.
    InvalidStop {
        /// The offending gate's actor.
        gate: ActorId,
        /// The rejected distance.
        stop_before_m: f64,
        /// The gate's position along the route.
        at_m: f64,
    },
}

/// A validated ordered polyline with declared gate passages.
///
/// Construct only through [`RoutePlan::try_new`], which refuses degenerate
/// geometry and out-of-range gates once, so the per-tick follower never
/// re-validates.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutePlan {
    points: Vec<[f64; 3]>,
    /// Cumulative arc length at each waypoint, `cum_m[0] == 0`, ascending.
    cum_m: Vec<f64>,
    speed_m_s: f64,
    /// Sorted by `at_m`.
    gates: Vec<RouteGate>,
}

impl RoutePlan {
    /// Builds and validates a route.
    ///
    /// # Errors
    ///
    /// [`RouteError`] naming the first violated rule.
    pub fn try_new(
        points: Vec<[f64; 3]>,
        speed_m_s: f64,
        gates: Vec<RouteGate>,
    ) -> Result<Self, RouteError> {
        if !speed_m_s.is_finite() || speed_m_s <= 0.0 {
            return Err(RouteError::InvalidSpeed { speed_m_s });
        }
        if points.len() < 2 {
            return Err(RouteError::TooFewPoints {
                points: points.len(),
            });
        }
        for (index, p) in points.iter().enumerate() {
            if !p.iter().all(|v| v.is_finite()) {
                return Err(RouteError::NonFinitePoint { index });
            }
        }
        let mut cum_m = Vec::with_capacity(points.len());
        cum_m.push(0.0);
        for index in 0..points.len() - 1 {
            let span = norm(sub(points[index + 1], points[index]));
            if span <= 0.0 {
                return Err(RouteError::DegenerateSegment { index });
            }
            cum_m.push(cum_m[index] + span);
        }
        let length_m = cum_m[cum_m.len() - 1];
        for (index, g) in gates.iter().enumerate() {
            if !g.at_m.is_finite() || !g.stop_before_m.is_finite() {
                return Err(RouteError::NonFiniteGate { index });
            }
            if index > 0 && g.at_m <= gates[index - 1].at_m {
                return Err(RouteError::GatesNotAscending { index });
            }
            if g.at_m < 0.0 || g.at_m > length_m {
                return Err(RouteError::GateBeyondRoute {
                    gate: g.gate,
                    at_m: g.at_m,
                    length_m,
                });
            }
            if g.stop_before_m < 0.0 || g.stop_before_m > g.at_m {
                return Err(RouteError::InvalidStop {
                    gate: g.gate,
                    stop_before_m: g.stop_before_m,
                    at_m: g.at_m,
                });
            }
        }
        Ok(Self {
            points,
            cum_m,
            speed_m_s,
            gates,
        })
    }

    /// The route's total arc length, in meters.
    #[must_use]
    pub fn length_m(&self) -> f64 {
        self.cum_m[self.cum_m.len() - 1]
    }

    /// The authored cruise speed, in m/s.
    #[must_use]
    pub fn speed_m_s(&self) -> f64 {
        self.speed_m_s
    }

    /// The declared gate passages, ascending in `at_m`.
    #[must_use]
    pub fn gates(&self) -> &[RouteGate] {
        &self.gates
    }

    /// How many waypoints the polyline has.
    #[must_use]
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// The world position at arc length `progress_m`, clamped to the route.
    #[must_use]
    pub fn position_at(&self, progress_m: f64) -> [f64; 3] {
        let progress_m = progress_m.clamp(0.0, self.length_m());
        // First index whose cum exceeds progress: its segment contains it.
        let hi = self.cum_m.partition_point(|&c| c <= progress_m);
        if hi >= self.points.len() {
            return self.points[self.points.len() - 1];
        }
        let index = hi - 1;
        let span = self.cum_m[index + 1] - self.cum_m[index];
        let t = (progress_m - self.cum_m[index]) / span;
        add(
            self.points[index],
            sub(self.points[index + 1], self.points[index]).map(|d| d * t),
        )
    }

    /// The unit travel direction at arc length `progress_m`. Every segment
    /// is non-degenerate by construction, so this is always defined.
    #[must_use]
    pub fn direction_at(&self, progress_m: f64) -> [f64; 3] {
        let progress_m = progress_m.clamp(0.0, self.length_m());
        let hi = self.cum_m.partition_point(|&c| c <= progress_m);
        let index = hi.saturating_sub(1).min(self.points.len() - 2);
        let d = sub(self.points[index + 1], self.points[index]);
        let n = norm(d);
        d.map(|v| v / n)
    }
}
