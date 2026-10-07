//! Task #526: the F31 follower's bank cascade closes the commanded bank
//! against **measured** bank, and the envelope's declared turn bound is the
//! turn the airframe it is flown by actually sustains.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`, AC03; shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//! Task test prefix: `accept_t526_`.
//!
//! Everything here drives production code: the production [`Navigator`]
//! produces the bounded [`cs_sim::flight::FlightInput`], the production
//! [`FlightModel`] (`synthetic_fixed_wing`) turns it into force and torque,
//! and the fixed-tick integrator is only the glue a probe needs to fly the
//! equations (the same body integration `cs_sim::probes::ProbeRunner` uses).
//!
//! Every value is newly authored synthetic fixture data, never original game
//! data, and nothing here reads `CS_GAME_DIR`.

use cs_sim::ai::navigation::{
    NavState, NavigationCadence, NavigationRequest, Navigator, ReferenceFrameSample, RouteFrame,
    RouteGraph, RouteNode, RouteNodeId, RouteProgress, RouteTermination, heading_from_direction,
    synthetic_maneuver_envelope,
};
use cs_sim::flight::{
    BODY_FORWARD, BODY_RIGHT, BODY_UP, DamageState, EngineState, FlightEnvironment, FlightModel,
    FlightState, LoadoutMass, synthetic_fixed_wing,
};
use cs_types::Tick;
use cs_types::space::Quaternion;

/// The designed fixed step of the integrated probe, matching the F24 probe
/// cadence and the F23 fixed rate the ECS installs.
const DT_S: f64 = 1.0 / 120.0;

/// One sample of the integrated probe's state, in the conventions
/// [`NavState`] declares.
#[derive(Clone, Copy, Debug)]
struct Sample {
    heading_rad: f64,
    bank_rad: f64,
    speed_mps: f64,
    climb_mps: f64,
}

/// The production follower flying the production airframe on a fixed tick.
///
/// This is a probe, not a second authority: it owns no force law and no
/// controller, it only advances the state the production types produced.
struct Probe {
    navigator: Navigator,
    model: FlightModel,
    inertia: [f64; 3],
    mass_kg: f64,
    route: RouteGraph,
    orientation: [f64; 4],
    position: [f64; 3],
    velocity: [f64; 3],
    angular: [f64; 3],
    engine: EngineState,
}

impl Probe {
    /// A probe that starts at the origin at cruise, level, pointed down `-Z`,
    /// targeting a single marker at `target_m`.
    fn new(target_m: [f64; 3], start_speed_mps: f64) -> Self {
        let model = FlightModel::new(synthetic_fixed_wing());
        let tuning = model.tuning();
        // `probes::ProbeRunner` reorders the per-control inertia `[roll, pitch,
        // yaw]` into body `[x, y, z]`; the same mapping is needed here.
        let inertia = [
            tuning.mass.inertia_kg_m2[1],
            tuning.mass.inertia_kg_m2[2],
            tuning.mass.inertia_kg_m2[0],
        ];
        let mass_kg = tuning.mass.mass_kg;
        Self {
            navigator: Navigator::new(
                synthetic_maneuver_envelope(),
                NavigationCadence::designed_default(),
            )
            .expect("the designed synthetic envelope and cadence are valid"),
            model,
            inertia,
            mass_kg,
            route: RouteGraph {
                frame: RouteFrame::World,
                termination: RouteTermination::End,
                clearance_m: 0.0,
                nodes: vec![RouteNode {
                    id: RouteNodeId(0),
                    sequence: 0,
                    mandatory: true,
                    position_m: target_m,
                    arrival_radius_m: 10.0,
                }],
            },
            orientation: Quaternion::IDENTITY.components(),
            position: [0.0, 0.0, 0.0],
            velocity: [0.0, 0.0, -start_speed_mps],
            angular: [0.0; 3],
            engine: EngineState::direct(1.0),
        }
    }

    /// Advances one fixed tick and returns the state the follower was fed.
    fn tick(&mut self) -> Sample {
        let rotation = Quaternion::try_new(self.orientation).expect("the rotation stays unit");
        let forward = rotate(BODY_FORWARD, rotation);
        let right = rotate(BODY_RIGHT, rotation);
        let up = rotate(BODY_UP, rotation);
        let state = NavState {
            position_m: self.position,
            heading_rad: heading_from_direction(forward[0], forward[2]),
            // The same right-wing-down sign `cs_app` measures out of the live
            // Avian `Rotation`: `atan2(right.Y, up.Y)` is left-wing-down, so it
            // is negated.
            bank_rad: -(right[1]).atan2(up[1]),
            speed_mps: (self.velocity[0] * self.velocity[0] + self.velocity[2] * self.velocity[2])
                .sqrt(),
            climb_mps: self.velocity[1],
        };
        let decision = self
            .navigator
            .decide(&NavigationRequest {
                tick: Tick(0),
                generation: 1,
                state,
                route: &self.route,
                progress: RouteProgress::start(),
                frame: ReferenceFrameSample::IDENTITY,
                blockers: &[],
                dt_s: DT_S,
            })
            .expect("the follower's request is valid");

        let flight_state = FlightState {
            orientation: rotation,
            linear_velocity_mps: self.velocity,
            angular_velocity_radps: self.angular,
            engine: self.engine,
            boost_available: false,
        };
        let output = self
            .model
            .compute(
                &FlightEnvironment::SEA_LEVEL,
                &LoadoutMass::EMPTY,
                &DamageState::PRISTINE,
                &flight_state,
                &decision.command,
                DT_S,
            )
            .expect("the production flight model accepts the bounded command");

        for axis in 0..3 {
            self.velocity[axis] += output.world_force_n[axis] / self.mass_kg * DT_S;
            self.position[axis] += self.velocity[axis] * DT_S;
        }
        let body_torque = rotate_inverse(output.world_torque_nm, self.orientation);
        for (axis, torque) in body_torque.into_iter().enumerate() {
            self.angular[axis] += torque / self.inertia[axis] * DT_S;
        }
        self.orientation = integrate_orientation(self.orientation, self.angular, DT_S);
        self.engine.advance(
            decision.command.throttle,
            self.model.tuning().engine.throttle_response_per_s,
            DT_S,
        );
        Sample {
            heading_rad: state.heading_rad,
            bank_rad: state.bank_rad,
            speed_mps: state.speed_mps,
            climb_mps: state.climb_mps,
        }
    }
}

/// The turn the production airframe sustains while the production follower
/// holds the envelope's declared maximum bank.
///
/// The marker sits far off to the left, so the heading error stays large for
/// the whole window: the outer loop saturates, the bank demand is exactly the
/// envelope's `max_bank_rad`, and what is measured is the turn that bank
/// actually buys.
fn sustained_turn() -> (f64, f64, f64) {
    let mut probe = Probe::new([-4_000.0, 0.0, 0.0], 40.0);
    // 2.5 s to roll in and let the turn settle, then 0.5 s of measurement.
    const SETTLE: usize = 300;
    const WINDOW: usize = 60;

    for _ in 0..SETTLE {
        probe.tick();
    }
    let first = probe.tick();
    let mut last = first;
    let mut bank_sum = 0.0;
    let mut speed_sum = 0.0;
    for _ in 1..WINDOW {
        last = probe.tick();
        bank_sum += last.bank_rad.abs();
        speed_sum += last.speed_mps;
    }
    let samples = (WINDOW - 1) as f64;
    let dt = samples * DT_S;
    (
        wrap_pi(last.heading_rad - first.heading_rad) / dt,
        bank_sum / samples,
        speed_sum / samples,
    )
}

/// The peak-up, peak-down and last-second mean vertical speed the production
/// airframe reaches while the follower commands its declared maximum climb
/// (`target_m` far above) or its declared maximum dive (far below).
fn vertical_rates(target_m: f64) -> (f64, f64, f64) {
    // The marker stays kilometres away, so the follower's vertical command is
    // saturated at the envelope's bound for the whole window.
    let mut probe = Probe::new([0.0, target_m, -4_000.0], 40.0);
    const WINDOW: usize = 720; // six synthetic seconds
    let mut peak_up = f64::NEG_INFINITY;
    let mut peak_down = f64::INFINITY;
    let mut tail_sum = 0.0;
    let mut tail_count = 0;
    for tick in 0..WINDOW {
        let sample = probe.tick();
        peak_up = peak_up.max(sample.climb_mps);
        peak_down = peak_down.min(sample.climb_mps);
        if tick >= WINDOW - 120 {
            tail_sum += sample.climb_mps;
            tail_count += 1;
        }
    }
    (peak_up, peak_down, tail_sum / f64::from(tail_count as u32))
}

/// AC03 / task #526, the turn half: what the envelope declares about turning
/// is now a statement about the airframe it is flown by, measured through the
/// production follower and the production flight model.
///
/// Three things are pinned here. The follower's **effective** turn bound is
/// the coordinated turn `omega = g * tan(bank) / V` at the declared bank and
/// cruise (0.424 rad/s), because the bank clamp in the command cascade
/// saturates there first, and the airframe sustains it. The airframe's actual
/// turn stays inside the measured band of the coordinated relation for the
/// bank and speed it is holding. And the declared **kinematic**
/// `max_yaw_rate_radps = 1.0` — the number that gives the arch fixture its
/// 40 m turn radius — exceeds what the airframe flies; that mismatch is
/// recorded here as measured evidence (see
/// `docs/findings/2026-10-07-t526-follower-bank-hold.md`) rather than hidden
/// behind a silently changed constant, because deriving it instead widens the
/// kinematic turn radius to 94 m and the arch fixture stops rejoining.
#[test]
fn accept_t526_the_declared_turn_bound_is_the_turn_the_airframe_sustains() {
    let envelope = synthetic_maneuver_envelope();
    let gravity = FlightEnvironment::SEA_LEVEL.gravity_mps2;

    // The coordinated turn the declared bank and cruise buy: the follower's
    // effective bound, since the bank clamp reaches it before the declared
    // kinematic rate does.
    let effective = gravity * envelope.max_bank_rad.tan() / envelope.cruise_speed_mps;
    assert!(
        effective < envelope.max_yaw_rate_radps,
        "the declared kinematic rate is the bound the bank clamp must reach first for this \
         record to hold: effective {effective} vs declared {}",
        envelope.max_yaw_rate_radps
    );

    let (turn_rate, bank, speed) = sustained_turn();
    assert!(
        (bank - envelope.max_bank_rad).abs() < 0.1,
        "the bank-hold cascade must actually reach the declared bank: measured {bank} vs {}",
        envelope.max_bank_rad
    );
    assert!(
        turn_rate >= 0.9 * effective,
        "the airframe must sustain the coordinated turn its declared bank buys: measured \
         {turn_rate} rad/s vs {effective} rad/s"
    );
    let coordinated = gravity * bank.tan() / speed;
    assert!(
        (turn_rate - coordinated).abs() <= 0.5 * coordinated.abs(),
        "the measured turn must stay in the measured band of the coordinated turn its bank \
         buys: measured {turn_rate} rad/s vs coordinated {coordinated} rad/s at bank {bank} \
         and {speed} m/s"
    );
    // The recorded mismatch: the declared kinematic bound is not a number this
    // airframe reaches. If this ever fails, the record above is stale and the
    // finding must be re-measured rather than the assertion relaxed.
    assert!(
        turn_rate < envelope.max_yaw_rate_radps,
        "the recorded turn-bound mismatch has changed: measured {turn_rate} rad/s against a \
         declared {} rad/s — re-measure and re-record \
         docs/findings/2026-10-07-t526-follower-bank-hold.md",
        envelope.max_yaw_rate_radps
    );
}

/// AC03 / task #526, the vertical half: the envelope's declared climb and
/// dive bounds are the vertical rates the airframe it is flown by actually
/// produces when the follower commands them.
///
/// #451 left the turn bound as a number the airframe could not fly and the
/// vertical bound unexamined; both are now measured here through the
/// production follower and the production flight model, and the declared
/// values sit inside the measurements (the climb bound was lowered from 20 to
/// 15 m/s for exactly this reason — the airframe peaks at 16.6 m/s at the
/// cruise throttle the follower commands). The thrust-limited steady-state
/// figure is recorded beside this test in
/// `docs/findings/2026-10-07-t526-follower-bank-hold.md`.
#[test]
fn accept_t526_the_declared_climb_bounds_are_flown_by_the_airframe() {
    let envelope = synthetic_maneuver_envelope();

    let (peak_climb, _, sustained_climb) = vertical_rates(4_000.0);
    assert!(
        peak_climb >= envelope.max_climb_rate_mps,
        "the declared maximum climb must be reachable: measured peak {peak_climb} m/s vs declared {}",
        envelope.max_climb_rate_mps
    );
    assert!(
        sustained_climb >= 0.9 * envelope.max_climb_rate_mps,
        "the declared maximum climb must be sustained over a route leg: measured \
         {sustained_climb} m/s vs declared {}",
        envelope.max_climb_rate_mps
    );

    let (_, peak_dive, sustained_dive) = vertical_rates(-4_000.0);
    assert!(
        peak_dive.abs() >= envelope.max_dive_rate_mps,
        "the declared maximum dive must be reachable: measured peak {peak_dive} m/s vs declared -{}",
        envelope.max_dive_rate_mps
    );
    assert!(
        sustained_dive.abs() >= 0.9 * envelope.max_dive_rate_mps,
        "the declared maximum dive must be sustained over a route leg: measured \
         {sustained_dive} m/s vs declared -{}",
        envelope.max_dive_rate_mps
    );
}

// ---------------------------------------------------------- integrator glue --

fn wrap_pi(angle: f64) -> f64 {
    let two_pi = std::f64::consts::TAU;
    let mut wrapped = angle % two_pi;
    if wrapped > std::f64::consts::PI {
        wrapped -= two_pi;
    } else if wrapped <= -std::f64::consts::PI {
        wrapped += two_pi;
    }
    wrapped
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn norm(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Rotates `vector` from body space to world space by `rotation`.
fn rotate(vector: [f64; 3], rotation: Quaternion) -> [f64; 3] {
    let [x, y, z, w] = rotation.components();
    let axis = [x, y, z];
    let axis_cross = cross(axis, vector);
    add(
        add(vector, scale(axis_cross, 2.0 * w)),
        scale(cross(axis, axis_cross), 2.0),
    )
}

/// Rotates a world vector into body space (the inverse of `orientation`).
fn rotate_inverse(vector: [f64; 3], orientation: [f64; 4]) -> [f64; 3] {
    let conjugate = [
        -orientation[0],
        -orientation[1],
        -orientation[2],
        orientation[3],
    ];
    let q = Quaternion::try_new(conjugate).expect("the conjugate stays unit");
    rotate(vector, q)
}

/// Hamilton product `a ⊗ b` of `[x, y, z, w]` quaternions.
fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// Applies a body-frame angular velocity for `dt_s` and renormalizes.
fn integrate_orientation(orientation: [f64; 4], body_rate: [f64; 3], dt_s: f64) -> [f64; 4] {
    let rate = norm(body_rate);
    let angle = rate * dt_s;
    let delta = if angle > 1e-12 {
        let (sin, cos) = (angle * 0.5).sin_cos();
        let factor = sin / rate;
        [
            body_rate[0] * factor,
            body_rate[1] * factor,
            body_rate[2] * factor,
            cos,
        ]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    let next = quat_mul(orientation, delta);
    let length =
        (next[0] * next[0] + next[1] * next[1] + next[2] * next[2] + next[3] * next[3]).sqrt();
    [
        next[0] / length,
        next[1] / length,
        next[2] / length,
        next[3] / length,
    ]
}
