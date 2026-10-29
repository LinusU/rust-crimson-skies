//! Task T452-REG-TANGLER-EFFECT (#2179): what a zero-damage `TANGLER` field
//! does to a flight model. Task test prefix: `accept_t452_tangler_`.
//!
//! `docs/findings/2026-10-29-t452-tangler-flight-effect.md` measured the
//! physical meaning the earlier stages left named: the force scalar `[ebp-8]`
//! of the actor force routine `0x48fc40` is the force block's **thrust
//! magnitude** (`0x48fde1..0x48fdf0` computes it, `0x48fe91..0x48feb8` is its
//! only use), so storing `0.0` into it while the tangle flag is set
//! (`0x48fdd0..0x48fdd8`) means a tangled aircraft produces no thrust force —
//! and its boost setter refuses to enable (`0x4b2136`). Lift, drag, gravity and
//! the control torque are untouched.
//!
//! These tests drive the production path end to end: a registered `wep_12`
//! fires through [`step_ordnance_session`], opens its measured field, tangles
//! the body it reaches every tick, and the body's [`FlightAircraft`] — flown by
//! the real Avian schedule — reports zero thrust while the tangle is live and
//! the full thrust again once it clears. The fixture carries the *shape* of
//! `wep_12` so the path is pinned in CI, where no original data exists; the
//! measured byte windows behind it are re-read from `$CS_ENGINE_IMAGE` by
//! `accept_t452_tangler_image_rows_match_the_engine_image` (`cs_content`).
//!
//! Every test calls production code. No test awards `verified_original`, and
//! nothing here is an original-run observation.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId, OrdnanceLauncherBinding, OrdnanceOrder,
    OrdnanceSession, OrdnanceSessionTick, OrdnanceStep, TanglerFieldId, register_measured_ordnance,
    step_ordnance_session,
};
use cs_app::physics::{
    FixtureBodySpec, FlightForcesPlugin, FlightSpawnSpec, FlightTangle, PhysicsFixture,
    spawn_flight_body,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DeclaredOrdnanceFamily, OriginalAmmoDeclaration, OriginalAreaDeclaration, OriginalClusterShape,
    OriginalComponentLaunch, OriginalComponentTable, OriginalDamageDeclaration,
    OriginalEffectBlock, OriginalLifetime, OriginalMountDeclaration, OriginalMountLayout,
    OriginalSlotArray, OriginalStatusEffectDeclaration, OriginalWeaponMedia, OriginalWeaponRecord,
    T452_REG_TANGLER_FORCE_SCALAR, TANGLE_FLOOR_S, TanglerFieldDeclaration,
    measure_component_behavior,
};
use cs_sim::damage::{ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver};
use cs_sim::flight::{EngineState, FlightInput, FlightModel, synthetic_fixed_wing};
use cs_sim::time::TickRate;
use cs_sim::weapons::{OrdnanceId, OrdnanceRegistry, StatusEffectTarget, SweptHit, TargetPath};
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
/// The ordnance session's tick length: the measured `TIME` is whole 30 Hz
/// ticks, so the field's lifetime is exactly `TIME * 30`.
const DT_S: f64 = 1.0 / 30.0;
const TICKS_PER_SECOND: u32 = 30;
const FIXED_HZ: u32 = 30;

/// The mount node's pose: the position the fire path reads. The field is
/// centred on the point of contact when the projectile bursts, not on this pose,
/// so the two must stay apart.
const MOUNT_ORIGIN: [f64; 3] = [4.0, 5.0, 10.0];

/// Where the fixture projectile is at the end of the burst tick: it leaves
/// [`MOUNT_ORIGIN`] along -z at the record's measured `VELOCITY` (300 m/s, 10 m
/// per 30 Hz tick) and has advanced on ticks 0, 1 and 2.
const BURST_CENTRE: [f64; 3] = [4.0, 5.0, -20.0];

/// Where in the burst tick the swept contact happens: half a tick, 5 m before
/// [`BURST_CENTRE`].
const CONTACT_TIME: f64 = 0.5;

/// The point of contact on the item's own swept segment
/// (`previous + t * (current - previous)`), where the field opens (task #2205).
const CONTACT_CENTRE: [f64; 3] = [4.0, 5.0, -15.0];

/// The tick a swept contact bursts the projectile in these tests.
const BURST_TICK: u64 = 2;

/// Where the victim body is: 8 m from the field centre, inside the measured
/// 10 m radius. `d2 = 64`, so the measured `max(2 s, 10 * (1 - d2 / RADIUS))`
/// lands on its 2 s floor — the field's own `TIME`, and therefore the exact
/// span this test can drive in one pass.
const VICTIM_ORIGIN: [f64; 3] = [12.0, 5.0, -15.0];

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
    step_with_impacts(world, session, damage, orders, targets, &[], at)
}

/// One step of a session that is told about swept contacts.
fn step_with_impacts(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    orders: &[OrdnanceOrder],
    targets: &[TargetPath],
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
            targets,
            impacts,
            blast_recipients: None,
            victim_nodes: None,
        },
    )
}

/// A swept contact on the victim, at half the burst tick's flight.
fn impact_on(victim: ActorId) -> SweptHit {
    SweptHit {
        projectile: projectile(0),
        target: victim,
        time_of_impact: CONTACT_TIME,
    }
}

/// Fires on tick 0, flies one quiet tick, and bursts the projectile on
/// [`BURST_TICK`] with a swept contact on the victim; returns that tick's
/// report. A launch alone opens no field (task #2203).
fn fire_and_burst(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    targets: &[TargetPath],
) -> OrdnanceSessionTick {
    let first = step(world, session, damage, &[launch_order()], targets, 0);
    assert_eq!(first.launched, vec![projectile(0)], "the record launches");
    assert!(
        first.tangler_opened.is_empty(),
        "a launch alone opens no field"
    );
    let quiet = step(world, session, damage, &[], targets, 1);
    assert!(quiet.tangler_opened.is_empty());
    step_with_impacts(
        world,
        session,
        damage,
        &[],
        targets,
        &[impact_on(actor(2))],
        BURST_TICK,
    )
}

// ------------------------------------------------------------------- flight ---

/// A flight world: the real physics fixture with the production
/// [`FlightForcesPlugin`] on the ordnance session's 30 Hz tick, so one flight
/// tick is one ordnance tick and the two clocks cannot drift apart.
fn flight_fixture() -> PhysicsFixture {
    PhysicsFixture::builder(FixtureBodySpec {
        mass_kg: 1.0,
        half_extents_m: [0.05, 0.05, 0.05],
        position_m: [-10_000.0, 0.0, 0.0],
        linear_velocity_m_s: [0.0; 3],
    })
    .fixed_hz(FIXED_HZ)
    .configure(|app| {
        app.add_plugins(FlightForcesPlugin);
    })
    .build()
    .expect("the fixture spec is valid")
}

/// The spawn spec of one cruising aircraft at `position_m`: engine at full
/// spool, throttle open, boost pressed, and a finite boost reserve. Forward is
/// body `-Z`, so the velocity is a level cruise along the body's nose.
fn cruise_spec(position_m: [f32; 3], boost_capacity_units: f64) -> FlightSpawnSpec {
    FlightSpawnSpec {
        position_m,
        linear_velocity_mps: [0.0, 0.0, -80.0],
        engine: EngineState::direct(1.0),
        boost_capacity_units,
        command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, true).expect("a legal command"),
        ..FlightSpawnSpec::level_at(position_m, [0.0, 0.0, -80.0])
    }
}

/// Spawns one cruising aircraft through the production path.
fn spawn_cruiser(fixture: &mut PhysicsFixture, position_m: [f32; 3]) -> Entity {
    spawn_flight_body(
        fixture.world_mut(),
        FlightModel::new(synthetic_fixed_wing()),
        &cruise_spec(position_m, 1.0),
    )
    .expect("the spawn spec is valid")
}

/// The last measured thrust of one body, newtons.
fn thrust_of(fixture: &PhysicsFixture, entity: Entity) -> f64 {
    fixture
        .world()
        .get::<cs_app::physics::FlightAircraft>(entity)
        .expect("a spawned flight body carries FlightAircraft")
        .last_output()
        .expect("a driven tick leaves an output")
        .instrument_state
        .thrust_n
}

// ------------------------------------------------------------------- tests ---

/// The measured tangle removes **only** the thrust term: two identical cruisers
/// on the same tick, one tangled, differ by exactly `thrust * body forward`.
/// Lift, drag, gravity and the control torque are identical, and the tangled
/// aircraft accepts no boost at all.
#[test]
fn accept_t452_tangler_flight_the_measured_tangle_zeroes_only_the_thrust_term() {
    let mut fixture = flight_fixture();
    let free = spawn_cruiser(&mut fixture, [0.0, 400.0, 0.0]);
    // Far enough apart that the two bodies never touch: the fixtures' own body
    // is parked at x = -10 000, and every aeroforce depends on the state, not
    // on the position.
    let tangled = spawn_cruiser(&mut fixture, [500.0, 400.0, 0.0]);

    // The tangle the ordnance session produces, built through the measured
    // declaration (same field, same actor, a live timer).
    let mut session = session_with_field();
    let mut damage = damage_resolver();
    let victim = path(actor(2), VICTIM_ORIGIN);
    let mut ordnance_world = World::new();
    launcher(&mut ordnance_world);
    fire_and_burst(
        &mut ordnance_world,
        &mut session,
        &mut damage,
        std::slice::from_ref(&victim),
    );
    let tangle = session
        .tangle_status(&actor(2))
        .expect("the field tangled the victim it reached");
    assert!(!session.boost_allowed(&actor(2)));
    fixture
        .world_mut()
        .entity_mut(tangled)
        .insert(FlightTangle(Some(tangle)));

    fixture.step(1);

    let free_output = fixture
        .world()
        .get::<cs_app::physics::FlightAircraft>(free)
        .expect("the free body")
        .last_output()
        .expect("a driven tick leaves an output");
    let tangled_output = fixture
        .world()
        .get::<cs_app::physics::FlightAircraft>(tangled)
        .expect("the tangled body")
        .last_output()
        .expect("a driven tick leaves an output");

    // The free body produces thrust: an idling 9 kN engine plus the 3 kN
    // boost the declared tuning states, with a full boost reserve.
    assert_eq!(
        free_output.instrument_state.thrust_n, 12_000.0,
        "the untangled cruiser produces engine + boost thrust"
    );
    assert!(
        free_output.accepted_boost_consumption > 0.0,
        "the untangled cruiser spends boost"
    );
    // The tangled body produces none, and spends none: the measured force
    // scalar is zero and the measured boost setter refuses to enable.
    assert_eq!(
        tangled_output.instrument_state.thrust_n, 0.0,
        "a tangled aircraft's thrust scalar is zero"
    );
    assert_eq!(tangled_output.diagnostics.thrust_n, 0.0);
    assert_eq!(
        tangled_output.accepted_boost_consumption, 0.0,
        "a tangle ends any boost: the setter refuses to enable"
    );

    // Nothing else moved: lift, drag, gravity, control authority and torque are
    // the free body's, value for value.
    assert_eq!(
        tangled_output.diagnostics.lift_n, free_output.diagnostics.lift_n,
        "the measured branch is not a lift change"
    );
    assert_eq!(
        tangled_output.diagnostics.drag_n, free_output.diagnostics.drag_n,
        "the measured branch is not a drag change"
    );
    assert_eq!(
        tangled_output.diagnostics.gravity_force_n,
        free_output.diagnostics.gravity_force_n
    );
    assert_eq!(
        tangled_output.diagnostics.control_authority, free_output.diagnostics.control_authority,
        "the measured branch is not a control lock"
    );
    assert_eq!(
        tangled_output.world_torque_nm, free_output.world_torque_nm,
        "the control torque is the same: the tangle touches the thrust term only"
    );

    // The force differs by exactly the thrust vector along the body's nose:
    // body forward is -Z, so removing the thrust adds its magnitude back on +Z.
    let free_force = free_output.world_force_n;
    let tangled_force = tangled_output.world_force_n;
    let thrust = free_output.instrument_state.thrust_n;
    assert_eq!(
        tangled_force[0], free_force[0],
        "no force on the body X axis"
    );
    assert_eq!(
        tangled_force[1], free_force[1],
        "no force on the body Y axis"
    );
    let expected = free_force[2] + thrust;
    assert!(
        (tangled_force[2] - expected).abs() <= 1e-9 * expected.abs().max(1.0),
        "the difference is exactly the thrust along the nose, nothing else: \
         {} against {}",
        tangled_force[2],
        expected
    );
}

/// The production chain: a registered `wep_12` fires through
/// [`step_ordnance_session`], its measured field opens where the projectile
/// bursts, lives its measured `TIME` from there and re-applies the tangle every
/// tick — and the body it reaches loses its thrust for exactly that span, then
/// gets it back at the held throttle.
#[test]
fn accept_t452_tangler_flight_a_registered_wep_12_tangles_a_body_for_its_field_time() {
    let mut fixture = flight_fixture();
    let plane = spawn_cruiser(
        &mut fixture,
        [
            VICTIM_ORIGIN[0] as f32,
            VICTIM_ORIGIN[1] as f32,
            VICTIM_ORIGIN[2] as f32,
        ],
    );
    // The body flies level at 80 m/s before the field ever touches it.
    fixture.step(1);
    assert!(
        thrust_of(&fixture, plane) > 0.0,
        "the cruiser is producing thrust before the field opens"
    );

    let mut session = session_with_field();
    let mut damage = damage_resolver();
    let victim = path(actor(2), VICTIM_ORIGIN);
    // The launcher lives in the same world as the aircraft: the fire path reads
    // the mount pose from it and the flight driver flies the body.
    launcher(fixture.world_mut());

    // The burst is on tick 2 and the field lives its measured `TIME` from
    // there, ticks 2..61, re-applying its per-hit routine each of them. Each hit
    // gives the measured 2 s floor, so the victim's own timer is refilled to 60
    // ticks from every hit: the last hit is at tick 61 and the timer therefore
    // clears at tick 121, 60 ticks after the field that put it there is gone
    // (the measured `RaiseToMax`, `[[tangler_fact]] tangler.status_timer`, makes
    // the tangle outlive its field by its own duration).
    let field_end = BURST_TICK + TANGLER_LIFETIME_TICKS;
    let tangle_boundary = field_end + TANGLER_LIFETIME_TICKS - 1;
    let mut applications = 0_u64;
    let mut field_evaluated = false;
    for at in 0..=tangle_boundary {
        let orders = if at == 0 {
            vec![launch_order()]
        } else {
            Vec::new()
        };
        let impacts: Vec<SweptHit> = if at == BURST_TICK {
            vec![impact_on(actor(2))]
        } else {
            Vec::new()
        };
        let tick = step_with_impacts(
            fixture.world_mut(),
            &mut session,
            &mut damage,
            &orders,
            std::slice::from_ref(&victim),
            &impacts,
            at,
        );
        if at == 0 {
            assert_eq!(tick.launched, vec![projectile(0)], "the record launches");
            assert!(
                tick.tangler_opened.is_empty() && session.tangler_fields().is_empty(),
                "a launch alone opens no field"
            );
        }
        if at == BURST_TICK {
            assert_eq!(
                tick.tangler_opened,
                vec![field_id(0)],
                "the burst opens the component's measured field"
            );
            let open = session.tangler_fields();
            assert_eq!(open.len(), 1);
            assert_eq!(
                open[0].centre,
                position(CONTACT_CENTRE),
                "the field opens at the point of contact, not the tick-end position"
            );
            assert_ne!(
                open[0].centre,
                position(BURST_CENTRE),
                "the centre is the contact, not the item's end-of-tick position"
            );
            assert_ne!(
                open[0].centre,
                position(MOUNT_ORIGIN),
                "the field is not centred where the weapon fired"
            );
            assert_eq!(
                open[0].lifetime_ticks, TANGLER_LIFETIME_TICKS,
                "TIME 2 s is 60 whole 30 Hz ticks"
            );
            assert_eq!(open[0].opened_at, Tick(BURST_TICK));
            assert_eq!(open[0].expires_at, Tick(field_end));
        }
        if (BURST_TICK..field_end).contains(&at) {
            assert_eq!(
                tick.tangler.len(),
                1,
                "tick {at}: the field is live and passes every actor it reaches"
            );
            assert_eq!(
                tick.tangler[0].applied.len(),
                1,
                "tick {at}: the field re-applies the tangle to the body"
            );
            assert_eq!(
                tick.tangler[0].applied[0].duration_ticks, TANGLER_LIFETIME_TICKS,
                "tick {at}: d2 = 64 lands on the measured 2 s duration floor"
            );
            applications += 1;
        } else {
            assert!(
                tick.tangler.is_empty(),
                "tick {at}: the field is not open and passes nobody"
            );
            if at >= field_end && !field_evaluated {
                assert_eq!(
                    tick.tangler_expired.len(),
                    1,
                    "the field is reported gone exactly once, on its boundary tick"
                );
                assert_eq!(tick.tangler_expired[0].expired_at, Tick(field_end));
                field_evaluated = true;
            }
        }

        // The tangle is the victim's own timer, refilled by every hit and then
        // counted down on its own boundary.
        if (BURST_TICK..tangle_boundary).contains(&at) {
            let tangle = session
                .tangle_status(&actor(2))
                .unwrap_or_else(|| panic!("tick {at}: the body is tangled"));
            let expected_expiry = if at < field_end {
                at + TANGLER_LIFETIME_TICKS
            } else {
                tangle_boundary
            };
            assert_eq!(
                tangle.expires_at,
                Tick(expected_expiry),
                "tick {at}: the measured timer"
            );
            assert!(
                !session.boost_allowed(&actor(2)),
                "tick {at}: a tangle refuses boost"
            );
            fixture
                .world_mut()
                .entity_mut(plane)
                .insert(FlightTangle(Some(tangle)));
            fixture.step(1);
            assert_eq!(
                thrust_of(&fixture, plane),
                0.0,
                "tick {at}: a tangled body produces no thrust"
            );
        } else if at >= tangle_boundary {
            assert!(
                !session.is_tangled(&actor(2)),
                "tick {at}: the tangle cleared on its own boundary"
            );
            assert!(
                session.boost_allowed(&actor(2)),
                "tick {at}: the boost refusal ends with the tangle"
            );
            assert_eq!(
                tick.tangles_expired.len(),
                1,
                "tick {at}: the tangle is reported cleared exactly once"
            );
            assert_eq!(tick.tangles_expired[0].expired_at, Tick(tangle_boundary));
            fixture
                .world_mut()
                .entity_mut(plane)
                .insert(FlightTangle::CLEAR);
            fixture.step(1);
            assert_eq!(
                thrust_of(&fixture, plane),
                12_000.0,
                "tick {at}: the thrust returns at the held throttle once the tangle clears"
            );
        } else {
            assert!(
                !session.is_tangled(&actor(2)),
                "tick {at}: nothing is tangled before the projectile bursts"
            );
            assert!(session.tangler_fields().is_empty());
        }
    }
    assert_eq!(
        applications, TANGLER_LIFETIME_TICKS,
        "the field applies exactly its measured TIME in ticks"
    );
}

/// The tangle is a live status, not a permanent capability: a body whose
/// producer stops writing (the component is removed) is not tangled on the next
/// tick, so a missed write can never freeze an aircraft with no thrust.
#[test]
fn accept_t452_tangler_flight_a_body_whose_producer_stops_is_not_tangled() {
    let mut fixture = flight_fixture();
    let plane = spawn_cruiser(&mut fixture, [0.0, 400.0, 0.0]);

    let mut session = session_with_field();
    let mut damage = damage_resolver();
    let victim = path(actor(2), VICTIM_ORIGIN);
    let mut ordnance_world = World::new();
    launcher(&mut ordnance_world);
    fire_and_burst(
        &mut ordnance_world,
        &mut session,
        &mut damage,
        std::slice::from_ref(&victim),
    );
    let tangle = session
        .tangle_status(&actor(2))
        .expect("the field tangled the victim it reached");

    fixture
        .world_mut()
        .entity_mut(plane)
        .insert(FlightTangle(Some(tangle)));
    fixture.step(1);
    assert_eq!(thrust_of(&fixture, plane), 0.0, "the tangle is in force");

    fixture
        .world_mut()
        .entity_mut(plane)
        .remove::<FlightTangle>();
    fixture.step(1);
    assert_eq!(
        thrust_of(&fixture, plane),
        12_000.0,
        "a body with no producer record is not tangled"
    );
}

/// The claim the earlier stages kept is resolved by the measurement behind it:
/// the suppressed force scalar is the force block's thrust magnitude, so the
/// registration report no longer lists `t452_reg.limit.tangler_force_scalar_meaning`
/// among the claims `wep_12` leaves open — and the claim id stays in the code,
/// so older reports and findings still resolve their citation.
#[test]
fn accept_t452_tangler_flight_the_suppressed_scalar_claim_is_resolved() {
    let behavior = measure_component_behavior(&tangler_row());
    assert!(
        behavior.resolves_claim(T452_REG_TANGLER_FORCE_SCALAR),
        "the TANGLER measurement resolves what the suppressed scalar is: {:?}",
        behavior.resolved
    );
    assert!(
        !behavior.names_claim(T452_REG_TANGLER_FORCE_SCALAR),
        "and it is therefore not one of the measurement's open claims"
    );

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
    assert_eq!(report.registered_records(), vec!["wep_12"]);
    let kept = report
        .claims_kept
        .iter()
        .find(|(record, _)| record == "wep_12")
        .expect("wep_12 keeps its named claims");
    assert!(
        !kept
            .1
            .iter()
            .any(|claim| claim == T452_REG_TANGLER_FORCE_SCALAR),
        "the resolved claim is not kept: {:?}",
        kept.1
    );
    // The one part the measurement did not pin — the field's origin — is
    // measured too (task #2203: the projectile's position at its burst), so no
    // claim of the field stays named.
    assert!(
        !kept
            .1
            .iter()
            .any(|claim| claim == "t452_reg.limit.tangler_field_origin"),
        "the field's measured origin is not kept: {:?}",
        kept.1
    );
    // The status the field applies now names the measured meaning.
    assert_eq!(
        tangler_field().status.name,
        cs_content::ordnance::TANGLE_STATUS_NAME
    );
    assert!(
        cs_content::ordnance::TANGLE_STATUS_NAME.contains("thrust"),
        "the status name states what the scalar is"
    );
    assert_eq!(tangler_field().status.floor_s, TANGLE_FLOOR_S);
}
