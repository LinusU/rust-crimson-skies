//! F39-E-ACTORID-SPACE: the runtime half of the declared-vs-spawned actor-id
//! contract — spawned wave instances take `ActorId`s from
//! [`SPAWNED_ACTOR_BASE`] upward, never the range placed actors live in.
//!
//! Task test prefix: `accept_f39_e_actorid_`.
//!
//! The contract's decision record is
//! `docs/findings/2026-10-13-f39-e-actorid-space.md`; the declared half — the
//! schema refusals around [`cs_content::objectives::ProgramActorRef`] — is
//! covered in `cs_content`'s `accept_f39_e_actorid_` unit tests, and the
//! session-level conflation case in `cs_app`'s. The boundary is **designed**:
//! no original spawn-id space is measured.

use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::counters::CountKind;
use cs_sim::objectives::runtime::{
    CountCondition, CountReaction, ObjectiveEventKind, ObjectiveRuntime, RuntimeLimits,
    SPAWNED_ACTOR_BASE, TickInput,
};
use cs_sim::objectives::spawn::IdempotencyKey;
use cs_sim::objectives::timer::{MissionTimer, TimerAction, TimerStart};
use cs_sim::time::ClockPolicy;
use cs_types::Tick;

/// The actor id the runtime hands the k-th admitted wave instance.
const fn spawned(k: u32) -> ActorId {
    ActorId(SPAWNED_ACTOR_BASE + k - 1)
}

/// A placed actor wearing a small record-index id — the shape the measured
/// importer (`cs_app::mission_world_actors`) mints for every pre-placed actor.
/// Under the pre-contract allocator, wave 1 took exactly this id.
const PLACED_INDEX: ActorId = ActorId(1);

const SESSION: SessionGeneration = SessionGeneration(1);
const WAVE: SymbolId = SymbolId(20);
const SECOND_WAVE: SymbolId = SymbolId(21);
const RAIDERS: SymbolId = SymbolId(40);
const RAIDERS_LOST: SymbolId = SymbolId(10);

fn runtime() -> ObjectiveRuntime {
    ObjectiveRuntime::new(
        SESSION,
        cs_sim::objectives::terminal::TerminalPrecedence::SyntheticConservative,
        RuntimeLimits::default(),
    )
}

fn wave_timer(symbol: SymbolId, key: &str, count: u32) -> MissionTimer {
    MissionTimer::new(
        symbol,
        ClockPolicy::authoritative_gameplay(),
        TimerStart::OnArm,
        1,
        TimerAction::SpawnGroup {
            key: IdempotencyKey(key.into()),
            group: RAIDERS,
            count,
        },
    )
    .unwrap()
}

fn input(tick: u64) -> TickInput<'static> {
    TickInput::at(Tick(tick))
}

/// The band is enforced by allocation, not by a check a caller could skip:
/// every admitted wave takes ids at `SPAWNED_ACTOR_BASE` and upward in
/// admission order, across admissions and across timers.
#[test]
fn accept_f39_e_actorid_every_admitted_instance_takes_a_band_id() {
    let mut runtime = runtime();
    runtime.add_timer(wave_timer(WAVE, "wave-1", 2)).unwrap();
    runtime
        .add_timer(wave_timer(SECOND_WAVE, "wave-2", 2))
        .unwrap();

    let first = runtime
        .step(&TickInput {
            timer_requests: &[cs_sim::objectives::timer::TimerRequest::Arm(WAVE)],
            committed_ticks: 1,
            ..input(1)
        })
        .unwrap();
    let admitted = first
        .events
        .iter()
        .find_map(|event| match &event.kind {
            ObjectiveEventKind::SpawnAdmitted { instances, .. } => Some(instances.clone()),
            _ => None,
        })
        .expect("the armed wave is admitted");
    assert_eq!(admitted, vec![spawned(1), spawned(2)]);

    let second = runtime
        .step(&TickInput {
            timer_requests: &[cs_sim::objectives::timer::TimerRequest::Arm(SECOND_WAVE)],
            committed_ticks: 1,
            ..input(2)
        })
        .unwrap();
    let admitted = second
        .events
        .iter()
        .find_map(|event| match &event.kind {
            ObjectiveEventKind::SpawnAdmitted { instances, .. } => Some(instances.clone()),
            _ => None,
        })
        .expect("the second armed wave is admitted");
    assert_eq!(admitted, vec![spawned(3), spawned(4)]);

    assert!(
        admitted.iter().all(|actor| actor.0 >= SPAWNED_ACTOR_BASE),
        "every spawned id sits inside the band: {admitted:?}"
    );
}

/// The collision the contract exists to prevent, exercised against the fact
/// stream itself: a placed actor whose record-index id is the same small
/// number a pre-contract allocator gave wave 1 must never be counted as the
/// wave instance — lifecycle facts keyed by `ActorId(1)` do not satisfy a
/// roster of spawned ids, because the wave's members live in the band now.
#[test]
fn accept_f39_e_actorid_a_placed_actor_is_never_counted_as_a_wave_instance() {
    let mut runtime = runtime();
    runtime.add_timer(wave_timer(WAVE, "wave-1", 2)).unwrap();
    runtime
        .add_condition(
            CountCondition::new(
                RAIDERS_LOST,
                CountKind::Destroyed,
                [spawned(1), spawned(2)],
                2,
            )
            .unwrap(),
            CountReaction::ReportOnly,
        )
        .unwrap();
    runtime
        .step(&TickInput {
            timer_requests: &[cs_sim::objectives::timer::TimerRequest::Arm(WAVE)],
            committed_ticks: 1,
            ..input(1)
        })
        .unwrap();
    assert_eq!(
        runtime.spawned_instances(&IdempotencyKey("wave-1".into())),
        Some(&[spawned(1), spawned(2)][..]),
        "wave 1's members are band ids"
    );

    // A placed actor destroyed under the small index the old allocator gave
    // wave 1: the roster must not see it.
    runtime
        .step(&TickInput {
            lifecycles: &[(PLACED_INDEX, LifecycleKind::Destroyed)],
            ..input(2)
        })
        .unwrap();
    assert!(
        !runtime.condition_met(RAIDERS_LOST),
        "a placed actor's destruction is not a wave instance's"
    );
    assert!(runtime.is_counted(CountKind::Destroyed, PLACED_INDEX));
    assert!(!runtime.is_counted(CountKind::Destroyed, spawned(1)));

    // The wave instances themselves, under their band ids, do satisfy it.
    runtime
        .step(&TickInput {
            lifecycles: &[
                (spawned(1), LifecycleKind::Destroyed),
                (spawned(2), LifecycleKind::Destroyed),
            ],
            ..input(3)
        })
        .unwrap();
    assert!(runtime.condition_met(RAIDERS_LOST));
}

/// Each session generation owns the whole band itself: a retry's first wave
/// takes `SPAWNED_ACTOR_BASE` again rather than continuing where the old
/// session's allocator stopped, and the two sessions' emissions can never be
/// told apart by id — only by generation, as the contract intends.
#[test]
fn accept_f39_e_actorid_each_session_starts_the_band_at_its_base() {
    for session in [SessionGeneration(2), SessionGeneration(3)] {
        let mut runtime = ObjectiveRuntime::new(
            session,
            cs_sim::objectives::terminal::TerminalPrecedence::SyntheticConservative,
            RuntimeLimits::default(),
        );
        runtime.add_timer(wave_timer(WAVE, "wave-1", 1)).unwrap();
        runtime
            .step(&TickInput {
                timer_requests: &[cs_sim::objectives::timer::TimerRequest::Arm(WAVE)],
                committed_ticks: 1,
                ..input(1)
            })
            .unwrap();
        assert_eq!(
            runtime.spawned_instances(&IdempotencyKey("wave-1".into())),
            Some(&[spawned(1)][..]),
            "session {session:?} restarts the band at its base"
        );
    }
}
