//! Task T452-REG-INSTANT-STEP (#2188): the measured `INSTANT` launch — the
//! flag's own one-step displacement of `RANGE·launch_dir` — expressed in
//! the runtime ordnance vocabulary.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-D`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//! The measured model: `docs/findings/2026-10-28-t452-r01-ordnance-launch-geometry.md`
//! as corrected by `docs/findings/2026-10-28-t452-r04-flight-duration-and-release-binding.md`
//! (§2 rule 3), with the byte windows pinned in
//! `data/owner-static/engine-facts.toml` (`launch.accel_instant_gate`,
//! `launch.inheritance_remainder_head`, `launch.inheritance_remainder_sub`,
//! `flight.spawn_ramp_scalars`, `launch.dt_displacement`,
//! `flight.distance_accumulate`, `launch.range_end`, `launch.state_clear`).
//!
//! The measured rule: the `INSTANT` record flag (`+0x74` bit `0x800`)
//! selects the *same* ramping spawn at `0x5af315` a non-zero `ACCELERATION`
//! does, and the same `+0x60` vector is computed — the lateral remainder
//! plus the launch-direction scalar's start. But the step ignores it: the
//! item's one and only step displaces it by `RANGE·launch_dir`, the flight
//! ends there, and the ordinary range-end decides removal.
//!
//! Every test calls production code: the lowering boundary
//! ([`cs_app::ordnance::measured_ordnance_declaration`], `lower_ordnance`),
//! the runtime boundary (`LaunchGeometry::try_new`), the production spawn
//! and per-tick advance ([`OrdnanceRuntime`]), and the session path
//! ([`OrdnanceSession`], [`step_ordnance_session`]). Nothing is
//! reimplemented here. `accept_t452_instant_retail_*` runs the same path
//! over the owner's installation. No test awards `verified_original`.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Transform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId, OrdnanceLauncherBinding, OrdnanceOrder,
    OrdnanceSession, OrdnanceStep, lower_ordnance, measured_ordnance_declaration,
    register_measured_ordnance, step_ordnance_session,
};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DeclaredInheritanceRule, DeclaredOrdnance, DeclaredOrdnanceDetails, DeclaredOrdnanceFamily,
    DeclaredProjectile, OriginalAmmoDeclaration, OriginalAreaDeclaration, OriginalClusterShape,
    OriginalComponentLaunch, OriginalComponentTable, OriginalDamageDeclaration,
    OriginalEffectBlock, OriginalLifetime, OriginalMountDeclaration, OriginalMountLayout,
    OriginalSlotArray, OriginalWeaponMedia, OriginalWeaponRecord, declared_synthetic_guided,
    declared_synthetic_provenance,
};
use cs_sim::damage::{ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::{
    GunDefinition, GunDefinitionError, GunMountKind, GunRate, HardpointKind, InheritanceRule,
    LaunchGeometry, MountTransform, OrdnanceComponent, OrdnanceDefinitionError, OrdnanceRegistry,
    OrdnanceRuntime, ProjectileId, SpreadCone, WeaponDamage, synthetic_ammunition,
    synthetic_effect, synthetic_mount, synthetic_sound,
};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

// ------------------------------------------------------------------ fixtures --

const SESSION: u64 = 61;
const PRODUCER: u32 = 71;
const DAMAGE_PRODUCER: u32 = 72;
const GENERATION: SceneGeneration = SceneGeneration(1);
const DT_S: f64 = 1.0 / 30.0;
const TICKS_PER_SECOND: u32 = 30;
const STILL_AIR: [f64; 3] = [0.0; 3];

/// The measured numbers the `wep_instant` fixture carries: `VELOCITY 450`,
/// `ACCELERATION 150`, `RANGE 900`, `DETONATION_DISTANCE 30`, `LOCK_ON`,
/// `INSTANT` — `wep_04`'s shape with the instant flag raised.
const LAUNCH_SPEED_MPS: f64 = 450.0;
const ACCELERATION_MPS2: f64 = 150.0;
const RANGE_M: f64 = 900.0;

/// The shooter's world velocity: a part along the launch direction (+Z) and
/// a lateral part, so the spawn's `+0x60` — computed, then ignored by the
/// step — is not the zero vector.
const SHOOTER: [f64; 3] = [30.0, 0.0, 200.0];

fn provenance() -> Provenance {
    Provenance::new(
        ClaimId::new(cs_app::ordnance::T452_ORIGINAL_WEAPONS_CLAIM).expect("a valid claim id"),
        ClaimStatus::ObservedTool,
        Some(
            SourceSpan::new(
                ContentHash::from_bytes([7; 32]),
                "ZBD/zrdr.zbd",
                Some("weapons.zrd"),
                0,
                0,
                None,
            )
            .expect("a member span is valid"),
        ),
    )
    .expect("an observed_tool provenance with a span is valid")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn unknown<T>() -> Resolved<T> {
    Resolved::unknown(claim(), "the original value is unmeasured").expect("a nonempty reason")
}

fn claim() -> ClaimId {
    ClaimId::new("t452.instant-test").expect("a valid claim id")
}

/// The guided fixture's declared projectile, for mutating.
fn projectile_of(declared: &DeclaredOrdnance) -> DeclaredProjectile {
    match declared.details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => (**projectile).clone(),
        DeclaredOrdnanceDetails::Nitro(_) => panic!("the fixture is a projectile"),
    }
}

/// Rebuilds a declared record around a mutated projectile.
fn with_projectile(projectile: DeclaredProjectile) -> DeclaredOrdnance {
    DeclaredOrdnance::try_new(
        weapon_id("synthetic.instant_fixture"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::GuidedRocket,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("the mutated record is structurally valid")
}

fn slots(slots: u32) -> OriginalSlotArray {
    OriginalSlotArray {
        member: "fixture.script".to_owned(),
        array: "RKA".to_owned(),
        slots,
        loop_bound: slots,
        deactivates_empty_slots: true,
    }
}

fn table(rows: Vec<OriginalComponentLaunch>) -> OriginalComponentTable {
    OriginalComponentTable {
        layout: OriginalMountLayout {
            rocket_slots: slots(8),
            ammunition_slots: slots(4),
            hardpoint_points: slots(2),
            findings: Vec::new(),
        },
        rows,
        excluded: vec!["wep_00".to_owned()],
        unknowns: Vec::new(),
    }
}

/// The measured shape of `wep_instant`: a guided record whose `INSTANT`
/// flag selects the one-step `RANGE·launch_dir` displacement.
fn instant_row() -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: "wep_instant".to_owned(),
        family: DeclaredOrdnanceFamily::GuidedRocket,
        mount: OriginalMountDeclaration::RocketSlot { slots: 8 },
        velocity: Some(LAUNCH_SPEED_MPS),
        fire_rate: Some(1.0),
        proximity_trigger_m: Some(30.0),
        lifetime: OriginalLifetime::Undeclared,
        area: OriginalAreaDeclaration {
            radius_m: Some(25.0),
            lifetime_s: None,
        },
        damage: OriginalDamageDeclaration {
            armor: Some(25.0),
            health: Some(25.0),
            single: None,
        },
        statuses: Vec::new(),
        cluster: OriginalClusterShape::Volley { munitions: 1 },
        unknowns: Vec::new(),
    }
}

fn instant_record() -> OriginalWeaponRecord {
    let mut first_values = BTreeMap::new();
    first_values.insert("VELOCITY".to_owned(), LAUNCH_SPEED_MPS);
    // `ACCELERATION` stays declared: the measured gate takes the ramping
    // branch for it too, but the `INSTANT` step never reads `+0x38`.
    first_values.insert("ACCELERATION".to_owned(), ACCELERATION_MPS2);
    first_values.insert("RANGE".to_owned(), RANGE_M);
    first_values.insert("DETONATION_DISTANCE".to_owned(), 30.0);
    first_values.insert("CLUSTER_SIZE".to_owned(), 1.0);
    first_values.insert("LOCK_ON".to_owned(), 1.0);
    first_values.insert("INSTANT".to_owned(), 1.0);
    OriginalWeaponRecord {
        record: "wep_instant".to_owned(),
        keys: first_values.keys().cloned().collect(),
        first_values,
        second_values: BTreeMap::new(),
        first_texts: BTreeMap::new(),
    }
}

fn instant_media() -> OriginalWeaponMedia {
    OriginalWeaponMedia {
        record: "wep_instant".to_owned(),
        keys: BTreeSet::new(),
        fire: Some(OriginalEffectBlock {
            keys: BTreeSet::from(["SOUND".to_owned()]),
            sounds: vec!["snd_missile_sm".to_owned()],
            ..OriginalEffectBlock::default()
        }),
        flyout: Some(OriginalEffectBlock {
            keys: BTreeSet::from(["MODEL".to_owned()]),
            model: Some("ap_rocket".to_owned()),
            ..OriginalEffectBlock::default()
        }),
        impact_rows: Vec::new(),
        destroy_animation: None,
        looped_sound_name: None,
    }
}

fn instant_ammo() -> OriginalAmmoDeclaration {
    OriginalAmmoDeclaration {
        record: "wep_instant".to_owned(),
        ammo_limit: Some(100.0),
        cluster_size: Some(1.0),
    }
}

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("the fixture session is nonzero")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("the fixture node keys are valid")
}

/// The mount key the measured lowering derives from the record's own name
/// (`original_mount_id` reads it, `weapon_mount_{id:02}` names it).
fn instant_mount() -> DamageNodeKey {
    key(&format!(
        "weapon_mount_{:02}",
        cs_content::ordnance::original_mount_id("wep_instant")
    ))
}

fn rate() -> TickRate {
    TickRate::new(TICKS_PER_SECOND).expect("the fixture rate is nonzero")
}

fn weapon_id(record: &str) -> ContentId {
    ContentId::from_source(ContentKind::Weapon, record).expect("a weapon id")
}

/// The `wep_instant` component lowered through the production measured path.
fn instant_component() -> OrdnanceComponent {
    let declared = measured_ordnance_declaration(
        &instant_row(),
        &instant_record(),
        &instant_media(),
        &instant_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("wep_instant's measured launch geometry is expressible now");
    lower_ordnance(&declared).expect("the measured record lowers")
}

/// The launcher's mount pose: at `[0, 0, 10]`, forward +Z, moving at
/// [`SHOOTER`].
fn mount_transform() -> MountTransform {
    MountTransform::try_new(
        WorldPosition::try_new([0.0, 0.0, 10.0]).expect("the fixture origin is finite"),
        UnitVec3::try_new([0.0, 0.0, 1.0]).expect("+Z is a unit vector"),
        SHOOTER,
    )
    .expect("the fixture mount is valid")
}

// ------------------------------------------------------------------- tests ---

/// A measured `INSTANT` record lowers with its own rule and its `RANGE`,
/// and the registry accepts it — the measured flag is no longer a blocker.
#[test]
fn accept_t452_instant_a_measured_instant_record_lowers_and_registers() {
    let component = instant_component();
    let projectile = component.as_projectile().expect("a rocket is a projectile");

    assert_eq!(
        projectile.launch().inheritance(),
        InheritanceRule::Instant,
        "the measured INSTANT flag lowers as its own rule"
    );
    assert_eq!(
        projectile.launch().instant_range_m(),
        Some(RANGE_M),
        "the one-step displacement carries the record's RANGE"
    );
    assert_eq!(
        projectile.launch().acceleration_mps2(),
        None,
        "the declared ACCELERATION is measured to play no part in the step"
    );
    assert_eq!(
        projectile.lifetime_ticks(),
        1,
        "the measured flight is one step long"
    );

    let mut registry = OrdnanceRegistry::new();
    let report = register_measured_ordnance(
        &table(vec![instant_row()]),
        &[instant_record()],
        &[instant_media()],
        &[instant_ammo()],
        &provenance(),
        &mut registry,
        TICKS_PER_SECOND,
    );
    assert_eq!(report.registered_records(), vec!["wep_instant"]);
    assert!(report.is_complete(), "{:?}", report.blockers());
}

/// The measured step, through the production runtime: the first — and only
/// — advance displaces the item `RANGE·launch_dir`, not `dt·velocity`, and
/// the flight ends there at its range-end.
#[test]
fn accept_t452_instant_the_first_advance_displaces_range_along_the_launch_direction() {
    let component = instant_component();
    let definition = component.as_projectile().expect("a projectile");
    let mut runtime = OrdnanceRuntime::new(session_id(), Tick(0), rate(), PRODUCER);
    let projectile_id = ProjectileId {
        session: SESSION,
        serial: 1,
    };
    runtime
        .launch(
            actor(1),
            projectile_id,
            definition,
            &mount_transform(),
            None,
            STILL_AIR,
        )
        .expect("the instant component launches");

    // The release carries the measured one-step rule, not a ramp: the
    // spawn's `+0x60` vector is still composed (the same ramping-branch
    // release the runtime models for the flag) and the step ignores it.
    let live = runtime.iter().next().expect("the launched item is live");
    let instant = live
        .instant_step()
        .expect("an INSTANT release carries its one-step rule");
    assert_eq!(instant.direction(), [0.0, 0.0, 1.0]);
    assert_eq!(instant.range_m(), RANGE_M);
    assert_eq!(instant.displacement_m(), [0.0, 0.0, RANGE_M]);
    assert!(live.launch_ramp().is_none(), "INSTANT is not the ramp");
    let release = live.air_velocity_m_s();

    let tick = runtime.advance(DT_S, STILL_AIR).expect("the tick advances");
    // One swept segment: origin → origin + RANGE·dir, exactly.
    assert_eq!(tick.segments.len(), 1, "the one-step flight sweeps once");
    let segment = &tick.segments[0];
    let from = segment.previous.to_array();
    let to = segment.current.to_array();
    let direction = [0.0, 0.0, 1.0];
    for axis in 0..3 {
        assert_eq!(
            to[axis],
            from[axis] + direction[axis] * RANGE_M,
            "axis {axis}: the displacement is exactly RANGE·launch_dir"
        );
    }
    let displacement = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    // ...and it is not the ordinary `dt·velocity` step the sibling rules
    // run: `dt·release` would have moved the item ~6.7 m along +Z and 1 m
    // along +X, not 900 m along +Z.
    let dt_step = [release[0] * DT_S, release[1] * DT_S, release[2] * DT_S];
    assert!(
        !(0..3).all(|axis| (displacement[axis] - dt_step[axis]).abs() < 1e-6),
        "the step is not dt·velocity: {displacement:?} vs {dt_step:?}"
    );
    assert!(
        displacement[2] > 100.0 * dt_step[2],
        "the measured step is a single RANGE displacement, not an integration"
    );
    // The measured flight is one step long: the item reaches its range-end
    // on the first advance and the ordinary expiry removes it silently.
    assert_eq!(
        tick.expired,
        vec![projectile_id],
        "the single step reaches RANGE and ends the flight"
    );
    assert!(
        tick.range_end_detonated.is_empty(),
        "a live target's loss never ran: no detonation, the silent removal"
    );
    assert_eq!(
        runtime.iter().count(),
        0,
        "nothing stays live after the one step"
    );

    // A second release under a different `dt` and a moving airmass lands
    // identically: the displacement is the node's own, not integrated.
    let second_id = ProjectileId {
        session: SESSION,
        serial: 2,
    };
    runtime
        .launch(
            actor(1),
            second_id,
            definition,
            &mount_transform(),
            None,
            [9.0, 8.0, 7.0],
        )
        .expect("the second instant component launches");
    let tick = runtime
        .advance(DT_S * 0.5, [9.0, 8.0, 7.0])
        .expect("the tick advances");
    let segment = &tick.segments[0];
    let from = segment.previous.to_array();
    let to = segment.current.to_array();
    for axis in 0..3 {
        assert_eq!(
            to[axis],
            from[axis] + direction[axis] * RANGE_M,
            "axis {axis}: dt and wind play no part in the measured step"
        );
    }
    assert_eq!(tick.expired, vec![second_id]);
}

/// The runtime boundary refuses the incoherent `INSTANT` pairings rather
/// than defaulting them: a rule with no range to displace, a range no rule
/// reads, a corrupt range, and a rule a gun cannot carry.
#[test]
fn accept_t452_instant_the_boundary_refuses_an_incoherent_pairing() {
    let mount = instant_mount();
    let geometry = |inheritance, instant_range_m| {
        LaunchGeometry::try_new(
            mount.clone(),
            HardpointKind::Underslung,
            LAUNCH_SPEED_MPS,
            inheritance,
            0,
            None,
            instant_range_m,
        )
    };
    assert_eq!(
        geometry(InheritanceRule::Instant, None),
        Err(OrdnanceDefinitionError::MissingInstantRange),
        "the one step would have no distance to displace"
    );
    assert_eq!(
        geometry(InheritanceRule::Instant, Some(0.0)),
        Err(OrdnanceDefinitionError::NonPositiveInstantRange {
            instant_range_m: 0.0
        }),
        "a zero range could not displace along the launch direction"
    );
    assert_eq!(
        geometry(InheritanceRule::Instant, Some(-RANGE_M)),
        Err(OrdnanceDefinitionError::NonPositiveInstantRange {
            instant_range_m: -RANGE_M
        }),
    );
    assert_eq!(
        geometry(InheritanceRule::Instant, Some(f64::NAN)),
        Err(OrdnanceDefinitionError::NonFiniteInstantRange),
    );
    assert_eq!(
        geometry(InheritanceRule::Full, Some(RANGE_M)),
        Err(OrdnanceDefinitionError::UnexpectedInstantRange {
            instant_range_m: RANGE_M
        }),
        "a Full launch has no step the range could drive"
    );
    assert_eq!(
        geometry(InheritanceRule::None, Some(RANGE_M)),
        Err(OrdnanceDefinitionError::UnexpectedInstantRange {
            instant_range_m: RANGE_M
        }),
    );
    // A range on the ramping rule is just as incoherent — the ramp
    // integrates `dt·velocity`, not a one-step displacement.
    assert_eq!(
        LaunchGeometry::try_new(
            mount.clone(),
            HardpointKind::Underslung,
            LAUNCH_SPEED_MPS,
            InheritanceRule::LateralRemainder,
            0,
            Some(ACCELERATION_MPS2),
            Some(RANGE_M),
        ),
        Err(OrdnanceDefinitionError::UnexpectedInstantRange {
            instant_range_m: RANGE_M
        }),
    );
    // And `INSTANT` plus a rate it never runs is refused, symmetrically.
    assert_eq!(
        LaunchGeometry::try_new(
            mount.clone(),
            HardpointKind::Underslung,
            LAUNCH_SPEED_MPS,
            InheritanceRule::Instant,
            0,
            Some(ACCELERATION_MPS2),
            Some(RANGE_M),
        ),
        Err(OrdnanceDefinitionError::UnexpectedAcceleration {
            acceleration_mps2: ACCELERATION_MPS2
        }),
    );
    assert_eq!(
        geometry(InheritanceRule::Instant, Some(RANGE_M))
            .expect("the measured pair is coherent")
            .instant_range_m(),
        Some(RANGE_M)
    );

    // The lowering boundary refuses the same pairings rather than
    // defaulting them: a declared `INSTANT` with no range does not lower.
    let mut projectile = projectile_of(&declared_synthetic_guided());
    projectile.launch.inheritance = known(DeclaredInheritanceRule::Instant);
    projectile.lifetime_ticks = known(1);
    assert_eq!(
        lower_ordnance(&with_projectile(projectile.clone())),
        Err(cs_app::ordnance::OrdnanceLowerError::Definition(
            OrdnanceDefinitionError::MissingInstantRange
        )),
        "INSTANT without its range cannot lower — nothing defaults it"
    );
    projectile.launch.instant_range_m = unknown();
    assert!(matches!(
        lower_ordnance(&with_projectile(projectile)),
        Err(cs_app::ordnance::OrdnanceLowerError::UnknownField { field, .. })
            if field == "launch.instant_range_m"
    ));

    // And the rule is ordnance-only: a gun has no one-step range
    // displacement, so its own boundary refuses it.
    assert_eq!(
        GunDefinition::try_new(
            synthetic_mount(),
            GunMountKind::Nose,
            "20mm",
            synthetic_ammunition(),
            GunRate::try_new(4).expect("a valid rate"),
            640.0,
            90,
            SpreadCone::try_new(0.004).expect("a valid spread"),
            WeaponDamage::try_new(6.0, 3.0).expect("valid damage"),
            InheritanceRule::Instant,
            synthetic_effect(),
            synthetic_sound(),
        ),
        Err(GunDefinitionError::OrdnanceOnlyInheritance),
        "the one-step displacement is not a gun rule"
    );
}

/// The same measured step through the session path: the launch order's own
/// tick advances the item once, so the item has already landed `RANGE`
/// metres out and its flight has ended when the step returns.
#[test]
fn accept_t452_instant_a_session_launch_lands_the_measured_distance_and_ends() {
    let mut session = instant_session();
    let mut world = World::new();
    instant_launcher(&mut world);
    let mut damage = damage_resolver();

    let launched = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0)],
        Tick(0),
    );
    assert_eq!(launched.launched.len(), 1, "the instant component launches");
    let projectile = launched.launched[0];
    // The launch tick's advance ran the one measured step: the item reached
    // its `RANGE` and the range-end removed it in the same step.
    assert_eq!(
        launched.expired,
        vec![projectile],
        "the one-step flight ends on the launch tick itself"
    );
    assert!(
        launched.detonations.is_empty() && launched.guidance_blasts.is_empty(),
        "a tracked item ends at its range-end silently"
    );
    assert_eq!(
        session.runtime().iter().count(),
        0,
        "nothing stays live after the one measured step"
    );

    // One more session tick must not move a phantom: the flight was one
    // step long, not one step per tick.
    let next = step(&mut world, &mut session, &mut damage, &[], Tick(1));
    assert!(
        next.launched.is_empty() && next.expired.is_empty(),
        "no second step exists to run"
    );
    assert_eq!(session.runtime().iter().count(), 0);
}

// -------------------------------------------------------------------- retail --

fn game_dir() -> std::path::PathBuf {
    let dir = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: accept_t452_instant_retail_* need the original \
             installation; run them with --include-ignored and CS_GAME_DIR set"
        )
    });
    std::path::PathBuf::from(dir)
}

/// The retail half: no record in the owner's installation declares the
/// `INSTANT` flag, so the measured one-step rule stays dormant in the
/// catalogue — the flag is measured-vocabulary completeness, not a retail
/// registration. Every record that does declare it, if one ever appears,
/// must lower as `Instant` rather than name a blocker.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_instant_retail_no_retail_record_is_instant() {
    let root = game_dir();
    let records =
        cs_content::ordnance::read_original_weapon_records(&root).expect("the weapon records read");
    for record in &records {
        let geometry = cs_content::ordnance::original_launch_geometry(record);
        assert_ne!(
            geometry.inheritance,
            cs_content::ordnance::OriginalInheritance::Instant,
            "{} unexpectedly declares INSTANT",
            record.record
        );
    }
    assert!(
        !records.is_empty(),
        "the installation's weapon records are present"
    );
}

// ------------------------------------------------------------------ harness --

/// The shared damage authority with the target's synthetic airframe graph
/// registered.
fn damage_resolver() -> DamageResolver {
    let mut damage = DamageResolver::new(session_id(), DAMAGE_PRODUCER);
    damage
        .register_actor(
            actor(2),
            cs_sim::damage::synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the fixture graph registers");
    damage
}

/// A session whose catalogue and registrations come from the production
/// measured path: `wep_instant`'s fixture shape lowered and registered.
fn instant_session() -> OrdnanceSession {
    let declared = measured_ordnance_declaration(
        &instant_row(),
        &instant_record(),
        &instant_media(),
        &instant_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("the instant record lowers");
    let mut catalogue = OrdnanceRegistry::new();
    catalogue
        .register(lower_ordnance(&declared).expect("the component lowers"))
        .expect("the catalogue takes it");
    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured component registers in the session");
    session
}

/// The instant launch's order for one tick.
fn launch_order(sequence: u32) -> OrdnanceOrder {
    OrdnanceOrder::Launch(OrdnanceLaunch {
        id: OrdnanceLaunchId {
            session: SESSION,
            tick: Tick(0),
            producer: PRODUCER,
            sequence,
        },
        shooter: actor(1),
        ordnance: cs_sim::weapons::OrdnanceId::try_new(weapon_id("wep_instant"))
            .expect("the weapon namespace"),
        target: None,
        engagement: OrdnanceEngagement {
            damage_target: actor(2),
            node: key("hull"),
            status_recipient: cs_sim::weapons::StatusEffectTarget::whole_actor(actor(2)),
        },
    })
}

/// The shooter's live launcher: a moving root carrying the binding and one
/// mount node for `wep_instant`, facing +Z.
fn instant_launcher(world: &mut World) -> Entity {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![weapon_id("wep_instant")],
                loadout: ContentId::from_source(ContentKind::Loadout, "fixture.loadout")
                    .expect("a loadout id"),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::new(
                SHOOTER[0] as f32,
                SHOOTER[1] as f32,
                SHOOTER[2] as f32,
            )),
        ))
        .id();
    world.spawn((
        MountPoseBinding {
            mount: instant_mount(),
            generation: GENERATION,
        },
        cs_app::scene::NodeVisualTransform(GlobalTransform::from(
            Transform::from_xyz(0.0, 0.0, 10.0).looking_to(Vec3::Z, Vec3::Y),
        )),
        ChildOf(root),
    ));
    root
}

fn step(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    orders: &[OrdnanceOrder],
    at: Tick,
) -> cs_app::ordnance::OrdnanceSessionTick {
    step_ordnance_session(
        world,
        session,
        damage,
        orders,
        &OrdnanceStep {
            at,
            dt_s: DT_S,
            wind_velocity_m_s: STILL_AIR,
            generation: GENERATION,
            guidance: None,
            targets: &[],
            impacts: &[],
            blast_recipients: None,
            victim_nodes: None,
        },
    )
}
