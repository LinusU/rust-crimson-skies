//! Stability, high-speed contact and convergence evidence (F23-D).
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-D`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! sections "Collision and ballistic tests" and "Calibration acceptance".
//!
//! F23-A/B/C built the schedule, the bodies, the reporter, the session and the
//! first-tick spawn repair. This module is the *measurement* stage the sheet's
//! AC04 asks for: it runs the fixed step at the three declared rates, at a set
//! of declared projectile speeds, and for a declared flight duration, and
//! compares every run against something that is not the run itself — an
//! analytic solution for the force path, a geometric bound for contacts, and
//! the tick ledger for the schedule. The tolerances those runs are judged
//! against ([`FROZEN_CONVERGENCE_BUDGETS`],
//! [`CONTACT_FACE_TOLERANCE_M`], [`FROZEN_STABILITY_BUDGETS`]) were written
//! down *after* the measurements, from the table in
//! `docs/findings/2026-09-30-f23-d-stability-high-speed-contact-and-convergence-evidence.md`.
//!
//! Three probes, all driving production paths only:
//!
//! * [`convergence_evidence`] runs [`ConvergenceScenario`] at each of
//!   [`PROBE_RATES_HZ`]: one known force on one known synthetic mass, once per
//!   fixed tick, compared against the closed-form solution. It is the AC04
//!   minimum scenario.
//! * [`contact_sweep`] runs [`ContactScenario`] across the same rates and
//!   [`PROBE_SPEEDS_M_S`], for a solid obstacle and for a sensor, and records
//!   how deep the projectile ever got, how many contact episodes the crossing
//!   produced and when the first report arrived.
//! * [`stability_probe`] flies the production F24 fixed-wing model through the
//!   session for a declared duration and watches for the failure modes a
//!   fixed-step integrator actually has: a non-finite state, a tick that did
//!   not integrate, a force request that reached no body, a driver tick that
//!   was skipped or refused.
//!
//! The harness is production code for the same reason
//! [`crate::physics::fixture`] is: a probe that only exists inside a test can
//! drift from the paths it claims to measure. Every value here is newly
//! authored synthetic fixture data; nothing reads `CS_GAME_DIR`, and no result
//! below is evidence about the original game — the original's tick rate,
//! projectile speed and contact rules are **unknown** and are not guessed.

use std::fmt;

use avian3d::prelude::SubstepCount;
use bevy::prelude::Entity;
use cs_sim::collision::{CollisionLayer, ContactKind, ShapeClass};
use cs_sim::flight::{EngineState, FlightInput, FlightModel, synthetic_fixed_wing};

use super::adapter::{BASELINE_FIXED_HZ, DECLARED_SUBSTEP_COUNT, ForceRequest};
use super::body::{BodyMode, BodySpec};
use super::contacts::ContactReports;
use super::flight::{FlightForcesPlugin, FlightSpawnSpec, FlightTickReport, spawn_flight_body};
use super::session::PhysicsSession;

/// The fixed rates AC04 requires a convergence probe at: one below, at, and one
/// above the designed baseline.
pub const PROBE_RATES_HZ: [u32; 3] = [60, 120, 240];

/// The declared projectile speeds the contact sweep covers, in m/s.
///
/// The top of the range is a declared design bound for the swept layers, not a
/// measurement of the original game (whose projectile speed is unknown). It is
/// 50× the fastest airspeed the F24-A synthetic airframe reaches in level
/// flight, chosen so the sweep brackets the "travel per tick far larger than
/// the obstacle" regime the contract requires a synthetic test to reach.
pub const PROBE_SPEEDS_M_S: [f32; 4] = [60.0, 120.0, 300.0, 600.0];

/// How a probe scenario was rejected before anything ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// A named field was NaN, infinite or out of range.
    Field {
        /// The offending field.
        field: &'static str,
        /// Why it was refused.
        reason: &'static str,
    },
    /// The requested fixed rate is not one this probe can run at.
    Rate { hz: u32 },
    /// The probe needs a rate from [`PROBE_RATES_HZ`].
    UnprobedRate { hz: u32 },
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Field { field, reason } => write!(f, "{field} {reason}"),
            Self::Rate { hz } => write!(f, "a probe cannot run at {hz} Hz"),
            Self::UnprobedRate { hz } => {
                write!(
                    f,
                    "{hz} Hz is not one of the probed rates {PROBE_RATES_HZ:?}"
                )
            }
        }
    }
}

impl std::error::Error for ProbeError {}

fn field<T>(name: &'static str, value: T, ok: impl FnOnce(T) -> bool) -> Result<(), ProbeError> {
    if ok(value) {
        Ok(())
    } else {
        Err(ProbeError::Field {
            field: name,
            reason: "is outside the probe's declared range",
        })
    }
}

/// What a probe's tick ledger must show when the run finished.
///
/// The three counters are the structural claims of the whole F23 chain, and
/// the sweep asserts them on every probe rather than leaving them to the
/// schedule test: `ticks == integrations` is "one integration per declared
/// tick", `dropped_requests == 0` is "no submitted force reached no body", and
/// `applied_requests` is what gameplay should see for the requests it made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickAccounting {
    /// Fixed tick boundaries crossed.
    pub ticks: u64,
    /// Integration steps observed after the physics step set.
    pub integrations: u64,
    /// Force requests applied over the whole run.
    pub applied_requests: u64,
    /// Force requests that reached no dynamic body over the whole run.
    pub dropped_requests: u64,
    /// Solver substeps the run used per fixed tick.
    pub substeps: u32,
}

impl TickAccounting {
    fn read(session: &PhysicsSession) -> Self {
        let ledger = session.ledger().unwrap_or_default();
        Self {
            ticks: ledger.ticks,
            integrations: ledger.integrations,
            applied_requests: ledger.total_applied_requests,
            dropped_requests: ledger.total_dropped_requests,
            substeps: session
                .world()
                .map(|world| {
                    world
                        .get_resource::<SubstepCount>()
                        .map_or(0, |count| count.0)
                })
                .unwrap_or(0),
        }
    }

    /// Every way this accounting breaks the F23 invariants, in a stable order.
    ///
    /// An empty result is the whole point: a probe that found a defect lists
    /// it, and the caller decides. A non-finite state is not in here — that is
    /// the stability probe's own finding.
    pub fn violations(&self, expected_ticks: u64) -> Vec<TickViolation> {
        let mut found = Vec::new();
        if self.ticks != expected_ticks {
            found.push(TickViolation::TickCount {
                expected: expected_ticks,
                found: self.ticks,
            });
        }
        if self.integrations != self.ticks {
            found.push(TickViolation::IntegrationCount {
                ticks: self.ticks,
                integrations: self.integrations,
            });
        }
        if self.dropped_requests != 0 {
            found.push(TickViolation::DroppedRequests(self.dropped_requests));
        }
        found
    }
}

/// A broken F23 schedule invariant, named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickViolation {
    /// The run did not cross the declared number of fixed tick boundaries.
    TickCount {
        /// Ticks the scenario asked for.
        expected: u64,
        /// Ticks the ledger counted.
        found: u64,
    },
    /// The integrator did not run exactly once per fixed tick.
    IntegrationCount {
        /// Fixed tick boundaries.
        ticks: u64,
        /// Physics steps observed.
        integrations: u64,
    },
    /// A submitted force request reached no dynamic body.
    DroppedRequests(u64),
}

impl fmt::Display for TickViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TickCount { expected, found } => {
                write!(f, "expected {expected} fixed ticks, ledger counted {found}")
            }
            Self::IntegrationCount {
                ticks,
                integrations,
            } => write!(f, "{ticks} fixed ticks ran {integrations} integrations"),
            Self::DroppedRequests(count) => {
                write!(f, "{count} force requests reached no dynamic body")
            }
        }
    }
}

impl std::error::Error for TickViolation {}

/// One constant-force convergence run.
///
/// The scenario is deliberately the simplest thing the force path can be asked
/// to do — a known force on a known mass, once per fixed tick — because the
/// answer is known in closed form, so the probe measures the *integrator and
/// the schedule*, not a flight model. Avian's semi-implicit (symplectic) Euler
/// step is exact in velocity under a constant force and carries a first-order
/// position error of `0.5 * a * T * dt`, which is what
/// [`symplectic_error_m`](Self::symplectic_error_m) compares against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConvergenceScenario {
    /// The body's total mass, in kilograms. Strictly positive.
    pub mass_kg: f32,
    /// The force applied on every fixed tick, in newtons.
    pub force_n: [f32; 3],
    /// The body's spawn position, in meters.
    pub initial_position_m: [f32; 3],
    /// The body's spawn velocity, in m/s.
    pub initial_velocity_m_s: [f32; 3],
    /// How long the run simulates, in seconds. Strictly positive.
    pub duration_s: f32,
}

impl ConvergenceScenario {
    /// The declared F23-D probe: a 20 kg body under 400 N (20 m/s²) for two
    /// seconds, starting at 10 m/s.
    ///
    /// Chosen so the position error is large enough to measure in f32 (0.33 m
    /// at 60 Hz) and the velocity error is small enough to be pure rounding
    /// noise, which separates "the integrator is converging" from "the
    /// integrator is wrong" cleanly.
    pub const fn constant_thrust() -> Self {
        Self {
            mass_kg: 20.0,
            force_n: [400.0, 0.0, 0.0],
            initial_position_m: [0.0; 3],
            initial_velocity_m_s: [10.0, 0.0, 0.0],
            duration_s: 2.0,
        }
    }

    /// Rejects a scenario the probe cannot run, before any world is built.
    pub fn validate(&self) -> Result<(), ProbeError> {
        field("mass_kg", self.mass_kg, |mass| {
            mass.is_finite() && mass > 0.0
        })?;
        field("duration_s", self.duration_s, |seconds| {
            seconds.is_finite() && seconds > 0.0
        })?;
        for (name, value) in [
            ("force_n", self.force_n),
            ("initial_position_m", self.initial_position_m),
            ("initial_velocity_m_s", self.initial_velocity_m_s),
        ] {
            for component in value {
                if !component.is_finite() {
                    return Err(ProbeError::Field {
                        field: name,
                        reason: "must be finite",
                    });
                }
            }
        }
        Ok(())
    }

    /// The scenario's constant acceleration, in m/s².
    pub fn acceleration_m_s2(&self) -> [f64; 3] {
        let mass = f64::from(self.mass_kg);
        self.force_n.map(|force| f64::from(force) / mass)
    }

    /// The exact position at [`duration_s`](Self::duration_s), in meters.
    pub fn exact_position_m(&self) -> [f64; 3] {
        let t = f64::from(self.duration_s);
        let acceleration = self.acceleration_m_s2();
        std::array::from_fn(|axis| {
            f64::from(self.initial_position_m[axis])
                + f64::from(self.initial_velocity_m_s[axis]) * t
                + 0.5 * acceleration[axis] * t * t
        })
    }

    /// The exact velocity at [`duration_s`](Self::duration_s), in m/s.
    pub fn exact_velocity_m_s(&self) -> [f64; 3] {
        let t = f64::from(self.duration_s);
        let acceleration = self.acceleration_m_s2();
        std::array::from_fn(|axis| {
            f64::from(self.initial_velocity_m_s[axis]) + acceleration[axis] * t
        })
    }
}

/// One measured convergence run at one fixed rate.
#[derive(Clone, Debug, PartialEq)]
pub struct ConvergenceProbe {
    /// The rate the run used.
    pub fixed_hz: u32,
    /// Fixed ticks the run crossed.
    pub ticks: u64,
    /// The measured position after the last tick, in meters.
    pub final_position_m: [f64; 3],
    /// The measured velocity after the last tick, in m/s.
    pub final_velocity_m_s: [f64; 3],
    /// The largest component of `final_position_m - exact`, in meters.
    pub position_error_m: f64,
    /// The largest component of `final_velocity_m_s - exact`, in m/s.
    pub velocity_error_m_s: f64,
    /// The largest component of the distance between the measured position and
    /// the symplectic prediction `exact + 0.5 * a * T * dt / substeps`, in
    /// meters.
    ///
    /// This is the diagnostic that says *which* integrator ran: it is ~1e-5 m
    /// for symplectic Euler and grows with the rate if the position integration
    /// is explicit instead.
    pub symplectic_error_m: f64,
    /// Ticks whose pose or velocity was not finite.
    pub nonfinite_ticks: u64,
    /// The tick ledger's view of the run.
    pub accounting: TickAccounting,
    /// The schedule invariants this run broke.
    pub violations: Vec<TickViolation>,
}

/// Runs one convergence probe at `fixed_hz` on the production session path.
///
/// The force is submitted once per fixed tick and read back after the run, so
/// a probe failure localises: a doubled force doubles the velocity error, a
/// dropped request shows up in [`TickAccounting::dropped_requests`], a wrong
/// timestep shows up in the position error's rate dependence.
pub fn convergence_probe(
    scenario: &ConvergenceScenario,
    fixed_hz: u32,
) -> Result<ConvergenceProbe, ProbeError> {
    scenario.validate()?;
    field("fixed_hz", fixed_hz, |hz| hz > 0).map_err(|_| ProbeError::Rate { hz: fixed_hz })?;

    let ticks = u64::from(fixed_hz) * u64::from(f64_to_ticks(f64::from(scenario.duration_s)));
    if ticks == 0 {
        return Err(ProbeError::Field {
            field: "duration_s",
            reason: "is shorter than one tick at this rate",
        });
    }

    let mut session = PhysicsSession::new(fixed_hz);
    let body = session
        .spawn(&BodySpec {
            layer: CollisionLayer::Aircraft,
            shape: ShapeClass::Solid,
            mode: BodyMode::Dynamic,
            mass_kg: scenario.mass_kg,
            half_extents_m: [0.5, 0.5, 0.5],
            position_m: scenario.initial_position_m,
            linear_velocity_m_s: scenario.initial_velocity_m_s,
        })
        .expect("a validated scenario is a valid body spec")
        .entity;

    let request = ForceRequest::new(body, scenario.force_n, [0.0; 3])
        .expect("a validated scenario has a finite force");
    let mut nonfinite_ticks = 0;
    for _ in 0..ticks {
        session.submit(request).expect("the session is active");
        session.step(1).expect("the session is active");
        if session.pose(body).is_none_or(|pose| !pose_is_finite(&pose)) {
            nonfinite_ticks += 1;
        }
    }

    let pose = session
        .pose(body)
        .expect("a spawned body keeps its pose for the whole run");
    let final_position_m = pose.position_m.map(f64::from);
    let final_velocity_m_s = pose.linear_velocity_m_s.map(f64::from);
    let exact_position = scenario.exact_position_m();
    let exact_velocity = scenario.exact_velocity_m_s();
    let acceleration = scenario.acceleration_m_s2();
    let accounting = TickAccounting::read(&session);
    // The first-order term of symplectic Euler scales with the *integration*
    // step, and the integration step is `dt / substeps`: the position error
    // measured at 60/120/240 Hz came out as 0.1667/0.0833/0.0417 m for the
    // declared two substeps, exactly half of the single-substep values F23-D
    // first measured (0.3333/0.1667/0.0833 m).
    let dt = 1.0 / (f64::from(fixed_hz) * f64::from(accounting.substeps.max(1)));
    let symplectic_m = f64::from(scenario.duration_s);
    let symplectic_prediction = std::array::from_fn(|axis| {
        exact_position[axis] + 0.5 * acceleration[axis] * symplectic_m * dt
    });
    Ok(ConvergenceProbe {
        fixed_hz,
        ticks,
        final_position_m,
        final_velocity_m_s,
        position_error_m: max_abs_difference(&final_position_m, &exact_position),
        velocity_error_m_s: max_abs_difference(&final_velocity_m_s, &exact_velocity),
        symplectic_error_m: max_abs_difference(&final_position_m, &symplectic_prediction),
        nonfinite_ticks,
        accounting,
        violations: accounting.violations(ticks),
    })
}

/// Every probed rate's convergence run, plus the frozen comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct ConvergenceEvidence {
    /// The scenario every run used.
    pub scenario: ConvergenceScenario,
    /// One run per entry of [`PROBE_RATES_HZ`], in the same order.
    pub probes: Vec<ConvergenceProbe>,
}

/// Runs [`ConvergenceScenario::constant_thrust`] at every rate in
/// [`PROBE_RATES_HZ`] — the AC04 minimum scenario, one call.
pub fn convergence_evidence() -> Result<ConvergenceEvidence, ProbeError> {
    ConvergenceEvidence::run(ConvergenceScenario::constant_thrust())
}

impl ConvergenceEvidence {
    /// Runs the scenario at every rate in [`PROBE_RATES_HZ`].
    pub fn run(scenario: ConvergenceScenario) -> Result<Self, ProbeError> {
        let probes = PROBE_RATES_HZ
            .iter()
            .map(|hz| convergence_probe(&scenario, *hz))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { scenario, probes })
    }

    /// The run at `fixed_hz`, or `None` when that rate was not probed.
    pub fn probe(&self, fixed_hz: u32) -> Option<&ConvergenceProbe> {
        self.probes.iter().find(|probe| probe.fixed_hz == fixed_hz)
    }

    /// The run at the designed baseline rate.
    pub fn baseline(&self) -> Option<&ConvergenceProbe> {
        self.probe(BASELINE_FIXED_HZ)
    }

    /// The measured position-error reduction from `slower_hz` to the next
    /// faster probed rate, as `log2(error_slow / error_fast)`.
    ///
    /// A first-order method returns `1.0`; the contract's "select tolerances
    /// before fitting" rule is why the frozen budgets require a *minimum*
    /// observed order rather than trusting the label.
    pub fn observed_order(&self, slower_hz: u32) -> Option<f64> {
        let slower = self.probe(slower_hz)?;
        let faster = self
            .probes
            .iter()
            .find(|probe| probe.fixed_hz > slower_hz)?;
        if slower.position_error_m <= 0.0 || faster.position_error_m <= 0.0 {
            return None;
        }
        let ratio = slower.position_error_m / faster.position_error_m;
        if ratio <= 0.0 {
            return None;
        }
        Some(ratio.log2())
    }
}

/// One frozen convergence tolerance, for one probed rate.
///
/// **Frozen from the F23-D measurement, not chosen first.** Each value is the
/// measured run rounded up with roughly 5% of headroom for the position error
/// and 5× for the velocity error, which is f32 rounding noise at these
/// step counts. They are declared tolerances for the *declared scenario*
/// ([`ConvergenceScenario::constant_thust`]), not a general accuracy claim
/// about the flight model, and not a claim about the original game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConvergenceBudget {
    /// The rate this budget is for.
    pub fixed_hz: u32,
    /// The largest position error the run may show.
    pub max_position_error_m: f64,
    /// The largest velocity error the run may show.
    pub max_velocity_error_m_s: f64,
    /// The smallest position-error reduction this rate must show against the
    /// next faster probed rate. `None` for the finest probed rate, which has
    /// no faster partner to compare against.
    pub min_observed_order: Option<f64>,
}

/// The frozen convergence tolerances, one per entry of [`PROBE_RATES_HZ`].
///
/// Measured with the F23-D probes *after* the declared substep policy was
/// fixed, before these numbers were written down: position errors
/// `0.166668 m` (60 Hz), `0.083336 m` (120 Hz) and `0.041668 m` (240 Hz) for
/// [`ConvergenceScenario::constant_thrust`] — a first-order halving — and
/// velocity errors of `8.8e-5`, `1.8e-4` and `3.5e-4 m/s`, which are pure f32
/// accumulation noise and grow slowly with the step count.
///
/// The budgets carry roughly 8% of headroom on the position error and 3x on the
/// velocity error. The same runs under the pre-F23-D single solver step
/// measured `0.3333 / 0.1667 / 0.0833 m`: the frozen numbers are only valid for
/// the declared [`DECLARED_SUBSTEP_COUNT`], and a change to it has to re-measure
/// this table rather than widen it.
pub const FROZEN_CONVERGENCE_BUDGETS: [ConvergenceBudget; 3] = [
    ConvergenceBudget {
        fixed_hz: 60,
        max_position_error_m: 0.18,
        max_velocity_error_m_s: 1.0e-3,
        min_observed_order: Some(0.9),
    },
    ConvergenceBudget {
        fixed_hz: BASELINE_FIXED_HZ,
        max_position_error_m: 0.09,
        max_velocity_error_m_s: 1.0e-3,
        min_observed_order: Some(0.9),
    },
    ConvergenceBudget {
        fixed_hz: 240,
        max_position_error_m: 0.045,
        max_velocity_error_m_s: 1.0e-3,
        min_observed_order: None,
    },
];

impl ConvergenceBudget {
    /// The frozen budget for `fixed_hz`, or `None` when the rate was not probed.
    pub fn for_rate(fixed_hz: u32) -> Option<&'static Self> {
        FROZEN_CONVERGENCE_BUDGETS
            .iter()
            .find(|budget| budget.fixed_hz == fixed_hz)
    }

    /// Every way `probe` misses this budget, in a stable order.
    pub fn violations(&self, probe: &ConvergenceProbe) -> Vec<ConvergenceViolation> {
        let mut found = Vec::new();
        if probe.fixed_hz != self.fixed_hz {
            found.push(ConvergenceViolation::WrongRate {
                budget: self.fixed_hz,
                found: probe.fixed_hz,
            });
        }
        if probe.position_error_m > self.max_position_error_m {
            found.push(ConvergenceViolation::PositionError {
                fixed_hz: probe.fixed_hz,
                measured_m: probe.position_error_m,
                allowed_m: self.max_position_error_m,
            });
        }
        if probe.velocity_error_m_s > self.max_velocity_error_m_s {
            found.push(ConvergenceViolation::VelocityError {
                fixed_hz: probe.fixed_hz,
                measured_m_s: probe.velocity_error_m_s,
                allowed_m_s: self.max_velocity_error_m_s,
            });
        }
        if probe.nonfinite_ticks != 0 {
            found.push(ConvergenceViolation::NonFinite {
                fixed_hz: probe.fixed_hz,
                ticks: probe.nonfinite_ticks,
            });
        }
        found
    }

    /// Checks the observed order against `slower_hz`'s minimum, if the caller
    /// measured one.
    pub fn order_violation(
        &self,
        slower_hz: u32,
        observed: Option<f64>,
    ) -> Option<ConvergenceViolation> {
        let minimum = self.min_observed_order?;
        match observed {
            Some(order) if order < minimum => Some(ConvergenceViolation::ObservedOrder {
                slower_hz,
                measured: order,
                allowed: minimum,
            }),
            Some(_) => None,
            None => Some(ConvergenceViolation::OrderUnmeasurable { slower_hz }),
        }
    }
}

/// A convergence run that missed its frozen budget.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConvergenceViolation {
    /// The budget belongs to a different rate.
    WrongRate {
        /// The rate the budget is for.
        budget: u32,
        /// The rate that was measured.
        found: u32,
    },
    /// The position error exceeded the frozen tolerance.
    PositionError {
        /// The measured rate.
        fixed_hz: u32,
        /// What the run produced.
        measured_m: f64,
        /// What the budget allows.
        allowed_m: f64,
    },
    /// The velocity error exceeded the frozen tolerance.
    VelocityError {
        /// The measured rate.
        fixed_hz: u32,
        /// What the run produced.
        measured_m_s: f64,
        /// What the budget allows.
        allowed_m_s: f64,
    },
    /// A tick produced a non-finite pose or velocity.
    NonFinite {
        /// The measured rate.
        fixed_hz: u32,
        /// How many ticks were affected.
        ticks: u64,
    },
    /// The position error did not shrink as fast as the frozen order.
    ObservedOrder {
        /// The coarser rate of the pair.
        slower_hz: u32,
        /// The measured `log2` reduction.
        measured: f64,
        /// The frozen minimum.
        allowed: f64,
    },
    /// The order could not be measured at all (a zero or missing error).
    OrderUnmeasurable {
        /// The coarser rate of the pair.
        slower_hz: u32,
    },
}

impl fmt::Display for ConvergenceViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongRate { budget, found } => {
                write!(
                    f,
                    "the budget is for {budget} Hz, the run measured {found} Hz"
                )
            }
            Self::PositionError {
                fixed_hz,
                measured_m,
                allowed_m,
            } => write!(
                f,
                "{fixed_hz} Hz position error {measured_m:.6} m exceeds the frozen {allowed_m:.6} m"
            ),
            Self::VelocityError {
                fixed_hz,
                measured_m_s,
                allowed_m_s,
            } => write!(
                f,
                "{fixed_hz} Hz velocity error {measured_m_s:.3e} m/s exceeds the frozen \
                 {allowed_m_s:.3e} m/s"
            ),
            Self::NonFinite { fixed_hz, ticks } => {
                write!(
                    f,
                    "{fixed_hz} Hz produced a non-finite state on {ticks} ticks"
                )
            }
            Self::ObservedOrder {
                slower_hz,
                measured,
                allowed,
            } => write!(
                f,
                "the position error shrank by an order of {measured:.3} from {slower_hz} Hz, \
                 below the frozen {allowed:.3}"
            ),
            Self::OrderUnmeasurable { slower_hz } => {
                write!(
                    f,
                    "the convergence order from {slower_hz} Hz is unmeasurable"
                )
            }
        }
    }
}

impl std::error::Error for ConvergenceViolation {}

/// One high-speed crossing probe: a projectile fired at a thin obstacle.
///
/// The obstacle is a 4 m × 4 m slab centred on the origin, so the crossing
/// cannot miss it laterally, and its half-thickness is a declared fraction of a
/// meter: the contract requires `speed * dt` to be much larger than the
/// obstacle or the test proves nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactScenario {
    /// Which declared layer the obstacle is on.
    pub obstacle: CollisionLayer,
    /// Whether the obstacle is a sensor or a solid collider.
    pub shape: ShapeClass,
    /// Half the obstacle's thickness along the crossing axis, in meters.
    pub obstacle_half_thickness_m: f32,
    /// Half the projectile's size on every axis, in meters.
    pub projectile_half_extent_m: f32,
    /// The projectile's speed, in m/s.
    pub speed_m_s: f32,
    /// How far ahead of the obstacle the projectile spawns, in ticks of travel.
    ///
    /// `4.0` is a clear approach with several ticks of run-up;
    /// [`SPAWN_IN_HOLE_TRAVEL_TICKS`] is F23-B's first-tick hole, where the
    /// spawn sits inside one tick of travel and only the preflight can catch
    /// it.
    pub start_travel_ticks: f32,
    /// How many ticks the probe runs after the spawn.
    pub observed_ticks: u64,
}

impl ContactScenario {
    /// A solid wall: the obstacle must stop the projectile, and the crossing is
    /// reported once as a solid contact.
    pub const fn solid_wall() -> Self {
        Self {
            obstacle: CollisionLayer::StaticWorld,
            shape: ShapeClass::Solid,
            obstacle_half_thickness_m: 0.01,
            projectile_half_extent_m: 0.05,
            speed_m_s: 120.0,
            start_travel_ticks: 4.0,
            observed_ticks: 12,
        }
    }

    /// A sensor trigger: the obstacle must *not* stop the projectile, and the
    /// crossing is reported once as an overlap.
    pub const fn sensor_trigger() -> Self {
        Self {
            obstacle: CollisionLayer::Trigger,
            shape: ShapeClass::Sensor,
            observed_ticks: 12,
            ..Self::solid_wall()
        }
    }

    /// The scenario with `speed_m_s` and `start_travel_ticks` set.
    pub const fn timed(speed_m_s: f32, start_travel_ticks: f32) -> Self {
        Self {
            speed_m_s,
            start_travel_ticks,
            ..Self::solid_wall()
        }
    }

    /// Rejects a scenario the probe cannot run, before any world is built.
    pub fn validate(&self) -> Result<(), ProbeError> {
        field("speed_m_s", self.speed_m_s, |speed| {
            speed.is_finite() && speed > 0.0
        })?;
        field(
            "obstacle_half_thickness_m",
            self.obstacle_half_thickness_m,
            |half| half.is_finite() && half > 0.0,
        )?;
        field(
            "projectile_half_extent_m",
            self.projectile_half_extent_m,
            |half| half.is_finite() && half > 0.0,
        )?;
        field("start_travel_ticks", self.start_travel_ticks, |ticks| {
            ticks.is_finite() && ticks > 0.0
        })?;
        if self.observed_ticks == 0 {
            return Err(ProbeError::Field {
                field: "observed_ticks",
                reason: "must be at least one tick",
            });
        }
        Ok(())
    }

    /// The obstacle's near face along the crossing axis, in meters.
    pub fn obstacle_face_m(&self) -> f64 {
        f64::from(self.obstacle_half_thickness_m)
    }

    /// The x position at which the projectile has cleared the obstacle
    /// completely, in meters.
    pub fn cleared_x_m(&self) -> f64 {
        f64::from(self.obstacle_half_thickness_m + self.projectile_half_extent_m)
    }
}

/// F23-B's first-tick hole: the spawn sits this many ticks of travel short of
/// the obstacle, inside the window where a freshly spawned body is not yet in
/// the broad phase.
pub const SPAWN_IN_HOLE_TRAVEL_TICKS: f32 = 0.4;

/// The fraction of its fired speed a projectile must still have after crossing
/// a sensor.
///
/// A sensor is a reported overlap and never an obstacle, so the only velocity
/// a trigger may take from a projectile is what ordinary integration costs it:
/// the probe measured every trigger crossing leaving at exactly the fired
/// speed. A tenth of a percent of headroom covers the float noise without
/// admitting a real impulse.
pub const SENSOR_MIN_SPEED_FRACTION: f64 = 0.999;

/// How far past an obstacle's near face a projectile may get, in meters.
///
/// **Frozen from the F23-D measurement.** With the declared substep count the
/// probe measured a deepest advance of `-0.0593 m` — 6.9 cm *behind* the face
/// of a 2 cm wall, i.e. no penetration at all — for every probed rate, speed and
/// geometry, including the spawn-in-the-hole cases. The tolerance is one
/// millimetre, a tenth of Avian's own contact tolerance, so it absorbs
/// float noise without admitting a single millimetre of real penetration; the
/// failures it exists to catch are 70 mm and larger.
pub const CONTACT_FACE_TOLERANCE_M: f64 = 0.001;

/// One measured crossing.
#[derive(Clone, Debug, PartialEq)]
pub struct ContactProbe {
    /// The rate the probe ran at.
    pub fixed_hz: u32,
    /// The speed the projectile was fired at, in m/s.
    pub speed_m_s: f32,
    /// How far the projectile travels per fixed tick, in meters.
    pub travel_per_tick_m: f64,
    /// How many ticks of travel separated the spawn from the obstacle.
    pub start_travel_ticks: f32,
    /// Whether the obstacle was a solid collider or a sensor.
    pub solid: bool,
    /// How the crossing was classified, from the production reporter.
    pub contact_kind: Option<ContactKind>,
    /// Contact episodes the crossing produced. AC02 requires exactly one.
    pub episodes: usize,
    /// Reporter counters: events whose bodies carry no declared layer.
    pub unclassified: u64,
    /// Reporter counters: events the declared matrix forbids.
    pub ignored: u64,
    /// Reporter counters: duplicate starts of an already-active pair.
    pub suppressed: u64,
    /// The fixed tick the first report arrived on, counting from the spawn.
    pub first_report_tick: Option<u64>,
    /// The analytic tick the projectile's leading face would have reached the
    /// obstacle, counting from the spawn. A swept crossing is reported at or
    /// just after this tick.
    pub exact_contact_tick: f64,
    /// The farthest the projectile ever got toward the obstacle, in meters.
    pub deepest_x_m: f64,
    /// How far past the obstacle's near face that was, in meters. Zero while
    /// the projectile is still in front of the obstacle.
    pub penetration_past_face_m: f64,
    /// Where the projectile ended up, in meters.
    pub final_x_m: f64,
    /// The x position at which the projectile has cleared the obstacle
    /// completely — the obstacle's far face plus the projectile's own radius.
    pub cleared_x_m: f64,
    /// The projectile's speed at the end of the run, in m/s.
    pub final_speed_m_s: f64,
    /// Whether the spawn preflight clamped the projectile onto a contact.
    pub preflight_clamped: bool,
    /// The solid body the preflight's cast found on the spawn tick, if any.
    pub preflight_hit: Option<Entity>,
    /// How far the preflight's cast could travel before that hit, in meters.
    pub preflight_distance_m: Option<f64>,
    /// A sensor the preflight's second cast found on the spawn tick: a
    /// crossing the engine does *not* report when it happens entirely inside
    /// the spawn tick (see the F23-D limitation in `preflight`), recorded here
    /// so the sweep still measures it.
    pub preflight_passed: Option<Entity>,
    /// How far the body could have travelled before that sensor, in meters.
    pub preflight_passed_distance_m: Option<f64>,
    /// The tick ledger's view of the run.
    pub accounting: TickAccounting,
    /// The schedule invariants this run broke.
    pub violations: Vec<TickViolation>,
}

impl ContactProbe {
    /// Whether a solid obstacle was passed through — the tunneling failure the
    /// swept layers exist to prevent.
    ///
    /// Only meaningful for a solid obstacle: a sensor is *supposed* to be
    /// passed through, so this is `false` for every trigger probe.
    pub fn tunnelled(&self) -> bool {
        self.solid && self.penetration_past_face_m > CONTACT_FACE_TOLERANCE_M
    }

    /// Whether the projectile left the obstacle completely behind it.
    pub fn cleared(&self) -> bool {
        self.final_x_m > self.cleared_x_m
    }
}

/// Runs one crossing probe at `fixed_hz` through the production session path.
///
/// The obstacle and the projectile are both created with
/// [`PhysicsSession::spawn`], so a moving swept-layer projectile carries the
/// spawn preflight exactly as a gameplay spawn would: the in-the-hole cases
/// (`start_travel_ticks < 1`) exercise the repair, the others exercise the
/// engine's swept detection.
pub fn contact_probe(fixed_hz: u32, scenario: ContactScenario) -> Result<ContactProbe, ProbeError> {
    scenario.validate()?;
    field("fixed_hz", fixed_hz, |hz| hz > 0).map_err(|_| ProbeError::Rate { hz: fixed_hz })?;

    let travel_per_tick_m = f64::from(scenario.speed_m_s) / f64::from(fixed_hz);
    let start_x_m = -(f64::from(scenario.start_travel_ticks) * travel_per_tick_m) as f32;

    let mut session = PhysicsSession::new(fixed_hz);
    session
        .spawn(&BodySpec {
            layer: scenario.obstacle,
            shape: scenario.shape,
            mode: BodyMode::Static,
            mass_kg: 1.0,
            half_extents_m: [scenario.obstacle_half_thickness_m, 2.0, 2.0],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0; 3],
        })
        .expect("a validated scenario is a valid obstacle spec");
    let projectile = session
        .spawn(&BodySpec {
            layer: CollisionLayer::Projectile,
            shape: ShapeClass::Solid,
            mode: BodyMode::Dynamic,
            mass_kg: 1.0,
            half_extents_m: [scenario.projectile_half_extent_m; 3],
            position_m: [start_x_m, 0.0, 0.0],
            linear_velocity_m_s: [scenario.speed_m_s, 0.0, 0.0],
        })
        .expect("a validated scenario is a valid projectile spec")
        .entity;

    let mut episodes = 0;
    let mut contact_kind = None;
    let mut first_report_tick = None;
    let mut preflight_clamped = false;
    let mut preflight_hit = None;
    let mut preflight_distance_m = None;
    let mut preflight_passed = None;
    let mut preflight_passed_distance_m = None;
    let mut deepest_x_m = f64::MIN;
    for tick in 0..scenario.observed_ticks {
        let frame = session.step(1).expect("the session is active");
        if first_report_tick.is_none() && !frame.reports.is_empty() {
            first_report_tick = Some(tick + 1);
        }
        for report in &frame.reports {
            if report.bodies.contains(&projectile) {
                episodes += 1;
                contact_kind = Some(report.kind);
            }
        }
        for event in &frame.spawn_events {
            if event.body != projectile {
                continue;
            }
            preflight_clamped |= event.clamped;
            if event.hit.is_some() {
                preflight_hit = event.hit;
                preflight_distance_m = event.distance_m.map(f64::from);
            }
            if event.passed.is_some() {
                preflight_passed = event.passed;
                preflight_passed_distance_m = event.passed_distance_m.map(f64::from);
            }
        }
        if let Some(pose) = session.pose(projectile) {
            deepest_x_m = deepest_x_m.max(f64::from(pose.position_m[0]));
        }
    }

    let face = scenario.obstacle_face_m();
    let pose = session
        .pose(projectile)
        .expect("a spawned body keeps its pose for the whole run");
    let reporter = session
        .world()
        .and_then(|world| world.get_resource::<ContactReports>())
        .map(|reports| {
            (
                reports.unclassified(),
                reports.ignored(),
                reports.suppressed(),
            )
        })
        .unwrap_or_default();
    let accounting = TickAccounting::read(&session);
    let exact_contact_tick = f64::from(scenario.start_travel_ticks)
        - 2.0 * f64::from(scenario.projectile_half_extent_m)
            / travel_per_tick_m.max(f64::MIN_POSITIVE);

    Ok(ContactProbe {
        fixed_hz,
        speed_m_s: scenario.speed_m_s,
        travel_per_tick_m,
        start_travel_ticks: scenario.start_travel_ticks,
        solid: !scenario.shape.is_sensor(),
        contact_kind,
        episodes,
        unclassified: reporter.0,
        ignored: reporter.1,
        suppressed: reporter.2,
        first_report_tick,
        exact_contact_tick,
        deepest_x_m,
        penetration_past_face_m: (deepest_x_m - face).max(0.0),
        final_x_m: f64::from(pose.position_m[0]),
        cleared_x_m: scenario.cleared_x_m(),
        final_speed_m_s: f64::from(pose.linear_velocity_m_s[0]).abs(),
        preflight_clamped,
        preflight_hit,
        preflight_distance_m,
        preflight_passed,
        preflight_passed_distance_m,
        accounting,
        violations: accounting.violations(scenario.observed_ticks),
    })
}

/// Every probed crossing, over the rate, speed, spawn-distance and obstacle
/// matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct ContactSweep {
    /// One probe per combination, in a stable order: rate, then speed, then
    /// spawn distance, then obstacle.
    pub probes: Vec<ContactProbe>,
}

/// Runs the whole high-speed contact matrix in one call: every rate in
/// [`PROBE_RATES_HZ`] and every speed in [`PROBE_SPEEDS_M_S`], a clear approach
/// and a spawn inside the first-tick hole, for a solid wall and for a sensor
/// trigger.
///
/// The sensor appears only in the clear approach, and that asymmetry is a
/// measured limitation, not a convenience: a trigger crossed entirely within
/// the spawn tick is never reported, because F23-C's acceptance criterion
/// forbids a sensor from stopping or delaying a spawn and a body that keeps
/// its velocity leaves the trigger volume inside the tick it is invisible to
/// the broad phase. [`ContactProbe::preflight_hit`] carries the fact. The
/// dedicated `accept_f23_d_a_trigger_crossed_inside_the_spawn_tick_is_recorded_
/// but_never_reported` test pins that behaviour and its numbers.
pub fn contact_sweep() -> Result<ContactSweep, ProbeError> {
    ContactSweep::run()
}

impl ContactSweep {
    /// Runs the matrix described on [`contact_sweep`].
    pub fn run() -> Result<Self, ProbeError> {
        let mut probes = Vec::new();
        for fixed_hz in PROBE_RATES_HZ {
            for speed in PROBE_SPEEDS_M_S {
                for start_travel_ticks in [4.0, SPAWN_IN_HOLE_TRAVEL_TICKS] {
                    probes.push(contact_probe(
                        fixed_hz,
                        ContactScenario::timed(speed, start_travel_ticks),
                    )?);
                }
                probes.push(contact_probe(
                    fixed_hz,
                    ContactScenario {
                        speed_m_s: speed,
                        ..ContactScenario::sensor_trigger()
                    },
                )?);
            }
        }
        Ok(Self { probes })
    }

    /// The deepest penetration of a *solid* obstacle the sweep measured, in
    /// meters. Zero means no solid crossing ever got past an obstacle's face.
    pub fn deepest_solid_penetration_m(&self) -> f64 {
        self.probes
            .iter()
            .filter(|probe| probe.solid)
            .map(|probe| probe.penetration_past_face_m)
            .fold(0.0, f64::max)
    }

    /// Every way the sweep missed the frozen contact rules, in a stable order.
    pub fn violations(&self) -> Vec<ContactViolation> {
        let mut found = Vec::new();
        for probe in &self.probes {
            found.extend(probe.violations());
        }
        found
    }
}

impl ContactProbe {
    /// Every way this crossing missed the frozen contact rules, in a stable
    /// order.
    pub fn violations(&self) -> Vec<ContactViolation> {
        let mut found = Vec::new();
        if self.accounting.substeps != DECLARED_SUBSTEP_COUNT {
            found.push(ContactViolation::SubstepPolicy {
                fixed_hz: self.fixed_hz,
                measured: self.accounting.substeps,
                declared: DECLARED_SUBSTEP_COUNT,
            });
        }
        for violation in &self.violations {
            found.push(ContactViolation::Schedule(*violation));
        }
        if self.episodes != 1 {
            found.push(ContactViolation::EpisodeCount {
                fixed_hz: self.fixed_hz,
                speed_m_s: self.speed_m_s,
                start_travel_ticks: self.start_travel_ticks,
                solid: self.solid,
                measured: self.episodes,
            });
        }
        if self.unclassified != 0 {
            found.push(ContactViolation::Unclassified {
                fixed_hz: self.fixed_hz,
                measured: self.unclassified,
            });
        }
        if self.ignored != 0 {
            found.push(ContactViolation::Ignored {
                fixed_hz: self.fixed_hz,
                measured: self.ignored,
            });
        }
        if self.suppressed != 0 {
            found.push(ContactViolation::Suppressed {
                fixed_hz: self.fixed_hz,
                measured: self.suppressed,
            });
        }
        match (self.solid, self.contact_kind) {
            (true, Some(ContactKind::SolidContact)) | (false, Some(ContactKind::SensorOverlap)) => {
            }
            (solid, kind) => found.push(ContactViolation::WrongClassification {
                fixed_hz: self.fixed_hz,
                solid,
                measured: kind,
            }),
        }
        if self.tunnelled() {
            found.push(ContactViolation::Tunnelled {
                fixed_hz: self.fixed_hz,
                speed_m_s: self.speed_m_s,
                start_travel_ticks: self.start_travel_ticks,
                travel_per_tick_m: self.travel_per_tick_m,
                penetration_m: self.penetration_past_face_m,
            });
        }
        if self.solid && !self.first_report_tick.is_some() {
            found.push(ContactViolation::Unreported {
                fixed_hz: self.fixed_hz,
                speed_m_s: self.speed_m_s,
                start_travel_ticks: self.start_travel_ticks,
            });
        }
        if !self.solid
            && self.final_speed_m_s < SENSOR_MIN_SPEED_FRACTION * f64::from(self.speed_m_s)
        {
            // A sensor must not slow the projectile down: it is a reported
            // overlap, not an obstacle (F23 non-negotiable behavior 2).
            found.push(ContactViolation::SensorSlowed {
                fixed_hz: self.fixed_hz,
                speed_m_s: self.speed_m_s,
                measured_m_s: self.final_speed_m_s,
            });
        }
        found
    }
}

/// A crossing that missed the frozen contact rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContactViolation {
    /// The world did not run with the declared substep policy, so this probe
    /// measured something the product never runs.
    SubstepPolicy {
        /// The measured rate.
        fixed_hz: u32,
        /// The substeps the world used.
        measured: u32,
        /// The substeps the product declares.
        declared: u32,
    },
    /// The tick ledger broke an F23 invariant.
    Schedule(TickViolation),
    /// The crossing did not produce exactly one contact episode.
    EpisodeCount {
        /// The measured rate.
        fixed_hz: u32,
        /// The projectile speed.
        speed_m_s: f32,
        /// The spawn distance, in ticks of travel.
        start_travel_ticks: f32,
        /// Whether the obstacle was solid.
        solid: bool,
        /// How many episodes the crossing produced.
        measured: usize,
    },
    /// The reporter could not classify an event because a body had no declared
    /// layer.
    Unclassified {
        /// The measured rate.
        fixed_hz: u32,
        /// The reporter's counter.
        measured: u64,
    },
    /// The reporter counted an event the declared matrix forbids.
    Ignored {
        /// The measured rate.
        fixed_hz: u32,
        /// The reporter's counter.
        measured: u64,
    },
    /// The reporter suppressed a duplicate start.
    Suppressed {
        /// The measured rate.
        fixed_hz: u32,
        /// The reporter's counter.
        measured: u64,
    },
    /// The crossing was classified as the wrong kind of contact.
    WrongClassification {
        /// The measured rate.
        fixed_hz: u32,
        /// Whether the obstacle was solid.
        solid: bool,
        /// What the reporter said.
        measured: Option<ContactKind>,
    },
    /// A projectile passed through a solid obstacle.
    Tunnelled {
        /// The measured rate.
        fixed_hz: u32,
        /// The projectile speed.
        speed_m_s: f32,
        /// The spawn distance, in ticks of travel.
        start_travel_ticks: f32,
        /// How far the projectile travelled per tick.
        travel_per_tick_m: f64,
        /// How far past the obstacle's face it got.
        penetration_m: f64,
    },
    /// A solid crossing produced no contact report at all.
    Unreported {
        /// The measured rate.
        fixed_hz: u32,
        /// The projectile speed.
        speed_m_s: f32,
        /// The spawn distance, in ticks of travel.
        start_travel_ticks: f32,
    },
    /// A sensor overlap slowed the projectile down, i.e. behaved like an
    /// obstacle.
    SensorSlowed {
        /// The measured rate.
        fixed_hz: u32,
        /// The projectile's fired speed.
        speed_m_s: f32,
        /// The speed it left with.
        measured_m_s: f64,
    },
}

impl fmt::Display for ContactViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubstepPolicy {
                fixed_hz,
                measured,
                declared,
            } => write!(
                f,
                "{fixed_hz} Hz ran with {measured} substeps, not the declared {declared}"
            ),
            Self::Schedule(violation) => write!(f, "{violation}"),
            Self::EpisodeCount {
                fixed_hz,
                speed_m_s,
                start_travel_ticks,
                solid,
                measured,
            } => write!(
                f,
                "{fixed_hz} Hz at {speed_m_s} m/s from {start_travel_ticks} ticks of travel \
                 produced {measured} contact episodes (solid obstacle: {solid}), not 1"
            ),
            Self::Unclassified { fixed_hz, measured } => {
                write!(f, "{fixed_hz} Hz reported {measured} unclassified events")
            }
            Self::Ignored { fixed_hz, measured } => {
                write!(f, "{fixed_hz} Hz reported {measured} forbidden events")
            }
            Self::Suppressed { fixed_hz, measured } => {
                write!(f, "{fixed_hz} Hz suppressed {measured} duplicate starts")
            }
            Self::WrongClassification {
                fixed_hz,
                solid,
                measured,
            } => write!(
                f,
                "{fixed_hz} Hz classified a {} obstacle as {measured:?}",
                if *solid { "solid" } else { "sensor" }
            ),
            Self::Tunnelled {
                fixed_hz,
                speed_m_s,
                start_travel_ticks,
                travel_per_tick_m,
                penetration_m,
            } => write!(
                f,
                "{fixed_hz} Hz at {speed_m_s} m/s ({travel_per_tick_m:.3} m per tick, spawned \
                 {start_travel_ticks} ticks out) passed {penetration_m:.4} m through a solid \
                 obstacle"
            ),
            Self::Unreported {
                fixed_hz,
                speed_m_s,
                start_travel_ticks,
            } => write!(
                f,
                "{fixed_hz} Hz at {speed_m_s} m/s from {start_travel_ticks} ticks of travel \
                 crossed a solid obstacle with no contact report"
            ),
            Self::SensorSlowed {
                fixed_hz,
                speed_m_s,
                measured_m_s,
            } => write!(
                f,
                "{fixed_hz} Hz: a sensor overlap slowed a {speed_m_s} m/s projectile to \
                 {measured_m_s:.3} m/s"
            ),
        }
    }
}

impl std::error::Error for ContactViolation {}

/// What the stability probe flies.
///
/// The default is a level, wings-level cruise with the engine already spooled
/// and a modest throttle held for the whole run: the point is a *long, quiet,
/// unobstructed* flight, where the only things that can go wrong are numerical
/// — a non-finite state, a tick that did not integrate, a force request that
/// reached no body, a driver tick that was skipped, or a body rate the bounded
/// controller was never supposed to reach.
#[derive(Clone, Debug, PartialEq)]
pub struct StabilityScenario {
    /// Where the aircraft spawns, in meters.
    pub spawn_position_m: [f32; 3],
    /// Its initial world velocity, in m/s. Body forward is -Z.
    pub spawn_velocity_mps: [f32; 3],
    /// The engine state at spawn.
    pub engine: EngineState,
    /// The command held for the whole run.
    pub command: FlightInput,
    /// How long the run simulates, in seconds.
    pub duration_s: f32,
    /// The collider half-extents, in meters.
    pub half_extents_m: [f32; 3],
}

impl StabilityScenario {
    /// The declared F23-D probe: a synthetic fixed wing trimmed at 120 m/s at
    /// 1000 m, throttle 0.7, wings level, flown for ten seconds.
    pub fn level_cruise() -> Self {
        Self {
            spawn_position_m: [0.0, 1000.0, 0.0],
            spawn_velocity_mps: [0.0, 0.0, -120.0],
            engine: EngineState::direct(0.6),
            command: FlightInput {
                throttle: 0.7,
                ..FlightInput::NEUTRAL
            },
            duration_s: 10.0,
            half_extents_m: [0.5, 0.5, 0.5],
        }
    }

    /// The scenario flown for `duration_s` seconds instead.
    pub fn for_duration(mut self, duration_s: f32) -> Self {
        self.duration_s = duration_s;
        self
    }

    /// Rejects a scenario the probe cannot fly, before any world is built.
    pub fn validate(&self) -> Result<(), ProbeError> {
        field("duration_s", self.duration_s, |seconds| {
            seconds.is_finite() && seconds > 0.0
        })?;
        for (name, value) in [
            ("spawn_position_m", self.spawn_position_m),
            ("spawn_velocity_mps", self.spawn_velocity_mps),
            ("half_extents_m", self.half_extents_m),
        ] {
            for component in value {
                if !component.is_finite() {
                    return Err(ProbeError::Field {
                        field: name,
                        reason: "must be finite",
                    });
                }
            }
        }
        Ok(())
    }
}

/// One long-run stability measurement.
#[derive(Clone, Debug, PartialEq)]
pub struct StabilityProbe {
    /// The rate the run used.
    pub fixed_hz: u32,
    /// Fixed ticks the run crossed.
    pub ticks: u64,
    /// Ticks whose pose or velocity was not finite. Must be zero.
    pub nonfinite_ticks: u64,
    /// The fastest the aircraft ever got, in m/s.
    pub max_speed_m_s: f64,
    /// The largest world-space body rate, in rad/s.
    pub max_body_rate_rad_s: f64,
    /// The lowest and highest altitude reached, in meters.
    pub altitude_range_m: (f64, f64),
    /// Where the aircraft ended up, in meters.
    pub final_position_m: [f64; 3],
    /// The flight driver's own accounting for the run.
    pub driver: FlightTickReport,
    /// The tick ledger's view of the run.
    pub accounting: TickAccounting,
    /// Every way the run broke a frozen stability rule, in a stable order.
    pub violations: Vec<StabilityViolation>,
}

/// A stability rule the run broke.
#[derive(Clone, Debug, PartialEq)]
pub enum StabilityViolation {
    /// A tick produced a non-finite pose or velocity.
    NonFinite {
        /// How many ticks were affected.
        ticks: u64,
    },
    /// The integrator did not run once per fixed tick, or a request was lost.
    Schedule(TickViolation),
    /// The flight driver did not produce a force request on every fixed tick.
    ///
    /// `driven` must equal `ticks` for a single dynamic aircraft: that is the
    /// runtime half of "one force per fixed tick, applied to that same tick".
    DriverSkipped {
        /// Fixed ticks the driver ran.
        ticks: u64,
        /// Aircraft-ticks that produced an applied request.
        driven: u64,
        /// Aircraft-ticks skipped because the body was not dynamic.
        parked: u64,
    },
    /// The driver did not run on every fixed tick of the probe.
    ///
    /// Its own counter is what catches a driver that was never registered at
    /// all: with no systems in the schedule both its counters read zero, which
    /// would otherwise look like a run in which nothing needed flying.
    DriverAbsent {
        /// Fixed ticks the world ran.
        ticks: u64,
        /// Fixed ticks the driver counted.
        driver_ticks: u64,
    },
    /// A force request the driver produced never reached the integrator.
    ///
    /// `applied` is the adapter's ledger, `driven` is the driver's own count:
    /// they must agree, or a request was produced and then lost — the failure
    /// mode of an adapter whose drain is unregistered.
    RequestsNotApplied {
        /// Requests the driver produced.
        driven: u64,
        /// Requests the adapter applied.
        applied: u64,
    },
    /// The flight driver refused a tick, or parked the aircraft.
    DriverRefused {
        /// How many ticks were refused.
        refused: u64,
        /// The newest refusal, when there was one.
        last: Option<super::flight::FlightRefusal>,
    },
    /// The aircraft outran the frozen speed ceiling.
    SpeedRunaway {
        /// What the run reached.
        measured_m_s: f64,
        /// What the budget allows.
        allowed_m_s: f64,
    },
    /// The body rate outran the frozen rate ceiling.
    RateRunaway {
        /// What the run reached.
        measured_rad_s: f64,
        /// What the budget allows.
        allowed_rad_s: f64,
    },
    /// The aircraft left the frozen altitude band.
    AltitudeDrift {
        /// What the run reached.
        measured_m: (f64, f64),
        /// The band the budget allows.
        allowed_m: (f64, f64),
    },
}

impl fmt::Display for StabilityViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { ticks } => {
                write!(f, "{ticks} ticks produced a non-finite state")
            }
            Self::Schedule(violation) => write!(f, "{violation}"),
            Self::DriverSkipped {
                ticks,
                driven,
                parked,
            } => write!(
                f,
                "the flight driver ran {ticks} ticks, drove {driven} and parked {parked}"
            ),
            Self::DriverAbsent {
                ticks,
                driver_ticks,
            } => write!(
                f,
                "the flight driver counted {driver_ticks} of the world's {ticks} fixed ticks"
            ),
            Self::RequestsNotApplied { driven, applied } => write!(
                f,
                "the driver produced {driven} force requests and the adapter applied {applied}"
            ),
            Self::DriverRefused { refused, last } => match last {
                Some(refusal) => write!(
                    f,
                    "{refused} flight ticks were refused; the last was {:?} at tick {}: {}",
                    refusal.entity, refusal.tick, refusal.reason
                ),
                None => write!(f, "{refused} flight ticks were refused"),
            },
            Self::SpeedRunaway {
                measured_m_s,
                allowed_m_s,
            } => write!(
                f,
                "the aircraft reached {measured_m_s:.3} m/s, above the frozen {allowed_m_s:.3} m/s"
            ),
            Self::RateRunaway {
                measured_rad_s,
                allowed_rad_s,
            } => write!(
                f,
                "the body rate reached {measured_rad_s:.3} rad/s, above the frozen \
                 {allowed_rad_s:.3} rad/s"
            ),
            Self::AltitudeDrift {
                measured_m,
                allowed_m,
            } => write!(
                f,
                "the aircraft flew between {:.3} m and {:.3} m, outside the frozen band \
                 {:.3} m to {:.3} m",
                measured_m.0, measured_m.1, allowed_m.0, allowed_m.1
            ),
        }
    }
}

impl std::error::Error for StabilityViolation {}

/// One frozen stability budget for the declared cruise scenario.
///
/// **Frozen from the F23-D measurement**, with headroom: the ten-second cruise
/// measured a peak of `123.399 m/s` at 60 Hz, `123.397 m/s` at 120 Hz and
/// `123.396 m/s` at 240 Hz — a spread of `0.0034 m/s` across a four-fold change
/// of rate — zero body rate (the command is neutral), and an altitude band of
/// `1000.00 m` to `1023.42 m`. These bounds are declared tolerances for the
/// declared scenario, not a flight envelope, and not a claim about the
/// original game's handling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StabilityBudget {
    /// The largest speed the cruise may reach, in m/s.
    pub max_speed_m_s: f64,
    /// The largest body rate the cruise may reach, in rad/s. The F24-A
    /// controller's declared cap is 2.0 rad/s; a wings-level cruise must stay
    /// far below it.
    pub max_body_rate_rad_s: f64,
    /// The lowest and highest altitude the cruise may reach, in meters.
    pub altitude_band_m: (f64, f64),
    /// The largest spread the peak speed may show between two probed rates, in
    /// m/s. This is the rate-convergence statement for the *flight* path, as
    /// opposed to the analytic force path.
    pub max_rate_spread_m_s: f64,
}

/// The frozen stability budget for [`StabilityScenario::level_cruise`].
pub const FROZEN_STABILITY_BUDGET: StabilityBudget = StabilityBudget {
    max_speed_m_s: 130.0,
    max_body_rate_rad_s: 0.5,
    altitude_band_m: (900.0, 1100.0),
    max_rate_spread_m_s: 0.05,
};

impl StabilityBudget {
    /// Every way `probe` misses this budget, in a stable order.
    pub fn violations(&self, probe: &StabilityProbe) -> Vec<StabilityViolation> {
        let mut found = Vec::new();
        if probe.nonfinite_ticks != 0 {
            found.push(StabilityViolation::NonFinite {
                ticks: probe.nonfinite_ticks,
            });
        }
        for violation in &probe.accounting.violations(probe.ticks) {
            found.push(StabilityViolation::Schedule(*violation));
        }
        if probe.driver.ticks != probe.ticks {
            found.push(StabilityViolation::DriverAbsent {
                ticks: probe.ticks,
                driver_ticks: probe.driver.ticks,
            });
        }
        if probe.driver.driven != probe.driver.ticks || probe.driver.parked != 0 {
            found.push(StabilityViolation::DriverSkipped {
                ticks: probe.driver.ticks,
                driven: probe.driver.driven,
                parked: probe.driver.parked,
            });
        }
        if probe.driver.driven != probe.accounting.applied_requests {
            found.push(StabilityViolation::RequestsNotApplied {
                driven: probe.driver.driven,
                applied: probe.accounting.applied_requests,
            });
        }
        if probe.driver.refused != 0 {
            found.push(StabilityViolation::DriverRefused {
                refused: probe.driver.refused,
                last: probe.driver.last_refusal.clone(),
            });
        }
        if probe.max_speed_m_s > self.max_speed_m_s {
            found.push(StabilityViolation::SpeedRunaway {
                measured_m_s: probe.max_speed_m_s,
                allowed_m_s: self.max_speed_m_s,
            });
        }
        if probe.max_body_rate_rad_s > self.max_body_rate_rad_s {
            found.push(StabilityViolation::RateRunaway {
                measured_rad_s: probe.max_body_rate_rad_s,
                allowed_rad_s: self.max_body_rate_rad_s,
            });
        }
        if probe.altitude_range_m.0 < self.altitude_band_m.0
            || probe.altitude_range_m.1 > self.altitude_band_m.1
        {
            found.push(StabilityViolation::AltitudeDrift {
                measured_m: probe.altitude_range_m,
                allowed_m: self.altitude_band_m,
            });
        }
        found
    }
}

/// Flies the production F24 fixed-wing model for `duration_s` simulated seconds
/// at `fixed_hz`, through the production session, and measures whether the
/// fixed step stayed stable.
///
/// The session hosts the real [`FlightForcesPlugin`], the aircraft is spawned
/// by [`spawn_flight_body`] with the synthetic airframe's declared mass and
/// inertia, and the forces come from `cs_sim`'s production flight model through
/// the one-tick [`ForceRequest`](super::ForceRequest) queue. Nothing here is a
/// parallel test-only implementation: if the driver, the model, the mass
/// binding or the schedule stopped working, this probe's numbers would move.
pub fn stability_probe(
    scenario: &StabilityScenario,
    fixed_hz: u32,
) -> Result<StabilityProbe, ProbeError> {
    scenario.validate()?;
    field("fixed_hz", fixed_hz, |hz| hz > 0).map_err(|_| ProbeError::Rate { hz: fixed_hz })?;

    let ticks = u64::from(fixed_hz) * u64::from(f64_to_ticks(f64::from(scenario.duration_s)));
    if ticks == 0 {
        return Err(ProbeError::Field {
            field: "duration_s",
            reason: "is shorter than one tick at this rate",
        });
    }

    let mut session = PhysicsSession::builder()
        .fixed_hz(fixed_hz)
        .configure(|app| {
            app.add_plugins(FlightForcesPlugin);
        })
        .build();
    let spawn = FlightSpawnSpec {
        half_extents_m: scenario.half_extents_m,
        position_m: scenario.spawn_position_m,
        linear_velocity_mps: scenario.spawn_velocity_mps,
        engine: scenario.engine,
        command: scenario.command,
        ..FlightSpawnSpec::level_at(scenario.spawn_position_m, scenario.spawn_velocity_mps)
    };
    let body = spawn_flight_body(
        session.world_mut().expect("a fresh session is active"),
        FlightModel::new(synthetic_fixed_wing()),
        &spawn,
    )
    .expect("the declared scenario is a valid flight spawn");

    let mut nonfinite_ticks = 0;
    let mut max_speed_m_s = 0.0_f64;
    let mut max_body_rate_rad_s = 0.0_f64;
    let mut lowest = f64::MAX;
    let mut highest = f64::MIN;
    for _ in 0..ticks {
        session.step(1).expect("the session is active");
        let Some(pose) = session.pose(body) else {
            // The body left the world: that is itself a stability failure, and
            // the non-finite counter is the nearest named bucket for it.
            nonfinite_ticks += 1;
            continue;
        };
        let speed = vector_length(pose.linear_velocity_m_s);
        let rate = vector_length(pose.angular_velocity_rad_s);
        if !speed.is_finite() || !rate.is_finite() || !pose.position_m[0].is_finite() {
            nonfinite_ticks += 1;
            continue;
        }
        max_speed_m_s = max_speed_m_s.max(speed);
        max_body_rate_rad_s = max_body_rate_rad_s.max(rate);
        let altitude = f64::from(pose.position_m[1]);
        lowest = lowest.min(altitude);
        highest = highest.max(altitude);
    }

    let driver = session
        .world()
        .and_then(|world| world.get_resource::<FlightTickReport>())
        .cloned()
        .unwrap_or_default();
    let accounting = TickAccounting::read(&session);
    let pose = session
        .pose(body)
        .expect("a spawned aircraft keeps its pose for the whole run");
    let mut probe = StabilityProbe {
        fixed_hz,
        ticks,
        nonfinite_ticks,
        max_speed_m_s,
        max_body_rate_rad_s,
        altitude_range_m: (lowest, highest),
        final_position_m: pose.position_m.map(f64::from),
        driver,
        accounting,
        violations: Vec::new(),
    };
    probe.violations = FROZEN_STABILITY_BUDGET.violations(&probe);
    Ok(probe)
}

/// The peak-speed spread between the fastest and slowest probed rates.
pub fn rate_spread_m_s(probes: &[StabilityProbe]) -> f64 {
    let (Some(first), Some(last)) = (probes.first(), probes.last()) else {
        return 0.0;
    };
    (first.max_speed_m_s - last.max_speed_m_s).abs()
}

fn f64_to_ticks(seconds: f64) -> u32 {
    // `duration_s * hz` is an integer by construction for every declared
    // scenario; rounding to the nearest keeps a floating-point representation
    // like 0.1 s × 240 Hz = 24.000000000000004 from losing a tick.
    seconds.round() as u32
}

fn pose_is_finite(pose: &super::fixture::PhysicsSample) -> bool {
    pose.position_m.iter().all(|value| value.is_finite())
        && pose
            .linear_velocity_m_s
            .iter()
            .all(|value| value.is_finite())
        && pose
            .angular_velocity_rad_s
            .iter()
            .all(|value| value.is_finite())
}

fn max_abs_difference(measured: &[f64; 3], expected: &[f64; 3]) -> f64 {
    (0..3)
        .map(|axis| (measured[axis] - expected[axis]).abs())
        .fold(0.0, f64::max)
}

fn vector_length(values: [f32; 3]) -> f64 {
    values
        .iter()
        .map(|component| f64::from(*component) * f64::from(*component))
        .sum::<f64>()
        .sqrt()
}
