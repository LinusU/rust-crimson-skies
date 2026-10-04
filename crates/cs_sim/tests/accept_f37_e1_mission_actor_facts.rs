//! F37-E1 acceptance: `MissionFacts::actors` populated from the
//! simulation's authoritative actor state, so `Condition::ActorIs` can
//! observe the contract's actor-state distinctions (task #612).
//!
//! Spec: `specs/F37-mission-ir-and-deterministic-runtime-core.md` and
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
//! contract `docs/contracts/SCRIPT-MISSION.md`. The `LifecycleKind` →
//! `ActorState` mapping asserted below is **measured or refused by name**:
//! F39-E4 measured `Destroyed`, `OwnershipCaptured` and `Despawned` as the
//! only lifecycle transitions with a counted category, and F39-E7 measured
//! that the original writes a detach as an event, never as a category.
//! Nothing here is original data; the suite is identical with and without
//! `CS_GAME_DIR`.

use std::collections::{BTreeMap, BTreeSet};

use cs_script::ir::{
    Action, ActorId, ActorState, Condition, IR_VERSION, MissionProgram, Objective, SymbolId,
};
use cs_script::runtime::{EventKind, SessionGeneration};
use cs_sim::damage::LifecycleKind;
use cs_sim::mission::{
    ActorFactInput, ActorStateProducer, DETACHED_IS_AN_EVENT, FACT_SNAPSHOT_VERSION, FactEffect,
    FactError, FactObservation, FactRecord, FactRestoreError, MissionSession,
    NO_TRANSITION_WRITES_THIS_STATE, ObservedError, PILOT_BAILOUT_WRITES_NO_STATE,
    SessionRestoreError, actor_state_of, actor_state_producer, lifecycle_fact_effect,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// The session generation every fixture here runs under.
const SESSION: SessionGeneration = SessionGeneration(7);
/// A mission actor.
const RAIDER: ActorId = ActorId(11);
/// A second mission actor.
const CONVOY: ActorId = ActorId(21);
/// An actor admitted mid-mission.
const NEWGUY: ActorId = ActorId(31);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid content id grammar")
}

fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

fn reward_id(key: &str) -> ContentId {
    cid(ContentKind::Blueprint, key)
}

fn objective(id: u32, condition: Condition, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("e1-obj-{id}")),
        condition,
        actions,
        span: None,
    }
}

fn program(objectives: Vec<Objective>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-e1"),
        variables: vec![],
        objectives,
    }
}

fn session(objectives: Vec<Objective>, rewards: Vec<ContentId>) -> MissionSession {
    MissionSession::launch(program(objectives), SESSION, rewards).unwrap()
}

fn input<'a>(
    registered: &'a [ActorId],
    lifecycles: &'a [(ActorId, LifecycleKind)],
) -> ActorFactInput<'a> {
    ActorFactInput {
        registered,
        lifecycles,
    }
}

/// The motivating defect: `Condition::ActorIs` only fires because the
/// destroyed transition wrote `Dead`. If the write stops happening the row
/// stays `Alive` and the objective never fires.
#[test]
fn accept_f37_e1_actor_is_observes_the_destroyed_transition() {
    let mut s = session(
        vec![objective(
            1,
            Condition::ActorIs {
                actor: RAIDER,
                state: ActorState::Dead,
            },
            vec![reward("r-kill")],
        )],
        vec![reward_id("r-kill")],
    );
    s.register_actor(RAIDER).unwrap();

    // Alive in play: the dead read does not fire on an actor's admission.
    let quiet = s.advance_observed(&input(&[], &[]), Tick(1)).unwrap();
    assert!(
        quiet.mission.events.is_empty(),
        "{:?}",
        quiet.mission.events
    );

    let tick = s
        .advance_observed(&input(&[], &[(RAIDER, LifecycleKind::Destroyed)]), Tick(2))
        .unwrap();
    // The transition wrote `Dead` to the table and to the facts the
    // evaluator read.
    assert_eq!(s.actor_facts().state(&RAIDER), Some(ActorState::Dead));
    assert_eq!(
        s.actor_facts().facts().actors.get(&RAIDER),
        Some(&ActorState::Dead)
    );
    // And the objective's declared reward reached the authoritative host.
    assert_eq!(tick.mission.host.rewards_granted, [reward_id("r-kill")]);
    assert_eq!(s.host().granted_rewards(), 1);
    // The observation reports exactly what the record did.
    assert_eq!(
        tick.facts,
        [FactObservation {
            actor: RAIDER,
            recorded: FactRecord::Lifecycle(LifecycleKind::Destroyed),
            effect: FactEffect::Writes(ActorState::Dead),
            state: Some(ActorState::Dead),
        }]
    );
}

/// Every `LifecycleKind` answers what it does to a fact row — measured or
/// refused by name — and every `ActorState` names its producer, the way
/// F39-E4's gate decides the counted categories. The two directions cannot
/// drift: one is derived from the other.
#[test]
fn accept_f37_e1_every_transition_and_state_is_measured_or_refused_by_name() {
    for kind in LifecycleKind::ALL {
        let effect = lifecycle_fact_effect(*kind);
        let written = actor_state_of(*kind);
        match kind {
            LifecycleKind::Destroyed => {
                assert_eq!(effect, FactEffect::Writes(ActorState::Dead));
                assert_eq!(written, Some(ActorState::Dead));
            }
            LifecycleKind::OwnershipCaptured => {
                assert_eq!(effect, FactEffect::Writes(ActorState::Captured));
                assert_eq!(written, Some(ActorState::Captured));
            }
            LifecycleKind::Despawned => {
                assert_eq!(effect, FactEffect::Writes(ActorState::Despawned));
                assert_eq!(written, Some(ActorState::Despawned));
            }
            LifecycleKind::PilotBailout => {
                assert_eq!(
                    effect,
                    FactEffect::RecordedUnwritten(PILOT_BAILOUT_WRITES_NO_STATE)
                );
                assert_eq!(written, None);
            }
            LifecycleKind::MissionRemoved => {
                assert_eq!(effect, FactEffect::RemovesFromAccounting);
                assert_eq!(written, None);
            }
        }
    }

    for state in [
        ActorState::Alive,
        ActorState::Disabled,
        ActorState::Dead,
        ActorState::Captured,
        ActorState::Escaped,
        ActorState::Detached,
        ActorState::Despawned,
    ] {
        let producer = actor_state_producer(state);
        match state {
            ActorState::Alive => assert_eq!(producer, ActorStateProducer::Registration),
            ActorState::Dead => assert_eq!(
                producer,
                ActorStateProducer::Lifecycle(LifecycleKind::Destroyed)
            ),
            ActorState::Captured => assert_eq!(
                producer,
                ActorStateProducer::Lifecycle(LifecycleKind::OwnershipCaptured)
            ),
            ActorState::Despawned => assert_eq!(
                producer,
                ActorStateProducer::Lifecycle(LifecycleKind::Despawned)
            ),
            ActorState::Disabled | ActorState::Escaped => assert_eq!(
                producer,
                ActorStateProducer::Unproduced(NO_TRANSITION_WRITES_THIS_STATE)
            ),
            ActorState::Detached => assert_eq!(
                producer,
                ActorStateProducer::Unproduced(DETACHED_IS_AN_EVENT)
            ),
        }
    }
}

/// `MissionRemoved` is not a death and not a despawn: the actor leaves the
/// facts entirely — absence is the event's own meaning — and its record is
/// closed, so a cinematic removal can never be read as a kill.
#[test]
fn accept_f37_e1_mission_removed_actor_leaves_the_facts() {
    let mut s = session(
        vec![
            objective(
                1,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Alive,
                },
                vec![reward("r-alive")],
            ),
            objective(
                2,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Despawned,
                },
                vec![reward("r-gone")],
            ),
            objective(
                3,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Dead,
                },
                vec![reward("r-dead")],
            ),
        ],
        vec![
            reward_id("r-alive"),
            reward_id("r-gone"),
            reward_id("r-dead"),
        ],
    );
    s.register_actor(RAIDER).unwrap();

    // The admission wrote `Alive`; the condition reads it.
    let first = s.advance_observed(&input(&[], &[]), Tick(1)).unwrap();
    assert_eq!(first.mission.host.rewards_granted, [reward_id("r-alive")]);
    assert_eq!(s.actor_facts().state(&RAIDER), Some(ActorState::Alive));

    // Removal: out of the map, not into another state.
    let removed = s
        .advance_observed(
            &input(&[], &[(RAIDER, LifecycleKind::MissionRemoved)]),
            Tick(2),
        )
        .unwrap();
    assert_eq!(removed.facts[0].effect, FactEffect::RemovesFromAccounting);
    assert_eq!(removed.facts[0].state, None);
    assert_eq!(s.actor_facts().state(&RAIDER), None);
    assert!(!s.actor_facts().facts().actors.contains_key(&RAIDER));
    assert!(
        removed.mission.host.rewards_granted.is_empty(),
        "a removal fired a despawn/dead read: {:?}",
        removed.mission.host.rewards_granted
    );
    // The record keeps the transition — closed — even though the actor is
    // absent from the facts.
    assert_eq!(
        s.actor_facts().lifecycle(&RAIDER),
        Some(&BTreeSet::from([LifecycleKind::MissionRemoved]))
    );

    // The record is closed: a late transition is refused, never folded.
    let refused = s.advance_observed(&input(&[], &[(RAIDER, LifecycleKind::Destroyed)]), Tick(3));
    assert!(matches!(
        refused,
        Err(ObservedError::Facts(FactError::ActorClosed {
            actor: RAIDER,
            terminal: LifecycleKind::MissionRemoved,
        }))
    ));
}

/// `PilotBailout` is recorded — the ledger knows it happened — but writes
/// no `ActorState`: a bailout is not a death and the refusal is named, not
/// defaulted.
#[test]
fn accept_f37_e1_pilot_bailout_records_but_writes_no_state() {
    let mut s = session(
        vec![
            objective(
                1,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Alive,
                },
                vec![reward("r-still")],
            ),
            objective(
                2,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Dead,
                },
                vec![reward("r-dead")],
            ),
        ],
        vec![reward_id("r-still"), reward_id("r-dead")],
    );
    s.register_actor(RAIDER).unwrap();

    let tick = s
        .advance_observed(
            &input(&[], &[(RAIDER, LifecycleKind::PilotBailout)]),
            Tick(1),
        )
        .unwrap();
    assert_eq!(
        tick.facts,
        [FactObservation {
            actor: RAIDER,
            recorded: FactRecord::Lifecycle(LifecycleKind::PilotBailout),
            effect: FactEffect::RecordedUnwritten(PILOT_BAILOUT_WRITES_NO_STATE),
            state: Some(ActorState::Alive),
        }]
    );
    // Still in play: the alive read fires and the dead read does not.
    assert_eq!(tick.mission.host.rewards_granted, [reward_id("r-still")]);
    assert!(
        s.actor_facts()
            .lifecycle(&RAIDER)
            .unwrap()
            .contains(&LifecycleKind::PilotBailout)
    );
}

/// A row holding several recorded transitions resolves them in the
/// declared precedence — a terminal transition wins — and the closed
/// record refuses everything after.
#[test]
fn accept_f37_e1_captured_then_despawned_resolves_in_declared_precedence() {
    let mut s = session(
        vec![
            objective(
                1,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Captured,
                },
                vec![reward("r-taken")],
            ),
            objective(
                2,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Despawned,
                },
                vec![reward("r-gone")],
            ),
            objective(
                3,
                Condition::ActorIs {
                    actor: CONVOY,
                    state: ActorState::Despawned,
                },
                vec![reward("r-convoy")],
            ),
            objective(
                4,
                Condition::ActorIs {
                    actor: CONVOY,
                    state: ActorState::Dead,
                },
                vec![reward("r-convoy-dead")],
            ),
        ],
        vec![
            reward_id("r-taken"),
            reward_id("r-gone"),
            reward_id("r-convoy"),
            reward_id("r-convoy-dead"),
        ],
    );
    s.register_actor(RAIDER).unwrap();
    s.register_actor(CONVOY).unwrap();

    let captured = s
        .advance_observed(
            &input(&[], &[(RAIDER, LifecycleKind::OwnershipCaptured)]),
            Tick(1),
        )
        .unwrap();
    assert_eq!(
        captured.mission.host.rewards_granted,
        [reward_id("r-taken")]
    );
    assert_eq!(s.actor_facts().state(&RAIDER), Some(ActorState::Captured));

    // The despawn wins over the earlier capture — the declared precedence.
    let gone = s
        .advance_observed(&input(&[], &[(RAIDER, LifecycleKind::Despawned)]), Tick(2))
        .unwrap();
    assert_eq!(gone.mission.host.rewards_granted, [reward_id("r-gone")]);
    assert_eq!(s.actor_facts().state(&RAIDER), Some(ActorState::Despawned));

    // A destroyed-then-despawned pair inside one input resolves to the
    // terminal kind: the despawn is the current state, not the earlier
    // death.
    let convoy = s
        .advance_observed(
            &input(
                &[],
                &[
                    (CONVOY, LifecycleKind::Destroyed),
                    (CONVOY, LifecycleKind::Despawned),
                ],
            ),
            Tick(3),
        )
        .unwrap();
    assert_eq!(s.actor_facts().state(&CONVOY), Some(ActorState::Despawned));
    assert_eq!(convoy.mission.host.rewards_granted, [reward_id("r-convoy")]);

    // Both records are closed: nothing may follow a terminal transition.
    for (actor, terminal) in [
        (RAIDER, LifecycleKind::Despawned),
        (CONVOY, LifecycleKind::Despawned),
    ] {
        let refused =
            s.advance_observed(&input(&[], &[(actor, LifecycleKind::Destroyed)]), Tick(4));
        assert!(matches!(
            refused,
            Err(ObservedError::Facts(FactError::ActorClosed {
                actor: a,
                terminal: t,
            })) if a == actor && t == terminal
        ));
    }
}

/// Every defect a producer can make is refused by name, and a refused
/// input folds nothing — the valid transition in the same input is not
/// recorded either.
#[test]
fn accept_f37_e1_producer_defects_refuse_the_whole_input() {
    let mut s = session(vec![objective(1, Condition::Const(false), vec![])], vec![]);

    // An actor the table never registered.
    let refused = s.advance_observed(&input(&[], &[(RAIDER, LifecycleKind::Destroyed)]), Tick(1));
    assert!(matches!(
        refused,
        Err(ObservedError::Facts(FactError::UnknownActor {
            actor: RAIDER
        }))
    ));
    assert!(!s.actor_facts().is_registered(&RAIDER));

    s.register_actor(RAIDER).unwrap();
    s.register_actor(CONVOY).unwrap();

    // One valid transition plus an unknown actor: the valid one folds
    // nothing either — the input is atomic.
    let refused = s.advance_observed(
        &input(
            &[],
            &[
                (RAIDER, LifecycleKind::Destroyed),
                (NEWGUY, LifecycleKind::Destroyed),
            ],
        ),
        Tick(1),
    );
    assert!(matches!(
        refused,
        Err(ObservedError::Facts(FactError::UnknownActor {
            actor: NEWGUY
        }))
    ));
    assert_eq!(
        s.actor_facts().state(&RAIDER),
        Some(ActorState::Alive),
        "a refused input folded the valid transition"
    );

    // A kind offered twice in one input is a replay.
    let refused = s.advance_observed(
        &input(
            &[],
            &[
                (RAIDER, LifecycleKind::Destroyed),
                (RAIDER, LifecycleKind::Destroyed),
            ],
        ),
        Tick(1),
    );
    assert!(matches!(
        refused,
        Err(ObservedError::Facts(FactError::DuplicateLifecycle {
            actor: RAIDER,
            kind: LifecycleKind::Destroyed,
        }))
    ));

    // A terminal offered inside the input closes what follows it.
    let refused = s.advance_observed(
        &input(
            &[],
            &[
                (RAIDER, LifecycleKind::Despawned),
                (RAIDER, LifecycleKind::Destroyed),
            ],
        ),
        Tick(1),
    );
    assert!(matches!(
        refused,
        Err(ObservedError::Facts(FactError::ActorClosed {
            actor: RAIDER,
            terminal: LifecycleKind::Despawned,
        }))
    ));

    // Registering twice — against the table or inside the slice — is a
    // defect.
    assert!(matches!(
        s.register_actor(RAIDER),
        Err(FactError::DuplicateActor { actor: RAIDER })
    ));
    let refused = s.advance_observed(&input(&[RAIDER], &[]), Tick(1));
    assert!(matches!(
        refused,
        Err(ObservedError::Facts(FactError::DuplicateActor {
            actor: RAIDER
        }))
    ));
    let refused = s.advance_observed(&input(&[NEWGUY, NEWGUY], &[]), Tick(1));
    assert!(matches!(
        refused,
        Err(ObservedError::Facts(FactError::DuplicateActor {
            actor: NEWGUY
        }))
    ));

    // The corrected input runs the same tick — a refusal never consumed it.
    let ok = s
        .advance_observed(
            &input(&[NEWGUY], &[(RAIDER, LifecycleKind::Destroyed)]),
            Tick(1),
        )
        .unwrap();
    assert_eq!(ok.mission.tick, Tick(1));
    assert_eq!(ok.facts.len(), 2, "{:?}", ok.facts);
    assert_eq!(s.actor_facts().state(&RAIDER), Some(ActorState::Dead));
    assert_eq!(s.actor_facts().state(&NEWGUY), Some(ActorState::Alive));
    assert_eq!(s.actor_facts().state(&CONVOY), Some(ActorState::Alive));
}

/// The facts the evaluator reads are the table's whole map each tick —
/// cumulative and authoritative, never accumulated by the caller — and an
/// actor admitted and transitioned in the same input fires on that tick.
#[test]
fn accept_f37_e1_facts_populate_every_tick_from_the_authoritative_record() {
    let mut s = session(
        vec![
            objective(
                1,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Alive,
                },
                vec![reward("r-raider")],
            ),
            objective(
                2,
                Condition::ActorIs {
                    actor: NEWGUY,
                    state: ActorState::Dead,
                },
                vec![reward("r-newguy")],
            ),
        ],
        vec![reward_id("r-raider"), reward_id("r-newguy")],
    );

    // Both actors enter play on tick 1; one dies the same tick.
    let first = s
        .advance_observed(
            &input(&[RAIDER, NEWGUY], &[(NEWGUY, LifecycleKind::Destroyed)]),
            Tick(1),
        )
        .unwrap();
    assert_eq!(
        first.mission.host.rewards_granted,
        [reward_id("r-raider"), reward_id("r-newguy")]
    );
    assert_eq!(
        s.actor_facts().facts().actors,
        BTreeMap::from([(RAIDER, ActorState::Alive), (NEWGUY, ActorState::Dead),])
    );

    // Tick 2 carries nothing; the recorded state still feeds the map.
    let second = s.advance_observed(&input(&[], &[]), Tick(2)).unwrap();
    assert!(second.mission.host.rewards_granted.is_empty());
    assert_eq!(s.actor_facts().facts().actors.len(), 2);
}

/// `Disabled`, `Escaped` and `Detached` have no producer: a program that
/// asks for them is answered by the record's silence, and `Detached` is
/// never the meaning of `MissionRemoved`.
#[test]
fn accept_f37_e1_unproduced_states_are_silence_not_guesses() {
    let mut s = session(
        vec![
            objective(
                1,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Disabled,
                },
                vec![reward("r-disabled")],
            ),
            objective(
                2,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Escaped,
                },
                vec![reward("r-escaped")],
            ),
            objective(
                3,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Detached,
                },
                vec![reward("r-detached")],
            ),
        ],
        vec![
            reward_id("r-disabled"),
            reward_id("r-escaped"),
            reward_id("r-detached"),
        ],
    );
    s.register_actor(RAIDER).unwrap();

    // Every non-terminal transition the engine records, then the removal:
    // none writes any of the three states.
    for (tick, kind) in [
        LifecycleKind::PilotBailout,
        LifecycleKind::OwnershipCaptured,
        LifecycleKind::Destroyed,
        LifecycleKind::MissionRemoved,
    ]
    .into_iter()
    .enumerate()
    {
        s.advance_observed(&input(&[], &[(RAIDER, kind)]), Tick(tick as u64 + 1))
            .unwrap();
    }
    assert_eq!(
        s.host().granted_rewards(),
        0,
        "an unproduced state fired on a recorded transition"
    );
    assert!(!s.actor_facts().facts().actors.contains_key(&RAIDER));
}

/// The fact table is gameplay-relevant — an unfired `ActorIs` must observe
/// the state the actor reached before the save — so it crosses the restore
/// beside the evaluator and host records.
#[test]
fn accept_f37_e1_fact_table_crosses_save_and_restore() {
    let build = || {
        program(vec![
            objective(
                1,
                Condition::ActorIs {
                    actor: RAIDER,
                    state: ActorState::Dead,
                },
                vec![reward("r-dead")],
            ),
            objective(
                2,
                Condition::ActorIs {
                    actor: CONVOY,
                    state: ActorState::Captured,
                },
                vec![reward("r-taken")],
            ),
        ])
    };
    let rewards = vec![reward_id("r-dead"), reward_id("r-taken")];
    let mut s = MissionSession::launch(build(), SESSION, rewards).unwrap();
    s.register_actor(RAIDER).unwrap();
    s.register_actor(CONVOY).unwrap();

    // The convoy is captured before the save and its objective fires; the
    // raider's is still pending.
    let first = s
        .advance_observed(
            &input(&[], &[(CONVOY, LifecycleKind::OwnershipCaptured)]),
            Tick(1),
        )
        .unwrap();
    assert_eq!(first.mission.host.rewards_granted, [reward_id("r-taken")]);

    let mut restored = MissionSession::restore(build(), s.snapshot()).unwrap();
    assert_eq!(
        restored.actor_facts().state(&CONVOY),
        Some(ActorState::Captured)
    );
    assert_eq!(
        restored.actor_facts().state(&RAIDER),
        Some(ActorState::Alive)
    );

    // The restored run observes the destroyed transition exactly as the
    // live run would: the pending condition sees the populated fact.
    let second = restored
        .advance_observed(&input(&[], &[(RAIDER, LifecycleKind::Destroyed)]), Tick(2))
        .unwrap();
    assert_eq!(second.mission.host.rewards_granted, [reward_id("r-dead")]);

    // The already-recorded capture cannot be replayed across the restore.
    let refused = restored.advance_observed(
        &input(&[], &[(CONVOY, LifecycleKind::OwnershipCaptured)]),
        Tick(3),
    );
    assert!(matches!(
        refused,
        Err(ObservedError::Facts(FactError::DuplicateLifecycle {
            actor: CONVOY,
            kind: LifecycleKind::OwnershipCaptured,
        }))
    ));
}

/// A fact record is checked rather than trusted: an old version, an actor
/// listed twice and a set holding two terminal kinds are all refused.
#[test]
fn accept_f37_e1_fact_record_is_checked_not_trusted() {
    let build = || program(vec![objective(1, Condition::Const(true), vec![])]);
    let mut s = MissionSession::launch(build(), SESSION, []).unwrap();
    s.register_actor(RAIDER).unwrap();
    let good = s.snapshot();

    let mut older = good.clone();
    older.facts.version = FACT_SNAPSHOT_VERSION + 1;
    assert!(matches!(
        MissionSession::restore(build(), older),
        Err(SessionRestoreError::Facts(
            FactRestoreError::SnapshotVersion { .. }
        ))
    ));

    let mut doubled = good.clone();
    doubled.facts.actors.push(doubled.facts.actors[0].clone());
    assert!(matches!(
        MissionSession::restore(build(), doubled),
        Err(SessionRestoreError::Facts(
            FactRestoreError::DuplicateActor { actor: RAIDER }
        ))
    ));

    let mut conflict = good.clone();
    conflict.facts.actors[0].1 = vec![LifecycleKind::Despawned, LifecycleKind::MissionRemoved];
    assert!(matches!(
        MissionSession::restore(build(), conflict),
        Err(SessionRestoreError::Facts(
            FactRestoreError::ConflictingTerminal { actor: RAIDER, .. }
        ))
    ));

    // The intact record still restores.
    let back = MissionSession::restore(build(), good).unwrap();
    assert!(back.actor_facts().is_registered(&RAIDER));
}

/// `step_observed` is the evaluation-only fold: the populated facts drive
/// the condition and the input's observations are returned, but no host
/// effect is applied.
#[test]
fn accept_f37_e1_step_observed_evaluates_without_host_effects() {
    let mut s = session(
        vec![objective(
            1,
            Condition::ActorIs {
                actor: RAIDER,
                state: ActorState::Dead,
            },
            vec![reward("r-dead")],
        )],
        vec![reward_id("r-dead")],
    );
    s.register_actor(RAIDER).unwrap();

    let step = s
        .step_observed(&input(&[], &[(RAIDER, LifecycleKind::Destroyed)]), Tick(1))
        .unwrap();
    assert_eq!(step.facts.len(), 1);
    // The evaluator saw the state — the reward intent is an event — but
    // the host was never asked to apply it.
    assert!(
        step.result
            .events
            .iter()
            .any(|event| matches!(event.kind, EventKind::RewardGranted(_))),
        "{:?}",
        step.result.events
    );
    assert_eq!(s.host().granted_rewards(), 0);
}
