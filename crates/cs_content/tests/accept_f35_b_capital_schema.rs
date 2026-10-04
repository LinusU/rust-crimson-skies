//! F35-B acceptance additions to the declared capital-ship schema: the
//! turret boresight the runtime's traverse cone centers on, and the
//! `[0, 360]` traverse bound a coherent mount must declare.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-B`. Ordinary build/test only; nothing here is original
//! data.
//!
//! Every test drives [`DeclaredCapitalShip::try_new`], so removing the
//! boresight or traverse validation fails an assertion.

use cs_content::capital::{
    CapitalSchemaError, CapitalSubsystemEffect, CapitalSubsystemKey, CapitalSubsystemKind,
    DeclaredCapitalParts, DeclaredCapitalShip, DeclaredSubsystem, DeclaredTurret,
    declared_synthetic_capital_ship,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn key(name: &str) -> CapitalSubsystemKey {
    CapitalSubsystemKey::new(name).expect("test subsystem keys are valid")
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f35b.test.schema").expect("valid")),
    ))
}

fn owner() -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Faction, "synthetic.raiders").expect("valid"),
        Provenance::designed(ClaimId::new("f35b.test.schema").expect("valid")),
    ))
}

/// A minimal declared ship carrying one turret: only the turret record's
/// fields differ between cases.
fn ship_with_turret(turret: DeclaredTurret) -> Result<DeclaredCapitalShip, CapitalSchemaError> {
    DeclaredCapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.leviathan").expect("valid"),
        Origin::SyntheticFixture,
        Provenance::designed(ClaimId::new("f35b.test.schema").expect("valid")),
        None,
        DeclaredCapitalParts {
            subsystems: vec![DeclaredSubsystem {
                key: key("turret_1"),
                kind: CapitalSubsystemKind::Turret,
                effect: Some(CapitalSubsystemEffect::WeaponAccess),
                lethal: false,
            }],
            engines: vec![],
            weapon_bays: vec![],
            launch_bays: vec![],
            turrets: vec![turret],
            docking_anchors: vec![],
            sections: vec![],
        },
        designed(0.0),
        owner(),
    )
}

fn turret(boresight: [f64; 3], traverse_deg: Resolved<f64>) -> DeclaredTurret {
    DeclaredTurret {
        key: key("turret_1"),
        weapon: Resolved::Known(Known::new(
            ContentId::from_source(ContentKind::Gun, "synthetic.gun").expect("valid"),
            Provenance::designed(ClaimId::new("f35b.test.schema").expect("valid")),
        )),
        traverse_deg,
        boresight,
    }
}

/// The declared turret's boresight must be a finite, nonzero direction: a
/// mount cannot bear on nowhere or on NaN.
#[test]
fn accept_f35_b_declared_turret_requires_a_finite_nonzero_boresight() {
    assert_eq!(
        ship_with_turret(turret([0.0, 0.0, 0.0], designed(180.0))),
        Err(CapitalSchemaError::ZeroBoresight {
            key: key("turret_1"),
        })
    );
    assert!(matches!(
        ship_with_turret(turret([f64::NAN, 0.0, 0.0], designed(180.0))),
        Err(CapitalSchemaError::NonFinite {
            field: "boresight",
            ..
        })
    ));
    // A non-unit boresight is accepted here — the runtime normalizes it on
    // construction — but the direction must point somewhere.
    assert!(ship_with_turret(turret([0.0, 2.0, 0.0], designed(180.0))).is_ok());
}

/// A declared traverse arc cannot exceed the whole circle: the runtime's
/// cone cannot represent more than 360 degrees, so the declaration refuses
/// it rather than letting a mount claim to sweep further than everything.
#[test]
fn accept_f35_b_declared_turret_traverse_is_bounded_to_the_circle() {
    assert_eq!(
        ship_with_turret(turret([0.0, 1.0, 0.0], designed(361.0))),
        Err(CapitalSchemaError::TraverseOutOfRange {
            key: key("turret_1"),
            value: 361.0,
        })
    );
    assert!(matches!(
        ship_with_turret(turret([0.0, 1.0, 0.0], designed(-10.0))),
        Err(CapitalSchemaError::Negative {
            field: "traverse_deg",
            ..
        })
    ));
    // The boundary itself is valid: a full-circle mount and a fixed gun.
    assert!(ship_with_turret(turret([0.0, 1.0, 0.0], designed(360.0))).is_ok());
    assert!(ship_with_turret(turret([0.0, 1.0, 0.0], designed(0.0))).is_ok());
}

/// The shared fixture declares the dorsal boresight and still validates —
/// the F35-B field cannot drift out of the fixture unnoticed.
#[test]
fn accept_f35_b_fixture_turret_carries_its_boresight() {
    let declared = declared_synthetic_capital_ship();
    let turret = &declared.turrets()[0];
    assert_eq!(turret.boresight, [0.0, 1.0, 0.0]);
    let Resolved::Known(traverse) = &turret.traverse_deg else {
        panic!("the fixture traverse is known");
    };
    assert_eq!(traverse.value, 180.0);
}
