//! F36-A acceptance: the interaction state machine, swept eligibility and
//! transfer policy (synthetic).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`. Ordinary build/test only; nothing here is original
//! data.
//!
//! The F36-A minimum scenario is split across
//! [`accept_f36_a_fast_pass_does_not_latch`] and
//! [`accept_f36_a_wrong_direction_does_not_latch`]: a pass that is too fast or
//! from the wrong direction does not latch. Every test drives production code,
//! so removing a refusal, a transition or the swept test fails to compile or
//! fails an assertion.

use cs_sim::damage::ActorId;
use cs_sim::interaction::{
    AbortReason, CameraTransfer, ControlOwner, EligibilityEnvelope, EligibilityRefusal,
    EnvelopeError, InteractionCompletion, InteractionId, InteractionKind, InteractionRefusal,
    InteractionState, LatchRefusal, PilotTransfer, TransferPolicy, VelocityTransfer,
    evaluate_eligibility, synthetic_docking_anchor, synthetic_docking_authorization,
    synthetic_docking_envelope, synthetic_docking_id, synthetic_docking_objective,
    synthetic_docking_transaction, synthetic_hook_trajectory,
};
use cs_types::Tick;

/// The slow, aligned approach the minimum scenario's positive case reuses:
/// the initiator sits 3 m behind the hook and closes at 1 m/s relative.
fn aligned_approach() -> ([f64; 3], [f64; 3]) {
    ([-3.0, 0.0, 0.0], [6.0, 0.0, 0.0])
}

fn eligibility(
    position: [f64; 3],
    velocity: [f64; 3],
    sweep_seconds: f64,
) -> Result<cs_sim::world_actors::anchor::AnchorSample, EligibilityRefusal> {
    evaluate_eligibility(
        Tick(0),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        position,
        velocity,
        sweep_seconds,
        &synthetic_docking_envelope(),
    )
}

// -------------------------------------------------- AC01 minimum scenario ---

/// The minimum scenario, part one: a pass that is too fast does not latch.
#[test]
fn accept_f36_a_fast_pass_does_not_latch() {
    let (position, _) = aligned_approach();
    // 75 m/s relative to the hook is far beyond the 10 m/s envelope.
    let refusal = eligibility(position, [80.0, 0.0, 0.0], 0.1).expect_err("too fast is refused");
    assert!(
        matches!(refusal, EligibilityRefusal::TooFast { .. }),
        "got {refusal:?}"
    );

    let mut transaction = synthetic_docking_transaction();
    transaction.advance().expect("available -> approaching");
    transaction.advance().expect("approaching -> eligible");
    assert_eq!(transaction.state(), InteractionState::Eligible);

    let result = transaction.latch(Err(refusal));
    assert_eq!(result, Err(LatchRefusal::NotEligible(refusal)));
    assert_eq!(
        transaction.state(),
        InteractionState::Eligible,
        "a refused pass stays eligible and never latches"
    );
}

/// The minimum scenario, part two: a pass from the wrong direction does not
/// latch even though it is inside the capture radius.
#[test]
fn accept_f36_a_wrong_direction_does_not_latch() {
    let (position, _) = aligned_approach();
    // Slow enough for the speed envelope, but travelling -X against a +X
    // docking axis.
    let refusal = eligibility(position, [-1.0, 0.0, 0.0], 0.5).expect_err("wrong way is refused");
    match refusal {
        EligibilityRefusal::WrongDirection { angle_deg } => {
            assert!(angle_deg > 35.0, "got {angle_deg} degrees");
        }
        other => panic!("expected WrongDirection, got {other:?}"),
    }

    let mut transaction = synthetic_docking_transaction();
    transaction.advance().expect("available -> approaching");
    transaction.advance().expect("approaching -> eligible");
    assert!(transaction.latch(Err(refusal)).is_err());
    assert_eq!(transaction.state(), InteractionState::Eligible);
}

/// A slow, aligned approach is eligible and latches.
#[test]
fn accept_f36_a_aligned_slow_approach_latches() {
    let (position, velocity) = aligned_approach();
    let sample = eligibility(position, velocity, 0.5).expect("aligned approach is eligible");
    assert_eq!(sample.tick, Tick(0));

    let mut transaction = synthetic_docking_transaction();
    assert_eq!(transaction.state(), InteractionState::Available);
    // Not eligible yet.
    assert_eq!(
        transaction.latch(Ok(sample)),
        Err(LatchRefusal::NotReady {
            state: InteractionState::Available
        })
    );
    transaction.advance().expect("available -> approaching");
    transaction.advance().expect("approaching -> eligible");
    transaction
        .latch(Ok(sample))
        .expect("an eligible approach latches");
    assert_eq!(transaction.state(), InteractionState::Latching);
}

/// The swept test reaches a moving hook that a single instantaneous radius
/// test would miss: at the sampled tick the initiator is 8 m out (well beyond
/// the 5 m radius) but its relative motion crosses the anchor inside the
/// sweep. A radius-only implementation refuses this; the production
/// [`evaluate_eligibility`] accepts it.
#[test]
fn accept_f36_a_sweep_reaches_a_hook_a_radius_test_misses() {
    let position: [f64; 3] = [-8.0, 0.0, 0.0];
    // Relative to the hook (5 m/s +X) the initiator closes at 2 m/s.
    let velocity = [7.0, 0.0, 0.0];
    let instantaneous = (position[0] - 0.0).abs();
    assert!(
        instantaneous > 5.0,
        "the instantaneous distance is outside the radius"
    );

    let sample = eligibility(position, velocity, 5.0)
        .expect("the sweep crosses the hook inside the capture radius");
    assert_eq!(sample.position_m, [0.0, 0.0, 0.0]);

    // The same inputs at a too-short sweep do not reach the hook.
    let refusal = eligibility(position, velocity, 0.4).expect_err("a short sweep does not reach");
    assert!(
        matches!(refusal, EligibilityRefusal::TooFar { .. }),
        "got {refusal:?}"
    );
}

// --------------------------------------------------------- transaction -----

/// The canonical chain runs to completion; completion validates the active
/// objective and the target's liveness, and writes no effects before it.
#[test]
fn accept_f36_a_completion_requires_authorization_and_a_live_target() {
    let (position, velocity) = aligned_approach();
    let sample = eligibility(position, velocity, 0.5).expect("eligible");

    let mut transaction = synthetic_docking_transaction();
    transaction.advance().expect("approaching");
    transaction.advance().expect("eligible");
    transaction.latch(Ok(sample)).expect("latch");
    transaction.advance().expect("transferring");
    transaction.advance().expect("released");
    assert_eq!(transaction.state(), InteractionState::Released);
    assert_eq!(transaction.effects(), None, "no effects before completion");

    // The wrong objective aborts the completion.
    let other = cs_types::content::ContentId::from_source(
        cs_types::content::ContentKind::Objective,
        "m01.other",
    )
    .expect("valid objective id");
    assert!(matches!(
        transaction.complete(&other, true),
        Err(InteractionRefusal::NotAuthorized { .. })
    ));
    assert_eq!(transaction.state(), InteractionState::Aborted);
    assert_eq!(transaction.effects(), None);

    // A destroyed target aborts too.
    let mut destroyed = synthetic_docking_transaction();
    destroyed.advance().expect("approaching");
    destroyed.advance().expect("eligible");
    destroyed.latch(Ok(sample)).expect("latch");
    destroyed.advance().expect("transferring");
    destroyed.advance().expect("released");
    assert_eq!(
        destroyed.complete(&synthetic_docking_objective(), false),
        Err(InteractionRefusal::TargetLost)
    );
    assert_eq!(destroyed.state(), InteractionState::Aborted);

    // The matching objective and a live target complete exactly once.
    let mut done = synthetic_docking_transaction();
    done.advance().expect("approaching");
    done.advance().expect("eligible");
    done.latch(Ok(sample)).expect("latch");
    done.advance().expect("transferring");
    done.advance().expect("released");
    let outcome = done
        .complete(&synthetic_docking_objective(), true)
        .expect("a live, authorized target completes");
    assert_eq!(outcome.completion, InteractionCompletion::Docked);
    assert_eq!(outcome.kind, InteractionKind::Docking);
    assert_eq!(outcome.id, synthetic_docking_id());
    let effects = outcome.effects;
    assert_eq!(effects.velocity, VelocityTransfer::MatchTarget);
    assert!(!effects.pilot_moved, "docking moves no pilot");
    assert!(!effects.inventory_moved, "docking moves no cargo");
    assert_eq!(effects.camera, CameraTransfer::FollowInitiator);
    assert_eq!(done.state(), InteractionState::Completed);
    assert_eq!(done.effects(), Some(effects));
    assert_eq!(
        done.complete(&synthetic_docking_objective(), true),
        Err(InteractionRefusal::AlreadyTerminal {
            state: InteractionState::Completed
        })
    );
}

/// An abort applies no effects, is idempotent, and cannot resume.
#[test]
fn accept_f36_a_abort_produces_no_effects_and_cannot_resume() {
    let (position, velocity) = aligned_approach();
    let sample = eligibility(position, velocity, 0.5).expect("eligible");

    let mut transaction = synthetic_docking_transaction();
    transaction.advance().expect("approaching");
    transaction.advance().expect("eligible");
    transaction.latch(Ok(sample)).expect("latch");

    let abort = transaction
        .abort(AbortReason::TargetDestroyed)
        .expect("abort from latching");
    assert_eq!(abort.from, InteractionState::Latching);
    assert_eq!(abort.reason, AbortReason::TargetDestroyed);
    assert_eq!(abort.id, synthetic_docking_id());
    assert_eq!(transaction.state(), InteractionState::Aborted);
    assert_eq!(transaction.effects(), None, "an abort transfers nothing");
    assert_eq!(
        transaction.abort(AbortReason::Pause),
        Err(InteractionRefusal::Aborted)
    );
    assert_eq!(transaction.advance(), Err(InteractionRefusal::Aborted));
    // A completed transaction refuses an abort too.
    let mut completed = synthetic_docking_transaction();
    completed.advance().expect("approaching");
    completed.advance().expect("eligible");
    completed.latch(Ok(sample)).expect("latch");
    completed.advance().expect("transferring");
    completed.advance().expect("released");
    completed
        .complete(&synthetic_docking_objective(), true)
        .expect("complete");
    assert_eq!(
        completed.abort(AbortReason::Retry),
        Err(InteractionRefusal::AlreadyTerminal {
            state: InteractionState::Completed
        })
    );
}

/// Exactly one control owner is active at every stage, and a dedicated latch
/// controller owns pose/control during latch and transfer.
#[test]
fn accept_f36_a_exactly_one_control_owner_through_the_transaction() {
    let (position, velocity) = aligned_approach();
    let sample = eligibility(position, velocity, 0.5).expect("eligible");

    let mut transaction = synthetic_docking_transaction();
    assert_eq!(transaction.control_owner(), ControlOwner::Initiator);
    transaction.advance().expect("approaching");
    assert_eq!(transaction.control_owner(), ControlOwner::Initiator);
    transaction.advance().expect("eligible");
    assert_eq!(transaction.control_owner(), ControlOwner::Initiator);
    transaction.latch(Ok(sample)).expect("latch");
    assert_eq!(transaction.control_owner(), ControlOwner::LatchController);
    transaction.advance().expect("transferring");
    assert_eq!(transaction.control_owner(), ControlOwner::LatchController);
    transaction.advance().expect("released");
    assert_eq!(
        transaction.control_owner(),
        transaction.policy().control_after_release
    );
    assert_eq!(transaction.control_owner(), ControlOwner::Target);
}

/// An aircraft swap moves the pilot and camera to the new actor; the docking
/// policy does not.
#[test]
fn accept_f36_a_aircraft_swap_policy_is_declared_per_transition() {
    let swap = TransferPolicy::aircraft_swap();
    assert_eq!(swap.pilot, PilotTransfer::MoveToTarget);
    assert_eq!(swap.velocity, VelocityTransfer::PreserveInitiator);
    let docking = TransferPolicy::docking();
    assert_eq!(docking.pilot, PilotTransfer::None);
    assert_ne!(swap, docking);
}

// ------------------------------------------------------------ schema -------

/// The envelope refuses a non-positive radius, negative speeds, an
/// out-of-range angle and a zero axis, and normalizes a valid axis.
#[test]
fn accept_f36_a_envelope_refuses_degenerate_values() {
    assert_eq!(
        EligibilityEnvelope::try_new(0.0, 10.0, 0.0, 35.0, [1.0, 0.0, 0.0]),
        Err(EnvelopeError::NonPositiveRadius { value: 0.0 })
    );
    assert_eq!(
        EligibilityEnvelope::try_new(f64::NAN, 10.0, 0.0, 35.0, [1.0, 0.0, 0.0]),
        Err(EnvelopeError::NonFinite {
            field: "capture_radius_m"
        })
    );
    assert_eq!(
        EligibilityEnvelope::try_new(5.0, -1.0, 0.0, 35.0, [1.0, 0.0, 0.0]),
        Err(EnvelopeError::NegativeSpeed {
            field: "max_relative_speed_m_s",
            value: -1.0
        })
    );
    assert_eq!(
        EligibilityEnvelope::try_new(5.0, 10.0, 0.0, 200.0, [1.0, 0.0, 0.0]),
        Err(EnvelopeError::AngleOutOfRange { value: 200.0 })
    );
    assert_eq!(
        EligibilityEnvelope::try_new(5.0, 10.0, 0.0, 35.0, [0.0, 0.0, 0.0]),
        Err(EnvelopeError::ZeroAxis)
    );

    let normalized =
        EligibilityEnvelope::try_new(5.0, 10.0, 0.0, 35.0, [2.0, 0.0, 0.0]).expect("valid");
    assert_eq!(normalized.approach_axis_local, [1.0, 0.0, 0.0]);
}

/// A non-finite input is refused rather than compared.
#[test]
fn accept_f36_a_non_finite_inputs_are_refused() {
    assert_eq!(
        eligibility([f64::NAN, 0.0, 0.0], [6.0, 0.0, 0.0], 0.5),
        Err(EligibilityRefusal::NonFinite)
    );
    assert_eq!(
        eligibility([-3.0, 0.0, 0.0], [6.0, 0.0, 0.0], f64::NAN),
        Err(EligibilityRefusal::NonFinite)
    );
}

/// The four kinds keep distinct labels and map to distinct completion events.
#[test]
fn accept_f36_a_kinds_map_to_distinct_completion_events() {
    let mut labels: Vec<&str> = InteractionKind::ALL.iter().map(|k| k.label()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), InteractionKind::ALL.len());
    for kind in InteractionKind::ALL {
        assert_eq!(InteractionKind::from_label(kind.label()), Some(*kind));
    }
    assert_eq!(
        InteractionKind::Docking.completion(),
        InteractionCompletion::Docked
    );
    assert_eq!(
        InteractionKind::PassengerPickup.completion(),
        InteractionCompletion::PassengersDelivered
    );
    assert_eq!(
        InteractionKind::Boarding.completion(),
        InteractionCompletion::Boarded
    );
    assert_eq!(
        InteractionKind::AircraftSwap.completion(),
        InteractionCompletion::AircraftSwapped
    );
    let mut completions: Vec<&str> = InteractionCompletion::ALL
        .iter()
        .map(|c| c.label())
        .collect();
    completions.sort_unstable();
    completions.dedup();
    assert_eq!(completions.len(), InteractionCompletion::ALL.len());
    assert_eq!(InteractionKind::from_label("nonsense"), None);
}

/// The id binds both actors and the session, and flags a foreign generation.
#[test]
fn accept_f36_a_id_binds_actors_and_session() {
    let id = InteractionId::new(
        7,
        3,
        ActorId {
            session: 7,
            serial: 1,
        },
        ActorId {
            session: 7,
            serial: 2,
        },
    );
    assert!(id.is_same_generation());
    let foreign = InteractionId::new(
        7,
        3,
        ActorId {
            session: 7,
            serial: 1,
        },
        ActorId {
            session: 8,
            serial: 9,
        },
    );
    assert!(!foreign.is_same_generation());
    assert_eq!(
        synthetic_docking_id(),
        InteractionId::new(
            7,
            1,
            ActorId {
                session: 7,
                serial: 1
            },
            ActorId {
                session: 7,
                serial: 2
            }
        )
    );
    assert!(
        synthetic_docking_authorization()
            .authorizes(InteractionKind::Docking, &synthetic_docking_objective())
    );
}
