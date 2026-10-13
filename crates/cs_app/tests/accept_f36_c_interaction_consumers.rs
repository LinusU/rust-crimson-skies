//! F36-C acceptance: the interaction session wired to its consumers
//! (synthetic).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-C`. Ordinary build/test only; nothing here is original
//! data.
//!
//! The minimum scenario is
//! [`accept_f36_c_destroyed_carrier_during_latch_returns_control_safely`].
//! Every test drives [`InteractionRuntime`] and the production
//! [`InteractionSession`], so removing the abort, the control resolution or
//! the joint commit fails an assertion.
//!
//! **F36-C.1** (`accept_f36_c1_`, task #658) adds the destroyed-carrier
//! forward: the damage consumer's per-actor destroyed event
//! ([`cs_app::damage::DamageActorDestroyed`], published by
//! [`cs_app::damage::publish_destroyed_actors`]) is driven through the real
//! [`cs_app::interaction::InteractionSchedulePlugin`] schedule, so the latch
//! aborts in a **running app** — no test here calls `on_actor_destroyed`
//! directly. These tests live in this binary rather than in a target of its
//! own for the measured disk cost the F33-C1 header in
//! `accept_f33_c_ally_lifecycle.rs` records: a Bevy/Avian-linked test binary
//! is ~123 MB, which exhausted the CI runner's disk twice.

use bevy::prelude::App;
use cs_app::damage::{lower_graph, lower_policy, publish_destroyed_actors};
use cs_app::interaction::{
    InteractionConsumerEvent, InteractionConsumerLog, InteractionRuntime, InteractionSchedulePlugin,
};
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_sim::damage::{
    ActorId, DamageChannel, DamageEvent, DamageEventKind, DamageNodeKey, DamageResolver, HitEvent,
    HitEventId, LifecycleKind, SYNTHETIC_HULL_INTEGRITY, SYNTHETIC_HULL_NODE,
};
use cs_sim::interaction::synthetic::SYNTHETIC_SESSION_ID;
use cs_sim::interaction::{
    AbortReason, ControlHolder, EligibilityRefusal, InitiatorMotion, InteractionCompletion,
    InteractionId, InteractionKind, InteractionRefusal, InteractionSession, InteractionState,
    InteractionTransaction, PilotId, SYNTHETIC_HOOK_TARGET, SYNTHETIC_INITIATOR, SYNTHETIC_SESSION,
    SessionRefusal, TransferLedger, TransferPolicy, TransferRefusal, evaluate_motion_eligibility,
    synthetic_docking_anchor, synthetic_docking_authorization, synthetic_docking_envelope,
    synthetic_docking_id, synthetic_docking_objective, synthetic_docking_transaction,
    synthetic_hook_trajectory,
};
use cs_types::Tick;

const PILOT: PilotId = PilotId(9);

fn ledger() -> TransferLedger {
    TransferLedger::new(
        SYNTHETIC_SESSION,
        [(SYNTHETIC_INITIATOR, PILOT)],
        [(SYNTHETIC_INITIATOR, 3)],
    )
}

fn good_pass() -> Result<cs_sim::world_actors::anchor::AnchorSample, EligibilityRefusal> {
    let motion = InitiatorMotion::try_new([9.4, 0.0, 0.0], [10.0, 0.0, 0.0], 1, 10).unwrap();
    evaluate_motion_eligibility(
        Tick(20),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        &motion,
        &synthetic_docking_envelope(),
    )
}

fn fast_pass() -> Result<cs_sim::world_actors::anchor::AnchorSample, EligibilityRefusal> {
    let motion = InitiatorMotion::try_new([10.0, 0.0, 0.0], [40.0, 0.0, 0.0], 1, 10).unwrap();
    evaluate_motion_eligibility(
        Tick(20),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        &motion,
        &synthetic_docking_envelope(),
    )
}

fn swap_transaction() -> InteractionTransaction {
    InteractionTransaction::begin(
        synthetic_docking_id(),
        InteractionKind::AircraftSwap,
        cs_sim::interaction::InteractionAuthorization::new(
            SYNTHETIC_SESSION,
            InteractionKind::AircraftSwap,
            synthetic_docking_objective(),
        ),
        TransferPolicy::aircraft_swap(),
    )
}

fn latched(tx: InteractionTransaction) -> (InteractionSession, InteractionId) {
    let id = tx.id();
    let mut session = InteractionSession::new(ledger());
    session.open(tx).expect("opens");
    assert_eq!(
        session.observe(id, good_pass()).expect("known"),
        Ok(InteractionState::Latching)
    );
    (session, id)
}

#[test]
fn accept_f36_c_destroyed_carrier_during_latch_returns_control_safely() {
    let (session, id) = latched(swap_transaction());
    assert_eq!(
        session.control_of(SYNTHETIC_INITIATOR),
        ControlHolder::Latch(id)
    );
    let mut runtime = InteractionRuntime(session);

    let aborts = runtime.on_actor_destroyed(SYNTHETIC_HOOK_TARGET);
    assert_eq!(aborts.len(), 1);
    assert_eq!(aborts[0].from, InteractionState::Latching);
    assert_eq!(aborts[0].reason, AbortReason::TargetDestroyed);

    let session = &mut runtime.0;
    assert_eq!(session.state(id), Some(InteractionState::Aborted));
    assert_eq!(
        session.control_of(SYNTHETIC_INITIATOR),
        ControlHolder::Actor(SYNTHETIC_INITIATOR),
        "the initiator is flown by its own controller again"
    );
    assert_eq!(session.ledger().pilot_of(SYNTHETIC_INITIATOR), Some(PILOT));
    assert_eq!(session.ledger().pilot_of(SYNTHETIC_HOOK_TARGET), None);
    assert_eq!(session.ledger().inventory_of(SYNTHETIC_HOOK_TARGET), 0);
    assert!(session.active_for(SYNTHETIC_INITIATOR).is_none());

    // No stale completion, no second destruction report.
    let late = session.complete(
        id,
        &synthetic_docking_objective(),
        true,
        [6.0, 0.0, 0.0],
        [5.0, 0.0, 0.0],
    );
    assert_eq!(
        late.unwrap_err(),
        SessionRefusal::Transaction(InteractionRefusal::Aborted)
    );
    assert!(session.actor_destroyed(SYNTHETIC_HOOK_TARGET).is_empty());
}

#[test]
fn accept_f36_c_destroyed_initiator_aborts_without_moving_the_pilot() {
    let (mut session, id) = latched(swap_transaction());
    let aborts = session.actor_destroyed(SYNTHETIC_INITIATOR);
    assert_eq!(aborts.len(), 1);
    assert_eq!(session.state(id), Some(InteractionState::Aborted));
    assert_eq!(session.ledger().pilot_of(SYNTHETIC_HOOK_TARGET), None);
}

#[test]
fn accept_f36_c_target_lost_at_completion_aborts_with_no_effects() {
    let (mut session, id) = latched(swap_transaction());
    session.release(id).expect("released");
    let refused = session.complete(
        id,
        &synthetic_docking_objective(),
        false,
        [6.0; 3],
        [5.0; 3],
    );
    assert_eq!(
        refused.unwrap_err(),
        SessionRefusal::Transaction(InteractionRefusal::TargetLost)
    );
    assert_eq!(session.state(id), Some(InteractionState::Aborted));
    assert_eq!(session.ledger().pilot_of(SYNTHETIC_INITIATOR), Some(PILOT));
}

#[test]
fn accept_f36_c_fast_pass_never_latches_through_the_session() {
    let tx = synthetic_docking_transaction();
    let id = tx.id();
    let mut session = InteractionSession::new(ledger());
    session.open(tx).unwrap();
    let outcome = session.observe(id, fast_pass()).unwrap();
    assert!(outcome.is_err(), "a too-fast pass is refused");
    assert_eq!(session.state(id), Some(InteractionState::Approaching));
    assert_eq!(
        session.control_of(SYNTHETIC_INITIATOR),
        ControlHolder::Actor(SYNTHETIC_INITIATOR)
    );
    assert_eq!(
        session.observe(id, good_pass()).unwrap(),
        Ok(InteractionState::Latching)
    );
}

#[test]
fn accept_f36_c_swap_commits_transaction_and_ledger_once() {
    let (mut session, id) = latched(swap_transaction());
    assert!(session.release(id).is_ok());
    let (outcome, report) = session
        .complete(id, &synthetic_docking_objective(), true, [6.0; 3], [5.0; 3])
        .expect("completes");
    assert_eq!(outcome.completion, InteractionCompletion::AircraftSwapped);
    assert_eq!(report.pilot_moved, Some(PILOT));
    assert_eq!(
        session.control_of(SYNTHETIC_INITIATOR),
        ControlHolder::Actor(SYNTHETIC_HOOK_TARGET)
    );
    assert_eq!(
        session.ledger().pilot_of(SYNTHETIC_HOOK_TARGET),
        Some(PILOT)
    );
    let again = session.complete(id, &synthetic_docking_objective(), true, [6.0; 3], [5.0; 3]);
    assert!(again.is_err(), "the swap is applied exactly once");
    assert_eq!(session.ledger().inventory_of(SYNTHETIC_HOOK_TARGET), 3);
}

#[test]
fn accept_f36_c_ledger_refusal_aborts_instead_of_publishing_a_completion() {
    // Target already piloted: the transaction would complete, the ledger
    // refuses, and the session must not leave a completed transaction behind.
    let occupied = TransferLedger::new(
        SYNTHETIC_SESSION,
        [
            (SYNTHETIC_INITIATOR, PILOT),
            (SYNTHETIC_HOOK_TARGET, PilotId(2)),
        ],
        [],
    );
    let tx = swap_transaction();
    let id = tx.id();
    let mut session = InteractionSession::new(occupied);
    session.open(tx).unwrap();
    session.observe(id, good_pass()).unwrap().unwrap();
    session.release(id).unwrap();
    let refused = session
        .complete(id, &synthetic_docking_objective(), true, [6.0; 3], [5.0; 3])
        .unwrap_err();
    assert!(matches!(
        refused,
        SessionRefusal::Transfer(TransferRefusal::TargetOccupied { .. })
    ));
    assert_eq!(session.state(id), Some(InteractionState::Aborted));
    assert_eq!(session.ledger().pilot_of(SYNTHETIC_INITIATOR), Some(PILOT));
}

#[test]
fn accept_f36_c_retry_restores_the_starting_actor_and_refuses_stale_attempts() {
    let (mut session, id) = latched(swap_transaction());
    session.release(id).unwrap();
    session
        .complete(id, &synthetic_docking_objective(), true, [6.0; 3], [5.0; 3])
        .unwrap();
    assert_eq!(session.ledger().pilot_of(SYNTHETIC_INITIATOR), None);

    let mut runtime = InteractionRuntime(session);
    runtime.on_retry(SYNTHETIC_SESSION + 1);
    let session = &mut runtime.0;
    assert_eq!(session.ledger().pilot_of(SYNTHETIC_INITIATOR), Some(PILOT));
    assert_eq!(session.ledger().inventory_of(SYNTHETIC_INITIATOR), 3);
    assert_eq!(session.state(id), None, "old attempts are dropped");
    assert!(matches!(
        session.open(swap_transaction()),
        Err(SessionRefusal::StaleSession { .. })
    ));
}

#[test]
fn accept_f36_c_retry_during_latch_aborts_and_pause_aborts_all() {
    let (mut session, id) = latched(swap_transaction());
    let aborts = session.retry(SYNTHETIC_SESSION + 1);
    assert_eq!(aborts[0].reason, AbortReason::Retry);
    assert_eq!(aborts[0].id, id);

    let (mut session, id) = latched(synthetic_docking_transaction());
    let aborts = session.abort_all(AbortReason::Pause);
    assert_eq!(aborts.len(), 1);
    assert_eq!(session.state(id), Some(InteractionState::Aborted));
}

#[test]
fn accept_f36_c_initiator_cannot_hold_two_interactions() {
    let (mut session, _) = latched(swap_transaction());
    let other = InteractionTransaction::begin(
        InteractionId::new(
            SYNTHETIC_SESSION,
            2,
            SYNTHETIC_INITIATOR,
            SYNTHETIC_HOOK_TARGET,
        ),
        InteractionKind::Docking,
        synthetic_docking_authorization(),
        TransferPolicy::docking(),
    );
    assert!(matches!(
        session.open(other),
        Err(SessionRefusal::InitiatorBusy { .. })
    ));
}

// ------------------------------------------------------- F36-C.1: the forward ---
//
// The per-actor destroyed event and its ECS forward into the runtime
// (task #658). Every test here goes through the **real schedule**: the
// plugin's `Update` system runs on `app.update()`, and nothing calls
// `InteractionRuntime::on_actor_destroyed` directly.

/// The declared synthetic airframe lowered through the production damage
/// boundary and registered with the real resolver under the interaction
/// fixture's own session — the latch's carrier is the actor the damage
/// resolver destroys.
fn carrier_resolver() -> DamageResolver {
    let declared = declared_synthetic_airframe_damage();
    let mut resolver = DamageResolver::new(SYNTHETIC_SESSION_ID, 1);
    resolver
        .register_actor(
            SYNTHETIC_HOOK_TARGET,
            lower_graph(&declared).expect("the declared graph lowers"),
            lower_policy(&declared).expect("the declared policy lowers"),
        )
        .expect("the carrier registers in the fixture session");
    resolver
}

/// One internal hit that exactly destroys the carrier's hull, stamped in
/// the fixture session, as the resolver receives a real ballistic hit.
fn lethal_hit() -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: SYNTHETIC_SESSION_ID,
            tick: Tick(0),
            producer: 1,
            sequence: 0,
        },
        None,
        SYNTHETIC_HOOK_TARGET,
        DamageNodeKey::new(SYNTHETIC_HULL_NODE).expect("the fixture hull key is valid"),
        DamageChannel::Internal,
        SYNTHETIC_HULL_INTEGRITY,
    )
    .expect("a finite, non-negative hit")
}

/// The carrier's destruction, resolved by the real resolver.
fn resolved_destruction() -> Vec<DamageEvent> {
    carrier_resolver()
        .resolve(Tick(0), &[lethal_hit()])
        .expect("the resolver accepts the batch")
        .events
}

/// F36-C.1 minimum scenario: the damage consumer destroys the carrier
/// mid-latch, and the real schedule forwards the destruction so the
/// interaction aborts with control returned to the initiator.
///
/// The whole path is production code: the declared graph lowers and
/// registers with the real [`DamageResolver`], the lethal hit is resolved
/// by it, [`publish_destroyed_actors`] publishes the resolution's
/// destructions, and one [`App::update`] over
/// [`InteractionSchedulePlugin`]'s schedule forwards them into the
/// runtime. A forward that never reads the event, or a publication that
/// never emits it, fails at the abort assertion.
#[test]
fn accept_f36_c1_destroyed_carrier_during_latch_aborts_through_the_real_schedule() {
    let (session, id) = latched(swap_transaction());
    let mut app = App::new();
    app.add_plugins(InteractionSchedulePlugin);
    app.insert_resource(InteractionRuntime(session));

    // The real resolver destroys the carrier: a lethal hit on its hull.
    let events = resolved_destruction();
    assert!(
        events.iter().any(|event| matches!(
            &event.kind,
            DamageEventKind::Lifecycle { actor, kind }
                if *actor == SYNTHETIC_HOOK_TARGET && *kind == LifecycleKind::Destroyed
        )),
        "the resolver emitted the carrier's destruction: {events:?}"
    );

    // The damage consumer publishes that destruction as the per-actor
    // event, and the real schedule forwards it: the latch aborts and
    // control resolves back to the initiator.
    let published = publish_destroyed_actors(app.world_mut(), &events);
    assert_eq!(published, vec![SYNTHETIC_HOOK_TARGET]);
    app.update();

    let runtime = app.world().resource::<InteractionRuntime>();
    assert_eq!(runtime.0.state(id), Some(InteractionState::Aborted));
    assert_eq!(
        runtime.0.control_of(SYNTHETIC_INITIATOR),
        ControlHolder::Actor(SYNTHETIC_INITIATOR),
        "the initiator is flown by its own controller again"
    );
    assert!(runtime.0.active_for(SYNTHETIC_INITIATOR).is_none());
    assert_eq!(
        runtime.0.ledger().pilot_of(SYNTHETIC_INITIATOR),
        Some(PILOT)
    );
    assert_eq!(runtime.0.ledger().pilot_of(SYNTHETIC_HOOK_TARGET), None);

    let log = app.world().resource::<InteractionConsumerLog>();
    assert_eq!(log.events().len(), 1, "one abort recorded: {log:?}");
    let InteractionConsumerEvent::Aborted(abort) = &log.events()[0] else {
        panic!("the forward recorded an abort, not a refusal: {log:?}");
    };
    assert_eq!(abort.id, id);
    assert_eq!(abort.from, InteractionState::Latching);
    assert_eq!(abort.reason, AbortReason::TargetDestroyed);
}

/// A destruction forwarded in a world with no interaction session is
/// reported, not dropped: the wiring gap surfaces in the log instead of
/// silently losing the abort.
#[test]
fn accept_f36_c1_destruction_without_an_interaction_runtime_is_reported() {
    let mut app = App::new();
    app.add_plugins(InteractionSchedulePlugin);

    let published = publish_destroyed_actors(app.world_mut(), &resolved_destruction());
    assert_eq!(published, vec![SYNTHETIC_HOOK_TARGET]);
    app.update();

    let log = app.world().resource::<InteractionConsumerLog>();
    assert_eq!(
        log.events(),
        &[InteractionConsumerEvent::NoRuntime {
            actor: SYNTHETIC_HOOK_TARGET
        }],
        "a destruction that cannot reach an interaction session is visible: {log:?}"
    );
}

/// Only the resolver's destruction publishes: a bailout is a separate
/// transition (F29 non-negotiable 3), so it leaves the active latch and
/// its control untouched.
#[test]
fn accept_f36_c1_a_bailout_publishes_no_destroyed_event() {
    let mut resolver = carrier_resolver();
    let bailout = resolver
        .record_lifecycle(SYNTHETIC_HOOK_TARGET, LifecycleKind::PilotBailout, Tick(0))
        .expect("the carrier is registered");

    let (session, id) = latched(swap_transaction());
    let mut app = App::new();
    app.add_plugins(InteractionSchedulePlugin);
    app.insert_resource(InteractionRuntime(session));

    let published = publish_destroyed_actors(app.world_mut(), std::slice::from_ref(&bailout));
    assert_eq!(published, Vec::<ActorId>::new());
    app.update();

    let log = app.world().resource::<InteractionConsumerLog>();
    assert!(log.is_empty(), "a bailout aborts nothing: {log:?}");
    let runtime = app.world().resource::<InteractionRuntime>();
    assert_eq!(runtime.0.state(id), Some(InteractionState::Latching));
    assert_eq!(
        runtime.0.control_of(SYNTHETIC_INITIATOR),
        ControlHolder::Latch(id)
    );
}

/// A batch that names the same destruction twice publishes once: one
/// actor's death is one event, however many reporters recorded it.
#[test]
fn accept_f36_c1_a_replayed_destruction_publishes_once() {
    let destroyed = DamageEvent {
        id: HitEventId {
            session: SYNTHETIC_SESSION_ID,
            tick: Tick(0),
            producer: 1,
            sequence: 0,
        },
        kind: DamageEventKind::Lifecycle {
            actor: SYNTHETIC_HOOK_TARGET,
            kind: LifecycleKind::Destroyed,
        },
    };
    let batch = [destroyed.clone(), destroyed];

    let (session, id) = latched(swap_transaction());
    let mut app = App::new();
    app.add_plugins(InteractionSchedulePlugin);
    app.insert_resource(InteractionRuntime(session));

    let published = publish_destroyed_actors(app.world_mut(), &batch);
    assert_eq!(published, vec![SYNTHETIC_HOOK_TARGET]);
    app.update();

    let log = app.world().resource::<InteractionConsumerLog>();
    assert_eq!(log.events().len(), 1, "one destruction, one abort: {log:?}");
    let runtime = app.world().resource::<InteractionRuntime>();
    assert_eq!(runtime.0.state(id), Some(InteractionState::Aborted));
}
