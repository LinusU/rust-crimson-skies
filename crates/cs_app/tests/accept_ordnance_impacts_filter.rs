//! Task #2207: `step_ordnance_session` hands each live item only its own swept
//! contacts. Task test prefix: `accept_ordnance_impacts_filter`.
//!
//! Before the fix every live item's fuse saw the whole `impacts` slice and
//! took its first entry, so one rocket's hit burst every rocket in the session
//! with the wrong victim, time of impact and tangler-field centre. The fixture
//! carries the shape of `wep_12` (as `accept_t452_tangler_flight.rs` does) so
//! the routing is pinned in CI without original data.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId, OrdnanceLauncherBinding, OrdnanceOrder,
    OrdnanceSession, OrdnanceSessionTick, OrdnanceStep, register_measured_ordnance,
    step_ordnance_session,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DeclaredOrdnanceFamily, OriginalAmmoDeclaration, OriginalAreaDeclaration, OriginalClusterShape,
    OriginalComponentLaunch, OriginalComponentTable, OriginalDamageDeclaration,
    OriginalEffectBlock, OriginalLifetime, OriginalMountDeclaration, OriginalMountLayout,
    OriginalSlotArray, OriginalStatusEffectDeclaration, OriginalWeaponMedia, OriginalWeaponRecord,
    TanglerFieldDeclaration,
};
use cs_sim::damage::{ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::{OrdnanceId, OrdnanceRegistry, StatusEffectTarget, SweptHit};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId as EvidenceClaimId, ClaimStatus, ContentHash};
use cs_types::net::SessionId;

const SESSION: u64 = 96;
const PRODUCER: u32 = 97;
const DAMAGE_PRODUCER: u32 = 98;
const GENERATION: SceneGeneration = SceneGeneration(1);
/// The ordnance session's tick length: the measured `TIME` is whole 30 Hz
/// ticks, so the field's lifetime is exactly `TIME * 30`.
const DT_S: f64 = 1.0 / 30.0;
const TICKS_PER_SECOND: u32 = 30;
/// The mount node's pose: the position the fire path reads. The field is
/// centred on the projectile's position when it bursts, not on this pose, so
/// the two must stay apart.
const MOUNT_ORIGIN: [f64; 3] = [4.0, 5.0, 10.0];

/// The tick a swept contact bursts the projectile in these tests.
const BURST_TICK: u64 = 2;

/// `wep_12`'s measured `TIME`, seconds (T452-R.02 §1).
const TANGLER_TIME_S: f64 = 2.0;

fn provenance() -> Provenance {
    Provenance::new(
        EvidenceClaimId::new(cs_app::ordnance::T452_ORIGINAL_WEAPONS_CLAIM)
            .expect("a valid claim id"),
        ClaimStatus::ObservedTool,
        Some(
            SourceSpan::new(
                ContentHash::from_bytes([11; 32]),
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

fn slots(count: u32) -> OriginalSlotArray {
    OriginalSlotArray {
        member: "fixture.script".to_owned(),
        array: "RKA".to_owned(),
        slots: count,
        loop_bound: count,
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

/// The measured shape of `wep_12` (T452.01, T452-R.02 §1): an unguided
/// area-denial record that declares `TANGLER` with `TIME 2` and no `RADIUS`,
/// no damage, and `CLUSTER_SIZE` as its per-slot stack.
fn tangler_row() -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: "wep_12".to_owned(),
        family: DeclaredOrdnanceFamily::AreaDenialEngine,
        mount: OriginalMountDeclaration::RocketSlot { slots: 8 },
        velocity: Some(300.0),
        fire_rate: Some(1.0),
        proximity_trigger_m: None,
        lifetime: OriginalLifetime::Undeclared,
        area: OriginalAreaDeclaration {
            radius_m: None,
            lifetime_s: None,
        },
        damage: OriginalDamageDeclaration {
            armor: Some(0.0),
            health: Some(0.0),
            single: None,
        },
        statuses: vec![OriginalStatusEffectDeclaration {
            key: "TANGLER",
            duration_s: Some(TANGLER_TIME_S),
            duration_unit: Some("TIME".to_owned()),
        }],
        cluster: OriginalClusterShape::Volley { munitions: 2 },
        unknowns: Vec::new(),
    }
}

fn tangler_record() -> OriginalWeaponRecord {
    let mut first_values = BTreeMap::new();
    first_values.insert("VELOCITY".to_owned(), 300.0);
    first_values.insert("RANGE".to_owned(), 900.0);
    first_values.insert("FIRE_RATE".to_owned(), 1.0);
    first_values.insert("CLUSTER_SIZE".to_owned(), 2.0);
    let mut second_values = BTreeMap::new();
    second_values.insert("TANGLER".to_owned(), TANGLER_TIME_S);
    let mut first_texts = BTreeMap::new();
    first_texts.insert("TANGLER".to_owned(), "TIME".to_owned());
    OriginalWeaponRecord {
        record: "wep_12".to_owned(),
        keys: first_values.keys().cloned().collect(),
        first_values,
        second_values,
        first_texts,
    }
}

fn tangler_media() -> OriginalWeaponMedia {
    OriginalWeaponMedia {
        record: "wep_12".to_owned(),
        keys: BTreeSet::new(),
        fire: Some(OriginalEffectBlock {
            keys: BTreeSet::from(["SOUND".to_owned()]),
            sounds: vec!["snd_tangler".to_owned()],
            ..OriginalEffectBlock::default()
        }),
        flyout: Some(OriginalEffectBlock {
            keys: BTreeSet::from(["MODEL".to_owned()]),
            model: Some("tangle_rocket".to_owned()),
            ..OriginalEffectBlock::default()
        }),
        impact_rows: Vec::new(),
        destroy_animation: None,
        looped_sound_name: None,
    }
}

fn tangler_ammo() -> OriginalAmmoDeclaration {
    OriginalAmmoDeclaration {
        record: "wep_12".to_owned(),
        ammo_limit: None,
        cluster_size: Some(2.0),
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
    OrdnanceId::try_new(ContentId::from_source(ContentKind::Weapon, "wep_12").expect("a weapon id"))
        .expect("the weapon namespace")
}

fn projectile(serial: u64) -> cs_sim::weapons::ProjectileId {
    cs_sim::weapons::ProjectileId {
        session: SESSION,
        serial,
    }
}

/// The measured field of the fixture record, through the production lowering.
fn tangler_field() -> TanglerFieldDeclaration {
    cs_app::ordnance::measured_tangler_field(&tangler_row())
        .expect("a TANGLER record declares a field")
}

/// A session whose catalogue holds the registered fixture component, with the
/// shooter's component registered and its measured field declared.
fn session_with_field() -> OrdnanceSession {
    let declared = cs_app::ordnance::measured_ordnance_declaration(
        &tangler_row(),
        &tangler_record(),
        &tangler_media(),
        &tangler_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("wep_12's measured values are expressible");
    let mut catalogue = OrdnanceRegistry::new();
    let report = register_measured_ordnance(
        &table(vec![tangler_row()]),
        &[tangler_record()],
        &[tangler_media()],
        &[tangler_ammo()],
        &provenance(),
        &mut catalogue,
        TICKS_PER_SECOND,
    );
    assert_eq!(
        report.registered_records(),
        vec!["wep_12"],
        "the measured record registers: {:?}",
        report.blocked_records()
    );
    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured component registers in the session");
    session
        .register_tangler_field(&ordnance_id(), tangler_field())
        .expect("the measured field is the shape this runtime opens");
    session
}

fn damage_resolver() -> DamageResolver {
    let mut damage = DamageResolver::new(session_id(), DAMAGE_PRODUCER);
    for serial in [2, 3] {
        damage
            .register_actor(
                actor(serial),
                cs_sim::damage::synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("the fixture graph registers");
    }
    damage
}

/// The shooter's live launcher: a root carrying the binding and one mount node
/// for `wep_12`'s measured mount id, at [`MOUNT_ORIGIN`].
fn launcher(world: &mut World) -> Entity {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![
                    ContentId::from_source(ContentKind::Weapon, "wep_12").expect("a weapon id"),
                ],
                loadout: ContentId::from_source(ContentKind::Loadout, "fixture.loadout")
                    .expect("a loadout id"),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::ZERO),
        ))
        .id();
    world.spawn((
        MountPoseBinding {
            mount: key("weapon_mount_12"),
            generation: GENERATION,
        },
        NodeVisualTransform(GlobalTransform::from_xyz(
            MOUNT_ORIGIN[0] as f32,
            MOUNT_ORIGIN[1] as f32,
            MOUNT_ORIGIN[2] as f32,
        )),
        ChildOf(root),
    ));
    root
}

/// Launch number `sequence` at a given victim.
fn launch_order(sequence: u32, victim: ActorId) -> OrdnanceOrder {
    OrdnanceOrder::Launch(OrdnanceLaunch {
        id: OrdnanceLaunchId {
            session: SESSION,
            tick: Tick(0),
            producer: PRODUCER,
            sequence,
        },
        shooter: actor(1),
        ordnance: ordnance_id(),
        target: None,
        engagement: OrdnanceEngagement {
            damage_target: victim,
            node: key("hull"),
            status_recipient: StatusEffectTarget::whole_actor(victim),
        },
    })
}

fn step(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    orders: &[OrdnanceOrder],
    impacts: &[SweptHit],
    at: u64,
) -> OrdnanceSessionTick {
    step_ordnance_session(
        world,
        session,
        damage,
        orders,
        &OrdnanceStep {
            at: Tick(at),
            dt_s: DT_S,
            wind_velocity_m_s: [0.0; 3],
            generation: GENERATION,
            guidance: None,
            targets: &[],
            impacts,
            blast_recipients: None,
            victim_nodes: None,
        },
    )
}

fn hit(serial: u64, victim: ActorId, time_of_impact: f64) -> SweptHit {
    SweptHit {
        projectile: projectile(serial),
        target: victim,
        time_of_impact,
    }
}

/// Two rockets launched on tick 0 (victims 2 and 3), one quiet tick, ready for
/// the contact tick.
fn two_live(world: &mut World, session: &mut OrdnanceSession, damage: &mut DamageResolver) {
    launcher(world);
    let first = step(
        world,
        session,
        damage,
        &[launch_order(0, actor(2)), launch_order(1, actor(3))],
        &[],
        0,
    );
    assert_eq!(first.launched, vec![projectile(0), projectile(1)]);
    let quiet = step(world, session, damage, &[], &[], 1);
    assert!(quiet.detonations.is_empty());
}

#[test]
fn accept_ordnance_impacts_filter_a_foreign_contact_leaves_the_other_item_live() {
    let mut world = World::new();
    let mut session = session_with_field();
    let mut damage = damage_resolver();
    two_live(&mut world, &mut session, &mut damage);

    let tick = step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &[hit(0, actor(2), 0.5)],
        BURST_TICK,
    );
    assert_eq!(tick.detonations.len(), 1, "only the struck item detonates");
    assert_eq!(tick.detonations[0].projectile, projectile(0));
    assert_eq!(tick.detonations[0].engagement.damage_target, actor(2));
    assert_eq!(
        session.tangler_fields().len(),
        1,
        "the unhit item opens no field"
    );

    // The other item stays live: it still detonates on its own contact later.
    let later = step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &[hit(1, actor(3), 0.25)],
        BURST_TICK + 1,
    );
    assert_eq!(later.detonations.len(), 1);
    assert_eq!(later.detonations[0].projectile, projectile(1));
    assert_eq!(later.detonations[0].engagement.damage_target, actor(3));
}

#[test]
fn accept_ordnance_impacts_filter_both_hit_each_detonates_with_its_own_contact() {
    let mut world = World::new();
    let mut session = session_with_field();
    let mut damage = damage_resolver();
    two_live(&mut world, &mut session, &mut damage);

    // Item 1's contact is earlier in the tick than item 0's, so the shared
    // slice is ordered [1, 0]; each item must still see only its own.
    let tick = step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &[hit(1, actor(3), 0.25), hit(0, actor(2), 0.75)],
        BURST_TICK,
    );
    assert_eq!(tick.detonations.len(), 2, "both items detonate");
    for detonation in &tick.detonations {
        let (victim, expected_time) = if detonation.projectile == projectile(0) {
            (actor(2), 0.75)
        } else {
            assert_eq!(detonation.projectile, projectile(1));
            (actor(3), 0.25)
        };
        assert_eq!(detonation.engagement.damage_target, victim);
        match detonation.trigger {
            cs_sim::weapons::FuseTrigger::Impact { target, time } => {
                assert_eq!(target, victim);
                assert_eq!(time, expected_time);
            }
            ref other => panic!("expected an impact trigger, got {other:?}"),
        }
    }
    // One field per detonating item. Where each is centred (the item's own
    // point on its segment) is #2205's contract, not this routing test's.
    assert_eq!(
        session.tangler_fields().len(),
        2,
        "each detonating item opens its own field"
    );
}
