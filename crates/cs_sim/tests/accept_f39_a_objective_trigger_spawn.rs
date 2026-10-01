//! F39-A acceptance: objective, trigger and spawn semantics (synthetic).
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-A`. Ordinary build/test only; nothing here is original data.

use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::damage::LifecycleKind;
use cs_sim::objectives::counters::{ActorCounters, CountKind};
use cs_sim::objectives::spawn::{Admission, Emission, EmissionLedger, IdempotencyKey, LedgerError};
use cs_sim::objectives::state::{ObjectiveCell, ObjectiveState};
use cs_sim::objectives::trigger::{
    CrossingKind, Movement, SweptTrigger, TriggerError, synthetic_small_volume,
};
use cs_types::Tick;

fn trigger() -> SweptTrigger {
    SweptTrigger::new(SymbolId(1), ActorId(7), synthetic_small_volume()).unwrap()
}

fn kinds(events: &[cs_sim::objectives::trigger::TriggerEvent]) -> Vec<CrossingKind> {
    events.iter().map(|e| e.kind).collect()
}

#[test]
fn accept_f39_a_high_speed_pass_through_emits_one_entry_and_one_exit() {
    let mut t = trigger();
    // 400 m in one tick across a 2 m cube: both endpoints are outside.
    let events = t
        .observe(
            Tick(1),
            Movement::Continuous {
                from_m: [-200.0, 0.0, 0.0],
                to_m: [200.0, 0.0, 0.0],
            },
        )
        .unwrap();
    assert_eq!(kinds(&events), [CrossingKind::Entry, CrossingKind::Exit]);
    assert!(!t.is_inside());
    // Continuing away emits nothing more.
    let again = t
        .observe(
            Tick(2),
            Movement::Continuous {
                from_m: [200.0, 0.0, 0.0],
                to_m: [600.0, 0.0, 0.0],
            },
        )
        .unwrap();
    assert!(again.is_empty());
}

#[test]
fn accept_f39_a_miss_and_dwell_and_exit() {
    let mut t = trigger();
    let miss = t
        .observe(
            Tick(1),
            Movement::Continuous {
                from_m: [-200.0, 50.0, 0.0],
                to_m: [200.0, 50.0, 0.0],
            },
        )
        .unwrap();
    assert!(miss.is_empty());
    let enter = t
        .observe(
            Tick(2),
            Movement::Continuous {
                from_m: [200.0, 50.0, 0.0],
                to_m: [0.0, 0.0, 0.0],
            },
        )
        .unwrap();
    assert_eq!(kinds(&enter), [CrossingKind::Entry]);
    let dwell = t
        .observe(
            Tick(3),
            Movement::Continuous {
                from_m: [0.0, 0.0, 0.0],
                to_m: [0.5, 0.0, 0.0],
            },
        )
        .unwrap();
    assert!(dwell.is_empty());
    let leave = t
        .observe(
            Tick(4),
            Movement::Continuous {
                from_m: [0.5, 0.0, 0.0],
                to_m: [50.0, 0.0, 0.0],
            },
        )
        .unwrap();
    assert_eq!(kinds(&leave), [CrossingKind::Exit]);
}

#[test]
fn accept_f39_a_teleport_does_not_collect_the_volume_between() {
    let mut t = trigger();
    let events = t
        .observe(
            Tick(1),
            Movement::Teleport {
                to_m: [200.0, 0.0, 0.0],
            },
        )
        .unwrap();
    assert!(events.is_empty());
    // A teleport that lands inside enters; one that leaves exits.
    let into = t
        .observe(Tick(2), Movement::Teleport { to_m: [0.0; 3] })
        .unwrap();
    assert_eq!(kinds(&into), [CrossingKind::Entry]);
    let out = t
        .observe(
            Tick(3),
            Movement::Teleport {
                to_m: [90.0, 0.0, 0.0],
            },
        )
        .unwrap();
    assert_eq!(kinds(&out), [CrossingKind::Exit]);
}

#[test]
fn accept_f39_a_bad_input_is_refused_without_changing_state() {
    let mut t = trigger();
    let nan = t.observe(
        Tick(1),
        Movement::Continuous {
            from_m: [f64::NAN, 0.0, 0.0],
            to_m: [0.0; 3],
        },
    );
    assert_eq!(nan, Err(TriggerError::NonFinite));
    t.observe(Tick(2), Movement::Teleport { to_m: [0.0; 3] })
        .unwrap();
    let replay = t.observe(Tick(2), Movement::Teleport { to_m: [9.0; 3] });
    assert_eq!(
        replay,
        Err(TriggerError::NotAdvancing {
            last: Tick(2),
            given: Tick(2)
        })
    );
    assert!(t.is_inside());
    let inverted = cs_sim::objectives::trigger::Volume::Aabb {
        min_m: [1.0; 3],
        max_m: [-1.0; 3],
    };
    assert!(SweptTrigger::new(SymbolId(1), ActorId(1), inverted).is_err());
}

#[test]
fn accept_f39_a_counters_keep_categories_apart() {
    let mut c = ActorCounters::default();
    assert!(c.record(CountKind::Escaped, ActorId(1)));
    assert!(!c.record(CountKind::Escaped, ActorId(1)));
    c.record(CountKind::Despawned, ActorId(2));
    assert_eq!(c.count(CountKind::Escaped), 1);
    assert_eq!(c.count(CountKind::Destroyed), 0);
    assert!(!c.contains(CountKind::Destroyed, ActorId(2)));
    assert_eq!(
        CountKind::from_lifecycle(LifecycleKind::Destroyed),
        Some(CountKind::Destroyed)
    );
    assert_eq!(
        CountKind::from_lifecycle(LifecycleKind::OwnershipCaptured),
        Some(CountKind::Captured)
    );
    assert_eq!(CountKind::from_lifecycle(LifecycleKind::PilotBailout), None);
}

#[test]
fn accept_f39_a_repeated_cue_does_not_spawn_or_speak_twice_and_retry_is_clean() {
    let s1 = SessionGeneration(1);
    let mut ledger = EmissionLedger::new(s1);
    let key = || IdempotencyKey("wave-2".into());
    let wave = Emission::Spawn(vec![ActorId(10), ActorId(11)]);
    assert_eq!(
        ledger.admit(s1, key(), wave.clone()),
        Ok(Admission::Admitted)
    );
    assert_eq!(
        ledger.admit(s1, key(), Emission::Spawn(vec![ActorId(12)])),
        Ok(Admission::Repeated(wave))
    );
    ledger
        .admit(s1, IdempotencyKey("radio-1".into()), Emission::Cue)
        .unwrap();
    assert_eq!(ledger.len(), 2);

    // A retry is a new session: old keys are gone, and old-session callers
    // are refused instead of leaking into the new session.
    let s2 = SessionGeneration(2);
    let mut retry = EmissionLedger::new(s2);
    assert!(retry.is_empty());
    assert_eq!(
        retry.admit(s1, key(), Emission::Cue),
        Err(LedgerError::StaleSession {
            ledger: s2,
            given: s1
        })
    );
    assert_eq!(
        retry.admit(s2, key(), Emission::Cue),
        Ok(Admission::Admitted)
    );
}

#[test]
fn accept_f39_a_objective_states_and_legal_transitions() {
    use ObjectiveState::*;
    let mut o = ObjectiveCell::new(Hidden);
    assert!(!o.state().is_visible());
    assert!(o.transition(Succeeded).is_err());
    assert_eq!(o.state(), Hidden);
    o.transition(Active).unwrap();
    o.transition(Succeeded).unwrap();
    assert!(o.state().is_final());
    assert!(o.transition(Failed).is_err());
    assert!(ObjectiveCell::new(Optional).state().is_visible());
    assert!(Optional.can_become(Succeeded));
    assert!(Pending.can_become(Superseded));
}
