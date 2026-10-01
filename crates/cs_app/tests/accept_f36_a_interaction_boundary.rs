//! Acceptance scenario F36-A: the declared → runtime conversion boundary and
//! the ECS binding record.
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`. Task test prefix: `accept_f36_a_`.
//!
//! These tests drive production code only: [`cs_app::interaction`]'s
//! [`lower_interaction`] and [`InteractionActorBinding`], plus the
//! `cs_sim::interaction` runtime the lowered records produce — the F36-A
//! minimum scenario runs end-to-end through the lowered envelope, so removing
//! the conversion or guessing an unknown value fails them.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_app::interaction::{InteractionActorBinding, InteractionLowerError, lower_interaction};
use cs_app::scene::SceneGeneration;
use cs_content::interaction::{
    DeclaredCameraTransfer, DeclaredControlOwner, DeclaredEligibility, DeclaredInteraction,
    DeclaredInteractionKind, DeclaredInventoryTransfer, DeclaredPilotTransfer,
    DeclaredTransferPolicy, DeclaredVelocityTransfer, declared_synthetic_interaction,
};
use cs_sim::damage::ActorId;
use cs_sim::interaction::{
    CameraTransfer, ControlOwner, EligibilityRefusal, InteractionId, InteractionKind,
    InteractionState, InteractionTransaction, LatchRefusal, PilotTransfer, TransferPolicy,
    VelocityTransfer, evaluate_eligibility, synthetic_docking_anchor, synthetic_hook_trajectory,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("the test claim id is valid")
}

fn provenance() -> Provenance {
    Provenance::designed(claim("f36a.test.boundary"))
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(value, provenance()))
}

fn actor(serial: u64) -> ActorId {
    ActorId { session: 7, serial }
}

fn identity() -> InteractionId {
    InteractionId::new(7, 1, actor(1), actor(2))
}

/// A minimal declared docking interaction the unknown cases mutate.
fn minimal_declared(
    authorization: Resolved<ContentId>,
    capture_radius_m: Resolved<f64>,
) -> DeclaredInteraction {
    DeclaredInteraction::try_new(
        ContentId::from_source(ContentKind::Mission, "synthetic.interaction").expect("valid"),
        Origin::SyntheticFixture,
        provenance(),
        DeclaredInteractionKind::Docking,
        authorization,
        DeclaredEligibility {
            capture_radius_m,
            max_relative_speed_m_s: designed(10.0),
            min_closing_speed_m_s: designed(0.0),
            max_approach_angle_deg: designed(35.0),
            approach_axis_local: Resolved::Known(Known::new([1.0, 0.0, 0.0], provenance())),
        },
        DeclaredTransferPolicy {
            velocity: DeclaredVelocityTransfer::MatchTarget,
            pilot: DeclaredPilotTransfer::None,
            inventory: DeclaredInventoryTransfer::None,
            camera: DeclaredCameraTransfer::FollowInitiator,
            control_after_release: DeclaredControlOwner::Target,
        },
    )
    .expect("the minimal declared interaction is valid")
}

fn known_objective() -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Objective, "m01.dock").expect("valid objective"),
        provenance(),
    ))
}

/// `lower_interaction` preserves the whole declared structure field-wise.
#[test]
fn accept_f36_a_lower_preserves_the_declared_structure() {
    let declared = declared_synthetic_interaction();
    let plan = lower_interaction(identity(), &declared).expect("the fixture lowers");

    assert_eq!(plan.id, identity());
    assert_eq!(plan.kind, InteractionKind::Docking);
    assert_eq!(plan.authorization.session, 7);
    assert_eq!(plan.authorization.kind, InteractionKind::Docking);
    assert_eq!(plan.authorization.objective, declared_objective(&declared));
    assert_eq!(plan.envelope.capture_radius_m, 5.0);
    assert_eq!(plan.envelope.max_relative_speed_m_s, 10.0);
    assert_eq!(plan.envelope.max_approach_angle_deg, 35.0);
    assert_eq!(plan.envelope.approach_axis_local, [1.0, 0.0, 0.0]);
    assert_eq!(plan.policy.velocity, VelocityTransfer::MatchTarget);
    assert_eq!(plan.policy.pilot, PilotTransfer::None);
    assert_eq!(plan.policy.camera, CameraTransfer::FollowInitiator);
    assert_eq!(plan.policy.control_after_release, ControlOwner::Target);
}

fn declared_objective(declared: &DeclaredInteraction) -> ContentId {
    declared
        .authorization()
        .clone()
        .known()
        .expect("the fixture authorization is known")
}

/// The lowered plan runs the F36-A minimum scenario end-to-end: the same
/// envelope that refuses a fast pass or a wrong-direction pass latches an
/// aligned one.
#[test]
fn accept_f36_a_lowered_plan_runs_the_minimum_scenario() {
    let plan = lower_interaction(identity(), &declared_synthetic_interaction())
        .expect("the fixture lowers");

    let mut transaction =
        InteractionTransaction::begin(plan.id, plan.kind, plan.authorization.clone(), plan.policy);
    transaction.advance().expect("approaching");
    transaction.advance().expect("eligible");

    // Too fast: refused and not latched.
    let fast = evaluate_eligibility(
        Tick(0),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        [-3.0, 0.0, 0.0],
        [80.0, 0.0, 0.0],
        0.1,
        &plan.envelope,
    )
    .expect_err("too fast is refused");
    assert!(matches!(fast, EligibilityRefusal::TooFast { .. }));
    assert_eq!(
        transaction.latch(Err(fast)),
        Err(LatchRefusal::NotEligible(fast))
    );
    assert_eq!(transaction.state(), InteractionState::Eligible);

    // Wrong direction: refused and not latched.
    let wrong = evaluate_eligibility(
        Tick(0),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        [-3.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        0.5,
        &plan.envelope,
    )
    .expect_err("wrong direction is refused");
    assert!(matches!(wrong, EligibilityRefusal::WrongDirection { .. }));
    assert!(transaction.latch(Err(wrong)).is_err());
    assert_eq!(transaction.state(), InteractionState::Eligible);

    // Aligned and slow: latches through the lowered envelope.
    let sample = evaluate_eligibility(
        Tick(0),
        &synthetic_hook_trajectory(),
        &synthetic_docking_anchor(),
        [-3.0, 0.0, 0.0],
        [6.0, 0.0, 0.0],
        0.5,
        &plan.envelope,
    )
    .expect("aligned is eligible");
    transaction.latch(Ok(sample)).expect("latches");
    assert_eq!(transaction.state(), InteractionState::Latching);
    assert_eq!(transaction.control_owner(), ControlOwner::LatchController);
}

/// An unknown authorization or an unknown eligibility field refuses to lower
/// rather than inventing a value.
#[test]
fn accept_f36_a_unknown_values_refuse_to_lower() {
    let unknown_objective: Resolved<ContentId> = Resolved::Unknown {
        claim_id: claim("f36a.unknown.objective"),
        reason: "no observed authorizing objective".to_owned(),
    };
    assert_eq!(
        lower_interaction(
            identity(),
            &minimal_declared(unknown_objective, designed(5.0))
        )
        .err(),
        Some(InteractionLowerError::UnknownAuthorization {
            claim_id: claim("f36a.unknown.objective"),
            reason: "no observed authorizing objective".to_owned(),
        })
    );

    let unknown_radius: Resolved<f64> = Resolved::Unknown {
        claim_id: claim("f36a.unknown.radius"),
        reason: "the original capture radius is unmeasured".to_owned(),
    };
    assert_eq!(
        lower_interaction(
            identity(),
            &minimal_declared(known_objective(), unknown_radius)
        )
        .err(),
        Some(InteractionLowerError::UnknownEligibility {
            field: "capture_radius_m",
            claim_id: claim("f36a.unknown.radius"),
            reason: "the original capture radius is unmeasured".to_owned(),
        })
    );
}

/// The binding record is generation-stamped, so a reload identifies a stale
/// binding by generation mismatch instead of a surviving pointer.
#[test]
fn accept_f36_a_binding_is_generation_stamped() {
    let binding = InteractionActorBinding {
        actor: actor(1),
        subject: ContentId::from_source(ContentKind::Mission, "synthetic.interaction")
            .expect("valid"),
        generation: SceneGeneration(4),
    };
    let reloaded = InteractionActorBinding {
        generation: SceneGeneration(5),
        ..binding.clone()
    };
    assert_eq!(binding.actor, actor(1));
    assert_eq!(binding.generation, SceneGeneration(4));
    assert_ne!(binding.generation, reloaded.generation);
    assert_eq!(binding.subject, reloaded.subject);
}

/// The transfer policy round-trips through the boundary, so an aircraft swap
/// keeps moving the pilot while a docking does not.
#[test]
fn accept_f36_a_policy_round_trips_through_the_boundary() {
    let declared = declared_synthetic_interaction();
    let plan = lower_interaction(identity(), &declared).expect("lowers");
    assert_ne!(plan.policy, TransferPolicy::aircraft_swap());
    assert_eq!(plan.policy, TransferPolicy::docking());
}
