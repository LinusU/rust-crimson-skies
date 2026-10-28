//! Task T452-REG-SILENT-EXPIRY (#2175): a record whose measured end of flight
//! is a silent expiry at the end of `RANGE` registers and is consumed through
//! the production session path.
//!
//! `docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md` §3
//! measured that the original's step ends such a flight by marking the
//! projectile dead with no detonation (`0x5b01ec`/`0x5b0318`). The lowering in
//! `cs_app::ordnance` used to block those records on
//! `t452_register.limit.silent_expiry`; this stage gives that end a runtime
//! variant, so the ids it alone held back register.
//!
//! Every test calls production code: the measured readers
//! ([`cs_content::ordnance`]), the lowering and registration boundary
//! ([`cs_app::ordnance`]) and the per-tick session ([`OrdnanceSession`],
//! [`step_ordnance_session`]). The synthetic fixture carries the *shape* of the
//! measured `wep_13` (area denial, no `VELOCITY`, no `RANGE`, `SMOKE_SCREEN`
//! with `TIME 8`) so the path is pinned in CI, where no original data exists;
//! `accept_t452_silent_retail_*` runs the same path over the owner's
//! installation. No test awards `verified_original`.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceConsumption, OrdnanceEngagement, OrdnanceEventKind, OrdnanceLaunch, OrdnanceLaunchId,
    OrdnanceLauncherBinding, OrdnanceOrder, OrdnanceSession, OrdnanceStep,
    OriginalRegistrationOutcome, T452_SILENT_EXPIRY_BLOCKED, lower_ordnance,
    measured_ordnance_declaration, read_and_register_measured_ordnance, register_measured_ordnance,
    step_ordnance_session,
};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DeclaredOrdnance, DeclaredOrdnanceFamily, OriginalAmmoDeclaration, OriginalAreaDeclaration,
    OriginalClusterShape, OriginalComponentLaunch, OriginalComponentTable,
    OriginalDamageDeclaration, OriginalEffectBlock, OriginalLifetime, OriginalMountDeclaration,
    OriginalMountLayout, OriginalSlotArray, OriginalStatusEffectDeclaration, OriginalWeaponMedia,
    OriginalWeaponRecord, T452_R02_SMOKE_STUN_EFFECT, T452_R02_TANGLER_EFFECT,
};
use cs_sim::damage::{ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::{BlastRecipient, FuseRule, OrdnanceId, OrdnanceRegistry, StatusEffectTarget};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::Provenance;
use cs_types::evidence::{ClaimId as EvidenceClaimId, ClaimStatus, ContentHash};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

// ------------------------------------------------------------------ fixtures --

const SESSION: u64 = 83;
const PRODUCER: u32 = 84;
const DAMAGE_PRODUCER: u32 = 85;
const GENERATION: SceneGeneration = SceneGeneration(1);
const DT_S: f64 = 1.0 / 30.0;
const TICKS_PER_SECOND: u32 = 30;

/// The parser's `RANGE` default: `500.0` at `0x5ad80b` (T452-R.01).
const ORIGINAL_RANGE_DEFAULT_M: f64 = 500.0;

fn provenance() -> Provenance {
    Provenance::new(
        EvidenceClaimId::new(cs_app::ordnance::T452_ORIGINAL_WEAPONS_CLAIM)
            .expect("a valid claim id"),
        ClaimStatus::ObservedTool,
        Some(
            SourceSpan::new(
                ContentHash::from_bytes([9; 32]),
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

/// The measured shape of `wep_13`: an unguided area-denial record with no
/// `VELOCITY`, no `RANGE` and no declared area radius, whose `SMOKE_SCREEN`
/// states `TIME 8` and whose `CLUSTER_SIZE` is its per-slot stack (T452.04,
/// T452-R.02 §1).
fn smoke_row() -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: "wep_13".to_owned(),
        family: DeclaredOrdnanceFamily::AreaDenialEngine,
        mount: OriginalMountDeclaration::RocketSlot { slots: 8 },
        velocity: None,
        fire_rate: Some(1.0),
        proximity_trigger_m: None,
        lifetime: OriginalLifetime::Undeclared,
        area: OriginalAreaDeclaration {
            radius_m: None,
            lifetime_s: Some(8.0),
        },
        damage: OriginalDamageDeclaration {
            armor: Some(0.0),
            health: Some(0.0),
            single: None,
        },
        statuses: vec![OriginalStatusEffectDeclaration {
            key: "SMOKE_SCREEN",
            duration_s: Some(8.0),
            duration_unit: Some("TIME".to_owned()),
        }],
        cluster: OriginalClusterShape::Volley { munitions: 6 },
        unknowns: Vec::new(),
    }
}

/// `wep_13`'s record: no `VELOCITY` and no `RANGE`, so both take the parser's
/// measured defaults, and its `CLUSTER_SIZE` is the volley.
fn smoke_record() -> OriginalWeaponRecord {
    let mut first_values = BTreeMap::new();
    first_values.insert("FIRE_RATE".to_owned(), 1.0);
    first_values.insert("CLUSTER_SIZE".to_owned(), 6.0);
    let mut second_values = BTreeMap::new();
    second_values.insert("SMOKE_SCREEN".to_owned(), 8.0);
    let mut first_texts = BTreeMap::new();
    first_texts.insert("SMOKE_SCREEN".to_owned(), "TIME".to_owned());
    OriginalWeaponRecord {
        record: "wep_13".to_owned(),
        keys: first_values.keys().cloned().collect(),
        first_values,
        second_values,
        first_texts,
    }
}

fn smoke_media() -> OriginalWeaponMedia {
    OriginalWeaponMedia {
        record: "wep_13".to_owned(),
        keys: BTreeSet::new(),
        fire: Some(OriginalEffectBlock {
            keys: BTreeSet::from(["SOUND".to_owned()]),
            sounds: vec!["snd_gotshot2".to_owned()],
            ..OriginalEffectBlock::default()
        }),
        flyout: Some(OriginalEffectBlock {
            keys: BTreeSet::from(["MODEL".to_owned()]),
            model: Some("smoker".to_owned()),
            ..OriginalEffectBlock::default()
        }),
        impact_rows: Vec::new(),
        destroy_animation: None,
        looped_sound_name: None,
    }
}

fn smoke_ammo() -> OriginalAmmoDeclaration {
    OriginalAmmoDeclaration {
        record: "wep_13".to_owned(),
        ammo_limit: None,
        cluster_size: Some(6.0),
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

fn ordnance_id() -> OrdnanceId {
    OrdnanceId::try_new(
        cs_types::content::ContentId::from_source(cs_types::content::ContentKind::Weapon, "wep_13")
            .expect("a weapon id"),
    )
    .expect("the weapon namespace")
}

fn projectile(serial: u64) -> cs_sim::weapons::ProjectileId {
    cs_sim::weapons::ProjectileId {
        session: SESSION,
        serial,
    }
}

/// The lowered `wep_13` fixture, through the production lowering.
fn lowered_smoke() -> DeclaredOrdnance {
    measured_ordnance_declaration(
        &smoke_row(),
        &smoke_record(),
        &smoke_media(),
        &smoke_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("wep_13's measured values are expressible once the range end exists")
}

// ------------------------------------------------------------------- tests ---

/// The measured area-denial record lowers to the measured range end: the
/// record's own `RANGE`, no proximity fuse (its `DETONATION_DISTANCE` needs
/// `LOCK_ON`, which it does not declare), no guidance and no area.
#[test]
fn accept_t452_silent_a_measured_area_denial_record_lowers_to_its_range_end() {
    let declared = lowered_smoke();
    let component = lower_ordnance(&declared).expect("the measured record lowers");
    let projectile = component
        .as_projectile()
        .expect("an area-denial engine is a projectile");

    assert_eq!(
        component.family(),
        cs_sim::weapons::OrdnanceFamily::AreaDenialEngine
    );
    assert_eq!(
        projectile.fuse(),
        FuseRule::ExpiresAtRange {
            range_m: ORIGINAL_RANGE_DEFAULT_M
        },
        "T452-R.02 §3: the record's measured end is the `RANGE` bound, not a timer"
    );
    assert_eq!(
        projectile.guidance(),
        cs_sim::weapons::GuidanceRule::Unguided,
        "an area-denial engine is unguided by definition"
    );
    // T452-R.04 §2: `RANGE 500` over the default `VELOCITY 120` is 4.166… s,
    // and the runtime's designed cap rounds that up to the session's tick. The
    // cap is not the end of the flight — the measured bound is — but it must
    // still bound the item.
    assert_eq!(projectile.lifetime_ticks(), 126);
    // No measured radius → no area (the smoke emitter's radius stays the named
    // claim `t452_r02.limit.smoke_radius`, T452-R.02 §1).
    assert_eq!(projectile.area_effect(), None);
    assert_eq!(projectile.stack().capacity_units(), 6);
    // The launch sound is the record's own.
    assert_eq!(
        projectile
            .media()
            .sound()
            .expect("the record declares a FIRE SOUND")
            .key(),
        "snd_gotshot2"
    );
}

/// A registered area-denial record flies to its measured `RANGE` through the
/// production session and is **consumed** there: no detonation, no area, no
/// damage, and the item is gone.
#[test]
fn accept_t452_silent_a_registered_area_denial_record_is_consumed_at_its_range() {
    let declared = lowered_smoke();
    let component = lower_ordnance(&declared).expect("the measured record lowers");
    let mut catalogue = OrdnanceRegistry::new();
    catalogue
        .register(component)
        .expect("the catalogue takes the measured component");

    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    let registered = session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured component registers in the session");
    assert_eq!(registered.len(), 1);

    let mut world = World::new();
    launcher(&mut world, Vec3::ZERO);
    let mut damage = damage_resolver();
    let hull_before = damage
        .remaining_integrity(&actor(2), &key("hull"))
        .expect("the target's hull is registered");

    let launch = step(
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
            ordnance: ordnance_id(),
            target: None,
            engagement: OrdnanceEngagement {
                damage_target: actor(2),
                node: key("hull"),
                status_recipient: StatusEffectTarget::whole_actor(actor(2)),
            },
        })],
        Tick(0),
    );
    assert_eq!(
        launch.launched,
        vec![projectile(0)],
        "the component launches"
    );

    // The blast recipient is supplied on every tick, so "no damage" is a
    // statement about the consumption and not about a missing observer: if the
    // item detonated, the target inside the recipient list would take the hit.
    let mut consumed: Option<(Tick, OrdnanceConsumption)> = None;
    let mut ticks = 0_u64;
    for at in 1..=200 {
        let tick = step_with_recipients(&mut world, &mut session, &mut damage, at);
        ticks += 1;
        assert!(
            tick.detonations.is_empty(),
            "tick {at}: a silent expiry detonates nothing: {:?}",
            tick.detonations
        );
        assert!(
            tick.guidance_blasts.is_empty(),
            "tick {at}: no guidance blast"
        );
        assert!(
            tick.areas_expired.is_empty() && tick.expired.is_empty(),
            "tick {at}: the measured end is neither an area nor the lifetime cap"
        );
        if let Some(entry) = tick.consumed.first().copied() {
            consumed = Some((Tick(at), entry));
            break;
        }
    }
    let (at, consumption) = consumed.expect("the item reaches its RANGE and is consumed");
    assert_eq!(consumption.projectile, projectile(0));
    assert_eq!(
        consumption.range_m, ORIGINAL_RANGE_DEFAULT_M,
        "the item is consumed at the record's own RANGE"
    );
    assert!(
        at.0 < 126,
        "the measured bound ended the flight before the designed cap fell (tick {} of 126), \
         so the cap is not what ended it",
        at.0
    );
    assert_eq!(ticks, at.0, "the loop breaks on the consumption tick");

    // Consumed, not triggered: the item is gone and its engagement with it.
    assert!(
        session.runtime().get(&projectile(0)).is_none(),
        "a consumed item leaves the live set"
    );
    assert_eq!(
        session.runtime().iter().count(),
        0,
        "nothing of the item is left live"
    );
    // The mirror pass reports the despawn, which is what a session observer sees.
    // (It is reported on the consumption tick, above, when there was a mirror.)
    // Nothing reached the target.
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key("hull")),
        Some(hull_before),
        "a silent expiry deals no damage"
    );
    assert!(!damage.is_destroyed(&actor(2)));
    // The network log names the end, with the bound, and never a trigger.
    assert!(
        session.events().iter().any(|event| matches!(
            event.kind,
            OrdnanceEventKind::Consumed { projectile: p, range_m }
                if p == projectile(0) && (range_m - ORIGINAL_RANGE_DEFAULT_M).abs() < f64::EPSILON
        )),
        "the consumed event carries the bound"
    );
    assert!(
        !session
            .events()
            .iter()
            .any(|event| matches!(event.kind, OrdnanceEventKind::Triggered { .. })),
        "no trigger was announced"
    );
    assert!(
        !session
            .events()
            .iter()
            .any(|event| matches!(event.kind, OrdnanceEventKind::Expired { .. })),
        "and no lifetime expiry either"
    );
}

/// The bound is a distance, not a clock: the same component released from a
/// moving launcher is consumed after the *same path length* and therefore in
/// fewer ticks, and never by the lifetime cap.
#[test]
fn accept_t452_silent_the_consumption_follows_the_path_not_the_clock() {
    let stationary_ticks = ticks_to_consumption(Vec3::ZERO);
    let moving_ticks = ticks_to_consumption(Vec3::new(0.0, 0.0, -100.0));
    assert!(
        moving_ticks < stationary_ticks,
        "a launcher flying along the launch line shortens the flight: {moving_ticks} vs \
         {stationary_ticks} ticks"
    );
    assert!(
        stationary_ticks < 126,
        "neither flight is ended by the lifetime cap: {stationary_ticks} ticks"
    );
}

/// The registry outcome for the ids the silent expiry alone held back: they
/// register, and no record is blocked on that claim any more.
#[test]
fn accept_t452_silent_the_silent_expiry_claim_is_no_longer_a_blocker() {
    let mut registry = OrdnanceRegistry::new();
    let report = register_measured_ordnance(
        &table(vec![smoke_row()]),
        &[smoke_record()],
        &[smoke_media()],
        &[smoke_ammo()],
        &provenance(),
        &mut registry,
        TICKS_PER_SECOND,
    );
    assert_eq!(report.declared, 1);
    assert_eq!(report.registered_records(), vec!["wep_13"]);
    assert!(
        report
            .blockers()
            .iter()
            .all(|blocker| blocker.claim != T452_SILENT_EXPIRY_BLOCKED),
        "the measured end of flight is expressible: {:?}",
        report.blockers()
    );
    // A launched sound the record does not declare is a measured silence with
    // its own runtime representation, so it blocks nothing here either.
    assert!(
        report
            .blockers()
            .iter()
            .all(|blocker| !blocker.claim.contains("silent_launch")),
        "{:?}",
        report.blockers()
    );
}

// -------------------------------------------------------------------- retail --

fn game_dir() -> std::path::PathBuf {
    let dir = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: accept_t452_silent_retail_* need the original \
             installation; run them with --include-ignored and CS_GAME_DIR set"
        )
    });
    std::path::PathBuf::from(dir)
}

/// The original installation's own area-denial record: it registers, flies to
/// its measured `RANGE` through the production session, and is consumed there
/// with no detonation, area or damage.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_silent_retail_the_measured_smoke_record_is_consumed_at_range() {
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

    // The registry is filled by the production path, not by this test.
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
        report.registered_records().contains(&"wep_13"),
        "the measured silent expiry now registers wep_13: {:?}",
        report.blocked_records()
    );
    // No id is blocked on the claim this stage resolves.
    for blocker in report.blockers() {
        assert_ne!(
            blocker.claim, T452_SILENT_EXPIRY_BLOCKED,
            "{} still names the resolved claim on {}",
            blocker.record, blocker.field
        );
    }
    // The ids that stay blocked are blocked for their own measured gaps.
    let claims_of = |record: &str| -> Vec<&str> {
        report
            .outcomes
            .iter()
            .find_map(|outcome| match outcome {
                OriginalRegistrationOutcome::Blocked {
                    record: name,
                    blockers,
                } if name == record => Some(blockers.iter().map(|b| b.claim.as_str()).collect()),
                OriginalRegistrationOutcome::Blocked { .. }
                | OriginalRegistrationOutcome::Registered { .. } => None,
            })
            .unwrap_or_else(|| panic!("{record} is blocked: {:?}", report.blocked_records()))
    };
    assert_eq!(claims_of("wep_12"), vec![T452_R02_TANGLER_EFFECT]);
    let flak = claims_of("wep_26");
    assert!(!flak.contains(&T452_SILENT_EXPIRY_BLOCKED), "{flak:?}");

    let row = table
        .rows
        .iter()
        .find(|row| row.record == "wep_13")
        .expect("the installation declares wep_13");
    let record = records
        .iter()
        .find(|record| record.record == "wep_13")
        .expect("wep_13 has a record");
    let declared = measured_ordnance_declaration(
        row,
        record,
        media
            .iter()
            .find(|media| media.record == "wep_13")
            .expect("wep_13 has media declarations"),
        ammo.iter()
            .find(|ammo| ammo.record == "wep_13")
            .expect("wep_13 has an ammunition declaration"),
        &provenance,
        TICKS_PER_SECOND,
    )
    .expect("wep_13's measured values are expressible");
    let component = catalogue
        .get(&ordnance_id())
        .expect("the production registration holds wep_13");
    let registered = component
        .as_projectile()
        .expect("an area-denial engine is a projectile");
    assert_eq!(
        registered.fuse(),
        FuseRule::ExpiresAtRange {
            range_m: ORIGINAL_RANGE_DEFAULT_M
        },
        "wep_13 declares no RANGE, so the parser's measured default is its bound"
    );

    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured component registers in the session");
    let mut world = World::new();
    launcher(&mut world, Vec3::ZERO);
    let mut damage = damage_resolver();
    let hull_before = damage
        .remaining_integrity(&actor(2), &key("hull"))
        .expect("the target's hull is registered");
    let launch = step(
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
            ordnance: ordnance_id(),
            target: None,
            engagement: OrdnanceEngagement {
                damage_target: actor(2),
                node: key("hull"),
                status_recipient: StatusEffectTarget::whole_actor(actor(2)),
            },
        })],
        Tick(0),
    );
    assert_eq!(launch.launched, vec![projectile(0)]);

    let mut consumed = None;
    for at in 1..=200 {
        let tick = step_with_recipients(&mut world, &mut session, &mut damage, at);
        assert!(tick.detonations.is_empty(), "tick {at}: no detonation");
        assert!(tick.expired.is_empty(), "tick {at}: not the lifetime cap");
        if let Some(entry) = tick.consumed.first().copied() {
            consumed = Some((Tick(at), entry));
            break;
        }
    }
    let (at, consumption) = consumed.expect("wep_13 reaches its RANGE and is consumed");
    assert_eq!(consumption.range_m, ORIGINAL_RANGE_DEFAULT_M);
    assert!(at.0 < 126, "the measured bound ended it at tick {}", at.0);
    assert!(session.runtime().get(&projectile(0)).is_none());
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key("hull")),
        Some(hull_before)
    );
}

/// The catalogue keeps the two claims T452-R.02 left on these records: the
/// smoke emitter's unmeasured radius and the tangler's flight effect. Nothing
/// in this stage resolves or removes either, and neither blocks registration
/// of a record that does not need it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_silent_retail_the_kept_claims_are_not_touched() {
    let root = game_dir();
    let table = cs_content::ordnance::read_original_component_table(&root)
        .expect("the component table reads");
    let mut registry = OrdnanceRegistry::new();
    let report = read_and_register_measured_ordnance(&root, &mut registry, TICKS_PER_SECOND)
        .expect("the installation's records read");

    let smoke = table
        .rows
        .iter()
        .find(|row| row.record == "wep_13")
        .expect("the installation declares wep_13");
    let behaviour = cs_content::ordnance::measure_component_behavior(smoke);
    assert!(
        behaviour.names_claim(T452_R02_SMOKE_STUN_EFFECT),
        "what the smoke emitter does to the actors inside it stays a named claim"
    );
    assert!(
        report
            .claims_kept
            .iter()
            .any(|(record, claims)| record == "wep_13"
                && claims.iter().any(|c| c == T452_R02_SMOKE_STUN_EFFECT)),
        "the registration report keeps it too: {:?}",
        report.claims_kept
    );
    // The tangler claim stays a blocker for wep_12: its area outlives the item
    // (follow-up T452-REG-TANGLER-EFFECT), which this stage does not address.
    let tangler = table
        .rows
        .iter()
        .find(|row| row.record == "wep_12")
        .expect("the installation declares wep_12");
    assert!(
        cs_content::ordnance::measure_component_behavior(tangler)
            .names_claim(T452_R02_TANGLER_EFFECT)
    );
}

// ------------------------------------------------------------------ harness --

/// Ticks from release to consumption from a launcher moving at `velocity`.
fn ticks_to_consumption(velocity: Vec3) -> u64 {
    let declared = lowered_smoke();
    let component = lower_ordnance(&declared).expect("the measured record lowers");
    let mut catalogue = OrdnanceRegistry::new();
    catalogue
        .register(component)
        .expect("the catalogue takes the measured component");
    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured component registers in the session");
    let mut world = World::new();
    launcher(&mut world, velocity);
    let mut damage = damage_resolver();
    step(
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
            ordnance: ordnance_id(),
            target: None,
            engagement: OrdnanceEngagement {
                damage_target: actor(2),
                node: key("hull"),
                status_recipient: StatusEffectTarget::whole_actor(actor(2)),
            },
        })],
        Tick(0),
    );
    for at in 1..=200 {
        let tick = step_with_recipients(&mut world, &mut session, &mut damage, at);
        assert!(
            tick.expired.is_empty(),
            "tick {at}: the lifetime cap must not end the flight"
        );
        if !tick.consumed.is_empty() {
            return at;
        }
    }
    panic!("the item was never consumed");
}

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
/// for the measured `wep_13` mount id, moving at `velocity`.
fn launcher(world: &mut World, velocity: Vec3) -> Entity {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![
                    cs_types::content::ContentId::from_source(
                        cs_types::content::ContentKind::Weapon,
                        "wep_13",
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
            LinearVelocity(velocity),
        ))
        .id();
    world.spawn((
        MountPoseBinding {
            mount: key("weapon_mount_13"),
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

/// One tick with the target inside a supplied blast recipient list, so a
/// detonation anywhere on the item's flight would reach it and be visible.
fn step_with_recipients(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    at: u64,
) -> cs_app::ordnance::OrdnanceSessionTick {
    let recipients = [BlastRecipient::new(
        actor(2),
        key("hull"),
        WorldPosition::try_new([0.0, 0.0, -10.0]).expect("a finite position"),
    )];
    step_ordnance_session(
        world,
        session,
        damage,
        &[],
        &OrdnanceStep {
            at: Tick(at),
            dt_s: DT_S,
            wind_velocity_m_s: [0.0; 3],
            generation: GENERATION,
            guidance: None,
            targets: &[],
            impacts: &[],
            blast_recipients: Some(&recipients),
            victim_nodes: None,
        },
    )
}
