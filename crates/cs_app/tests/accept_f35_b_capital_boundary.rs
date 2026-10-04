//! Acceptance scenario F35-B at the boundary: the declared fixture lowers
//! into a ship the session runtime moves, wounds and aims — so the F35-B
//! fields the boundary carries (turret boresight, section integrity pools)
//! are exercised through production code, not just field-compared.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-B`. Task test prefix: `accept_f35_b_`.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_app::capital::lower_capital_ship;
use cs_content::capital::{
    CapitalSubsystemEffect, CapitalSubsystemKey, CapitalSubsystemKind, DeclaredCapitalParts,
    DeclaredCapitalShip, DeclaredSection, DeclaredSubsystem, declared_synthetic_capital_ship,
};
use cs_script::ir::ActorId;
use cs_sim::capital::{
    CapitalHit, CapitalRuntimeError, CapitalShipSet, HitOutcome, SubsystemKey, TurretRefusal,
};
use cs_types::Tick;
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
        Provenance::designed(ClaimId::new("f35b.test.boundary").expect("valid")),
    ))
}

fn designed_owner(value: &str) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Faction, value).expect("valid"),
        Provenance::designed(ClaimId::new("f35b.test.boundary").expect("valid")),
    ))
}

fn hit(target: ActorId, subsystem: &str, damage: f64, at: u64) -> CapitalHit {
    CapitalHit::try_new(target, key(subsystem), damage, Tick(at)).expect("a valid hit")
}

fn approx(a: [f64; 3], b: [f64; 3]) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() < 1e-9, "component {i}: {a:?} vs {b:?}");
    }
}

/// The declared fixture lowers with its F35-B additions intact: each
/// section's integrity pool and each turret's normalized boresight read
/// back verbatim — an unknown stays an unknown.
#[test]
fn accept_f35_b_lower_preserves_boresight_and_integrity_pools() {
    let declared = declared_synthetic_capital_ship();
    let ship = lower_capital_ship(ActorId(1), &declared).expect("the fixture lowers");

    // Every declared section pool arrives verbatim — provenance included.
    for section in declared.sections() {
        let runtime_key = key(section.key.as_str());
        assert_eq!(
            ship.integrity_pool(&runtime_key),
            Some(&section.integrity),
            "section {} lowers its pool field-wise",
            section.key,
        );
    }
    assert_eq!(ship.remaining_integrity(&key("gas_cell_1")), Some(120.0));

    // The dorsal boresight lowers verbatim (it is already unit).
    let turret = ship.turrets().next().expect("one turret");
    assert_eq!(turret.boresight, [0.0, 1.0, 0.0]);
    assert_eq!(ship.turret_aim(&key("turret_1")), Some([0.0, 1.0, 0.0]));
}

/// The F35-B scenario end-to-end across the boundary: a lowered ship runs
/// its course in the set, an engine hit halves its rate, its section pools
/// absorb until a lethal cell empties, and a destroyed ship freezes.
#[test]
fn accept_f35_b_lowered_ship_moves_wounds_and_freezes_end_to_end() {
    let declared = declared_synthetic_capital_ship();
    let ship = lower_capital_ship(ActorId(1), &declared).expect("lowers");
    let mut set = CapitalShipSet::new(10).expect("the declared rate");
    set.register(ship, None).expect("registers");

    // Full power: 50 committed ticks carry the drive 50 ticks → 100 m.
    set.advance_to(Tick(50)).expect("advances");
    approx(
        set.pose(ActorId(1)).expect("posed").position_m,
        [100.0, 0.0, 0.0],
    );

    // A landed hit kills an engine — motion halves, the hull is intact.
    set.apply_hit(&hit(ActorId(1), "engine_1", 10.0, 50))
        .expect("resolves");
    set.advance_to(Tick(100)).expect("advances");
    approx(
        set.pose(ActorId(1)).expect("posed").position_m,
        [150.0, 0.0, 0.0],
    );
    assert!(!set.is_destroyed(ActorId(1)).expect("known"));

    // The exposed weapon bay is a weakpoint; the gas cell absorbs then
    // kills the ship.
    let outcome = set
        .apply_hit(&hit(ActorId(1), "weapon_bay_1", 50.0, 100))
        .expect("exposed at tick 100: 100 % 120 = 100 ∈ [50,110)");
    assert!(matches!(outcome, HitOutcome::Destroyed { .. }));
    assert_eq!(
        set.apply_hit(&hit(ActorId(1), "gas_cell_1", 100.0, 100)),
        Ok(HitOutcome::Damaged {
            subsystem: key("gas_cell_1"),
            remaining_integrity: 20.0,
        })
    );
    let outcome = set
        .apply_hit(&hit(ActorId(1), "gas_cell_1", 20.0, 100))
        .expect("resolves");
    let HitOutcome::Destroyed { outcome, .. } = outcome else {
        panic!("depleting the lethal cell destroys the ship, got {outcome:?}");
    };
    assert!(outcome.actor_destroyed);
    assert!(set.is_destroyed(ActorId(1)).expect("known"));

    // The wreck froze mid-course: it holds the pose it died on.
    approx(
        set.pose(ActorId(1)).expect("posed").position_m,
        [150.0, 0.0, 0.0],
    );
    set.advance_to(Tick(110)).expect("the wreck holds");
    approx(
        set.pose(ActorId(1)).expect("posed").position_m,
        [150.0, 0.0, 0.0],
    );
}

/// A live lowered ship's turret runs the runtime's commands: the boresight
/// the boundary carried centers the traverse cone, and the weapon's
/// explicit unknown refuses fire by claim.
#[test]
fn accept_f35_b_lowered_turret_aims_but_refuses_an_unknown_weapon() {
    let declared = declared_synthetic_capital_ship();
    let ship = lower_capital_ship(ActorId(1), &declared).expect("lowers");
    let mut set = CapitalShipSet::new(10).expect("valid");
    set.register(ship, None).expect("registers");

    // The mount aims across the cone the declared boresight centers: -X is
    // 90° off +Y — exactly on a 180° cone's rim — and lands verbatim.
    let aim = set
        .aim_turret(ActorId(1), &key("turret_1"), [-1.0, 0.0, 0.0])
        .expect("aims");
    assert!(aim.within_arc);
    assert!((aim.off_boresight_deg - 90.0).abs() < 1e-9);
    approx(aim.aim, [-1.0, 0.0, 0.0]);

    // The antiparallel command (180° off) has no nearest rim: the mount's
    // deterministic edge case bears on the cone rim toward -Z.
    let aim = set
        .aim_turret(ActorId(1), &key("turret_1"), [0.0, -1.0, 0.0])
        .expect("aims");
    assert!(!aim.within_arc);
    assert_eq!(aim.off_boresight_deg, 180.0);
    approx(aim.aim, [0.0, 0.0, -1.0]);

    // The unknown weapon binding refuses fire by the claim the boundary
    // carried through.
    let refused = set
        .may_fire(ActorId(1), &key("turret_1"))
        .expect_err("an unmeasured gun never fires");
    assert!(matches!(
        refused,
        CapitalRuntimeError::Turret(TurretRefusal::WeaponUnknown { .. })
    ));
}

/// An unknown section integrity lowers through and blocks the hit at the
/// boundary's far side — the runtime sees the claim, not a guessed pool.
#[test]
fn accept_f35_b_lowered_unknown_integrity_blocks_the_hit() {
    let declared = DeclaredCapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.leviathan").expect("valid"),
        Origin::SyntheticFixture,
        Provenance::designed(ClaimId::new("f35b.test.boundary").expect("valid")),
        None,
        DeclaredCapitalParts {
            subsystems: vec![DeclaredSubsystem {
                key: declared_key("keel"),
                kind: CapitalSubsystemKind::StructuralSection,
                effect: Some(CapitalSubsystemEffect::MissionCondition),
                lethal: true,
            }],
            engines: vec![],
            weapon_bays: vec![],
            launch_bays: vec![],
            turrets: vec![],
            docking_anchors: vec![],
            sections: vec![DeclaredSection {
                key: declared_key("keel"),
                integrity: Resolved::Unknown {
                    claim_id: ClaimId::new("f35b.test.boundary.unknown-keel").expect("valid"),
                    reason: "keel integrity unmeasured".to_owned(),
                },
            }],
        },
        designed(0.0),
        designed_owner("synthetic.raiders"),
    )
    .expect("an unknown integrity is a valid declared record");

    let ship = lower_capital_ship(ActorId(7), &declared).expect("lowers");
    assert!(
        matches!(
            ship.integrity_pool(&key("keel")),
            Some(Resolved::Unknown { .. })
        ),
        "the unknown pool lowers verbatim"
    );

    let mut set = CapitalShipSet::new(10).expect("valid");
    set.register(
        ship,
        Some(cs_sim::world_actors::trajectory::Pose {
            position_m: [0.0; 3],
            orientation: cs_sim::world_actors::Quat::IDENTITY,
            velocity_m_s: [0.0; 3],
            angular_velocity_rad_s: [0.0; 3],
        }),
    )
    .expect("a course-less ship registers on a pose");
    let outcome = set
        .apply_hit(&hit(ActorId(7), "keel", 10.0, 0))
        .expect("resolves");
    let HitOutcome::Blocked { claim_id, .. } = outcome else {
        panic!("the unknown pool blocks the hit, got {outcome:?}");
    };
    assert_eq!(claim_id.as_str(), "f35b.test.boundary.unknown-keel");
    assert!(!set.is_destroyed(ActorId(7)).expect("known"));
}
