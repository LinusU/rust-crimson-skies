//! Task T452-RUNTIME-REGISTER (#2156/#2171): the measured original ordnance
//! lowering and its registration in [`cs_sim::weapons::OrdnanceRegistry`].
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-D`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Every test calls production code: the measured readers
//! ([`cs_content::ordnance`]), the lowering boundary
//! ([`cs_app::ordnance::measured_ordnance_declaration`],
//! [`cs_app::ordnance::register_measured_ordnance`], `lower_ordnance`) and the
//! per-tick session ([`OrdnanceSession`], [`step_ordnance_session`]). Nothing is
//! reimplemented here.
//!
//! The two synthetic fixtures carry the *shape* of two measured records (the
//! guided `wep_05` geometry and an `ACCELERATION`-ramping record) so the
//! lowering's field-by-field mapping is pinned in CI, where no original data
//! exists; `accept_t452_register_retail_*` runs the same production path over
//! the owner's installation. No test awards `verified_original`.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    ORIGINAL_ORDNANCE_UNIT_MASS_KG, OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId,
    OrdnanceLauncherBinding, OrdnanceOrder, OrdnanceSession, OrdnanceStep,
    OriginalRegistrationOutcome, OriginalRegistrationReport, T452_INHERITANCE_BLOCKED,
    T452_MEASURED_BEHAVIOUR_CLAIM, T452_PER_BOX_WEIGHT_CLAIM, T452_REG_INHERITANCE_TASK,
    T452_SILENT_EXPIRY_BLOCKED, T452_SILENT_EXPIRY_TASK, lower_ordnance,
    measured_ordnance_declaration, read_and_register_measured_ordnance, register_measured_ordnance,
    step_ordnance_session,
};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DeclaredOrdnance, DeclaredOrdnanceDetails, DeclaredOrdnanceFamily, DeclaredStackLoad,
    ORIGINAL_HARDPOINT_WEIGHT_PER_BOX, OriginalAmmoDeclaration, OriginalAreaDeclaration,
    OriginalClusterShape, OriginalComponentLaunch, OriginalComponentTable,
    OriginalDamageDeclaration, OriginalEffectBlock, OriginalLifetime, OriginalMountDeclaration,
    OriginalMountLayout, OriginalSlotArray, OriginalWeaponMedia, OriginalWeaponRecord,
    declared_synthetic_guided, declared_unknown,
};
use cs_sim::damage::{ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::{AreaEffect, FuseRule, GuidanceRule, LostTargetBehavior, OrdnanceRegistry};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, Origin, Provenance};
use cs_types::evidence::{ClaimId as EvidenceClaimId, ClaimStatus, ContentHash};
use cs_types::net::SessionId;

// ------------------------------------------------------------------ fixtures --

pub(crate) const SESSION: u64 = 61;
pub(crate) const PRODUCER: u32 = 71;
const DAMAGE_PRODUCER: u32 = 72;
const GENERATION: SceneGeneration = SceneGeneration(1);
const DT_S: f64 = 1.0 / 30.0;
const TICKS_PER_SECOND: u32 = 30;

pub(crate) fn provenance() -> Provenance {
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

/// The measured shape of the guided `wep_05`: a full-inheritance seeker whose
/// own `DETONATION_DISTANCE` is its proximity trigger and whose `CLUSTER_SIZE`
/// is its per-slot stack.
pub(crate) fn guided_row() -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: "wep_05".to_owned(),
        family: DeclaredOrdnanceFamily::GuidedRocket,
        mount: OriginalMountDeclaration::RocketSlot { slots: 8 },
        velocity: Some(1200.0),
        fire_rate: Some(1.0),
        proximity_trigger_m: Some(15.0),
        lifetime: OriginalLifetime::Undeclared,
        area: OriginalAreaDeclaration {
            radius_m: Some(15.0),
            lifetime_s: None,
        },
        damage: OriginalDamageDeclaration {
            armor: Some(60.0),
            health: Some(40.0),
            single: None,
        },
        statuses: Vec::new(),
        cluster: OriginalClusterShape::Volley { munitions: 4 },
        unknowns: Vec::new(),
    }
}

pub(crate) fn guided_record() -> OriginalWeaponRecord {
    let mut first_values = BTreeMap::new();
    first_values.insert("VELOCITY".to_owned(), 1200.0);
    first_values.insert("RANGE".to_owned(), 1000.0);
    first_values.insert("DETONATION_DISTANCE".to_owned(), 15.0);
    first_values.insert("IMPACT_PROXIMITY".to_owned(), 15.0);
    first_values.insert("ARMOR_DAMAGE".to_owned(), 60.0);
    first_values.insert("HEALTH_DAMAGE".to_owned(), 40.0);
    first_values.insert("CLUSTER_SIZE".to_owned(), 4.0);
    first_values.insert("LOCK_ON".to_owned(), 1.0);
    OriginalWeaponRecord {
        record: "wep_05".to_owned(),
        keys: first_values.keys().cloned().collect(),
        first_values,
        second_values: BTreeMap::new(),
        first_texts: BTreeMap::new(),
    }
}

pub(crate) fn guided_media() -> OriginalWeaponMedia {
    OriginalWeaponMedia {
        record: "wep_05".to_owned(),
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

pub(crate) fn guided_ammo() -> OriginalAmmoDeclaration {
    OriginalAmmoDeclaration {
        record: "wep_05".to_owned(),
        ammo_limit: None,
        cluster_size: Some(4.0),
    }
}

/// One measured record the runtime cannot express: `ACCELERATION` non-zero, so
/// its launch inheritance is the measured lateral-remainder ramp.
fn ramping_row() -> (OriginalComponentLaunch, OriginalWeaponRecord) {
    let row = OriginalComponentLaunch {
        record: "wep_04".to_owned(),
        family: DeclaredOrdnanceFamily::GuidedRocket,
        mount: OriginalMountDeclaration::RocketSlot { slots: 8 },
        velocity: Some(450.0),
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
    };
    let mut first_values = BTreeMap::new();
    first_values.insert("VELOCITY".to_owned(), 450.0);
    first_values.insert("ACCELERATION".to_owned(), 150.0);
    first_values.insert("RANGE".to_owned(), 900.0);
    first_values.insert("DETONATION_DISTANCE".to_owned(), 30.0);
    first_values.insert("CLUSTER_SIZE".to_owned(), 1.0);
    first_values.insert("LOCK_ON".to_owned(), 1.0);
    let record = OriginalWeaponRecord {
        record: "wep_04".to_owned(),
        keys: first_values.keys().cloned().collect(),
        first_values,
        second_values: BTreeMap::new(),
        first_texts: BTreeMap::new(),
    };
    (row, record)
}

fn ramping_media() -> OriginalWeaponMedia {
    OriginalWeaponMedia {
        record: "wep_04".to_owned(),
        keys: BTreeSet::new(),
        fire: guided_media().fire,
        flyout: guided_media().flyout,
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

pub(crate) fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("the fixture session is nonzero")
}

pub(crate) fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("the fixture node keys are valid")
}

pub(crate) fn rate() -> TickRate {
    TickRate::new(TICKS_PER_SECOND).expect("the fixture rate is nonzero")
}

// ------------------------------------------------------------------- tests ---

/// The measured guided record lowers field by field and the registry accepts
/// it: family, launch geometry, stack, fuse, guidance, lifetime, area, damage
/// and media all carry their measured value.
#[test]
fn accept_t452_register_a_measured_component_registers_with_its_measured_values() {
    let declared = measured_ordnance_declaration(
        &guided_row(),
        &guided_record(),
        &guided_media(),
        &guided_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("every measured value of wep_05 is expressible");
    let component = lower_ordnance(&declared).expect("the measured record lowers");
    let projectile = component
        .as_projectile()
        .expect("a guided rocket is a projectile");

    assert_eq!(
        component.family(),
        cs_sim::weapons::OrdnanceFamily::GuidedRocket
    );
    // T452-R.03-R.05: the mount key names the measured mount id, `wep_05` -> 5.
    assert_eq!(
        projectile.launch().mount().as_str(),
        "weapon_mount_05",
        "the mount key carries the measured mount id"
    );
    // T452-R.01: `VELOCITY` is metres per second.
    assert_eq!(projectile.launch().launch_speed_mps(), 1200.0);
    // T452-R.04 §1: the refire interval delays the *next* release, so the
    // component's own release delay is the measured zero.
    assert_eq!(projectile.launch().release_delay_ticks(), 0);
    // T452-R.03-B: the per-slot stack is the record's `CLUSTER_SIZE`.
    assert_eq!(projectile.stack().capacity_units(), 4);
    // The measured per-type mass term is an absence: the airframe weight total
    // reads no weapon record field.
    assert_eq!(
        projectile.stack().unit_mass_kg(),
        ORIGINAL_ORDNANCE_UNIT_MASS_KG
    );
    // T452-R.02 §3: the record's own `DETONATION_DISTANCE` is the trigger.
    let FuseRule::Proximity(fuse) = projectile.fuse() else {
        panic!("wep_05's DETONATION_DISTANCE is its proximity fuse");
    };
    assert_eq!(fuse.trigger_radius_m(), 15.0);
    // T452-R.02 §4: a `LOCK_ON` seeker coasts then detonates at the end of
    // `RANGE`.
    assert_eq!(
        projectile.guidance(),
        GuidanceRule::Targeted {
            lost_target: LostTargetBehavior::CoastThenDetonate
        }
    );
    // T452-R.04 §2: `RANGE 1000` at `VELOCITY 1200` is 0.833 s, rounded up to
    // the session's 30 Hz tick.
    assert_eq!(projectile.lifetime_ticks(), 25);
    // T1318/T452-R.02 §2: `IMPACT_PROXIMITY` is the blast radius and a blast is
    // instantaneous.
    assert_eq!(
        projectile.area_effect(),
        Some(AreaEffect::try_new(15.0, 1).expect("a one-tick blast area is valid"))
    );
    assert_eq!(projectile.channels().armor, 60.0);
    assert_eq!(projectile.channels().internal, 40.0);
    // T452-R.03-C: the projectile's own `FLYOUT` model and `FIRE` sound.
    assert_eq!(projectile.media().visual().key(), "ap_rocket");
    assert_eq!(
        projectile.media().sound().map(ContentId::key),
        Some("snd_missile_sm")
    );

    // The registry accepts what the lowering produced, and refuses a second
    // registration of the same id.
    let table = table(vec![guided_row()]);
    let mut registry = OrdnanceRegistry::new();
    let report = register_measured_ordnance(
        &table,
        &[guided_record()],
        &[guided_media()],
        &[guided_ammo()],
        &provenance(),
        &mut registry,
        TICKS_PER_SECOND,
    );
    assert_eq!(report.registered_records(), vec!["wep_05"]);
    assert!(report.blockers().is_empty(), "{:?}", report.blockers());
    assert!(
        registry
            .get(
                &cs_sim::weapons::OrdnanceId::try_new(declared.ordnance().clone())
                    .expect("the weapon namespace")
            )
            .is_some()
    );
    // The measured per-box weight the stack cannot carry is still named.
    assert!(
        report.claims_kept.iter().any(|(record, claims)| {
            record == "wep_05"
                && claims
                    .iter()
                    .any(|claim| claim == T452_PER_BOX_WEIGHT_CLAIM)
        }),
        "the flat {} weight per enabled box stays named: {:?}",
        ORIGINAL_HARDPOINT_WEIGHT_PER_BOX,
        report.claims_kept
    );
}

/// A component registered from the measured catalogue launches through the
/// production session path, with its measured launch speed, stack and fuse.
#[test]
fn accept_t452_register_a_registered_measured_component_launches_in_a_session() {
    let declared = measured_ordnance_declaration(
        &guided_row(),
        &guided_record(),
        &guided_media(),
        &guided_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("wep_05's measured values are expressible");
    let ordnance = cs_sim::weapons::OrdnanceId::try_new(declared.ordnance().clone())
        .expect("the weapon namespace");
    let component = lower_ordnance(&declared).expect("the measured record lowers");
    let mut catalogue = OrdnanceRegistry::new();
    catalogue
        .register(component)
        .expect("the catalogue takes it");

    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    let registered = session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured component registers in the session");
    assert_eq!(registered.len(), 1);
    assert_eq!(
        registered[0].mount.as_ref().map(DamageNodeKey::as_str),
        Some("weapon_mount_05")
    );

    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();

    let launched = step(
        &mut world,
        &mut session,
        &mut damage,
        &[OrdnanceOrder::Launch(OrdnanceLaunch {
            id: OrdnanceLaunchId {
                session: SESSION,
                tick: Tick(0),
                producer: PRODUCER,
                sequence: 0,
            },
            shooter: actor(1),
            ordnance: ordnance.clone(),
            target: None,
            engagement: OrdnanceEngagement {
                damage_target: actor(2),
                node: key("hull"),
                status_recipient: cs_sim::weapons::StatusEffectTarget::whole_actor(actor(2)),
            },
        })],
        Tick(0),
    );
    assert_eq!(
        launched.launched.len(),
        1,
        "the measured component launches"
    );
    let live = session
        .runtime()
        .iter()
        .next()
        .expect("the launched item is live");
    // The measured launch speed is what the item leaves the rail with: the
    // launcher is stationary, so the whole of it is `VELOCITY`.
    let speed = live.air_velocity_m_s();
    let speed = (speed[0] * speed[0] + speed[1] * speed[1] + speed[2] * speed[2]).sqrt();
    assert!(
        (speed - 1200.0).abs() < 1.0,
        "the item leaves the rail at the measured VELOCITY, got {speed}"
    );
    // One launch spends one unit of the measured `CLUSTER_SIZE` stack.
    let stock = session
        .stack_remaining(&actor(1), &ordnance)
        .expect("a projectile has a stack");
    assert_eq!(stock.capacity_units, 4);
    assert_eq!(stock.remaining_units, 3);
}

/// The registry refuses an id with an unmeasured field: lowering names the
/// field and its claim, and nothing reaches the registry.
#[test]
fn accept_t452_register_lowering_refuses_a_field_no_measurement_states() {
    // A declared component whose unit mass is explicitly unknown: the boundary
    // refuses it by name rather than flying a fabricated mass.
    let fixture = declared_synthetic_guided();
    let DeclaredOrdnanceDetails::Projectile(projectile) = fixture.details() else {
        panic!("the guided fixture is a projectile");
    };
    let mut projectile = (**projectile).clone();
    projectile.stack = DeclaredStackLoad {
        capacity_units: projectile.stack.capacity_units.clone(),
        unit_mass_kg: declared_unknown("no measurement states the per-round mass"),
    };
    let refused = DeclaredOrdnance::try_new(
        fixture.ordnance().clone(),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::GuidedRocket,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        provenance(),
    )
    .expect("an unresolved field is a legal record")
    .clone();
    let error = lower_ordnance(&refused).expect_err("the boundary refuses an unknown mass");
    let text = error.to_string();
    assert!(
        text.contains("stack.unit_mass_kg"),
        "the refusal names the field: {text}"
    );

    // And the measured path itself refuses a record whose fuse the runtime
    // cannot express, registering nothing and naming the claim and task.
    let (row, record) = ramping_row();
    let error = measured_ordnance_declaration(
        &row,
        &record,
        &ramping_media(),
        &ramping_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect_err("the ramping record's inheritance is unexpressible");
    let blocker = error
        .iter()
        .find(|blocker| blocker.claim == T452_INHERITANCE_BLOCKED)
        .unwrap_or_else(|| panic!("the inheritance claim is named: {error:?}"));
    assert_eq!(blocker.field, "launch.inheritance");
    assert_eq!(blocker.resolving_task, T452_REG_INHERITANCE_TASK);
    assert!(blocker.affects.contains("launch geometry"));
}

/// Every declared id is either registered or blocked with a named claim, the
/// content it affects and the task that resolves it — never silently dropped.
#[test]
fn accept_t452_register_every_id_is_registered_or_names_its_blocker() {
    let (ramping, ramping_record) = ramping_row();
    let table = table(vec![guided_row(), ramping]);
    let mut registry = OrdnanceRegistry::new();
    let report: OriginalRegistrationReport = register_measured_ordnance(
        &table,
        &[guided_record(), ramping_record],
        &[guided_media(), ramping_media()],
        &[guided_ammo(), ramping_ammo()],
        &provenance(),
        &mut registry,
        TICKS_PER_SECOND,
    );
    assert_eq!(report.declared, 2);
    assert_eq!(report.registered_records(), vec!["wep_05"]);
    assert_eq!(report.blocked_records(), vec!["wep_04"]);
    assert!(!report.is_complete());
    for blocker in report.blockers() {
        assert!(!blocker.record.is_empty());
        assert!(!blocker.field.is_empty());
        assert!(!blocker.claim.is_empty());
        assert!(!blocker.affects.is_empty());
        assert!(!blocker.resolving_task.is_empty());
    }
}

/// A record with no declared `FIRE` `SOUND` is a measured silence: it registers,
/// its media names no sound (so the launch effect, copied from it, has none), while a record
/// that declares one still names it.
#[test]
fn accept_t452_media_silence_a_record_without_fire_sound_registers_and_plays_nothing() {
    let mut silent_media = guided_media();
    silent_media.fire = None;
    let declared = measured_ordnance_declaration(
        &guided_row(),
        &guided_record(),
        &silent_media,
        &guided_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("a measured silent launch is expressible");
    let component = lower_ordnance(&declared).expect("the silent record lowers");
    let projectile = component.as_projectile().expect("a rocket is a projectile");
    assert_eq!(projectile.media().sound(), None, "no sound is invented");
    assert!(
        projectile.media().visual().key().contains("ap_rocket"),
        "the visual is still the record's own FLYOUT model"
    );
    let mut registry = OrdnanceRegistry::new();
    registry
        .register(component)
        .expect("the silent component registers");

    let declared = measured_ordnance_declaration(
        &guided_row(),
        &guided_record(),
        &guided_media(),
        &guided_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("the sounding record declares");
    let component = lower_ordnance(&declared).expect("the sounding record lowers");
    assert_eq!(
        component
            .as_projectile()
            .expect("a rocket is a projectile")
            .media()
            .sound()
            .map(ContentId::key),
        Some("snd_missile_sm"),
        "a record that declares a sound still names it"
    );
}

/// The measured end of flight the runtime cannot express is a claim, not a
/// defaulted `Impact` or `Timed` fuse.
#[test]
fn accept_t452_register_a_silent_flight_end_is_a_claim_not_a_fuse() {
    let mut row = guided_row();
    row.record = "wep_12".to_owned();
    row.family = DeclaredOrdnanceFamily::AreaDenialEngine;
    row.proximity_trigger_m = Some(35.0);
    row.damage = OriginalDamageDeclaration {
        armor: Some(0.0),
        health: Some(0.0),
        single: None,
    };
    let mut record = guided_record();
    record.record = "wep_12".to_owned();
    let mut media = guided_media();
    media.record = "wep_12".to_owned();
    let mut ammo = guided_ammo();
    ammo.record = "wep_12".to_owned();
    let error = measured_ordnance_declaration(
        &row,
        &record,
        &media,
        &ammo,
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect_err("the record's measured end of flight has no runtime variant");
    let blocker = error
        .iter()
        .find(|blocker| blocker.claim == T452_SILENT_EXPIRY_BLOCKED)
        .unwrap_or_else(|| panic!("the silent-expiry claim is named: {error:?}"));
    assert_eq!(blocker.field, "fuse");
    assert_eq!(blocker.resolving_task, T452_SILENT_EXPIRY_TASK);
    assert!(
        blocker.affects.contains("silent removal"),
        "the claim states what the measurement found: {}",
        blocker.affects
    );
}

// -------------------------------------------------------------------- retail --

fn game_dir() -> std::path::PathBuf {
    let dir = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: accept_t452_register_retail_* need the original \
             installation; run them with --include-ignored and CS_GAME_DIR set"
        )
    });
    std::path::PathBuf::from(dir)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_register_retail_the_measured_catalogue_registers_what_it_measures() {
    let mut registry = OrdnanceRegistry::new();
    let report = read_and_register_measured_ordnance(&game_dir(), &mut registry, 30)
        .expect("the installation's records read");

    // Sixteen declared ids, every one accounted for.
    assert_eq!(report.declared, 16);
    assert_eq!(report.outcomes.len(), 16);

    // The measurements this stage can express: ten ids register. They are the
    // full-inheritance records plus the timer-fused flare and the torpedo.
    assert_eq!(
        report.registered_records(),
        vec![
            "wep_05", "wep_06", "wep_07", "wep_08", "wep_09", "wep_10", "wep_11", "wep_14",
            "wep_15", "wep_24",
        ],
        "the ids whose measured values the runtime vocabulary expresses"
    );

    // The six that cannot be expressed are blocked with the measured value's
    // claim, its content and its resolving task — never defaulted.
    let mut blocked: Vec<(&str, Vec<&str>)> = Vec::new();
    for outcome in &report.outcomes {
        if let OriginalRegistrationOutcome::Blocked { record, blockers } = outcome {
            blocked.push((
                record.as_str(),
                blockers
                    .iter()
                    .map(|blocker| blocker.claim.as_str())
                    .collect(),
            ));
        }
    }
    let expected = [
        ("wep_04", T452_INHERITANCE_BLOCKED),
        ("wep_12", T452_SILENT_EXPIRY_BLOCKED),
        ("wep_13", T452_SILENT_EXPIRY_BLOCKED),
        ("wep_25", T452_INHERITANCE_BLOCKED),
        ("wep_26", T452_INHERITANCE_BLOCKED),
        ("wep_27", T452_INHERITANCE_BLOCKED),
    ];
    assert_eq!(blocked.len(), expected.len(), "{blocked:?}");
    for (record, claim) in expected {
        let (_, claims) = blocked
            .iter()
            .find(|(name, _)| *name == record)
            .unwrap_or_else(|| panic!("{record} is blocked: {blocked:?}"));
        assert!(
            claims.contains(&claim),
            "{record} must name {claim}, got {claims:?}"
        );
    }
    // The flak record additionally has no declared launch sound, whose claim
    // names its own task.
    let (_, flak) = blocked
        .iter()
        .find(|(name, _)| *name == "wep_26")
        .expect("wep_26 is blocked");
    assert!(
        !flak.iter().any(|claim| claim.contains("silent_launch")),
        "a missing launch sound is a measured silence, not a blocker: {flak:?}"
    );

    // A registered id is a *runtime component*, not a catalogue entry: its
    // measured values are the record's own.
    let wep_05 =
        cs_types::content::ContentId::from_source(cs_types::content::ContentKind::Weapon, "wep_05")
            .expect("a weapon id");
    let id = cs_sim::weapons::OrdnanceId::try_new(wep_05).expect("the weapon namespace");
    let component = registry
        .get(&id)
        .expect("wep_05 registered as a flyable component");
    let projectile = component.as_projectile().expect("a rocket is a projectile");
    assert_eq!(projectile.launch().launch_speed_mps(), 1200.0);
    assert_eq!(projectile.stack().capacity_units(), 4);
    assert_eq!(projectile.channels().armor, 60.0);
    assert_eq!(projectile.channels().internal, 40.0);
    let FuseRule::Proximity(fuse) = projectile.fuse() else {
        panic!("wep_05's own DETONATION_DISTANCE is its fuse");
    };
    assert_eq!(fuse.trigger_radius_m(), 15.0);

    // The live-marker record carries the measured `BEEPER` `TIME`.
    let wep_10 = cs_sim::weapons::OrdnanceId::try_new(
        cs_types::content::ContentId::from_source(cs_types::content::ContentKind::Weapon, "wep_10")
            .expect("a weapon id"),
    )
    .expect("the weapon namespace");
    let marker = registry
        .get(&wep_10)
        .and_then(|component| component.as_projectile())
        .expect("wep_10 registered");
    assert_eq!(marker.status().len(), 1);
    assert_eq!(
        marker.status()[0].kind(),
        cs_sim::weapons::StatusEffectKind::Marker
    );
    // `TIME 20 s` at the 30 Hz rate the report was asked for.
    assert_eq!(marker.status()[0].duration_ticks(), 600);

    // Every registered id also appears in the catalogue layer, so the two maps
    // agree about identity; nothing measured was removed from the catalogue.
    for record in report.registered_records() {
        assert!(
            report.claims_kept.iter().any(|(name, _)| name == record),
            "{record} keeps its named claims"
        );
    }
    assert!(
        !report.is_complete(),
        "six measured values stay unexpressible"
    );
    for blocker in report.blockers() {
        assert!(
            !blocker.affects.is_empty() && !blocker.resolving_task.is_empty(),
            "{}: the blocker names its content and task",
            blocker.record
        );
    }
    // The measured per-box weight is recorded for every id, registered or not.
    for (record, claims) in &report.claims_kept {
        assert!(
            claims
                .iter()
                .any(|claim| claim == T452_PER_BOX_WEIGHT_CLAIM),
            "{record} keeps the per-box weight claim"
        );
    }
    // The claim id of every blocker is a real claim, not an empty string.
    assert!(report.blockers().iter().all(|blocker| {
        blocker.claim.starts_with(T452_MEASURED_BEHAVIOUR_CLAIM) || !blocker.claim.is_empty()
    }));
}

/// The retail half of the launch criterion: a component **registered from the
/// owner's measured catalogue** launches through the production session path
/// with its measured fields, not with a fixture's shape.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_register_retail_a_registered_measured_component_launches() {
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

    let row = table
        .rows
        .iter()
        .find(|row| row.record == "wep_05")
        .expect("the installation declares wep_05");
    let record = records
        .iter()
        .find(|record| record.record == "wep_05")
        .expect("wep_05 has a record");
    let declared_media = media
        .iter()
        .find(|media| media.record == "wep_05")
        .expect("wep_05 has media declarations");
    let declared_ammo = ammo
        .iter()
        .find(|ammo| ammo.record == "wep_05")
        .expect("wep_05 has an ammunition declaration");
    let declared = measured_ordnance_declaration(
        row,
        record,
        declared_media,
        declared_ammo,
        &provenance,
        TICKS_PER_SECOND,
    )
    .expect("wep_05's measured values are expressible");

    // The registry is filled by the production registration path, not by this
    // test: the component the session launches is the registered one.
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
        report.registered_records().contains(&"wep_05"),
        "{report:?}"
    );
    let ordnance = cs_sim::weapons::OrdnanceId::try_new(declared.ordnance().clone())
        .expect("the weapon namespace");
    let registered_component = catalogue
        .get(&ordnance)
        .expect("the measured registration holds wep_05");
    let projectile = registered_component
        .as_projectile()
        .expect("a guided rocket is a projectile");
    assert_eq!(projectile.launch().launch_speed_mps(), 1200.0);
    assert_eq!(projectile.stack().capacity_units(), 4);

    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    let registered = session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured component registers in the session");
    assert_eq!(registered.len(), 1);
    assert_eq!(
        registered[0].mount.as_ref().map(DamageNodeKey::as_str),
        Some("weapon_mount_05")
    );

    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();
    let launched = step(
        &mut world,
        &mut session,
        &mut damage,
        &[OrdnanceOrder::Launch(OrdnanceLaunch {
            id: OrdnanceLaunchId {
                session: SESSION,
                tick: Tick(0),
                producer: PRODUCER,
                sequence: 0,
            },
            shooter: actor(1),
            ordnance: ordnance.clone(),
            target: None,
            engagement: OrdnanceEngagement {
                damage_target: actor(2),
                node: key("hull"),
                status_recipient: cs_sim::weapons::StatusEffectTarget::whole_actor(actor(2)),
            },
        })],
        Tick(0),
    );
    assert_eq!(
        launched.launched.len(),
        1,
        "the measured catalogue's component launches"
    );
    let live = session
        .runtime()
        .iter()
        .next()
        .expect("the launched item is live");
    let speed = live.air_velocity_m_s();
    let speed = (speed[0] * speed[0] + speed[1] * speed[1] + speed[2] * speed[2]).sqrt();
    assert!(
        (speed - 1200.0).abs() < 1.0,
        "the item leaves the rail at the record's measured VELOCITY, got {speed}"
    );
    let stock = session
        .stack_remaining(&actor(1), &ordnance)
        .expect("a projectile has a stack");
    assert_eq!(stock.capacity_units, 4);
    assert_eq!(stock.remaining_units, 3);
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

/// The shooter's live launcher: a root carrying the binding and one mount node
/// for the measured `wep_05` mount id.
fn launcher(world: &mut World) -> Entity {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![
                    cs_types::content::ContentId::from_source(
                        cs_types::content::ContentKind::Weapon,
                        "wep_05",
                    )
                    .expect("a weapon id"),
                ],
                loadout: cs_types::content::ContentId::from_source(
                    cs_types::content::ContentKind::Loadout,
                    "fixture.loadout",
                )
                .expect("a loadout id"),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::ZERO),
        ))
        .id();
    world.spawn((
        MountPoseBinding {
            mount: key("weapon_mount_05"),
            generation: GENERATION,
        },
        cs_app::scene::NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, 10.0)),
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
