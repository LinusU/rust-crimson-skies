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

use cs_app::interaction::InteractionRuntime;
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
