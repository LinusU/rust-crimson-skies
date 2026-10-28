//! Task T452-REG-INHERITANCE-RAMP (#2174): the measured lateral-remainder
//! launch inheritance and its `ACCELERATION` ramp, expressed in the runtime
//! ordnance vocabulary.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-D`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//! The measured model: `docs/findings/2026-10-28-t452-r01-ordnance-launch-geometry.md`
//! as corrected by `docs/findings/2026-10-28-t452-r04-flight-duration-and-release-binding.md`,
//! with the byte windows pinned in `data/owner-static/engine-facts.toml`
//! (`launch.accel_instant_gate`, `launch.inheritance_remainder_head`,
//! `launch.inheritance_remainder_sub`, `launch.accel_step`,
//! `launch.dt_displacement`, `launch.spawn_target_speed`).
//!
//! The measured rule: a non-zero `ACCELERATION` selects the ramping spawn at
//! `0x5af315`. The speed scalar `+0x640` opens at `|launcher| + 1e-4`, its
//! target `+0x644` is `VELOCITY + |launcher|`, and the velocity is
//! `launcher − dir·|launcher|` (the lateral remainder) plus `dir·scalar`
//! recomposed each step — `scalar += dt·ACCELERATION`, clamped at the
//! target, then `dt·velocity` displaces.
//!
//! Every test calls production code: the lowering boundary
//! ([`cs_app::ordnance::measured_ordnance_declaration`], `lower_ordnance`),
//! the runtime boundary (`LaunchGeometry::try_new`), the production spawn
//! and per-tick advance ([`OrdnanceRuntime`]), and the session path
//! ([`OrdnanceSession`], [`step_ordnance_session`]). Nothing is
//! reimplemented here. `accept_t452_inherit_retail_*` runs the same path
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
    DeclaredOrdnanceFamily, OriginalAmmoDeclaration, OriginalAreaDeclaration, OriginalClusterShape,
    OriginalComponentLaunch, OriginalComponentTable, OriginalDamageDeclaration,
    OriginalEffectBlock, OriginalLifetime, OriginalMountDeclaration, OriginalMountLayout,
    OriginalSlotArray, OriginalWeaponMedia, OriginalWeaponRecord,
};
use cs_sim::damage::{ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::guns::RAMP_START_SPEED_MPS;
use cs_sim::weapons::{
    HardpointKind, InheritanceRule, LaunchGeometry, MountTransform, OrdnanceComponent,
    OrdnanceDefinitionError, OrdnanceRegistry, OrdnanceRuntime, ProjectileId,
};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, Provenance};
use cs_types::evidence::{ClaimId as EvidenceClaimId, ClaimStatus, ContentHash};
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

/// The measured `wep_04` numbers the fixture carries: `VELOCITY 450`,
/// `ACCELERATION 150`.
const LAUNCH_SPEED_MPS: f64 = 450.0;
const ACCELERATION_MPS2: f64 = 150.0;
/// `dt·ACCELERATION` at the session's 30 Hz tick: the scalar's per-tick gain.
const GAIN_PER_TICK_MPS: f64 = ACCELERATION_MPS2 * DT_S;

/// The shooter's world velocity: a part along the launch direction (+Z) and
/// a lateral part, so the measured remainder is not the zero vector.
const SHOOTER: [f64; 3] = [30.0, 0.0, 200.0];

fn shooter_speed() -> f64 {
    (SHOOTER[0] * SHOOTER[0] + SHOOTER[1] * SHOOTER[1] + SHOOTER[2] * SHOOTER[2]).sqrt()
}

/// The measured lateral remainder: `shooter − dir·|shooter|` for `dir = +Z`.
fn expected_remainder() -> [f64; 3] {
    [SHOOTER[0], SHOOTER[1], SHOOTER[2] - shooter_speed()]
}

fn provenance() -> Provenance {
    Provenance::new(
        EvidenceClaimId::new(cs_app::ordnance::T452_ORIGINAL_WEAPONS_CLAIM)
            .expect("a valid claim id"),
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

/// The measured shape of `wep_04`: a guided record whose non-zero
/// `ACCELERATION` selects the lateral-remainder ramp.
fn ramping_row() -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: "wep_04".to_owned(),
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

fn ramping_record() -> OriginalWeaponRecord {
    let mut first_values = BTreeMap::new();
    first_values.insert("VELOCITY".to_owned(), LAUNCH_SPEED_MPS);
    first_values.insert("ACCELERATION".to_owned(), ACCELERATION_MPS2);
    first_values.insert("RANGE".to_owned(), 900.0);
    first_values.insert("DETONATION_DISTANCE".to_owned(), 30.0);
    first_values.insert("CLUSTER_SIZE".to_owned(), 1.0);
    first_values.insert("LOCK_ON".to_owned(), 1.0);
    OriginalWeaponRecord {
        record: "wep_04".to_owned(),
        keys: first_values.keys().cloned().collect(),
        first_values,
        second_values: BTreeMap::new(),
        first_texts: BTreeMap::new(),
    }
}

fn ramping_media() -> OriginalWeaponMedia {
    OriginalWeaponMedia {
        record: "wep_04".to_owned(),
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

fn ramping_ammo() -> OriginalAmmoDeclaration {
    OriginalAmmoDeclaration {
        record: "wep_04".to_owned(),
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

fn rate() -> TickRate {
    TickRate::new(TICKS_PER_SECOND).expect("the fixture rate is nonzero")
}

fn weapon_id(record: &str) -> ContentId {
    ContentId::from_source(cs_types::content::ContentKind::Weapon, record).expect("a weapon id")
}

/// The `wep_04` component lowered through the production measured path.
fn ramping_component() -> OrdnanceComponent {
    let declared = measured_ordnance_declaration(
        &ramping_row(),
        &ramping_record(),
        &ramping_media(),
        &ramping_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("wep_04's measured launch geometry is expressible now");
    lower_ordnance(&declared).expect("the measured record lowers")
}

/// The launcher's mount pose: at the origin, forward +Z, moving at
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

/// A measured `ACCELERATION` record lowers with the lateral-remainder rule
/// and its own rate, and the registry accepts it.
#[test]
fn accept_t452_inherit_a_measured_ramping_record_lowers_and_registers() {
    let component = ramping_component();
    let projectile = component.as_projectile().expect("a rocket is a projectile");

    assert_eq!(
        projectile.launch().inheritance(),
        InheritanceRule::LateralRemainder,
        "a non-zero ACCELERATION selects the measured ramp, not a share"
    );
    assert_eq!(
        projectile.launch().acceleration_mps2(),
        Some(ACCELERATION_MPS2),
        "the record's own ACCELERATION is the ramp's per-second gain"
    );
    assert_eq!(projectile.launch().launch_speed_mps(), LAUNCH_SPEED_MPS);

    let mut registry = OrdnanceRegistry::new();
    let report = register_measured_ordnance(
        &table(vec![ramping_row()]),
        &[ramping_record()],
        &[ramping_media()],
        &[ramping_ammo()],
        &provenance(),
        &mut registry,
        TICKS_PER_SECOND,
    );
    assert_eq!(report.registered_records(), vec!["wep_04"]);
    assert!(report.is_complete(), "{:?}", report.blockers());
}

/// The runtime boundary refuses the incoherent pairings: a ramp with no
/// rate, a rate no rule integrates, and a corrupt rate.
#[test]
fn accept_t452_inherit_the_boundary_refuses_an_incoherent_ramp() {
    let mount = key("weapon_mount_04");
    let geometry = |inheritance, acceleration_mps2| {
        LaunchGeometry::try_new(
            mount.clone(),
            HardpointKind::Underslung,
            LAUNCH_SPEED_MPS,
            inheritance,
            0,
            acceleration_mps2,
        )
    };
    assert_eq!(
        geometry(InheritanceRule::LateralRemainder, None),
        Err(OrdnanceDefinitionError::MissingAcceleration),
        "the scalar's gain is what runs the ramp: no rate, no rule"
    );
    assert_eq!(
        geometry(InheritanceRule::Full, Some(ACCELERATION_MPS2)),
        Err(OrdnanceDefinitionError::UnexpectedAcceleration {
            acceleration_mps2: ACCELERATION_MPS2
        }),
        "a Full launch has no scalar the rate could drive"
    );
    assert_eq!(
        geometry(InheritanceRule::LateralRemainder, Some(0.0)),
        Err(OrdnanceDefinitionError::NonPositiveAcceleration {
            acceleration_mps2: 0.0
        }),
        "a zero rate would never reach the target — and zero selects Full"
    );
    assert_eq!(
        geometry(InheritanceRule::LateralRemainder, Some(f64::NAN)),
        Err(OrdnanceDefinitionError::NonFiniteAcceleration),
    );
    assert_eq!(
        geometry(InheritanceRule::LateralRemainder, Some(ACCELERATION_MPS2))
            .expect("the measured pair is coherent")
            .acceleration_mps2(),
        Some(ACCELERATION_MPS2)
    );
}

/// The release composition of the measured rule, through the production
/// spawn path: the lateral remainder stays, the launch-direction scalar
/// opens at `|launcher| + 1e-4` — so the item leaves at the launcher's
/// speed, not at `VELOCITY`.
#[test]
fn accept_t452_inherit_the_release_keeps_the_remainder_and_starts_at_the_launchers_speed() {
    let component = ramping_component();
    let definition = component.as_projectile().expect("a projectile");
    let mut runtime = OrdnanceRuntime::new(session_id(), Tick(0), rate(), PRODUCER);
    runtime
        .launch(
            actor(1),
            ProjectileId {
                session: SESSION,
                serial: 1,
            },
            definition,
            &mount_transform(),
            None,
            STILL_AIR,
        )
        .expect("the ramping component launches");

    let live = runtime.iter().next().expect("the launched item is live");
    let ramp = live
        .launch_ramp()
        .expect("a lateral-remainder release carries its ramp");
    let s = shooter_speed();
    // The scalar's measured open: `|launcher| + 1e-4` (the spawn's `+0x640`).
    assert!(
        (ramp.speed_m_s() - (s + RAMP_START_SPEED_MPS)).abs() < 1e-9,
        "the scalar opens at |launcher| + 1e-4: got {}",
        ramp.speed_m_s()
    );
    // The measured target: `VELOCITY + |launcher|` (the spawn's `+0x644`).
    assert!(
        (ramp.target_m_s() - (LAUNCH_SPEED_MPS + s)).abs() < 1e-9,
        "the target is VELOCITY + |launcher|: got {}",
        ramp.target_m_s()
    );
    assert_eq!(ramp.acceleration_mps2(), ACCELERATION_MPS2);
    assert!((ramp.direction()[2] - 1.0).abs() < 1e-9);
    // The lateral remainder is the launcher's velocity minus its
    // launch-direction part — the measured `+0x60` subtraction.
    let remainder = expected_remainder();
    for axis in 0..3 {
        assert!(
            (ramp.remainder_m_s()[axis] - remainder[axis]).abs() < 1e-9,
            "remainder axis {axis}: got {:?}, want {remainder:?}",
            ramp.remainder_m_s()
        );
    }
    // The release vector itself: remainder + dir·scalar — the launcher's
    // own velocity plus the measured `1e-4` along the launch direction.
    let release = live.air_velocity_m_s();
    assert!(
        (release[0] - SHOOTER[0]).abs() < 1e-6
            && (release[1] - SHOOTER[1]).abs() < 1e-6
            && (release[2] - (SHOOTER[2] + RAMP_START_SPEED_MPS)).abs() < 1e-6,
        "the item leaves at the launcher's velocity plus 1e-4·dir: got {release:?}"
    );
}

/// The measured step: every tick the scalar gains `dt·ACCELERATION` clamped
/// at `VELOCITY + |launcher|`, the velocity is recomposed as
/// `remainder + dir·scalar`, and the ramp never overshoots.
#[test]
fn accept_t452_inherit_the_scalar_ramps_at_acceleration_and_clamps_at_the_target() {
    let component = ramping_component();
    let definition = component.as_projectile().expect("a projectile");
    let mut runtime = OrdnanceRuntime::new(session_id(), Tick(0), rate(), PRODUCER);
    runtime
        .launch(
            actor(1),
            ProjectileId {
                session: SESSION,
                serial: 1,
            },
            definition,
            &mount_transform(),
            None,
            STILL_AIR,
        )
        .expect("the ramping component launches");

    let s = shooter_speed();
    let target = LAUNCH_SPEED_MPS + s;
    // `target − start = VELOCITY − 1e-4`, so the ramp takes
    // ceil((450 − 1e-4) / (150·dt)) = 90 ticks at 30 Hz.
    let clamp_tick = ((LAUNCH_SPEED_MPS - RAMP_START_SPEED_MPS) / GAIN_PER_TICK_MPS).ceil() as u64;
    assert_eq!(
        clamp_tick, 90,
        "the measured ramp takes VELOCITY/ACCELERATION"
    );

    let mut travelled_expect = 0.0;
    let mut last_scalar = s + RAMP_START_SPEED_MPS;
    for tick_index in 1..=clamp_tick + 5 {
        runtime.advance(DT_S, STILL_AIR).expect("the tick advances");
        let live = runtime.iter().next().expect("the item is still live");
        let ramp = live.launch_ramp().expect("the ramp is live");
        let expected =
            (s + RAMP_START_SPEED_MPS + GAIN_PER_TICK_MPS * tick_index as f64).min(target);
        assert!(
            (ramp.speed_m_s() - expected).abs() < 1e-6,
            "tick {tick_index}: scalar {}, want {expected}",
            ramp.speed_m_s()
        );
        assert!(
            ramp.speed_m_s() > last_scalar || ramp.speed_m_s() == target,
            "tick {tick_index}: the scalar gains dt·ACCELERATION or holds at its target"
        );
        assert!(
            ramp.speed_m_s() <= target,
            "tick {tick_index}: the ramp never overshoots its target"
        );
        last_scalar = ramp.speed_m_s();
        // Every tick recomposes the velocity from the parts: the lateral
        // remainder stays put while the along-axis component is the scalar.
        let remainder = expected_remainder();
        let velocity = live.air_velocity_m_s();
        for axis in 0..3 {
            let want = remainder[axis] + ramp.direction()[axis] * ramp.speed_m_s();
            assert!(
                (velocity[axis] - want).abs() < 1e-6,
                "tick {tick_index} axis {axis}: velocity {}, want remainder + dir·scalar = {want}",
                velocity[axis]
            );
        }
        // The lateral remainder really is retained: the off-axis component
        // is the launcher's, unchanged by the ramp.
        assert!((velocity[0] - SHOOTER[0]).abs() < 1e-6);
        // The swept segment is `dt·velocity`: the step displaces what the
        // recomposed velocity says, and `OrdnanceState::advance` accumulates
        // the same length — the measured path-length counter.
        let speed =
            (velocity[0] * velocity[0] + velocity[1] * velocity[1] + velocity[2] * velocity[2])
                .sqrt();
        travelled_expect += speed * DT_S;
        let segment = live.segment();
        let from = segment.previous.to_array();
        let to = segment.current.to_array();
        let flown =
            ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2) + (to[2] - from[2]).powi(2))
                .sqrt();
        assert!(
            (flown - speed * DT_S).abs() < 1e-6,
            "tick {tick_index}: the displacement is dt·velocity, got {flown}"
        );
    }

    // After the clamp the item flies at `VELOCITY + |launcher|` along the
    // launch direction and does not gain further — and the path the item
    // actually flew is the ramp's own segments.
    let live = runtime.iter().next().expect("live at the end");
    let ramp = live.launch_ramp().expect("the ramp is live");
    assert_eq!(ramp.speed_m_s(), target, "the scalar is clamped, not lost");
    assert!(ramp.is_complete());
    let velocity = live.air_velocity_m_s();
    let along = velocity[2];
    assert!(
        (along - (LAUNCH_SPEED_MPS + SHOOTER[2])).abs() < 1e-6,
        "the along-axis speed ends at VELOCITY + the launcher's along-axis speed: {along}"
    );
    assert!(
        (live.state().travelled_m() - travelled_expect).abs() < 1e-4,
        "the travelled distance is the ramped path, not a nominal speed: {}",
        live.state().travelled_m()
    );
}

/// The same ramp through the session path: the launch order's own tick
/// already advances the item once (the step's `advance` runs after the
/// orders), so the scalar a caller reads is the ramped one.
#[test]
fn accept_t452_inherit_a_session_launch_runs_the_measured_ramp() {
    let mut session = ramping_session();
    let mut world = World::new();
    ramping_launcher(&mut world);
    let mut damage = damage_resolver();

    let launched = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0)],
        Tick(0),
    );
    assert_eq!(launched.launched.len(), 1, "the ramping component launches");

    let s = shooter_speed();
    let (scalar, target, velocity) = ramp_of(&session);
    // The launch tick's advance ran once: scalar = |launcher| + 1e-4 + dt·a.
    let expected = s + RAMP_START_SPEED_MPS + GAIN_PER_TICK_MPS;
    assert!(
        (scalar - expected).abs() < 1e-6,
        "the session's own advance ramps the scalar: got {scalar}"
    );
    assert_eq!(target, LAUNCH_SPEED_MPS + s);
    let remainder = expected_remainder();
    assert!(
        (velocity[2] - (remainder[2] + scalar)).abs() < 1e-6,
        "the session's item flies remainder + dir·scalar: {velocity:?}"
    );

    // One more session tick, one more measured gain.
    step(&mut world, &mut session, &mut damage, &[], Tick(1));
    let (scalar, _, _) = ramp_of(&session);
    assert!(
        (scalar - (s + RAMP_START_SPEED_MPS + 2.0 * GAIN_PER_TICK_MPS)).abs() < 1e-6,
        "the second session tick gains dt·ACCELERATION again: {scalar}"
    );
}

/// An `INSTANT` record still names its unexpressible launch rather than
/// borrowing the ramp it does not run.
#[test]
fn accept_t452_inherit_an_instant_record_still_names_its_blocker() {
    let mut record = ramping_record();
    record.record = "wep_instant".to_owned();
    record.first_values.remove("ACCELERATION");
    record.first_values.insert("INSTANT".to_owned(), 1.0);
    record.keys = record.first_values.keys().cloned().collect();
    let mut row = ramping_row();
    row.record = "wep_instant".to_owned();
    let mut media = ramping_media();
    media.record = "wep_instant".to_owned();
    let mut ammo = ramping_ammo();
    ammo.record = "wep_instant".to_owned();
    let blockers = measured_ordnance_declaration(
        &row,
        &record,
        &media,
        &ammo,
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect_err("the INSTANT one-step displacement is not the ramp");
    assert!(
        blockers.iter().any(
            |blocker| blocker.claim == cs_app::ordnance::T452_INHERITANCE_BLOCKED
                && blocker.field == "launch.inheritance"
        ),
        "the INSTANT launch names its claim: {blockers:?}"
    );
}

// -------------------------------------------------------------------- retail --

fn game_dir() -> std::path::PathBuf {
    let dir = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: accept_t452_inherit_retail_* need the original \
             installation; run them with --include-ignored and CS_GAME_DIR set"
        )
    });
    std::path::PathBuf::from(dir)
}

/// The retail half: a component lowered from the owner's `wep_04` record —
/// `VELOCITY 450`, `ACCELERATION 150` — launches through the production
/// session path and runs the measured ramp toward `VELOCITY + |launcher|`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_inherit_retail_the_measured_wep_04_ramps_through_the_session() {
    let root = game_dir();
    let table = cs_content::ordnance::read_original_component_table(&root)
        .expect("the component table reads");
    let records =
        cs_content::ordnance::read_original_weapon_records(&root).expect("the weapon records read");
    let media = cs_content::ordnance::read_original_weapon_media(&root)
        .expect("the media declarations read");
    let ammo = cs_content::ordnance::read_original_ammo_declarations(&root)
        .expect("the ammunition declarations read");
    let provenance = cs_content::ordnance::read_original_weapons_provenance(
        &root,
        cs_app::ordnance::T452_ORIGINAL_WEAPONS_CLAIM,
    )
    .expect("the record member's provenance is measured");

    let declared = measured_ordnance_declaration(
        table
            .rows
            .iter()
            .find(|row| row.record == "wep_04")
            .expect("the installation declares wep_04"),
        records
            .iter()
            .find(|record| record.record == "wep_04")
            .expect("wep_04 has a record"),
        media
            .iter()
            .find(|media| media.record == "wep_04")
            .expect("wep_04 has media declarations"),
        ammo.iter()
            .find(|ammo| ammo.record == "wep_04")
            .expect("wep_04 has an ammunition declaration"),
        &provenance,
        TICKS_PER_SECOND,
    )
    .expect("wep_04's measured values are expressible now");

    let mut catalogue = OrdnanceRegistry::new();
    let report = register_measured_ordnance(
        &table,
        &records,
        &media,
        &ammo,
        &provenance,
        &mut catalogue,
        TICKS_PER_SECOND,
    );
    assert!(
        report.registered_records().contains(&"wep_04"),
        "{report:?}"
    );

    // The retail record's own numbers are the ramp's: `ACCELERATION 150`,
    // `VELOCITY 450`, the lateral-remainder rule.
    let ordnance =
        cs_sim::weapons::OrdnanceId::try_new(weapon_id("wep_04")).expect("the weapon namespace");
    let projectile = catalogue
        .get(&ordnance)
        .and_then(|component| component.as_projectile())
        .expect("wep_04 registered");
    assert_eq!(projectile.launch().launch_speed_mps(), 450.0);
    assert_eq!(projectile.launch().acceleration_mps2(), Some(150.0));
    assert_eq!(
        projectile.launch().inheritance(),
        InheritanceRule::LateralRemainder
    );
    let lifetime_ticks = projectile.lifetime_ticks();

    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured component registers in the session");

    let mut world = World::new();
    ramping_launcher(&mut world);
    let mut damage = damage_resolver();
    let launched = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0)],
        Tick(0),
    );
    assert_eq!(launched.launched.len(), 1);

    let s = shooter_speed();
    let target = LAUNCH_SPEED_MPS + s;
    // The launch tick advanced once; each following step advances once more.
    let mut advances = 1_u64;
    let mut clamped_seen = false;
    loop {
        let Some((scalar, complete)) = session
            .runtime()
            .iter()
            .next()
            .and_then(|live| live.launch_ramp().map(|r| (r.speed_m_s(), r.is_complete())))
        else {
            break;
        };
        let expected = (s + RAMP_START_SPEED_MPS + GAIN_PER_TICK_MPS * advances as f64).min(target);
        assert!(
            (scalar - expected).abs() < 1e-6,
            "retail advance {advances}: scalar {scalar}, want {expected}"
        );
        assert!(scalar <= target, "the retail ramp never overshoots");
        clamped_seen |= complete;
        if advances >= lifetime_ticks + 5 {
            break;
        }
        step(&mut world, &mut session, &mut damage, &[], Tick(advances));
        advances += 1;
    }
    // The measured ramp runs `VELOCITY/ACCELERATION` seconds — 90 ticks at
    // 30 Hz — and this record's measured flight is long enough to show it:
    // the scalar must have reached `VELOCITY + |launcher|` and stayed.
    assert!(
        lifetime_ticks > 91,
        "wep_04's measured flight of {lifetime_ticks} ticks covers the 90-tick ramp"
    );
    assert!(
        clamped_seen,
        "the scalar reached VELOCITY + |launcher| before the flight ended"
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
/// measured path: `wep_04`'s fixture shape lowered and registered.
fn ramping_session() -> OrdnanceSession {
    let declared = measured_ordnance_declaration(
        &ramping_row(),
        &ramping_record(),
        &ramping_media(),
        &ramping_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("the ramping record lowers");
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

/// The ramping launch's order for one tick.
fn launch_order(sequence: u32) -> OrdnanceOrder {
    OrdnanceOrder::Launch(OrdnanceLaunch {
        id: OrdnanceLaunchId {
            session: SESSION,
            tick: Tick(0),
            producer: PRODUCER,
            sequence,
        },
        shooter: actor(1),
        ordnance: cs_sim::weapons::OrdnanceId::try_new(weapon_id("wep_04"))
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
/// mount node for `wep_04`, facing +Z — so the shooter has both an
/// along-direction and a lateral velocity component.
fn ramping_launcher(world: &mut World) -> Entity {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![weapon_id("wep_04")],
                loadout: ContentId::from_source(
                    cs_types::content::ContentKind::Loadout,
                    "fixture.loadout",
                )
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
            mount: key("weapon_mount_04"),
            generation: GENERATION,
        },
        cs_app::scene::NodeVisualTransform(GlobalTransform::from(
            Transform::from_xyz(0.0, 0.0, 10.0).looking_to(Vec3::Z, Vec3::Y),
        )),
        ChildOf(root),
    ));
    root
}

/// `(speed scalar, target, air velocity)` of the session's live item.
fn ramp_of(session: &OrdnanceSession) -> (f64, f64, [f64; 3]) {
    let live = session
        .runtime()
        .iter()
        .next()
        .expect("the launched item is live");
    let ramp = live.launch_ramp().expect("a ramped item");
    (ramp.speed_m_s(), ramp.target_m_s(), live.air_velocity_m_s())
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
            wind_velocity_m_s: [0.0; 3],
            generation: GENERATION,
            guidance: None,
            targets: &[],
            impacts: &[],
            blast_recipients: None,
            victim_nodes: None,
        },
    )
}
