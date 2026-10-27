//! Acceptance scenario F30-G: the mission session's targeting runs through
//! [`TargetingSchedulePlugin`] once per rendered frame.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-G`. Task test prefix: `accept_f30_g_`.
//!
//! Before this stage `mission_session::weapons` invoked
//! [`apply_selection_edges`] itself on the tick's action list, so nothing
//! published [`TargetingFrameInputs`] or [`PendingTargetDamage`] and the
//! rendered-frame schedule's consumer pass — the HUD reticle, the spyglass
//! readout, the weapon-guidance aid — never ran for a mission session.
//!
//! These tests drive production wiring only: a headless app carries the
//! session the composed mission would open — [`open_mission_weapons`] and its
//! [`MissionWeaponSessions::install`], the player's rig through
//! [`attach_player_rig`], and the [`MissionWeaponsPlugin`] that owns the
//! publishers. No test inserts [`TargetingFrameInputs`], calls a targeting
//! entry, or touches the store: every observation is read from the resources
//! the installed schedule publishes ([`TargetConsumers`],
//! [`TargetingScheduleReport`]), and the destruction that clears the view is
//! the designed guns' own resolution. Every value here is newly authored
//! synthetic fixture data, never original game data.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::prelude::{App, Camera3d, GlobalTransform, Transform, Vec3};
use cs_app::damage::DamageActorBinding;
use cs_app::mission_actors::ground_target_sweep;
use cs_app::mission_session::{
    MISSION_FACTION_HOSTILE, MissionCockpitActions, MissionPlayerBody, MissionWeaponsPlugin,
    attach_player_rig, open_mission_weapons,
};
use cs_app::physics::{BASELINE_FIXED_HZ, PhysicsAdapterPlugin};
use cs_app::scene::SceneGenerations;
use cs_app::targeting::{
    ClearedTarget, PendingTargetDamage, TargetConsumers, TargetingScheduleReport, TargetingSession,
};
use cs_app::weapons::SessionDamageResolver;
use cs_sim::damage::{
    ActorId, AttributionRule, DamageGraph, DamageNode, DamageNodeKey, DamageNodeKind, DamagePolicy,
    LifecycleKind,
};
use cs_sim::targeting::{SelectionClearReason, TargetClass, TargetRecord};
use cs_sim::time::TickRate;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

/// The session generation every actor and authority shares.
const SESSION: u64 = 7;
/// The synthetic ground target's serial within the session.
const TARGET_SERIAL: u64 = 40;
/// How far ahead of the target the player parks; the convoy fixtures'
/// standoff (BUG-M01-CONVOY-GUN-HITS, #1998).
const STANDOFF_M: f32 = 80.0;
/// The frame budget the destruction loop gets — the convoy test's own.
const MAX_FRAMES: usize = 400;

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn player() -> ActorId {
    ActorId {
        session: session_id(),
        serial: cs_app::mission_session::MISSION_PLAYER_SERIAL,
    }
}

fn claim() -> ClaimId {
    ClaimId::new("f30-g-mission-schedule.accept-test").expect("claim")
}

/// The app a composed mission session runs its targeting under: the physics
/// adapter the fixed tick counts from, the F27/F29 schedules, and
/// [`MissionWeaponsPlugin`] — which installs the [`TargetingSchedulePlugin`]
/// under test, publishes the frame's [`TargetingFrameInputs`] in `PreUpdate`
/// and mirrors the tick's resolved damage into [`PendingTargetDamage`].
fn mission_app() -> App {
    let mut app = cs_app::asset_stack::headless_app();
    app.add_plugins(PhysicsAdapterPlugin::new(BASELINE_FIXED_HZ));
    app.add_plugins((
        cs_app::weapons::WeaponSchedulePlugin,
        cs_app::damage::DamageSchedulePlugin,
        MissionWeaponsPlugin,
    ));
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(1.0 / 60.0),
    ));
    // The scene generation the session's bindings stamp, minted from the
    // resource the schedule's generation checks read.
    let mut generations = SceneGenerations::default();
    let generation = generations.take_next();
    app.insert_resource(generations);
    // The session the composition would open at PostStartup — installed
    // directly here, with the player's live body and rig standing in for
    // the spawn the stage's Startup would run.
    open_mission_weapons(
        session_id(),
        TickRate::new(BASELINE_FIXED_HZ).expect("the baseline rate is nonzero"),
        generation,
    )
    .expect("the designed session opens")
    .install(app.world_mut())
    .expect("the session installs");
    let body = app
        .world_mut()
        .spawn((
            MissionPlayerBody,
            Transform::default(),
            GlobalTransform::default(),
            Position::default(),
            Rotation::default(),
            LinearVelocity::default(),
            AngularVelocity::default(),
        ))
        .id();
    attach_player_rig(app.world_mut(), body).expect("the player's rig attaches");
    // The scene camera the crosshair ray reads: the body pose the publisher
    // frames from.
    app.world_mut().spawn((
        Camera3d::default(),
        Transform::default(),
        GlobalTransform::default(),
    ));
    app.finish();
    app.cleanup();
    app
}

/// Delivers the frame's cockpit edges through the same resource the boundary
/// writes — the seam the session's own publishers read.
fn deliver(app: &mut App, actions: &[FlightCommand]) {
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: actions.iter().copied().map(Action::Flight).collect(),
        ..MissionCockpitActions::default()
    });
    app.update();
}

/// A synthetic damageable ground target the designed guns can destroy:
/// registered with the shared resolver, the targeting store and the sweep
/// the F27 step's rounds test — the same fixture shape the convoy tests
/// build (BUG-M01-CONVOY-GUN-HITS, #1998).
fn synthetic_ground_target(app: &mut App, centre: Vec3) -> ActorId {
    let generation = app.world().resource::<SceneGenerations>().latest();
    let actor = ActorId {
        session: session_id(),
        serial: TARGET_SERIAL,
    };
    let node = DamageNode::new(
        DamageNodeKey::new("t_truck_syn").expect("a damage node key"),
        DamageNodeKind::InternalStructure,
        Resolved::Known(Known::new(
            cs_app::mission_targets::DESIGNED_TARGET_INTEGRITY,
            Provenance::designed(claim()),
        )),
    )
    .with_lethal(true);
    let subject = ContentId::from_source(ContentKind::Blueprint, "world-target.t_truck_syn")
        .expect("subject");
    let graph = DamageGraph::try_new(subject.clone(), vec![node]).expect("graph");
    app.world_mut()
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .register_actor(
            actor,
            graph,
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("registers");
    let position = [
        f64::from(centre.x),
        f64::from(centre.y),
        f64::from(centre.z),
    ];
    app.world_mut()
        .resource_mut::<TargetingSession>()
        .store_mut()
        .register(TargetRecord {
            actor,
            faction: ContentId::from_source(ContentKind::Faction, MISSION_FACTION_HOSTILE)
                .expect("faction"),
            class: TargetClass::WorldObject,
            objective: true,
            revealed: true,
            phase_eligible: true,
            position: WorldPosition::try_new(position).expect("finite"),
        })
        .expect("rosters");
    let (bundle, _) = ground_target_sweep(
        actor,
        DamageNodeKey::new("t_truck_syn").expect("a damage node key"),
        position,
        [0.0; 3],
        position,
    );
    app.world_mut().spawn((
        Transform::from_translation(centre),
        bundle,
        DamageActorBinding {
            actor,
            graph: subject,
            generation,
        },
    ));
    actor
}

/// The world point the player's body sits at.
fn player_position(app: &mut App) -> Vec3 {
    let mut query = app
        .world_mut()
        .query_filtered::<&Transform, bevy::prelude::With<MissionPlayerBody>>();
    query
        .iter(app.world())
        .next()
        .expect("the player body exists")
        .translation
}

/// Parks the player `STANDOFF_M` short of `point`, nose on it, at rest — the
/// convoy tests' aim, so the designed guns' swept rounds cross the target's
/// box.
fn aim_player_at(app: &mut App, point: Vec3, from_side: Vec3) {
    let player_entity = {
        let mut query = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<MissionPlayerBody>>();
        query
            .iter(app.world())
            .next()
            .expect("the player body exists")
    };
    let eye = point + from_side.normalize() * STANDOFF_M;
    let look = Transform::from_translation(eye).looking_at(point, Vec3::Y);
    let mut entity = app.world_mut().entity_mut(player_entity);
    entity.insert((
        look,
        GlobalTransform::from(look),
        Position(eye),
        Rotation(look.rotation),
        LinearVelocity(Vec3::ZERO),
        AngularVelocity(Vec3::ZERO),
    ));
    // The scene camera rides the same pose, so the crosshair ray the frame
    // publisher reads points where the player aims.
    let camera = {
        let mut query = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<Camera3d>>();
        query
            .iter(app.world())
            .next()
            .expect("the scene camera exists")
    };
    let mut entity = app.world_mut().entity_mut(camera);
    entity.insert((look, GlobalTransform::from(look)));
}

/// The installed session's report for the last rendered frame.
fn targeting_report(app: &App) -> TargetingScheduleReport {
    app.world().resource::<TargetingScheduleReport>().clone()
}

/// **The schedule the mission session installs publishes the views every
/// rendered frame — and a delivered selection edge lands through the frame's
/// own [`TargetingFrameInputs`].**
///
/// No test touches [`TargetingFrameInputs`]: the frame's presses reach the
/// selection pass only because `publish_mission_targeting_frame` carried
/// them there, and the view the consumer pass publishes names the target the
/// edge selected.
#[test]
fn accept_f30_g_mission_frame_publishes_views_and_edges_reach_selection() {
    let mut app = mission_app();

    // Frame one: the plugin the mission session installs ran the whole pass
    // set on inputs the session's own publisher minted — nothing external
    // fed it.
    app.update();
    {
        let report = targeting_report(&app);
        assert_eq!(report.frames, 1, "the schedule ran once per frame");
        assert_eq!(report.session, Some(session_id()));
        assert!(
            !report.missing_inputs,
            "the session published this frame's inputs itself"
        );
        let consumers = app.world().resource::<TargetConsumers>();
        assert_eq!(
            consumers.bound().map(|binding| binding.observer),
            Some(player()),
            "the consumer pass bound the views to the mission's observer"
        );
        assert!(report.consumer_error.is_none());
        assert!(report.selection_error.is_none());
        assert!(report.damage_error.is_none());
    }

    // A hostile target the roster's own record names; parked ahead of the
    // player's spawn inside the reticule's cone.
    let centre = player_position(&mut app) + Vec3::new(0.0, 0.0, -120.0);
    let actor = synthetic_ground_target(&mut app, centre);
    aim_player_at(&mut app, centre, Vec3::new(1.0, 0.0, 0.3));

    // The delivered edge is the measured `Q` — the under-reticule select —
    // which reaches the schedule only through the frame inputs the mission
    // publishes: the store's own record of a selection acting on the
    // crosshair ray `mission_crosshair` produced.
    deliver(&mut app, &[FlightCommand::TargetUnderReticule]);
    {
        let report = targeting_report(&app);
        let selection = report.selection.clone().expect("the edge pass ran");
        assert_eq!(
            selection.selection,
            Some(actor),
            "the frame's own inputs carried the under-reticule edge: \
             {selection:?}"
        );
        let hud = app
            .world()
            .resource::<TargetConsumers>()
            .hud()
            .expect("a hud view")
            .clone();
        assert_eq!(
            hud.reticle.map(|reticle| reticle.target),
            Some(actor),
            "the published view frames what the frame's edge selected"
        );
        assert!(
            app.world()
                .resource::<TargetConsumers>()
                .spyglass()
                .expect("a spyglass view")
                .has_target()
        );
        assert!(
            app.world()
                .resource::<TargetConsumers>()
                .guidance()
                .expect("a guidance view")
                .has_aid()
        );
    }
    // The fixed tick of the same frame drained the shared queue and reset
    // the delivery cursor with it: the edge reached the selection pass and
    // the weapon publisher exactly once each.
    let pending = app.world().resource::<MissionCockpitActions>();
    assert!(
        pending.actions.is_empty(),
        "the first fixed tick of the frame took the edge, as designed"
    );
    assert_eq!(
        pending.targeting_published, 0,
        "the drain reset the cursor the frame's publisher left behind"
    );
}

/// **A target the tick's own resolution destroyed is absent from the views
/// the same rendered frame publishes.**
///
/// The chain under test is the mission session's own: the delivered fire
/// edge mints the tick's weapon input, `drive_weapon_session` resolves the
/// rounds through the shared resolver, `publish_mission_target_damage`
/// mirrors the resolution into `PendingTargetDamage`, and the installed
/// schedule's `Update` pass records the destruction before the consumer pass
/// publishes — so the frame's views already say the wreck is gone.
#[test]
fn accept_f30_g_target_destroyed_mid_tick_is_absent_from_views() {
    let mut app = mission_app();
    app.update();

    let centre = player_position(&mut app) + Vec3::new(0.0, 0.0, -120.0);
    let actor = synthetic_ground_target(&mut app, centre);

    // Select it through the frame path, the way the destruction test needs
    // a held selection to clear.
    aim_player_at(&mut app, centre, Vec3::new(1.0, 0.0, 0.3));
    deliver(&mut app, &[FlightCommand::TargetUnderReticule]);
    assert_eq!(
        app.world()
            .resource::<TargetingSession>()
            .selection()
            .current(),
        Some(actor),
        "the frame's select landed"
    );

    // Fire until the resolver records the destruction; every frame the loop
    // runs is a rendered frame the schedule published views for.
    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        aim_player_at(&mut app, centre, Vec3::new(1.0, 0.0, 0.3));
        deliver(&mut app, &[FlightCommand::FirePrimary]);
        if app
            .world()
            .resource::<SessionDamageResolver>()
            .resolver()
            .is_destroyed(&actor)
        {
            killed = true;
            break;
        }
    }
    assert!(killed, "the designed guns destroyed the synthetic target");

    // The frame that resolved the destruction already published the views:
    // the wreck is gone from all three, and the clear names the damage
    // system's own reason.
    {
        let consumers = app.world().resource::<TargetConsumers>();
        let hud = consumers.hud().expect("a hud view");
        assert_eq!(
            hud.reticle, None,
            "a target destroyed this frame is not framed by it"
        );
        assert_eq!(
            hud.cleared,
            Some(ClearedTarget {
                actor,
                reason: SelectionClearReason::Ended(LifecycleKind::Destroyed),
            }),
            "the view says who went and why"
        );
        assert!(!consumers.spyglass().expect("a spyglass view").has_target());
        assert!(!consumers.guidance().expect("a guidance view").has_aid());
    }
    {
        let report = targeting_report(&app);
        assert!(
            report
                .damage
                .as_ref()
                .is_some_and(|damage| damage.lifecycle == 1),
            "the schedule's damage pass recorded the resolver's own \
             transition this frame: {report:?}"
        );
        assert!(report.damage_error.is_none());
        assert!(report.consumer_error.is_none());
    }
    assert!(
        app.world().get_resource::<PendingTargetDamage>().is_none(),
        "the batch was consumed exactly once, not replayed next frame"
    );

    // And the next frame reports nothing stale: the destroyed actor stays
    // gone and nothing is re-cleared.
    app.update();
    {
        let consumers = app.world().resource::<TargetConsumers>();
        let hud = consumers.hud().expect("a hud view");
        assert_eq!(hud.reticle, None);
        assert_eq!(hud.cleared, None, "nothing was cleared this frame");
    }
}
