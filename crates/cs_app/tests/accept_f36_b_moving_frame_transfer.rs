//! F36-B acceptance: moving-frame eligibility and atomic transfer
//! (synthetic).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-B`. Ordinary build/test only; nothing here is original
//! data.
//!
//! The minimum scenario is
//! [`accept_f36_b_dock_on_moving_carrier_across_rebase_without_false_speed`]:
//! the initiator closes on a moving hook, the world origin is rebased between
//! two samples, and the swept eligibility still latches at the true relative
//! speed. Every test drives production code ([`initiator_motion`],
//! [`evaluate_motion_eligibility`], the transaction and the
//! [`TransferLedger`]), so removing the world-frame velocity, the continuity
//! check or the atomic commit fails to compile or fails an assertion.

use cs_app::interaction::{AnchorMotionError, initiator_motion};
use cs_app::origin::{OriginEpoch, OriginShift, SpatialAnchor, WorldOrigin};
use cs_sim::interaction::{
    AbortReason, EligibilityRefusal, InitiatorMotion, InteractionCompletion, InteractionId,
    InteractionOutcome, InteractionState, PilotId, SYNTHETIC_HOOK_TARGET, SYNTHETIC_INITIATOR,
    TransferLedger, TransferPolicy, TransferRefusal, VelocityTransfer, evaluate_motion_eligibility,
    synthetic_docking_anchor, synthetic_docking_authorization, synthetic_docking_envelope,
    synthetic_docking_id, synthetic_docking_objective, synthetic_docking_transaction,
    synthetic_hook_trajectory,
};
use cs_sim::interaction::{InteractionKind, InteractionTransaction};
use cs_types::Tick;
use cs_types::space::WorldPosition;

const TICKS_PER_SECOND: u32 = 10;
/// The carrier moves 5 m/s along +X (50 m in 100 ticks at 10 ticks/s).
const CARRIER_SPEED_M_S: f64 = 5.0;
/// The initiator flies 6 m/s along +X: 1 m/s relative to the carrier.
const INITIATOR_STEP_M: f32 = 0.6;
const FAR_ORIGIN_M: f64 = 250_000.0;

fn world(components: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(components).expect("test coordinates are finite")
}

/// An initiator one tick before the dock sample, rebased across a far origin
/// move, then advanced one tick in the new frame.
fn rebased_initiator() -> (SpatialAnchor, [f32; 3], WorldOrigin) {
    let origin = WorldOrigin::new(OriginEpoch(0), world([0.0, 0.0, 0.0]));
    let mut anchors = [SpatialAnchor::new(&origin, world([6.4, 0.0, 0.0])).expect("in frame")];
    let before_local = anchors[0].local().to_array();
    let shift = OriginShift::rebase(origin, world([FAR_ORIGIN_M, 0.0, 0.0])).expect("rebases");
    shift.apply(&mut anchors).expect("rebase applies");
    let [mut anchor] = anchors;
    anchor
        .advance_local(&shift.to(), [INITIATOR_STEP_M, 0.0, 0.0])
        .expect("advances in the new frame");
    (anchor, before_local, shift.to())
}

fn eligible_transaction() -> InteractionTransaction {
    let mut transaction = synthetic_docking_transaction();
    transaction.advance().expect("available -> approaching");
    transaction.advance().expect("approaching -> eligible");
    assert_eq!(transaction.state(), InteractionState::Eligible);
    transaction
}

// -------------------------------------------------- AC02 minimum scenario ---

#[test]
fn accept_f36_b_dock_on_moving_carrier_across_rebase_without_false_speed() {
    let (anchor, _, origin) = rebased_initiator();
    assert_eq!(origin.epoch(), OriginEpoch(1), "a rebase happened");

    let motion = initiator_motion(&anchor, TICKS_PER_SECOND).expect("a continuous path");
    let [vx, vy, vz] = motion.velocity_m_s();
    assert!(
        (vx - 6.0).abs() < 1e-4,
        "world speed is the true 6 m/s: {vx}"
    );
    assert_eq!((vy, vz), (0.0, 0.0));

    // Tick 20: the carrier hook is at x = 10 and moving at 5 m/s.
    let sample = evaluate_motion_eligibility(
        Tick(20),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        &motion,
        &synthetic_docking_envelope(),
    )
    .expect("a 1 m/s relative approach across a rebase is eligible");
    assert!((sample.velocity_m_s[0] - CARRIER_SPEED_M_S).abs() < 1e-9);

    let mut transaction = eligible_transaction();
    transaction
        .latch(Ok(sample))
        .expect("the aligned pass latches");
    assert_eq!(transaction.state(), InteractionState::Latching);
}

/// The failure the world-frame path prevents: differencing the pre-rebase
/// local position against the post-rebase one is a quarter-million-metre jump
/// in one tick, which the same envelope refuses as a false speed.
#[test]
fn accept_f36_b_local_difference_across_rebase_is_a_false_speed() {
    let (anchor, before_local, _) = rebased_initiator();
    let after_local = anchor.local().to_array();
    let naive = InitiatorMotion::try_new(
        before_local.map(f64::from),
        after_local.map(f64::from),
        1,
        TICKS_PER_SECOND,
    )
    .expect("finite");
    assert!(
        naive.velocity_m_s()[0].abs() > 1.0e5,
        "the naive speed is false"
    );

    let refusal = evaluate_motion_eligibility(
        Tick(20),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        &naive,
        &synthetic_docking_envelope(),
    )
    .expect_err("a false speed is refused");
    assert!(
        matches!(refusal, EligibilityRefusal::TooFar { .. }),
        "{refusal:?}"
    );
}

#[test]
fn accept_f36_b_teleported_initiator_has_no_continuous_path() {
    let origin = WorldOrigin::new(OriginEpoch(0), world([0.0, 0.0, 0.0]));
    let mut anchor = SpatialAnchor::new(&origin, world([6.4, 0.0, 0.0])).expect("in frame");
    assert_eq!(
        initiator_motion(&anchor, TICKS_PER_SECOND),
        Err(AnchorMotionError::NoContinuousPath),
        "a fresh record has no sweep"
    );
    anchor
        .advance_local(&origin, [0.6, 0.0, 0.0])
        .expect("moves");
    assert!(initiator_motion(&anchor, TICKS_PER_SECOND).is_ok());
    anchor
        .teleport(&origin, world([7.0, 0.0, 0.0]))
        .expect("teleports");
    assert_eq!(
        initiator_motion(&anchor, TICKS_PER_SECOND),
        Err(AnchorMotionError::NoContinuousPath),
        "a teleport discards the sweep, so no speed is inferred"
    );
}

#[test]
fn accept_f36_b_fast_pass_across_rebase_still_refused() {
    let origin = WorldOrigin::new(OriginEpoch(0), world([0.0, 0.0, 0.0]));
    let mut anchors = [SpatialAnchor::new(&origin, world([6.4, 0.0, 0.0])).expect("in frame")];
    let shift = OriginShift::rebase(origin, world([FAR_ORIGIN_M, 0.0, 0.0])).expect("rebases");
    shift.apply(&mut anchors).expect("applies");
    let [mut anchor] = anchors;
    // 8 m per tick = 80 m/s, 75 m/s relative.
    anchor
        .advance_local(&shift.to(), [8.0, 0.0, 0.0])
        .expect("moves");
    let motion = initiator_motion(&anchor, TICKS_PER_SECOND).expect("continuous");
    let refusal = evaluate_motion_eligibility(
        Tick(20),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        &motion,
        &synthetic_docking_envelope(),
    )
    .expect_err("too fast");
    assert!(
        matches!(refusal, EligibilityRefusal::TooFast { .. }),
        "{refusal:?}"
    );
}

// ------------------------------------------------------- atomic transfer ---

const PILOT: PilotId = PilotId(100);

fn completed(policy: TransferPolicy, kind: InteractionKind) -> InteractionOutcome {
    let mut transaction = InteractionTransaction::begin(
        synthetic_docking_id(),
        kind,
        synthetic_docking_authorization(),
        policy,
    );
    while transaction.state() != InteractionState::Released {
        transaction.advance().expect("advances to released");
    }
    transaction
        .complete(&synthetic_docking_objective(), true)
        .expect("completes")
}

fn swap_ledger() -> TransferLedger {
    TransferLedger::new(
        synthetic_docking_id().session,
        [(SYNTHETIC_INITIATOR, PILOT)],
        [(SYNTHETIC_INITIATOR, 3)],
    )
}

#[test]
fn accept_f36_b_swap_moves_pilot_and_cargo_exactly_once() {
    let outcome = completed(TransferPolicy::aircraft_swap(), InteractionKind::Docking);
    let mut ledger = swap_ledger();
    let report = ledger
        .apply(&outcome, [6.0, 0.0, 0.0], [5.0, 0.0, 0.0])
        .expect("applies");
    assert_eq!(report.pilot_moved, Some(PILOT));
    assert_eq!(report.inventory_moved, 3);
    assert_eq!(report.camera_actor, SYNTHETIC_HOOK_TARGET);
    assert_eq!(report.control_actor, SYNTHETIC_HOOK_TARGET);
    assert_eq!(
        report.velocity_m_s,
        [6.0, 0.0, 0.0],
        "swap preserves velocity"
    );
    assert_eq!(ledger.pilot_of(SYNTHETIC_INITIATOR), None);
    assert_eq!(ledger.pilot_of(SYNTHETIC_HOOK_TARGET), Some(PILOT));
    assert_eq!(ledger.inventory_of(SYNTHETIC_INITIATOR), 0);
    assert_eq!(ledger.inventory_of(SYNTHETIC_HOOK_TARGET), 3);

    let again = ledger.apply(&outcome, [6.0, 0.0, 0.0], [5.0, 0.0, 0.0]);
    assert_eq!(again, Err(TransferRefusal::AlreadyApplied(outcome.id)));
    assert_eq!(
        ledger.inventory_of(SYNTHETIC_HOOK_TARGET),
        3,
        "no duplicate cargo"
    );
}

#[test]
fn accept_f36_b_docking_matches_target_velocity_and_moves_nothing() {
    let outcome = completed(TransferPolicy::docking(), InteractionKind::Docking);
    assert_eq!(outcome.completion, InteractionCompletion::Docked);
    assert_eq!(outcome.effects.velocity, VelocityTransfer::MatchTarget);
    let mut ledger = swap_ledger();
    let report = ledger
        .apply(&outcome, [6.0, 0.0, 0.0], [5.0, 0.0, 0.0])
        .expect("applies");
    assert_eq!(report.velocity_m_s, [5.0, 0.0, 0.0]);
    assert_eq!(report.pilot_moved, None);
    assert_eq!(ledger.pilot_of(SYNTHETIC_INITIATOR), Some(PILOT));
    assert_eq!(ledger.inventory_of(SYNTHETIC_INITIATOR), 3);
}

#[test]
fn accept_f36_b_refused_transfer_changes_nothing() {
    let outcome = completed(TransferPolicy::aircraft_swap(), InteractionKind::Docking);

    // The target already has a pilot: refused, and the initiator keeps theirs.
    let mut ledger = TransferLedger::new(
        outcome.id.session,
        [
            (SYNTHETIC_INITIATOR, PILOT),
            (SYNTHETIC_HOOK_TARGET, PilotId(200)),
        ],
        [(SYNTHETIC_INITIATOR, 3)],
    );
    let before = ledger.clone();
    assert_eq!(
        ledger.apply(&outcome, [0.0; 3], [0.0; 3]),
        Err(TransferRefusal::TargetOccupied {
            target: SYNTHETIC_HOOK_TARGET,
            pilot: PilotId(200)
        })
    );
    assert_eq!(ledger, before, "a refusal is atomic");

    // No pilot to move.
    let mut empty = TransferLedger::new(outcome.id.session, [], []);
    assert_eq!(
        empty.apply(&outcome, [0.0; 3], [0.0; 3]),
        Err(TransferRefusal::NoPilot(SYNTHETIC_INITIATOR))
    );

    // Cargo that would overflow the target is refused before the pilot moves.
    let mut full = TransferLedger::new(
        outcome.id.session,
        [(SYNTHETIC_INITIATOR, PILOT)],
        [(SYNTHETIC_INITIATOR, 3), (SYNTHETIC_HOOK_TARGET, u32::MAX)],
    );
    let before = full.clone();
    assert_eq!(
        full.apply(&outcome, [0.0; 3], [0.0; 3]),
        Err(TransferRefusal::InventoryOverflow(SYNTHETIC_HOOK_TARGET))
    );
    assert_eq!(full, before);
}

#[test]
fn accept_f36_b_stale_session_is_refused_and_retry_restores_start() {
    let outcome = completed(TransferPolicy::aircraft_swap(), InteractionKind::Docking);
    let mut ledger =
        TransferLedger::new(outcome.id.session + 1, [(SYNTHETIC_INITIATOR, PILOT)], []);
    assert_eq!(
        ledger.apply(&outcome, [0.0; 3], [0.0; 3]),
        Err(TransferRefusal::StaleSession {
            ledger: outcome.id.session + 1,
            interaction: outcome.id.session
        })
    );

    // Apply in the right session, then retry into a newer one.
    let mut ledger = swap_ledger();
    ledger.apply(&outcome, [0.0; 3], [0.0; 3]).expect("applies");
    ledger.retry(outcome.id.session + 1);
    assert_eq!(ledger.session(), outcome.id.session + 1);
    assert_eq!(
        ledger.pilot_of(SYNTHETIC_INITIATOR),
        Some(PILOT),
        "start actor returns"
    );
    assert_eq!(
        ledger.pilot_of(SYNTHETIC_HOOK_TARGET),
        None,
        "no duplicate pilot"
    );
    assert_eq!(ledger.inventory_of(SYNTHETIC_INITIATOR), 3);
    // The old interaction can no longer apply to the new generation.
    assert!(matches!(
        ledger.apply(&outcome, [0.0; 3], [0.0; 3]),
        Err(TransferRefusal::StaleSession { .. })
    ));
}

#[test]
fn accept_f36_b_aborted_interaction_has_no_outcome_to_apply() {
    let mut transaction = eligible_transaction();
    let abort = transaction
        .abort(AbortReason::TargetDestroyed)
        .expect("aborts");
    assert_eq!(abort.reason, AbortReason::TargetDestroyed);
    assert!(transaction.effects().is_none());
    let id: InteractionId = transaction.id();
    assert_eq!(id, synthetic_docking_id());
}
