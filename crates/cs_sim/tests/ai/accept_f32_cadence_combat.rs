//! Acceptance scenario F32-CADENCE: the per-actor fire-discipline schedule
//! across ticks, its deferral and expiry, its refusal of a foreign session's
//! actor, and its refusal of a tick that goes backwards.
//!
//! Task: #555 (`F32-CADENCE`). Spec:
//! `specs/F32-ai-combat-formations-aces-and-difficulty.md`. Task test prefix:
//! `accept_f32_cadence_`.
//!
//! Every test drives `CombatRuntime::step` and the session-owned
//! `FireDiscipline` it advances. The schedule counts in simulation ticks
//! (`cs_types::Tick`) and carries no rate of its own. Every value here is
//! newly authored synthetic fixture data.

use cs_sim::ai::combat::{
    CandidateView, CombatError, CombatRole, CombatRuntime, CombatStep, CombatantRequest,
    DifficultyTier, FireHoldReason, RoleAssignment, SYNTHETIC_SESSION, synthetic_actor,
    synthetic_arsenal, synthetic_candidate, synthetic_combat_runtime,
};
use cs_sim::damage::ActorId;
use cs_sim::targeting::Allegiance;
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

/// The Standard tier's fighter cadence the fixtures declare.
const FIGHTER_CADENCE: u64 = 24;

fn hostile() -> Vec<CandidateView> {
    vec![synthetic_candidate(
        40,
        [300.0, 0.0, 0.0],
        Some(Allegiance::Hostile),
        true,
    )]
}

fn try_step(
    runtime: &mut CombatRuntime,
    observer: ActorId,
    now: u64,
) -> Result<CombatStep, CombatError> {
    runtime.step(&CombatantRequest {
        observer,
        now: Tick(now),
        observer_position: WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
        assignment: &RoleAssignment::new(observer, CombatRole::FighterAttack),
        formation: None,
        protected_alive: None,
        candidates: &hostile(),
        arsenal: Some(&synthetic_arsenal()),
        ace: None,
        tier: DifficultyTier::Standard,
    })
}

fn is_firing(step: &CombatStep) -> bool {
    step.trace().firing.as_ref().expect("a target").is_firing()
}

#[test]
fn accept_f32_cadence_defers_a_shot_inside_the_window_and_expires_on_the_boundary() {
    let mut runtime = synthetic_combat_runtime();
    let actor = synthetic_actor(1);

    assert!(is_firing(&try_step(&mut runtime, actor, 100).unwrap()));
    assert_eq!(
        runtime.fire_discipline().ready_at(actor),
        Tick(100 + FIGHTER_CADENCE)
    );

    let held = try_step(&mut runtime, actor, 100 + FIGHTER_CADENCE - 1).unwrap();
    assert!(!is_firing(&held));
    assert_eq!(
        held.trace().firing.as_ref().unwrap().hold(),
        Some(FireHoldReason::DisciplineHold { remaining_ticks: 1 }),
        "one tick before the boundary the cadence still holds"
    );

    assert!(
        is_firing(&try_step(&mut runtime, actor, 100 + FIGHTER_CADENCE).unwrap()),
        "at ready_at the cadence has expired"
    );
}

#[test]
fn accept_f32_cadence_refuses_an_actor_of_another_session_generation() {
    let mut runtime = synthetic_combat_runtime();
    let ours = synthetic_actor(1);
    assert!(is_firing(&try_step(&mut runtime, ours, 100).unwrap()));

    // The same serial in another generation is a different actor: refused
    // by name, and it neither inherits nor writes any schedule.
    let foreign = ActorId {
        session: SessionId::new(SYNTHETIC_SESSION + 1).expect("nonzero"),
        serial: ours.serial,
    };
    let err = try_step(&mut runtime, foreign, 101).expect_err("refused");
    assert!(matches!(err, CombatError::ForeignSession { .. }));
    assert_eq!(runtime.fire_discipline().ready_at(foreign), Tick(0));
    assert_eq!(runtime.fire_discipline().last_step(foreign), None);
    assert_eq!(runtime.fire_discipline().len(), 1);

    // A fresh runtime (a new generation) starts with no schedule at all.
    let fresh = synthetic_combat_runtime();
    assert!(fresh.fire_discipline().is_empty());
    assert_eq!(fresh.fire_discipline().ready_at(ours), Tick(0));
}

#[test]
fn accept_f32_cadence_refuses_a_tick_that_goes_backwards() {
    let mut runtime = synthetic_combat_runtime();
    let actor = synthetic_actor(1);
    assert!(is_firing(&try_step(&mut runtime, actor, 100).unwrap()));
    let held = try_step(&mut runtime, actor, 110).unwrap();
    assert!(!is_firing(&held));

    // Stepping back to before the shot would read the actor as ready.
    let err = try_step(&mut runtime, actor, 50).expect_err("refused");
    assert_eq!(
        err,
        CombatError::TickWentBackwards {
            actor,
            last: Tick(110),
            now: Tick(50),
        }
    );
    assert_eq!(
        runtime.fire_discipline().ready_at(actor),
        Tick(100 + FIGHTER_CADENCE),
        "the refusal left the schedule untouched"
    );
    assert_eq!(runtime.fire_discipline().last_step(actor), Some(Tick(110)));

    // The same tick again is not backwards, and another actor's clock is
    // its own.
    assert!(try_step(&mut runtime, actor, 110).is_ok());
    assert!(is_firing(
        &try_step(&mut runtime, synthetic_actor(2), 50).unwrap()
    ));
}
