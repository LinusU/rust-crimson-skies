//! Acceptance scenario F35-A: the declared → runtime conversion boundary
//! and the ECS binding record.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Task test prefix: `accept_f35_a_`.
//!
//! These tests drive production code only: [`cs_app::capital`]'s
//! [`lower_capital_ship`] and [`CapitalActorBinding`], plus the
//! `cs_sim::capital::CapitalShip` the lowered records produce — the F35-A
//! minimum scenario runs end-to-end through the lowered ship, so removing
//! the conversion or silently defaulting an unknown value fails them.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_app::capital::{CapitalActorBinding, CapitalLowerError, lower_capital_ship};
use cs_app::scene::SceneGeneration;
use cs_content::capital::{
    CapitalSubsystemEffect, CapitalSubsystemKey, CapitalSubsystemKind, DeclaredCapitalParts,
    DeclaredCapitalShip, DeclaredEngine, DeclaredSection, DeclaredSubsystem,
    declared_synthetic_capital_ship,
};
use cs_script::ir::ActorId;
use cs_sim::capital::{PropulsionError, SYNTHETIC_MASS_KG, SubsystemKey, synthetic_capital_ship};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn key(name: &str) -> SubsystemKey {
    SubsystemKey::new(name).expect("test subsystem keys are valid")
}

fn declared_key(name: &str) -> CapitalSubsystemKey {
    CapitalSubsystemKey::new(name).expect("test subsystem keys are valid")
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f35a.test.boundary").expect("valid")),
    ))
}

fn designed_owner(value: &str) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Faction, value).expect("valid"),
        Provenance::designed(ClaimId::new("f35a.test.boundary").expect("valid")),
    ))
}

/// A minimal declared ship the unknown-ownership case mutates.
fn minimal_declared(ownership: Resolved<ContentId>) -> DeclaredCapitalShip {
    DeclaredCapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.leviathan").expect("valid"),
        Origin::SyntheticFixture,
        Provenance::designed(ClaimId::new("f35a.test.boundary").expect("valid")),
        None,
        DeclaredCapitalParts {
            subsystems: vec![
                DeclaredSubsystem {
                    key: declared_key("engine_1"),
                    kind: CapitalSubsystemKind::Engine,
                    effect: Some(CapitalSubsystemEffect::Propulsion),
                    lethal: false,
                },
                DeclaredSubsystem {
                    key: declared_key("keel"),
                    kind: CapitalSubsystemKind::StructuralSection,
                    effect: Some(CapitalSubsystemEffect::MissionCondition),
                    lethal: true,
                },
            ],
            engines: vec![DeclaredEngine {
                key: declared_key("engine_1"),
                thrust_n: designed(400_000.0),
                axis: [1.0, 0.0, 0.0],
            }],
            weapon_bays: vec![],
            launch_bays: vec![],
            turrets: vec![],
            docking_anchors: vec![],
            sections: vec![DeclaredSection {
                key: declared_key("keel"),
                integrity: designed(200.0),
            }],
        },
        designed(5_000.0),
        ownership,
    )
    .expect("the minimal declared ship is valid")
}

/// `lower_capital_ship` preserves the whole declared structure: every
/// subsystem, engine, bay, turret, anchor, the cargo pool, ownership and the
/// authored trajectory map field-wise; nothing is dropped or repaired.
#[test]
fn accept_f35_a_lower_preserves_the_declared_structure() {
    let declared = declared_synthetic_capital_ship();
    let ship = lower_capital_ship(ActorId(1), &declared).expect("the fixture lowers");

    assert_eq!(ship.subject(), declared.subject());
    assert_eq!(ship.graph().len(), declared.subsystems().len());
    assert_eq!(ship.engines().count(), declared.engines().len());
    assert_eq!(ship.turrets().count(), declared.turrets().len());
    assert_eq!(
        ship.docking_anchors().count(),
        declared.docking_anchors().len()
    );
    assert_eq!(ship.cargo(), declared.cargo());
    assert!(ship.trajectory().is_some());

    // The unknown turret weapon lowers as the same unknown, by claim.
    let turret = ship.turrets().next().expect("a turret lowers");
    let Resolved::Unknown { claim_id, .. } = &turret.weapon else {
        panic!("the unknown weapon stayed unknown");
    };
    assert_eq!(claim_id.as_str(), "f35a.turret-weapon-unmeasured");

    // Ownership follows the declared owner.
    let Resolved::Known(declared_owner) = declared.ownership() else {
        panic!("the fixture has a known owner");
    };
    assert_eq!(&ship.ownership().owner, &declared_owner.value);

    // The bay exposure cycle survives the boundary.
    assert_eq!(
        ship.bay_state(&key("weapon_bay_1"), cs_types::Tick(50)),
        Some(cs_sim::capital::BayState::Exposed)
    );
}

/// The F35-A minimum scenario end-to-end through the boundary: lower the
/// declared fixture, disable both engines and measure the motion response
/// while the hull stays intact.
#[test]
fn accept_f35_a_lowered_ship_measures_engine_loss_end_to_end() {
    let declared = declared_synthetic_capital_ship();
    let mut ship = lower_capital_ship(ActorId(1), &declared).expect("lowers");

    let intact = ship
        .propulsive_acceleration_m_s2(1.0, SYNTHETIC_MASS_KG)
        .expect("intact engines compute");
    assert!(intact[0] > 0.0);

    ship.disable(&key("engine_1")).expect("known engine");
    let one = ship
        .propulsive_acceleration_m_s2(1.0, SYNTHETIC_MASS_KG)
        .expect("one engine computes");
    assert!(one[0] > 0.0 && one[0] < intact[0]);

    ship.disable(&key("engine_2")).expect("known engine");
    let none = ship
        .propulsive_acceleration_m_s2(1.0, SYNTHETIC_MASS_KG)
        .expect("no engine computes zero");
    assert_eq!(none, [0.0; 3]);

    assert!(!ship.is_destroyed());
    assert_eq!(
        ship.subsystem_state(&key("keel")),
        Some(cs_sim::capital::SubsystemState::Intact)
    );
}

/// An unknown initial owner is refused by claim: no session switches guns,
/// targeting or docking eligibility under a guessed owner.
#[test]
fn accept_f35_a_lower_refuses_unknown_ownership() {
    let claim = ClaimId::new("f35a.test.unknown-owner").expect("valid");
    let declared = minimal_declared(Resolved::Unknown {
        claim_id: claim.clone(),
        reason: "initial owner unmeasured".to_owned(),
    });
    assert_eq!(
        lower_capital_ship(ActorId(1), &declared),
        Err(CapitalLowerError::UnknownOwnership {
            claim_id: claim,
            reason: "initial owner unmeasured".to_owned(),
        })
    );

    // A known owner lowers.
    let declared = minimal_declared(designed_owner("synthetic.raiders"));
    assert!(lower_capital_ship(ActorId(1), &declared).is_ok());
}

/// An unresolved engine thrust lowers through verbatim: the runtime refuses
/// to compute a total rather than treating the engine as zero.
#[test]
fn accept_f35_a_unknown_thrust_lowers_through_verbatim() {
    // The declared record is immutable after construction; the runtime's
    // refusal is exercised by the synthetic ship path below and by the
    // content schema, so build the unknown case through the declared
    // constructor instead.
    let declared = DeclaredCapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.leviathan").expect("valid"),
        Origin::SyntheticFixture,
        Provenance::designed(ClaimId::new("f35a.test.boundary").expect("valid")),
        None,
        DeclaredCapitalParts {
            subsystems: vec![DeclaredSubsystem {
                key: declared_key("engine_1"),
                kind: CapitalSubsystemKind::Engine,
                effect: Some(CapitalSubsystemEffect::Propulsion),
                lethal: false,
            }],
            engines: vec![DeclaredEngine {
                key: declared_key("engine_1"),
                thrust_n: Resolved::Unknown {
                    claim_id: ClaimId::new("f35a.test.unknown-thrust").expect("valid"),
                    reason: "engine thrust unmeasured".to_owned(),
                },
                axis: [1.0, 0.0, 0.0],
            }],
            weapon_bays: vec![],
            launch_bays: vec![],
            turrets: vec![],
            docking_anchors: vec![],
            sections: vec![],
        },
        designed(5_000.0),
        designed_owner("synthetic.raiders"),
    )
    .expect("an unknown thrust is a valid declared record");

    let ship = lower_capital_ship(ActorId(1), &declared).expect("the record lowers");
    assert!(matches!(
        ship.engines().next().expect("one engine").thrust_n,
        Resolved::Unknown { .. }
    ));
    assert_eq!(
        ship.engine_thrust_n(),
        Err(PropulsionError::UnknownThrust {
            key: key("engine_1")
        })
    );
    assert!(ship.propulsive_force_n(1.0).is_err());
}

/// The binding record ties an entity's actor, subject and generation: a
/// reload under a new generation produces a distinguishable record.
#[test]
fn accept_f35_a_capital_actor_binding_is_generation_qualified() {
    let declared = declared_synthetic_capital_ship();
    let subject = declared.subject().clone();
    let binding = CapitalActorBinding {
        actor: ActorId(1),
        subject: subject.clone(),
        generation: SceneGeneration(1),
    };
    let reloaded = CapitalActorBinding {
        generation: SceneGeneration(1).next(),
        ..binding.clone()
    };
    assert_ne!(binding, reloaded, "a reload stamps a new generation");
    assert_eq!(binding.actor, ActorId(1));
    assert_eq!(binding.subject, subject);

    // The synthetic ship's subject matches the declared fixture's, so the
    // binding keys the same content.
    assert_eq!(synthetic_capital_ship().subject(), &subject);
}
