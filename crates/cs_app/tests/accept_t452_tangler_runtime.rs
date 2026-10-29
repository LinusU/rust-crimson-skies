//! Task T452-REG-TANGLER-RUNTIME (#2186): a registered `wep_12` opens its
//! measured tangler field at fire time through the production ordnance
//! session. Task test prefix: `accept_t452_tangler_`.
//!
//! `docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md` §1
//! (`TANGLER`) measured that a `TANGLER` record builds an area field at fire
//! time (`0x4b94e0`), that the field lives its record `TIME` (`wep_12`: 2 s)
//! and that its tick (`0x4b9590`) re-applies the weapon's per-hit routine to
//! every live actor inside `RADIUS²` (image default 10 m), the owner included.
//! `docs/findings/2026-10-28-t452-tangler-effect.md` measured what that hit
//! does (the `TANGLER` branch at `0x4b9ded` zeroes the damage channels and
//! applies a timed tangle of `max(2 s, 10 * (1 - d2 / RADIUS))`), and that the
//! force routine `0x48fc40` zeroes its force scalar `[ebp-8]` while the tangle
//! is live, that a re-hit only raises the timer, and that a tangle ends any
//! boost.
//!
//! These tests drive the registration and the fire path: a synthetic fixture
//! carries the *shape* of `wep_12` so the path is pinned in CI, where no
//! original data exists; `accept_t452_tangler_runtime_retail_*` runs the same
//! path over the owner's installation. Every test calls production code. No
//! test awards `verified_original`.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    ExpiredTanglerField, OrdnanceEngagement, OrdnanceEventKind, OrdnanceLaunch, OrdnanceLaunchId,
    OrdnanceLauncherBinding, OrdnanceOrder, OrdnanceSession, OrdnanceSessionTick, OrdnanceStep,
    TanglerFieldId, TanglerFieldRegistrationError, lower_ordnance, measured_ordnance_declaration,
    measured_tangler_field, register_measured_ordnance, step_ordnance_session,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DeclaredOrdnance, DeclaredOrdnanceFamily, OriginalAmmoDeclaration, OriginalAreaDeclaration,
    OriginalClusterShape, OriginalComponentLaunch, OriginalComponentTable,
    OriginalDamageDeclaration, OriginalEffectBlock, OriginalLifetime, OriginalMountDeclaration,
    OriginalMountLayout, OriginalSlotArray, OriginalStatusEffectDeclaration, OriginalWeaponMedia,
    OriginalWeaponRecord, T452_REG_TANGLER_FORCE_SCALAR, TANGLE_FLOOR_S, TanglerFieldDeclaration,
};
use cs_sim::damage::{ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::{OrdnanceId, OrdnanceRegistry, StatusEffectTarget, TargetPath};
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::{ClaimId as EvidenceClaimId, ClaimStatus, ContentHash};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

// ------------------------------------------------------------------ fixtures --

const SESSION: u64 = 96;
const PRODUCER: u32 = 97;
const DAMAGE_PRODUCER: u32 = 98;
const GENERATION: SceneGeneration = SceneGeneration(1);
const DT_S: f64 = 1.0 / 30.0;
const TICKS_PER_SECOND: u32 = 30;

/// The mount node's pose: the position the fire path reads, and therefore the
/// field centre. Deliberately not the world origin, so a field centred on the
/// origin cannot pass.
const MOUNT_ORIGIN: [f64; 3] = [4.0, 5.0, 10.0];

/// `wep_12`'s measured `TIME`, seconds (T452-R.02 §1).
const TANGLER_TIME_S: f64 = 2.0;

/// The measured field lifetime in whole 30 Hz ticks: `ceil(2 s * 30)`.
const TANGLER_LIFETIME_TICKS: u64 = 60;

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

fn field_id(serial: u64) -> TanglerFieldId {
    TanglerFieldId {
        session: SESSION,
        serial,
    }
}

fn path(target: ActorId, at: [f64; 3]) -> TargetPath {
    TargetPath::try_new(target, at, at).expect("the fixture path is finite")
}

fn position(at: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(at).expect("the fixture positions are finite")
}

/// `wep_12`'s measured declaration through the production lowering.
fn lowered_tangler() -> DeclaredOrdnance {
    measured_ordnance_declaration(
        &tangler_row(),
        &tangler_record(),
        &tangler_media(),
        &tangler_ammo(),
        &provenance(),
        TICKS_PER_SECOND,
    )
    .expect("wep_12's measured values are expressible now its field has a runtime")
}

/// The measured field of the fixture record, through the production lowering.
fn tangler_field() -> TanglerFieldDeclaration {
    measured_tangler_field(&tangler_row()).expect("a TANGLER record declares a field")
}

/// A session whose catalogue holds the registered fixture component, with the
/// shooter's component registered and (optionally) its measured field declared.
fn session_with_field(declare_field: bool) -> OrdnanceSession {
    let declared = lowered_tangler();
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
    if declare_field {
        session
            .register_tangler_field(&ordnance_id(), tangler_field())
            .expect("the measured field is the shape this runtime opens");
    }
    session
}

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

fn launch_order() -> OrdnanceOrder {
    OrdnanceOrder::Launch(OrdnanceLaunch {
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
    })
}

fn step(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    orders: &[OrdnanceOrder],
    targets: &[TargetPath],
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
            targets,
            impacts: &[],
            blast_recipients: None,
            victim_nodes: None,
        },
    )
}

// ------------------------------------------------------------------- tests ---

/// `wep_12` registers, and its declared area is the measured field rather than
/// the runtime's one-shot bounded blast: no `AreaEffect`, and the field's own
/// measured radius and `TIME`.
#[test]
fn accept_t452_tangler_runtime_a_registered_wep_12_carries_its_measured_field() {
    let component = lower_ordnance(&lowered_tangler()).expect("the measured record lowers");
    let projectile = component
        .as_projectile()
        .expect("an area-denial engine is a projectile");
    assert_eq!(
        projectile.area_effect(),
        None,
        "a tangler field is not the runtime's one-shot blast area"
    );

    let field = tangler_field();
    assert_eq!(field.record, "wep_12");
    assert_eq!(field.radius_m, 10.0, "the image's measured default RADIUS");
    assert_eq!(field.lifetime_s, TANGLER_TIME_S);
    assert!(field.created_at_fire_time);
    assert!(field.reapplied_every_tick);
    assert!(
        field.includes_owner,
        "the measured tick excludes nobody, the owner included"
    );
    assert_eq!(
        field.status.claim, T452_REG_TANGLER_FORCE_SCALAR,
        "the scalar's physical identity stays named on the field"
    );
    assert_eq!(field.status.floor_s, TANGLE_FLOOR_S);
}

/// The registered record opens its field on the tick it fires, centred on the
/// pose the fire path read — not the launcher's actor record and not the world
/// origin — and the field's owner is the shooter.
#[test]
fn accept_t452_tangler_runtime_a_launch_opens_the_measured_field_at_fire_time() {
    let mut session = session_with_field(true);
    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();
    let victim = path(actor(2), MOUNT_ORIGIN);

    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order()],
        std::slice::from_ref(&victim),
        0,
    );
    assert_eq!(first.launched, vec![projectile(0)], "the record launches");
    assert_eq!(
        first.tangler_opened,
        vec![field_id(0)],
        "the launch opens the component's measured field"
    );

    let open = session.tangler_fields();
    assert_eq!(open.len(), 1);
    let field = &open[0];
    assert_eq!(field.record, "wep_12");
    assert_eq!(field.owner, actor(1), "the shooter owns the field");
    assert_eq!(
        field.centre,
        position(MOUNT_ORIGIN),
        "the centre is the fire path's pose"
    );
    assert_ne!(
        field.centre,
        position([0.0, 0.0, 0.0]),
        "the centre is never the world origin"
    );
    // The effect the fire path reported is the same pose: one fire path, one
    // position, not two readings.
    assert_eq!(
        first.effects[0].origin, field.centre,
        "the field opens where the launch effect says the weapon fired"
    );
    assert_eq!(
        field.lifetime_ticks, TANGLER_LIFETIME_TICKS,
        "TIME 2 s is 60 whole 30 Hz ticks"
    );
    assert_eq!(field.opened_at, Tick(0));
    assert_eq!(field.expires_at, Tick(TANGLER_LIFETIME_TICKS));
    assert_eq!(field.radius_m, 10.0);
}

/// A component with no measured field registered opens none, so the field is a
/// property of the registration and not of every launch.
#[test]
fn accept_t452_tangler_runtime_a_launch_opens_nothing_without_a_registered_field() {
    let mut session = session_with_field(false);
    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();
    let victim = path(actor(2), MOUNT_ORIGIN);

    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order()],
        std::slice::from_ref(&victim),
        0,
    );
    assert_eq!(first.launched, vec![projectile(0)]);
    assert!(first.tangler_opened.is_empty());
    assert!(session.tangler_fields().is_empty());
    assert!(!session.is_tangled(&actor(2)));
    assert_eq!(session.tangler_field_ids(), Vec::<OrdnanceId>::new());
}

/// Only the measured shape is expressible: a declaration the runtime cannot
/// open at fire time or re-apply per tick is refused by name.
#[test]
fn accept_t452_tangler_runtime_a_field_that_is_not_created_at_fire_time_is_refused() {
    let mut session = session_with_field(false);
    let mut not_at_fire = tangler_field();
    not_at_fire.created_at_fire_time = false;
    let error = session
        .register_tangler_field(&ordnance_id(), not_at_fire)
        .expect_err("the runtime cannot open a field at any other point");
    assert_eq!(
        error,
        TanglerFieldRegistrationError::NotCreatedAtFireTime {
            record: "wep_12".to_owned()
        }
    );
    assert!(error.to_string().contains("wep_12"));
    assert!(session.tangler_field_ids().is_empty());

    let mut not_per_tick = tangler_field();
    not_per_tick.reapplied_every_tick = false;
    assert_eq!(
        session
            .register_tangler_field(&ordnance_id(), not_per_tick)
            .expect_err("a field applied once is not the measured field"),
        TanglerFieldRegistrationError::NotReappliedEveryTick {
            record: "wep_12".to_owned()
        }
    );
}

/// The field lives exactly its measured `TIME` in whole ticks and re-applies the
/// tangle on every one of them: 60 applications from the fire tick, then the
/// field is gone and reported once.
#[test]
fn accept_t452_tangler_runtime_the_field_lasts_its_time_and_reapplies_each_tick() {
    let mut session = session_with_field(true);
    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();
    let victim = path(actor(2), MOUNT_ORIGIN);

    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order()],
        std::slice::from_ref(&victim),
        0,
    );
    assert_eq!(first.tangler.len(), 1, "the field applies on its fire tick");
    assert_eq!(first.tangler[0].applied.len(), 1);
    assert_eq!(
        first.tangler[0].applied[0].duration_ticks, 300,
        "d2 = 0 is the measured formula's 10 s maximum"
    );

    let mut applications = 1;
    for at in 1..=TANGLER_LIFETIME_TICKS {
        let tick = step(
            &mut world,
            &mut session,
            &mut damage,
            &[],
            std::slice::from_ref(&victim),
            at,
        );
        if at < TANGLER_LIFETIME_TICKS {
            assert_eq!(
                tick.tangler.len(),
                1,
                "tick {at}: the field is live before its boundary"
            );
            assert_eq!(tick.tangler[0].applied.len(), 1, "tick {at}: re-applied");
            assert!(
                tick.tangler_expired.is_empty(),
                "tick {at}: not yet at the boundary"
            );
            applications += 1;
        } else {
            assert!(
                tick.tangler.is_empty(),
                "tick {at}: the field reached its boundary and is gone"
            );
            assert_eq!(
                tick.tangler_expired,
                vec![ExpiredTanglerField {
                    instance: field_id(0),
                    record: "wep_12".to_owned(),
                    expired_at: Tick(TANGLER_LIFETIME_TICKS),
                }],
                "the field is reported exactly once, on its boundary tick"
            );
        }
    }
    assert_eq!(
        applications, TANGLER_LIFETIME_TICKS,
        "the field applies exactly its TIME in ticks, no more and no fewer"
    );
    assert!(session.tangler_fields().is_empty());
}

/// The measured tick walks the whole actor list with the owner included: the
/// shooter's own actor, when the fire path lists it, is tangled by the field it
/// fired exactly like any other victim.
#[test]
fn accept_t452_tangler_runtime_the_shooter_inside_the_field_is_tangled() {
    let mut session = session_with_field(true);
    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();
    let owner = path(actor(1), MOUNT_ORIGIN);
    let victim = path(actor(2), MOUNT_ORIGIN);

    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order()],
        &[owner, victim],
        0,
    );
    let applied: Vec<ActorId> = first.tangler[0]
        .applied
        .iter()
        .map(|application| application.target)
        .collect();
    assert_eq!(
        applied,
        vec![actor(1), actor(2)],
        "the field's own shooter is a victim like any other: 0x4b9590 excludes nobody"
    );
    assert!(
        session.is_tangled(&actor(1)),
        "the shooter is tangled by the field it fired"
    );
    assert!(
        !session.boost_allowed(&actor(1)),
        "and its own boost is refused while the tangle is live"
    );
}

/// An actor outside the measured 10 m radius is untouched; one just inside is
/// tangled. The boundary is the measured strict `d2 < RADIUS²`.
#[test]
fn accept_t452_tangler_runtime_an_actor_outside_the_radius_is_untouched() {
    let mut session = session_with_field(true);
    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();
    let inside = actor(2);
    let at_the_edge = actor(3);
    let outside = actor(4);
    let paths = [
        path(inside, MOUNT_ORIGIN),
        path(
            at_the_edge,
            [MOUNT_ORIGIN[0] + 10.0, MOUNT_ORIGIN[1], MOUNT_ORIGIN[2]],
        ),
        path(
            outside,
            [MOUNT_ORIGIN[0] + 10.5, MOUNT_ORIGIN[1], MOUNT_ORIGIN[2]],
        ),
    ];

    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order()],
        &paths,
        0,
    );
    let applied: Vec<ActorId> = first.tangler[0]
        .applied
        .iter()
        .map(|application| application.target)
        .collect();
    assert_eq!(
        applied,
        vec![inside],
        "only the actor strictly inside RADIUS² is tangled"
    );
    assert!(session.is_tangled(&inside));
    assert!(
        !session.is_tangled(&at_the_edge),
        "a hit at exactly RADIUS is outside the measured comparison"
    );
    assert!(!session.is_tangled(&outside));
}

/// A re-hit raises the tangle timer and never shortens it, and the tangle
/// outlives the field: it is the victim's own timer, counted down on its own
/// boundary, and it ends an active boost while it is live.
#[test]
fn accept_t452_tangler_runtime_a_rehit_never_shortens_the_timer_and_it_expires_after_its_duration()
{
    let mut session = session_with_field(true);
    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();
    let victim = actor(2);

    // Fire tick: at the centre, so the tangle is the formula's 10 s maximum.
    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order()],
        &[path(victim, MOUNT_ORIGIN)],
        0,
    );
    assert_eq!(first.tangler[0].applied[0].duration_ticks, 300);
    let tangle = session.tangle_status(&victim).expect("tangled");
    assert_eq!(tangle.remaining_ticks, 300);
    assert_eq!(tangle.expires_at, Tick(300));
    assert!(
        !session.boost_allowed(&victim),
        "the measured tangle refuses a boost while it is live"
    );

    // A later tick inside the field at 2 m: the formula divides the *squared*
    // distance by the unsquared RADIUS, so 10 * (1 - 4/10) = 6 s = 180 ticks —
    // shorter than the live timer, so the timer must not move.
    let nearer = [MOUNT_ORIGIN[0] + 2.0, MOUNT_ORIGIN[1], MOUNT_ORIGIN[2]];
    let second = step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        &[path(victim, nearer)],
        1,
    );
    assert_eq!(
        second.tangler[0].applied[0].duration_ticks, 180,
        "the nearer hit's own duration is shorter"
    );
    assert_eq!(
        second.tangler[0].applied[0].remaining_ticks, 299,
        "a re-hit raises to the maximum and never shortens the timer: the same boundary from \
         tick 1"
    );
    assert_eq!(
        session
            .tangle_status(&victim)
            .expect("still tangled")
            .expires_at,
        Tick(300)
    );

    // The field dies at tick 60; the victim leaves the radius, so nothing
    // re-raises the timer, and it keeps counting down on its own boundary at
    // tick 300, after the field that applied it is gone.
    let away = [MOUNT_ORIGIN[0] + 10.5, MOUNT_ORIGIN[1], MOUNT_ORIGIN[2]];
    for at in 2..=TANGLER_LIFETIME_TICKS {
        let tick = step(
            &mut world,
            &mut session,
            &mut damage,
            &[],
            &[path(victim, away)],
            at,
        );
        assert!(
            tick.tangler.iter().all(|pass| pass.applied.is_empty()),
            "tick {at}: an actor outside RADIUS is not re-tangled"
        );
    }
    assert!(session.tangler_fields().is_empty(), "the field is gone");
    let after_field = session
        .tangle_status(&victim)
        .expect("the tangle outlives the field");
    assert_eq!(after_field.remaining_ticks, 240);
    assert_eq!(after_field.expires_at, Tick(300));

    // Nobody is re-applying it any more: it clears on its own boundary tick.
    let mut cleared = None;
    for at in (TANGLER_LIFETIME_TICKS + 1)..=300 {
        let tick = step(&mut world, &mut session, &mut damage, &[], &[], at);
        if let Some(expired) = tick.tangles_expired.first().copied() {
            cleared = Some((at, expired));
            break;
        }
    }
    let (at, expired) = cleared.expect("the tangle reaches its own boundary");
    assert_eq!(at, 300, "the measured 10 s duration, in whole ticks");
    assert_eq!(expired.target, victim);
    assert_eq!(expired.duration_ticks, 300);
    assert!(!session.is_tangled(&victim));
    assert!(
        session.boost_allowed(&victim),
        "the boost is available again once the tangle cleared"
    );
    // The tangle began once and cleared once: a re-hit is the same status.
    let started = session
        .events()
        .iter()
        .filter(|event| matches!(event.kind, OrdnanceEventKind::TangleStarted { .. }))
        .count();
    assert_eq!(started, 1, "one tangle episode, not one per re-application");
}

/// Over the owner's installation the same production path registers the real
/// `wep_12` and opens its measured field: 10 m, `TIME 2`, centred on the fire
/// path's pose.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_tangler_runtime_retail_the_measured_wep_12_opens_its_field() {
    let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: accept_t452_tangler_runtime_retail_* need the original \
             installation; run them with --ignored and CS_GAME_DIR set"
        )
    }));
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
        report.registered_records().contains(&"wep_12"),
        "the measured tangler record registers now its field has a runtime: {:?}",
        report.blocked_records()
    );
    assert!(
        report.blocked_records().is_empty(),
        "every declared id registers: {:?}",
        report.blocked_records()
    );
    let field = report
        .tangler_fields
        .iter()
        .find(|field| field.record == "wep_12")
        .expect("the registered record reports its measured field");
    assert_eq!(field.radius_m, 10.0, "the image's default RADIUS");
    assert_eq!(field.lifetime_s, 2.0, "weP_12's own TIME");
    assert!(field.created_at_fire_time && field.reapplied_every_tick);
    assert!(field.includes_owner);

    let row = table
        .rows
        .iter()
        .find(|row| row.record == "wep_12")
        .expect("the installation declares wep_12");
    let record = records
        .iter()
        .find(|record| record.record == "wep_12")
        .expect("wep_12 has a record");
    let declared = measured_ordnance_declaration(
        row,
        record,
        media
            .iter()
            .find(|media| media.record == "wep_12")
            .expect("wep_12 has media declarations"),
        ammo.iter()
            .find(|ammo| ammo.record == "wep_12")
            .expect("wep_12 has an ammunition declaration"),
        &provenance,
        TICKS_PER_SECOND,
    )
    .expect("the measured record lowers");

    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    session
        .register(actor(1), std::slice::from_ref(&declared))
        .expect("the measured record registers for the shooter");
    session
        .register_tangler_field(&ordnance_id(), field.clone())
        .expect("the measured field is the shape the runtime opens");

    let mut world = World::new();
    launcher(&mut world);
    let mut damage = damage_resolver();
    let victim = actor(2);
    let first = step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order()],
        &[path(victim, MOUNT_ORIGIN)],
        0,
    );
    assert_eq!(first.launched, vec![projectile(0)]);
    assert_eq!(first.tangler_opened, vec![field_id(0)]);
    let open = session.tangler_fields();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].centre, position(MOUNT_ORIGIN));
    assert_eq!(open[0].lifetime_ticks, TANGLER_LIFETIME_TICKS);
    assert_eq!(open[0].radius_m, 10.0);
    assert!(session.is_tangled(&victim));
    assert!(!session.boost_allowed(&victim));
}
