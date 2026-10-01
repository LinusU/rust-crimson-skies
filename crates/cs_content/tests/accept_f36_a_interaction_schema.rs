//! Acceptance scenario F36-A: the declared, provenance-carrying interaction
//! schema — its validation, its `Resolved` discipline and the synthetic
//! fixture.
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`. Task test prefix: `accept_f36_a_`.
//!
//! These tests drive production code only: [`cs_content::interaction`]'s
//! [`DeclaredInteraction`], its envelope, its validators and the
//! `declared_synthetic_interaction` fixture. Every value here is newly
//! authored synthetic fixture data, never original game data.

use cs_content::interaction::{
    DeclaredCameraTransfer, DeclaredControlOwner, DeclaredEligibility, DeclaredInteraction,
    DeclaredInteractionCompletion, DeclaredInteractionKind, DeclaredInventoryTransfer,
    DeclaredPilotTransfer, DeclaredTransferPolicy, DeclaredVelocityTransfer,
    EligibilitySchemaError, InteractionSchemaError, declared_synthetic_interaction,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("the test claim id is valid")
}

fn provenance() -> Provenance {
    Provenance::designed(claim("f36a.test.schema"))
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(value, provenance()))
}

fn known_axis(value: [f64; 3]) -> Resolved<[f64; 3]> {
    Resolved::Known(Known::new(value, provenance()))
}

fn known_objective() -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Objective, "m01.dock").expect("valid objective"),
        provenance(),
    ))
}

fn envelope() -> DeclaredEligibility {
    DeclaredEligibility {
        capture_radius_m: designed(5.0),
        max_relative_speed_m_s: designed(10.0),
        min_closing_speed_m_s: designed(0.0),
        max_approach_angle_deg: designed(35.0),
        approach_axis_local: known_axis([1.0, 0.0, 0.0]),
    }
}

fn policy() -> DeclaredTransferPolicy {
    DeclaredTransferPolicy {
        velocity: DeclaredVelocityTransfer::MatchTarget,
        pilot: DeclaredPilotTransfer::None,
        inventory: DeclaredInventoryTransfer::None,
        camera: DeclaredCameraTransfer::FollowInitiator,
        control_after_release: DeclaredControlOwner::Target,
    }
}

fn base() -> DeclaredInteraction {
    DeclaredInteraction::try_new(
        ContentId::from_source(ContentKind::Mission, "synthetic.interaction")
            .expect("valid subject"),
        Origin::SyntheticFixture,
        provenance(),
        DeclaredInteractionKind::Docking,
        known_objective(),
        envelope(),
        policy(),
    )
    .expect("the base declared interaction is valid")
}

/// The fixture is a valid, synthetic docking record with a designed envelope.
#[test]
fn accept_f36_a_declared_fixture_is_valid_and_distinct() {
    let fixture = declared_synthetic_interaction();
    assert_eq!(fixture.kind(), DeclaredInteractionKind::Docking);
    assert_eq!(fixture.completion(), DeclaredInteractionCompletion::Docked);
    assert_eq!(fixture.origin(), &Origin::SyntheticFixture);
    assert_eq!(
        fixture.provenance().class,
        cs_types::evidence::ClaimStatus::Designed
    );
    assert_eq!(
        fixture.envelope().capture_radius_m.clone().known(),
        Some(5.0)
    );
    assert_eq!(
        fixture.transfer().velocity,
        DeclaredVelocityTransfer::MatchTarget
    );
    assert_eq!(
        fixture.transfer().control_after_release,
        DeclaredControlOwner::Target
    );
    assert!(fixture.envelope().validate().is_ok());

    let derived = base();
    assert_eq!(fixture.subject(), derived.subject());
    assert_eq!(fixture.kind(), derived.kind());
}

/// Every load-bearing value can be an explicit unknown, and it stays one:
/// validation passes and nothing is defaulted in its place.
#[test]
fn accept_f36_a_unknown_values_stay_unknown() {
    let unknown_axis: Resolved<[f64; 3]> = Resolved::Unknown {
        claim_id: claim("f36a.unknown.axis"),
        reason: "the original docking axis is unmeasured".to_owned(),
    };
    let unknown_objective: Resolved<ContentId> = Resolved::Unknown {
        claim_id: claim("f36a.unknown.objective"),
        reason: "no observed authorizing objective".to_owned(),
    };
    let unknown_radius: Resolved<f64> = Resolved::Unknown {
        claim_id: claim("f36a.unknown.radius"),
        reason: "the original capture radius is unmeasured".to_owned(),
    };

    let declared = DeclaredInteraction::try_new(
        ContentId::from_source(ContentKind::Mission, "synthetic.interaction").expect("valid"),
        Origin::Designed,
        provenance(),
        DeclaredInteractionKind::Docking,
        unknown_objective,
        DeclaredEligibility {
            capture_radius_m: unknown_radius,
            ..envelope()
        },
        policy(),
    )
    .expect("unknown values are valid records");
    assert!(matches!(declared.authorization(), Resolved::Unknown { .. }));
    assert!(matches!(
        declared.envelope().capture_radius_m,
        Resolved::Unknown { .. }
    ));
    assert!(declared.envelope().validate().is_ok());

    // A zero-axis and a non-finite value are refused, not silently defaulted.
    let bad_axis: Resolved<[f64; 3]> = known_axis([0.0, 0.0, 0.0]);
    assert_eq!(
        DeclaredEligibility {
            approach_axis_local: bad_axis,
            ..envelope()
        }
        .validate(),
        Err(EligibilitySchemaError::ZeroAxis)
    );
    assert_eq!(
        DeclaredEligibility {
            capture_radius_m: designed(f64::NAN),
            ..envelope()
        }
        .validate(),
        Err(EligibilitySchemaError::NonFinite {
            field: "capture_radius_m"
        })
    );
    assert_eq!(
        unknown_axis,
        Resolved::<[f64; 3]>::Unknown {
            claim_id: claim("f36a.unknown.axis"),
            reason: "the original docking axis is unmeasured".to_owned(),
        }
    );
}

/// The envelope validator refuses a non-positive radius, negative speeds and
/// an out-of-range angle.
#[test]
fn accept_f36_a_schema_rejects_bad_envelope_values() {
    assert_eq!(
        DeclaredEligibility {
            capture_radius_m: designed(0.0),
            ..envelope()
        }
        .validate(),
        Err(EligibilitySchemaError::NonPositiveRadius { value: 0.0 })
    );
    assert_eq!(
        DeclaredEligibility {
            max_relative_speed_m_s: designed(-1.0),
            ..envelope()
        }
        .validate(),
        Err(EligibilitySchemaError::NegativeSpeed {
            field: "max_relative_speed_m_s",
            value: -1.0
        })
    );
    assert_eq!(
        DeclaredEligibility {
            min_closing_speed_m_s: designed(-2.0),
            ..envelope()
        }
        .validate(),
        Err(EligibilitySchemaError::NegativeSpeed {
            field: "min_closing_speed_m_s",
            value: -2.0
        })
    );
    assert_eq!(
        DeclaredEligibility {
            max_approach_angle_deg: designed(181.0),
            ..envelope()
        }
        .validate(),
        Err(EligibilitySchemaError::AngleOutOfRange { value: 181.0 })
    );

    // `try_new` surfaces the same refusal.
    assert_eq!(
        DeclaredInteraction::try_new(
            ContentId::from_source(ContentKind::Mission, "synthetic.interaction").expect("valid"),
            Origin::SyntheticFixture,
            provenance(),
            DeclaredInteractionKind::Docking,
            known_objective(),
            DeclaredEligibility {
                capture_radius_m: designed(-5.0),
                ..envelope()
            },
            policy(),
        ),
        Err(InteractionSchemaError::Eligibility(
            EligibilitySchemaError::NonPositiveRadius { value: -5.0 }
        ))
    );
}

/// A known authorization must be an objective; a non-objective id is refused
/// rather than treated as a mission phase.
#[test]
fn accept_f36_a_authorization_must_be_an_objective() {
    let not_objective = Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.fighter").expect("valid"),
        provenance(),
    ));
    assert_eq!(
        DeclaredInteraction::try_new(
            ContentId::from_source(ContentKind::Mission, "synthetic.interaction").expect("valid"),
            Origin::SyntheticFixture,
            provenance(),
            DeclaredInteractionKind::Docking,
            not_objective,
            envelope(),
            policy(),
        )
        .err(),
        Some(InteractionSchemaError::AuthorizationNotObjective {
            id: ContentId::from_source(ContentKind::Airframe, "synthetic.fighter").expect("valid"),
        })
    );
}

/// The kind labels and completion events are distinct and round-trip.
#[test]
fn accept_f36_a_kind_labels_and_completions_are_distinct() {
    let mut labels: Vec<&str> = DeclaredInteractionKind::ALL
        .iter()
        .map(|kind| kind.label())
        .collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), DeclaredInteractionKind::ALL.len());
    for kind in DeclaredInteractionKind::ALL {
        assert_eq!(
            DeclaredInteractionKind::from_label(kind.label()),
            Some(*kind)
        );
    }
    assert_eq!(
        DeclaredInteractionKind::AircraftSwap.completion(),
        DeclaredInteractionCompletion::AircraftSwapped
    );
    assert_eq!(
        DeclaredInteractionKind::PassengerPickup.completion(),
        DeclaredInteractionCompletion::PassengersDelivered
    );
    assert_eq!(DeclaredInteractionCompletion::Boarded.label(), "boarded");
}
