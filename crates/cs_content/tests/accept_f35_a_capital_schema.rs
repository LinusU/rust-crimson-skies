//! Acceptance scenario F35-A: the declared, provenance-carrying capital-ship
//! schema — its validation, its `Resolved` discipline and the synthetic
//! fixture.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Task test prefix: `accept_f35_a_`.
//!
//! These tests drive production code only: [`cs_content::capital`]'s
//! [`DeclaredCapitalShip`], its parts, the validators and the
//! `declared_synthetic_capital_ship` fixture. Every value here is newly
//! authored synthetic fixture data, never original game data.

use cs_content::capital::{
    CapitalSchemaError, CapitalSubsystemEffect, CapitalSubsystemKey, CapitalSubsystemKeyError,
    CapitalSubsystemKind, DeclaredCapitalParts, DeclaredCapitalShip, DeclaredDockingAnchor,
    DeclaredEngine, DeclaredExposure, DeclaredKeyframe, DeclaredLaunchBay, DeclaredSection,
    DeclaredSubsystem, DeclaredTrajectory, DeclaredTurret, DeclaredWeaponBay, ExposureSchemaError,
    declared_synthetic_capital_ship,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

fn key(name: &str) -> CapitalSubsystemKey {
    CapitalSubsystemKey::new(name).expect("test subsystem keys are valid")
}

fn provenance() -> Provenance {
    Provenance::designed(ClaimId::new("f35a.test.schema").expect("valid"))
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(value, provenance()))
}

fn designed_socket(value: [f64; 3]) -> Resolved<[f64; 3]> {
    Resolved::Known(Known::new(value, provenance()))
}

fn designed_capacity(value: u32) -> Resolved<u32> {
    Resolved::Known(Known::new(value, provenance()))
}

fn unknown(value: &str) -> Resolved<ContentId> {
    Resolved::Unknown {
        claim_id: ClaimId::new(value).expect("valid"),
        reason: "unmeasured".to_owned(),
    }
}

fn owner() -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Faction, "synthetic.raiders").expect("valid"),
        provenance(),
    ))
}

fn valid_exposure() -> DeclaredExposure {
    DeclaredExposure {
        concealed_ticks: 10,
        opening_ticks: 2,
        exposed_ticks: 20,
        closing_ticks: 2,
    }
}

fn subsystem(
    name: &str,
    kind: CapitalSubsystemKind,
    effect: Option<CapitalSubsystemEffect>,
    lethal: bool,
) -> DeclaredSubsystem {
    DeclaredSubsystem {
        key: key(name),
        kind,
        effect,
        lethal,
    }
}

/// A minimal, valid, declared ship the negative cases mutate.
fn base_parts() -> DeclaredCapitalParts {
    DeclaredCapitalParts {
        subsystems: vec![
            subsystem(
                "engine_1",
                CapitalSubsystemKind::Engine,
                Some(CapitalSubsystemEffect::Propulsion),
                false,
            ),
            subsystem(
                "weapon_bay_1",
                CapitalSubsystemKind::WeaponBay,
                Some(CapitalSubsystemEffect::WeaponAccess),
                false,
            ),
            subsystem(
                "launch_bay_1",
                CapitalSubsystemKind::LaunchBay,
                Some(CapitalSubsystemEffect::Launching),
                false,
            ),
            subsystem(
                "turret_1",
                CapitalSubsystemKind::Turret,
                Some(CapitalSubsystemEffect::WeaponAccess),
                false,
            ),
            subsystem(
                "docking_anchor_1",
                CapitalSubsystemKind::DockingAnchor,
                Some(CapitalSubsystemEffect::Docking),
                false,
            ),
            subsystem(
                "gas_cell_1",
                CapitalSubsystemKind::GasCell,
                Some(CapitalSubsystemEffect::Vulnerability),
                true,
            ),
            subsystem(
                "keel",
                CapitalSubsystemKind::StructuralSection,
                Some(CapitalSubsystemEffect::MissionCondition),
                true,
            ),
        ],
        engines: vec![DeclaredEngine {
            key: key("engine_1"),
            thrust_n: designed(100.0),
            axis: [1.0, 0.0, 0.0],
        }],
        weapon_bays: vec![DeclaredWeaponBay {
            key: key("weapon_bay_1"),
            exposure: valid_exposure(),
        }],
        launch_bays: vec![DeclaredLaunchBay {
            key: key("launch_bay_1"),
            exposure: valid_exposure(),
            socket_offset_m: designed_socket([0.0, -5.0, 0.0]),
            capacity: designed_capacity(4),
        }],
        turrets: vec![DeclaredTurret {
            key: key("turret_1"),
            weapon: unknown("f35a.test.turret"),
            traverse_deg: designed(180.0),
            boresight: [0.0, 1.0, 0.0],
        }],
        docking_anchors: vec![DeclaredDockingAnchor {
            key: key("docking_anchor_1"),
            offset_m: designed_socket([0.0, 0.0, 20.0]),
        }],
        sections: vec![
            DeclaredSection {
                key: key("gas_cell_1"),
                integrity: designed(120.0),
            },
            DeclaredSection {
                key: key("keel"),
                integrity: designed(200.0),
            },
        ],
    }
}

fn ship(
    parts: DeclaredCapitalParts,
    cargo: Resolved<f64>,
) -> Result<DeclaredCapitalShip, CapitalSchemaError> {
    DeclaredCapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.leviathan").expect("valid"),
        Origin::SyntheticFixture,
        provenance(),
        None,
        parts,
        cargo,
        owner(),
    )
}

/// The declared synthetic fixture validates and carries every part the
/// deliverable names.
#[test]
fn accept_f35_a_declared_fixture_is_valid_and_distinct() {
    let declared = declared_synthetic_capital_ship();
    assert_eq!(declared.origin(), &Origin::SyntheticFixture);
    assert_eq!(declared.engines().len(), 2);
    assert_eq!(declared.weapon_bays().len(), 1);
    assert_eq!(declared.launch_bays().len(), 1);
    assert_eq!(declared.turrets().len(), 1);
    assert_eq!(declared.docking_anchors().len(), 1);
    assert_eq!(declared.sections().len(), 2);
    assert!(declared.trajectory().is_some());

    for kind in [
        CapitalSubsystemKind::Engine,
        CapitalSubsystemKind::WeaponBay,
        CapitalSubsystemKind::LaunchBay,
        CapitalSubsystemKind::Turret,
        CapitalSubsystemKind::DockingAnchor,
        CapitalSubsystemKind::GasCell,
        CapitalSubsystemKind::StructuralSection,
    ] {
        assert!(
            declared
                .subsystems()
                .iter()
                .any(|subsystem| subsystem.kind == kind),
            "the fixture keeps {kind} distinct"
        );
    }

    // The launch bay carries its socket transform, not just a key.
    let launch = &declared.launch_bays()[0];
    let Resolved::Known(socket) = &launch.socket_offset_m else {
        panic!("the launch socket is known in the fixture");
    };
    assert_eq!(socket.value, [0.0, -5.0, 0.0]);
}

/// Every load-bearing fixture value is known with designed provenance — a
/// declared fixture can never be mistaken for measured original data.
#[test]
fn accept_f35_a_fixture_values_carry_designed_provenance() {
    let declared = declared_synthetic_capital_ship();
    for engine in declared.engines() {
        let Resolved::Known(thrust) = &engine.thrust_n else {
            panic!("engine thrust is known in the fixture");
        };
        assert_eq!(thrust.provenance.class, ClaimStatus::Designed);
        assert!(thrust.value.is_finite() && thrust.value > 0.0);
    }
    let Resolved::Known(cargo) = declared.cargo() else {
        panic!("cargo is known in the fixture");
    };
    assert_eq!(cargo.provenance.class, ClaimStatus::Designed);
    let Resolved::Known(owner) = declared.ownership() else {
        panic!("the fixture declares its owner");
    };
    assert_eq!(owner.provenance.class, ClaimStatus::Designed);
    assert_eq!(owner.value.kind(), ContentKind::Faction);
}

/// An explicitly unknown value stays unknown: a turret without a measured
/// weapon keeps its claim and reason, and an unknown cargo capacity
/// validates as unknown.
#[test]
fn accept_f35_a_unknown_values_stay_unknown() {
    let declared = declared_synthetic_capital_ship();
    let Resolved::Unknown { claim_id, .. } = &declared.turrets()[0].weapon else {
        panic!("the fixture turret weapon is unresolved");
    };
    assert_eq!(claim_id.as_str(), "f35a.turret-weapon-unmeasured");

    let mut parts = base_parts();
    parts.engines[0].thrust_n = Resolved::Unknown {
        claim_id: ClaimId::new("f35a.test.unknown-thrust").expect("valid"),
        reason: "engine thrust unmeasured".to_owned(),
    };
    let declared = ship(
        parts,
        Resolved::Unknown {
            claim_id: ClaimId::new("f35a.test.unknown-cargo").expect("valid"),
            reason: "cargo capacity unmeasured".to_owned(),
        },
    )
    .expect("unknown values validate");
    assert!(matches!(
        declared.engines()[0].thrust_n,
        Resolved::Unknown { .. }
    ));
    assert!(matches!(declared.cargo(), Resolved::Unknown { .. }));
}

/// Validation refuses duplicate and dangling identities and kind mismatches
/// — the declared rule cannot drift from the runtime's.
#[test]
fn accept_f35_a_schema_rejects_duplicate_and_dangling_keys() {
    // No parts at all.
    let mut parts = base_parts();
    parts.subsystems.clear();
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::EmptyShip)
    );

    // Two shared identities share one key.
    let mut parts = base_parts();
    parts.subsystems.push(subsystem(
        "engine_1",
        CapitalSubsystemKind::Engine,
        None,
        false,
    ));
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::DuplicateKey {
            key: key("engine_1")
        })
    );

    // A detail record names no subsystem.
    let mut parts = base_parts();
    parts.engines[0].key = key("ghost");
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::UnknownSubsystem { key: key("ghost") })
    );

    // A detail record's kind disagrees with the subsystem's.
    let mut parts = base_parts();
    parts.engines[0].key = key("weapon_bay_1");
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::KindMismatch {
            key: key("weapon_bay_1"),
            expected: CapitalSubsystemKind::Engine,
            actual: CapitalSubsystemKind::WeaponBay,
        })
    );

    // Two detail records reuse one subsystem key.
    let mut parts = base_parts();
    parts.engines.push(DeclaredEngine {
        key: key("engine_1"),
        thrust_n: designed(2.0),
        axis: [1.0, 0.0, 0.0],
    });
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::DuplicateKey {
            key: key("engine_1")
        })
    );
}

/// Validation refuses effects a kind cannot carry and lethal parts of a
/// non-structural kind.
#[test]
fn accept_f35_a_schema_rejects_bad_effects_and_lethal_kinds() {
    let mut parts = base_parts();
    parts.subsystems[0].effect = Some(CapitalSubsystemEffect::Launching);
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::EffectKindMismatch {
            key: key("engine_1"),
            kind: CapitalSubsystemKind::Engine,
            effect: CapitalSubsystemEffect::Launching,
        })
    );

    let mut parts = base_parts();
    parts.subsystems[1].lethal = true;
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::LethalKind {
            key: key("weapon_bay_1"),
            kind: CapitalSubsystemKind::WeaponBay,
        })
    );
}

/// Known numbers are validated, never clamped: a zero axis, a zero capacity,
/// a corrupt cargo pool and an invalid exposure window are all refused.
#[test]
fn accept_f35_a_schema_rejects_corrupt_numbers() {
    let mut parts = base_parts();
    parts.engines[0].axis = [0.0, 0.0, 0.0];
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::ZeroAxis {
            key: key("engine_1")
        })
    );

    let mut parts = base_parts();
    parts.engines[0].axis = [f64::NAN, 0.0, 0.0];
    assert!(matches!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::NonFinite { field: "axis", .. })
    ));

    let mut parts = base_parts();
    parts.engines[0].thrust_n = designed(-1.0);
    assert!(matches!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::Negative {
            field: "thrust_n",
            value: -1.0,
            ..
        })
    ));

    let mut parts = base_parts();
    parts.launch_bays[0].capacity = designed_capacity(0);
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::ZeroCapacity {
            key: key("launch_bay_1")
        })
    );

    // Cargo cannot be negative or corrupt.
    let mut parts = base_parts();
    parts.sections.clear();
    assert_eq!(
        ship(parts, designed(-1.0)),
        Err(CapitalSchemaError::Negative {
            key: "capital.cargo".to_owned(),
            field: "cargo",
            value: -1.0,
        })
    );

    // An exposure window with no open phase.
    let mut parts = base_parts();
    parts.weapon_bays[0].exposure.exposed_ticks = 0;
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::Exposure {
            key: key("weapon_bay_1"),
            source: ExposureSchemaError::NoExposedTicks,
        })
    );

    // A zero-length cycle.
    let zero = DeclaredExposure {
        concealed_ticks: 0,
        opening_ticks: 0,
        exposed_ticks: 0,
        closing_ticks: 0,
    };
    assert_eq!(zero.validate(), Err(ExposureSchemaError::ZeroCycle));
}

/// A declared cycle whose phases sum past the tick counter is refused, so a
/// window the runtime cannot represent is never accepted by the schema.
#[test]
fn accept_f35_a_schema_rejects_overflowing_exposure() {
    let overflow = DeclaredExposure {
        concealed_ticks: u64::MAX,
        opening_ticks: 0,
        exposed_ticks: 1,
        closing_ticks: 0,
    };
    assert_eq!(overflow.validate(), Err(ExposureSchemaError::CycleOverflow));

    // The boundary refuses it too, rather than lowering a wrapped cycle.
    let mut parts = base_parts();
    parts.weapon_bays[0].exposure = overflow;
    assert_eq!(
        ship(parts, designed(1.0)),
        Err(CapitalSchemaError::Exposure {
            key: key("weapon_bay_1"),
            source: ExposureSchemaError::CycleOverflow,
        })
    );
}

/// A declared trajectory is carried whole, including its tick rate and keys.
#[test]
fn accept_f35_a_declared_trajectory_is_carried() {
    let trajectory = DeclaredTrajectory {
        ticks_per_second: 10,
        keyframes: vec![
            DeclaredKeyframe {
                tick: 0,
                position_m: [0.0, 0.0, 0.0],
                orientation: [0.0, 0.0, 0.0, 1.0],
            },
            DeclaredKeyframe {
                tick: 500,
                position_m: [1000.0, 0.0, 0.0],
                orientation: [0.0, 0.0, 0.0, 1.0],
            },
        ],
    };
    let declared = DeclaredCapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.leviathan").expect("valid"),
        Origin::SyntheticFixture,
        provenance(),
        Some(trajectory.clone()),
        base_parts(),
        designed(1000.0),
        owner(),
    )
    .expect("a ship with a trajectory validates");
    assert_eq!(declared.trajectory(), Some(&trajectory));
}

/// Node keys share the content-id grammar: lowercased, bounded, no path
/// separators.
#[test]
fn accept_f35_a_key_grammar_matches_content_id_discipline() {
    assert_eq!(
        CapitalSubsystemKey::new(""),
        Err(CapitalSubsystemKeyError::Empty)
    );
    assert_eq!(
        CapitalSubsystemKey::new("UPPER_CASE"),
        Ok(key("upper_case"))
    );
    assert!(matches!(
        CapitalSubsystemKey::new("a/b"),
        Err(CapitalSubsystemKeyError::BadCharacter { ch: '/' })
    ));
    assert!(matches!(
        CapitalSubsystemKey::new("..."),
        Err(CapitalSubsystemKeyError::NoAlphanumeric)
    ));
    assert!(matches!(
        CapitalSubsystemKey::new(&"x".repeat(129)),
        Err(CapitalSubsystemKeyError::TooLong { len: 129 })
    ));
}
