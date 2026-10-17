//! M01-PLAY-WEAPONS acceptance (#1437): the player fires guns and rockets in
//! M01 and destroys a target.
//!
//! Spec: the M01 play slice (`missions/M01.md`'s "blow up the fuel truck and
//! the cargo zeppelin's fuel tanks"), through the F27 (`specs/F27-*.md`) gun
//! and F28 (`specs/F28-*.md`) rocket production paths. Task test prefix:
//! `accept_m01_play_weapons_`.
//!
//! These tests drive the composition a running mission uses
//! ([`build_headless`] over the synthetic stage the campaign suite already
//! fields): the mission weapon session is opened by the plugin, the rig hangs
//! on the live player body, and every tick's input is published by production
//! code — no test calls `step_weapon_session` or `step_ordnance_session`. The
//! fire keys are the real delivered boundaries: `Space`/`FirePrimary` for the
//! guns and `LeftControl`/`FireSecondary` for the rockets, injected as the
//! frame's delivered cockpit edges.
//!
//! The loadout is **designed**, not measured: the original Devastator's guns,
//! ammunition and rockets were not recovered, so every record carries
//! `Origin::Designed` with the `m01-play-weapons.designed-loadout` claim. The
//! tests assert the *path* (fire → swept hit → F29 damage → destruction →
//! debris → score), which is the task's acceptance, and say nothing about the
//! original's numbers.
//!
//! Every value here is newly authored synthetic fixture data. No
//! `CS_GAME_DIR` access.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{App, ChildOf, GlobalTransform, Transform, Vec3};
use cs_app::damage::{DamageActorBinding, DamageScheduleReport};
use cs_app::debris::{PartDebrisBinding, SpawnedDebris};
use cs_app::input::{DeviceEvent, FrameInput, InputSession, SessionMode};
use cs_app::mission_session::{
    MissionCockpitActions, MissionComposition, MissionHost, MissionWeaponReport, MissionWeapons,
    admit_actor, build_headless, designed_loadout, is_fire_guns, is_fire_rockets, teardown,
};
use cs_app::playtest::command::playtest_action_map;
use cs_app::score::SessionScoreLedger;
use cs_app::targeting::TargetingSession;
use cs_app::weapons::PartSweptBox;
use cs_sim::control::LocalSeatId;
use cs_sim::damage::{
    ActorId, AttributionRule, DamageNodeKey, DamagePolicy, synthetic_airframe_graph,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::input::{Action, DeviceClass, DeviceId, FlightCommand, Key};
use cs_types::net::{ActorAllocator, SessionId};
use cs_types::space::WorldPosition;

use crate::vs_m01_rt_window::{Scratch, synthetic_stage};

/// The damage node a test target's part and a rocket's engagement both name.
const HULL: &str = "hull";
/// The target's serial, distinct from the player's `MISSION_PLAYER_SERIAL`.
const TARGET_SERIAL: u64 = 2;
/// How far ahead of the player's own position the target sits, in metres.
const TARGET_AHEAD_M: f32 = 120.0;
/// The target part's half extents, large enough that the round's first
/// segment crosses it.
const TARGET_HALF_EXTENTS_M: [f64; 3] = [40.0, 40.0, 40.0];
/// The most rendered frames a test waits for a kill before failing.
const MAX_FRAMES: usize = 400;

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("the test node key is valid")
}

fn claim() -> ClaimId {
    ClaimId::new("m01-play-weapons.accept-test").expect("the claim id is valid")
}

fn target_actor(session: SessionId) -> ActorId {
    ActorId {
        session,
        serial: TARGET_SERIAL,
    }
}

/// The composed synthetic mission, with the weapon session opened and the
/// player's rig attached, after one `update`.
fn composed(label: &str) -> (Scratch, App) {
    let scratch = Scratch::new(label);
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    // One update runs Startup (spawns the player), PostStartup (opens the
    // weapon session and hangs the rig) and the first fixed ticks.
    app.update();
    (scratch, app)
}

/// The session the composed mission serves.
fn served(app: &App) -> SessionId {
    app.world().resource::<MissionHost>().served()
}

/// The scene generation the composition's live entities carry.
fn generation(app: &App) -> cs_app::scene::SceneGeneration {
    app.world()
        .resource::<cs_app::scene::SceneGenerations>()
        .latest()
}

/// The player's live position and forward (its body's `-Z` nose axis).
fn player_pose(app: &App) -> (Vec3, Vec3) {
    let player = app.world().resource::<MissionComposition>().player;
    let world = app.world();
    let (translation, rotation) = if let Some(global) = world.get::<GlobalTransform>(player) {
        (global.translation(), global.rotation())
    } else if let Some(transform) = world.get::<Transform>(player) {
        (transform.translation, transform.rotation)
    } else {
        panic!("the player body has a live transform");
    };
    let forward = -(rotation * Vec3::Z);
    (translation, forward)
}

/// Spawns one damage-bound target ahead of the player, registers its graph
/// with the shared resolver and its roster record with the targeting store,
/// and admits it to the session's score ledger — the spawn host's own duties.
fn spawn_target(app: &mut App) -> ActorId {
    let session = served(app);
    let generation = generation(app);
    let actor = target_actor(session);
    let graph = synthetic_airframe_graph();
    let subject = graph.subject().clone();
    let (position, forward) = player_pose(app);
    let centre = position + forward * TARGET_AHEAD_M;
    let world = app.world_mut();

    // Register the target's graph with the shared resolver the mission's
    // schedules serve, exactly as the spawn host does.
    {
        let mut shared = world.resource_mut::<cs_app::weapons::SessionDamageResolver>();
        shared
            .resolver_mut()
            .register_actor(
                actor,
                graph,
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("the synthetic target graph registers");
    }
    // Register with the targeting store the mission's session opened.
    {
        let mut targeting = world.resource_mut::<TargetingSession>();
        targeting
            .store_mut()
            .register(cs_sim::targeting::TargetRecord {
                actor,
                faction: ContentId::from_source(
                    ContentKind::Faction,
                    cs_app::mission_session::MISSION_FACTION_HOSTILE,
                )
                .expect("the designed hostile faction id is valid"),
                class: cs_sim::targeting::TargetClass::Aircraft,
                objective: true,
                revealed: true,
                phase_eligible: true,
                position: WorldPosition::try_new([
                    f64::from(centre.x),
                    f64::from(centre.y),
                    f64::from(centre.z),
                ])
                .expect("the target centre is finite"),
            })
            .expect("the target registers in the roster");
    }
    {
        let mut ledger = world.resource_mut::<SessionScoreLedger>();
        let _ = admit_actor(&mut ledger, actor);
    }

    let root = world
        .spawn((
            Transform::from_translation(centre),
            GlobalTransform::from_translation(centre),
            LinearVelocity(Vec3::ZERO),
            DamageActorBinding {
                actor,
                graph: subject,
                generation,
            },
        ))
        .id();
    let debris = Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Airframe, "m01-play-weapons.test-wreck")
            .expect("the wreck id is valid"),
        Provenance::designed(claim()),
    ));
    world.spawn((
        PartSweptBox::new(actor, key(HULL), TARGET_HALF_EXTENTS_M),
        PartDebrisBinding::new(actor, key(HULL), debris, generation),
        cs_app::scene::NodeVisualTransform(GlobalTransform::from_translation(centre)),
        Transform::from_translation(centre),
        ChildOf(root),
    ));
    actor
}

/// Publishes one frame's delivered cockpit edge and runs the composed ticks.
fn fire(app: &mut App, command: FlightCommand) {
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: vec![Action::Flight(command)],
    });
    app.update();
}

/// Whether the target has been destroyed: the damage schedule published its
/// destruction, or its authored debris has spawned.
fn destroyed(app: &mut App, actor: ActorId) -> bool {
    let world = app.world_mut();
    if let Some(report) = world.get_resource::<DamageScheduleReport>()
        && report.destroyed.contains(&actor)
    {
        return true;
    }
    world.iter_entities().any(|entity| {
        entity
            .get::<SpawnedDebris>()
            .is_some_and(|s| s.actor == actor)
    })
}

/// **The composed stage a test drives, with a target bound and aimed.**
pub(crate) fn armed_stage(label: &str) -> (Scratch, App, ActorId) {
    let (scratch, mut app) = composed(label);
    // The weapon session opened for the served generation and the rig hangs on
    // the player: both are the plugin's own work.
    assert!(
        app.world().get_resource::<MissionWeapons>().is_some(),
        "the composed mission opened a weapon session"
    );
    assert_eq!(
        app.world().resource::<MissionWeapons>().player().serial,
        cs_app::mission_session::MISSION_PLAYER_SERIAL,
        "the session flies the player actor"
    );
    let actor = spawn_target(&mut app);
    (scratch, app, actor)
}

/// **The fire-guns key destroys a target: the player's gun bank fires, the
/// round's swept hit applies F29 damage, the destroyed part spawns its debris
/// and the kill reaches the session's score ledger.**
#[test]
fn accept_m01_play_weapons_the_fire_guns_key_destroys_a_target_and_scores_it() {
    let (_scratch, mut app, actor) = armed_stage("m01-play-weapons-guns");

    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        fire(&mut app, FlightCommand::FirePrimary);
        if destroyed(&mut app, actor) {
            killed = true;
            break;
        }
    }
    assert!(
        killed,
        "the fire-guns key destroyed the target: damage={:?} weapon={:?} mission={:?}",
        app.world().get_resource::<DamageScheduleReport>(),
        app.world()
            .get_resource::<cs_app::weapons::WeaponScheduleReport>(),
        app.world().get_resource::<MissionWeaponReport>(),
    );
    let score = app.world().resource::<SessionScoreLedger>().score();
    assert!(score > 0, "the kill reached the score ledger: {score}");
    teardown(&mut app);
}

/// **The fire-rockets key launches a rocket that detonates on the target,
/// applies F29 damage and scores the kill.**
#[test]
fn accept_m01_play_weapons_the_fire_rockets_key_destroys_a_target_and_scores_it() {
    let (_scratch, mut app, actor) = armed_stage("m01-play-weapons-rockets");

    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        fire(&mut app, FlightCommand::FireSecondary);
        if destroyed(&mut app, actor) {
            killed = true;
            break;
        }
    }
    assert!(
        killed,
        "the fire-rockets key destroyed the target: {:?}",
        app.world().get_resource::<DamageScheduleReport>()
    );
    let score = app.world().resource::<SessionScoreLedger>().score();
    assert!(
        score > 0,
        "the rocket kill reached the score ledger: {score}"
    );
    teardown(&mut app);
}

/// **Firing a rocket with no declared-hostile target is refused by name, not
/// invented at a default target.**
#[test]
fn accept_m01_play_weapons_a_rocket_with_no_target_is_refused_by_name() {
    let (_scratch, mut app) = composed("m01-play-weapons-no-target");
    fire(&mut app, FlightCommand::FireSecondary);
    let report = app
        .world()
        .get_resource::<MissionWeaponReport>()
        .expect("the publisher ran");
    assert!(
        !report.rocket_refusals.is_empty(),
        "a rocket with no hostile target is refused by name: {report:?}"
    );
    assert!(
        report
            .rocket_refusals
            .iter()
            .any(|reason| reason.contains("hostile")),
        "the refusal names the missing hostile target: {report:?}"
    );
    teardown(&mut app);
}

/// **The fire keys are bound: the playtest's own action map (the map the
/// mission's input session runs) delivers `FirePrimary` for `Space` and
/// `FireSecondary` for `Left Control`.**
#[test]
fn accept_m01_play_weapons_the_fire_keys_are_bound() {
    let mut session = InputSession::new(
        playtest_action_map(),
        LocalSeatId(0),
        SessionMode::SinglePlayer,
        Tick(0),
    );
    session
        .collector_mut()
        .connect_device(keyboard())
        .expect("the fixture keyboard connects");
    let events = vec![DeviceEvent::KeyboardFrame {
        device: keyboard(),
        keys: vec![Key::Space, Key::LeftControl],
    }];
    let delivered = session
        .pump_frame(FrameInput::Devices(&events), 1)
        .expect("the frame pumps")
        .delivered;
    assert_eq!(
        delivered,
        vec![
            Action::Flight(FlightCommand::FirePrimary),
            Action::Flight(FlightCommand::FireSecondary),
        ],
        "Space fires guns and Left Control fires rockets"
    );
    assert!(is_fire_guns(delivered[0]), "the guns edge is the gun fire");
    assert!(
        is_fire_rockets(delivered[1]),
        "the rockets edge is the rocket fire; the mission fires on `MISSION_FIRE_ROCKETS`"
    );
}

/// The fixture keyboard device.
fn keyboard() -> DeviceId {
    DeviceId::stable(DeviceClass::Keyboard, "kbd.m01-play-weapons/0")
        .expect("the fixture identity is valid")
}

/// **The loadout is designed and says so: every gun and the rocket are
/// `Origin::Designed` under the M01 claim, and no record claims to be
/// original.**
#[test]
fn accept_m01_play_weapons_the_loadout_is_designed_and_labelled() {
    let loadout = designed_loadout();
    assert!(!loadout.guns.is_empty(), "the loadout carries guns");
    for gun in &loadout.guns {
        assert_eq!(
            gun.origin(),
            &cs_types::content::Origin::Designed,
            "the gun {} is designed, not original",
            gun.gun()
        );
        assert!(
            !gun.origin().is_original(),
            "a designed gun never claims original data"
        );
    }
    let rocket = cs_app::mission_session::designed_rocket();
    assert_eq!(rocket.origin(), &cs_types::content::Origin::Designed);
    assert!(!rocket.origin().is_original());
}

/// **The player's serial is one the mission's scripted-actor allocator can
/// never issue.**
///
/// M01's scripted actors (M01-PLAY-ACTORS, #1435) are allocated from a
/// per-session `ActorAllocator` that hands out `1`, `2`, `3`, ...; a player
/// serial inside that range would collide with the first spawned actor and
/// make the player and that actor one identity — the F27 self-hit exclusion
/// would read the player's shot on it as a self-hit and the rocket's target
/// search would skip it as the shooter. The player therefore owns the top of
/// the space, the one serial `ActorAllocator::allocate` refuses to issue.
#[test]
fn accept_m01_play_weapons_the_player_serial_is_reserved_from_the_actor_allocator() {
    let session = SessionId::new(7).expect("the test session id is valid");
    let mut allocator = ActorAllocator::new(session);
    for _ in 0..10_000 {
        let actor = allocator
            .allocate()
            .expect("the serial space is not exhausted");
        assert_ne!(
            actor.serial,
            cs_app::mission_session::MISSION_PLAYER_SERIAL,
            "a scripted session actor never takes the player's reserved serial"
        );
    }
    assert_eq!(
        cs_app::mission_session::MISSION_PLAYER_SERIAL,
        u64::MAX,
        "the reserved serial is the top of the space, which `allocate` cannot issue"
    );
}
