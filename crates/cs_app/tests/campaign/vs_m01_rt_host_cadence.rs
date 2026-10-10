//! Acceptance stage VS-M01-RT-HOST-CADENCE: the session tick the composed
//! per-tick entry asks a world-actor session for (Rally #1281,
//! `VS-M01-RT-HOST-CADENCE`).
//!
//! Task test prefix: `accept_vs_m01_rt_host_cadence_`. Minimum scenario: *a
//! route follower stepped by the composed entry advances by exactly
//! `speed * dt * ticks` for the ticks offered*.
//!
//! The composition owns exactly one timeline: `physics::BASELINE_FIXED_HZ`,
//! the fixed rate `PhysicsTickLedger` commits and
//! [`MissionContent::tick_rate`] starts the environment session on. The
//! composition's per-tick entry — `mission_session::mission_host_tick`,
//! Rally #1278 — hands that committed tick to
//! `WorldActorSession::step(to: tick)`, so the world-actor set is stepped
//! once per composed tick while its own `dt_seconds()` is
//! `1 / ticks_per_second`. The two numbers therefore have to be the same
//! number, and both members below hold that relationship where it can
//! actually break:
//!
//! * `..._the_session_runs_on_the_composition_timeline` pins the rate the
//!   composed entry steps the session at against the rate the session
//!   declares.
//! * `..._a_route_follower_keeps_its_authored_speed_for_one_composition_second`
//!   is the behavioural half: a follower authored at `12 m/s` must travel
//!   exactly twelve metres across one real second of composed ticks. Under
//!   the measured divergence (a 64 Hz session stepped 120 times a second) it
//!   travelled 22.5 m — 1.875× its authored speed.
//!
//! #1278's entry lands on its own branch; neither member needs it, because
//! what they pin is the rate relationship any such entry steps at.
//!
//! Both are synthetic: they build a declared world-actor program in memory
//! and drive production code
//! ([`cs_app::world_actors::lower_world_actors`] →
//! [`cs_app::world_actors::WorldActorSession::launch`] →
//! [`cs_app::world_actors::WorldActorSession::step`] → the canonical pose).
//! No original data, no installation, no `verified_original` claim: the
//! original's world-actor cadence stays unmeasured, and the constant under
//! test is a designed contract of the reimplementation. The decision and the
//! readings behind it are recorded in
//! `docs/findings/2026-10-10-vs-m01-rt-host-cadence.md`.

use cs_app::mission_session::MissionContent;
use cs_app::mission_world_actors::SESSION_TICKS_PER_SECOND;
use cs_app::world_actors::{WorldActorSession, WorldActorTick, lower_world_actors};
use cs_content::objectives::ProgramActor;
use cs_content::world_actors::{
    DeclaredMotion, DeclaredRoute, DeclaredWorldActor, DeclaredWorldActorKind,
    DeclaredWorldActorParts, DeclaredWorldActorProgram, declared_synthetic_world_actors,
};
use cs_script::ir::ActorId;
use cs_script::runtime::SessionGeneration;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// The declared follower's authored cruise speed, in metres per second.
const SPEED_M_S: f64 = 12.0;

/// The declared program's single follower, and the runtime id the lowering
/// hands it (`lower_actor` maps `ProgramActor(n)` onto `ActorId(n)`).
const FOLLOWER: ProgramActor = ProgramActor(0);
const RUNTIME_FOLLOWER: ActorId = ActorId(0);

/// A designed value: the schema refuses an unknown, never a default.
fn designed<T>(value: T, claim: &'static str) -> Resolved<T> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new(claim).expect("a valid claim id")),
    ))
}

/// The one route follower under test: a straight 1 km line, no gates, no
/// pickups, no scripted schedule — only `speed_m_s` and the rate the program
/// declares, which is the rate production binding passes
/// (`bind_mission_world_actors(..., SESSION_TICKS_PER_SECOND)`).
///
/// The subject, origin and provenance come from the same synthetic fixture
/// F34's suite lowers, so this file authors motion only.
fn program() -> DeclaredWorldActorProgram {
    let base = declared_synthetic_world_actors();
    let follower = DeclaredWorldActor {
        actor: FOLLOWER,
        subject: base.subject().clone(),
        kind: DeclaredWorldActorKind::Road,
        faction: designed(
            ContentId::from_source(ContentKind::Faction, "enemy").expect("faction id"),
            "f34-world.faction-designed",
        ),
        objective: None,
        motion: DeclaredMotion::Route(DeclaredRoute {
            points: designed(
                vec![[0.0, 0.0, 0.0], [1_000.0, 0.0, 0.0]],
                "f34-world.route-designed",
            ),
            speed_m_s: designed(SPEED_M_S, "f34-world.speed-designed"),
            start_progress_m: designed(0.0, "f34-world.progress-designed"),
            gates: Vec::new(),
        }),
        sockets: Vec::new(),
    };
    DeclaredWorldActorProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        DeclaredWorldActorParts {
            // Exactly what the production binding declares: the session's own
            // cadence, one number shared by the program and its actors.
            ticks_per_second: designed(SESSION_TICKS_PER_SECOND, "f34-world.rate-designed"),
            actors: vec![follower],
            support: Vec::new(),
            pickups: Vec::new(),
            transitions: Vec::new(),
        },
    )
    .expect("a single route follower with a known rate assembles")
}

fn launch() -> WorldActorSession {
    WorldActorSession::launch(
        lower_world_actors(&program()).expect("the program lowers"),
        SessionGeneration(1),
    )
    .expect("the lowered program launches")
}

fn step_to(session: &mut WorldActorSession, to: u64) -> u64 {
    session
        .step(&WorldActorTick {
            to: Tick(to),
            commands: Vec::new(),
            probes: Vec::new(),
        })
        .expect("a legal tick")
        .tick
        .0
}

fn follower_x_m(session: &WorldActorSession) -> f64 {
    session
        .set()
        .pose(RUNTIME_FOLLOWER)
        .expect("the follower is registered")
        .position_m[0]
}

/// The pinned rate relationship: the composed entry steps the world-actor
/// session once per committed fixed tick, so the cadence the session
/// declares **is** the composition's timeline.
///
/// Fails whenever either side becomes a number of its own: a session
/// declared at 64 while the composition flies at 120 is exactly the
/// divergence this task closes.
#[test]
fn accept_vs_m01_rt_host_cadence_the_session_runs_on_the_composition_timeline() {
    let composed_ticks_per_second = MissionContent::tick_rate().ticks_per_second();
    assert_eq!(
        SESSION_TICKS_PER_SECOND, composed_ticks_per_second,
        "the composed entry hands the world-actor session its own committed tick, so the \
         session's declared cadence has to be the composition's fixed timeline: session {} \
         Hz, composition {} Hz",
        SESSION_TICKS_PER_SECOND, composed_ticks_per_second,
    );
}

/// The behavioural half of the same relationship, read off a real follower:
/// one full second of composed ticks advances a 12 m/s route follower by
/// exactly twelve metres, i.e. `speed * dt * ticks` with `dt` of the session
/// and `ticks` of the composition.
///
/// Under the measured divergence the same step travelled 22.5 m
/// (`12 * 120 / 64`), because `WorldActorSet::step` applied `1/64 s` on each
/// of 120 ticks a second.
#[test]
fn accept_vs_m01_rt_host_cadence_a_route_follower_keeps_its_authored_speed_for_one_composition_second()
 {
    let composed_ticks = u64::from(MissionContent::tick_rate().ticks_per_second());
    let mut session = launch();

    let before_m = follower_x_m(&session);
    let reached = step_to(&mut session, composed_ticks);
    let after_m = follower_x_m(&session);

    assert_eq!(
        reached, composed_ticks,
        "the session reaches exactly the tick the composed entry asked it for"
    );

    let travelled_m = after_m - before_m;
    assert!(
        (travelled_m - SPEED_M_S).abs() < 1e-9,
        "one second of composed ticks must advance a {SPEED_M_S} m/s follower by exactly \
         {SPEED_M_S} m; it advanced {travelled_m} m in {composed_ticks} ticks at \
         {SESSION_TICKS_PER_SECOND} ticks/s (a rate mismatch runs the follower at \
         {} times its authored speed)",
        travelled_m / SPEED_M_S,
    );
}
