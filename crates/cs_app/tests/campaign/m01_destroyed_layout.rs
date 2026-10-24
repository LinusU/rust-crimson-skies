//! Rally #2054 (`M01 destroyed convoy trucks stay drawn whole`): a destroyed
//! ground target presents its authored `destroyed` layout and its intact one
//! stops drawing — the layout swap #2047's convoy-kill smoke found missing.
//! Task test prefix: `accept_m01_destroyed_layout_`.
//!
//! The synthetic tests drive the production pass in a small app; the retail
//! tests need `CS_GAME_DIR` and read the bound slots or drive the composed
//! headless stage.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::prelude::{App, GlobalTransform, Transform, Vec3, Visibility};
use cs_app::damage::DamageScheduleReport;
use cs_app::mission_actors::{
    MissionGroundTarget, MissionGroundTargetReport, MissionGroundTargetSweep,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    GroundTargetLayoutReport, MissionHost, PresentedLayout, WorldSceneryNode,
    apply_ground_target_layout, build_headless, stage_for, teardown,
};
use cs_app::mission_targets::{DESIGNED_TARGET_INTEGRITY, bind_mission_targets};
use cs_app::weapons::{PartSweptBox, SessionDamageResolver};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageGraph, DamageNode, DamageNodeKey,
    DamageNodeKind, DamagePolicy, DamageResolver, HitEvent, HitEventId,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

const TRUCKS: [&str; 4] = ["t_truck01", "t_truck02", "t_truck03", "bridge_truck01"];
const MAX_FRAMES: usize = 400;
const STANDOFF_M: f32 = 80.0;

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("a damage node key")
}

fn claim() -> ClaimId {
    ClaimId::new("m01-destroyed-layout.accept-test").expect("claim")
}

/// The visible state of every drawn node of `target`'s presented sets, as
/// `(intact shown, intact hidden, destroyed shown, destroyed hidden)` — the
/// entities' own `Visibility`, never the pass's self-report.
fn visible_node_state(
    app: &mut App,
    intact: &[u32],
    destroyed: &[u32],
) -> (usize, usize, usize, usize) {
    let mut state = (0, 0, 0, 0);
    for (node, visibility) in app
        .world_mut()
        .query::<(&WorldSceneryNode, &Visibility)>()
        .iter(app.world())
    {
        let shown = *visibility != Visibility::Hidden;
        if intact.contains(&node.node_slot) {
            if shown {
                state.0 += 1;
            } else {
                state.1 += 1;
            }
        } else if destroyed.contains(&node.node_slot) {
            if shown {
                state.2 += 1;
            } else {
                state.3 += 1;
            }
        }
    }
    state
}

/// A `MissionGroundTarget` for the synthetic app: `intact`/`destroyed` are
/// the stored slots each presentation owns.
fn ground_target(
    actor: ActorId,
    node: &str,
    intact: Vec<u32>,
    destroyed: Vec<u32>,
) -> MissionGroundTarget {
    MissionGroundTarget {
        node: node.to_owned(),
        declared: Some(0),
        damage_actor: actor,
        generation: cs_app::scene::SceneGeneration::default(),
        position_m: [0.0; 3],
        target_point_m: [0.0; 3],
        healthy: None,
        destroyed: None,
        intact_slots: intact,
        destroyed_slots: destroyed,
        coupling: Vec::new(),
        objective: false,
        other_target: false,
        description: None,
        help_label: None,
        roster: None,
        allegiance_open: None,
    }
}

/// The synthetic pass app: a session resolver owning one lethal-node actor
/// `t_truck_syn`, the ground target carrying its presented slots, and the
/// drawn scenery nodes — all `Visibility::Inherited`, as
/// `attach_world_scenery` spawns them.
fn layout_app(intact: Vec<u32>, destroyed: Vec<u32>) -> (App, ActorId) {
    let session = cs_types::net::SessionId::new(1).expect("nonzero");
    let actor = ActorId { session, serial: 7 };
    let mut resolver = DamageResolver::new(session, 0);
    let node = DamageNode::new(
        key("t_truck_syn"),
        DamageNodeKind::InternalStructure,
        Resolved::Known(Known::new(
            DESIGNED_TARGET_INTEGRITY,
            Provenance::designed(claim()),
        )),
    )
    .with_lethal(true);
    let subject = ContentId::from_source(ContentKind::Blueprint, "world-target.t_truck_syn")
        .expect("subject");
    resolver
        .register_actor(
            actor,
            DamageGraph::try_new(subject, vec![node]).expect("graph"),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the actor registers");
    let mut app = App::new();
    app.insert_resource(SessionDamageResolver::new(resolver))
        .init_resource::<GroundTargetLayoutReport>()
        .add_systems(bevy::prelude::Update, apply_ground_target_layout);
    app.world_mut().spawn(ground_target(
        actor,
        "t_truck_syn",
        intact.clone(),
        destroyed.clone(),
    ));
    for slot in intact.iter().chain(destroyed.iter()) {
        app.world_mut()
            .spawn((WorldSceneryNode { node_slot: *slot }, Visibility::Inherited));
    }
    (app, actor)
}

/// Destroys `actor`'s only node through the session damage authority's own
/// resolve — the same write the damage schedule runs.
fn destroy(app: &mut App, actor: ActorId, node: &str) {
    let session = actor.session;
    let tick = Tick(0);
    let hit = HitEvent::try_new(
        HitEventId {
            session,
            tick,
            producer: 0,
            sequence: 0,
        },
        None,
        actor,
        key(node),
        DamageChannel::Internal,
        DESIGNED_TARGET_INTEGRITY,
    )
    .expect("a hit event");
    app.world_mut()
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .resolve(tick, &[hit])
        .expect("the hit resolves");
}

/// The composed-app convenience: `entity`'s stored slot sets and the drawn
/// state they measure, in one borrow.
fn target_state(
    app: &mut App,
    entity: bevy::prelude::Entity,
) -> ((usize, usize, usize, usize), Vec<u32>, Vec<u32>) {
    let (intact, destroyed) = {
        let target = app
            .world()
            .get::<MissionGroundTarget>(entity)
            .expect("the ground target component");
        (target.intact_slots.clone(), target.destroyed_slots.clone())
    };
    let state = visible_node_state(app, &intact, &destroyed);
    (state, intact, destroyed)
}

fn visibility_of(app: &mut App, slot: u32) -> Visibility {
    *app.world_mut()
        .query::<(&WorldSceneryNode, &Visibility)>()
        .iter(app.world())
        .find(|(node, _)| node.node_slot == slot)
        .map(|(_, visibility)| visibility)
        .unwrap_or_else(|| panic!("no drawn node for slot {slot}"))
}

/// **The pass swaps the drawn nodes of a destroyed target to its authored
/// `destroyed` layout.**
///
/// Drives the production `apply_ground_target_layout` against a resolver
/// whose node flips destroyed through `resolve` — the authority's own write
/// — and reads the entities' `Visibility`, never the pass's self-report.
#[test]
fn accept_m01_destroyed_layout_the_pass_swaps_the_drawn_node_state() {
    let (mut app, actor) = layout_app(vec![11, 12], vec![13]);
    app.update();
    assert_eq!(visibility_of(&mut app, 11), Visibility::Inherited);
    assert_eq!(visibility_of(&mut app, 12), Visibility::Inherited);
    assert_eq!(
        visibility_of(&mut app, 13),
        Visibility::Hidden,
        "the authored initial state keeps the destroyed layout hidden"
    );
    assert_eq!(
        app.world().resource::<GroundTargetLayoutReport>().rows["t_truck_syn"].presented,
        PresentedLayout::Intact
    );

    destroy(&mut app, actor, "t_truck_syn");
    app.update();
    assert_eq!(
        visibility_of(&mut app, 11),
        Visibility::Hidden,
        "the intact presentation stops drawing"
    );
    assert_eq!(visibility_of(&mut app, 12), Visibility::Hidden);
    assert_eq!(
        visibility_of(&mut app, 13),
        Visibility::Inherited,
        "the destroyed layout draws"
    );
    let row = &app.world().resource::<GroundTargetLayoutReport>().rows["t_truck_syn"];
    assert_eq!(row.presented, PresentedLayout::Destroyed);
    assert_eq!(row.destroyed_shown, 1);
    assert_eq!(row.intact_hidden, 2);

    // Convergent: a second run asserts the same state.
    app.update();
    assert_eq!(visibility_of(&mut app, 11), Visibility::Hidden);
    assert_eq!(visibility_of(&mut app, 13), Visibility::Inherited);
}

/// **A destroyed target with no authored `destroyed` child stops drawing:**
/// the pass asserts nothing where the original's presentation is unmeasured.
#[test]
fn accept_m01_destroyed_layout_no_destroyed_child_stops_drawing() {
    let (mut app, actor) = layout_app(vec![11, 12], Vec::new());
    app.update();
    destroy(&mut app, actor, "t_truck_syn");
    app.update();
    assert_eq!(visibility_of(&mut app, 11), Visibility::Hidden);
    assert_eq!(visibility_of(&mut app, 12), Visibility::Hidden);
    assert_eq!(
        app.world().resource::<GroundTargetLayoutReport>().rows["t_truck_syn"].presented,
        PresentedLayout::Removed
    );
}

/// **The retail binding gives every convoy truck its own presented slots,
/// and a nested target's intact set never covers a live truck's.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_destroyed_layout_retail_bound_slots_keep_nested_targets_separate() {
    let root = game_dir();
    let binding = bind_mission_targets(&root, "zbd/c3/m01", "zbd/c3");
    let target = |node: &str| {
        binding
            .targets
            .iter()
            .find(|target| target.node == node)
            .unwrap_or_else(|| panic!("{node} bound"))
    };

    for node in TRUCKS {
        let bound = target(node);
        assert!(
            !bound.intact_slots.is_empty(),
            "{node}: its intact presentation has drawn slots"
        );
        assert!(
            bound
                .intact_slots
                .iter()
                .all(|slot| !bound.destroyed_slots.contains(slot)),
            "{node}: the intact and destroyed sets are disjoint: {:?} vs {:?}",
            bound.intact_slots,
            bound.destroyed_slots
        );
    }
    // The authored wreck layouts: the three convoy trucks carry a
    // `destroyed` child; the tanker does not — its wreck presentation is the
    // unmeasured `bridge_destroy01` animation, and the slot set is honestly
    // empty rather than guessed.
    for node in ["t_truck01", "t_truck02", "t_truck03"] {
        assert!(
            !target(node).destroyed_slots.is_empty(),
            "{node}: the authored destroyed layout has slots"
        );
    }
    assert!(
        target("bridge_truck01").destroyed_slots.is_empty(),
        "bridge_truck01 stores no destroyed layout"
    );

    // Nesting: the trucks ride inside `susp_bridge`'s subtree, so the
    // bridge's intact set must own none of a truck's slots — destroying the
    // bridge must not hide a truck the authority holds in play.
    let bridge = target("susp_bridge");
    for node in TRUCKS {
        let truck = target(node);
        for slot in truck.intact_slots.iter().chain(&truck.destroyed_slots) {
            assert!(
                !bridge.intact_slots.contains(slot),
                "susp_bridge's intact set contains {node}'s slot {slot}"
            );
        }
    }
}

/// **On the composed retail stage a destroyed convoy truck draws its wreck
/// and its intact layout is gone.**
///
/// This drives the same headless composition the mission runs: real bound
/// slots, real spawned `WorldSceneryNode` entities and the real damage
/// authority — the truck is destroyed through `resolve`, the schedule's own
/// write path, so a swap that never ran still shows the intact truck.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_destroyed_layout_retail_a_destroyed_truck_draws_its_wreck() {
    let root = game_dir();
    let plan =
        plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("M01's launch plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();

    // The authored initial layout: intact drawn, destroyed hidden.
    for node in TRUCKS {
        let entity = app
            .world()
            .resource::<MissionGroundTargetReport>()
            .target(node)
            .unwrap_or_else(|| panic!("{node} spawned"))
            .entity;
        let ((shown, hidden, wreck_shown, wreck_hidden), _, _) = target_state(&mut app, entity);
        assert!(
            shown > 0 && hidden == 0 && wreck_shown == 0,
            "{node} begins drawn intact: {shown} shown, {hidden} hidden, wreck {wreck_shown} shown, {wreck_hidden} hidden"
        );
    }

    let session = app.world().resource::<MissionHost>().served();
    for node in ["t_truck01", "bridge_truck01"] {
        let bound = app
            .world()
            .resource::<MissionGroundTargetReport>()
            .target(node)
            .unwrap_or_else(|| panic!("{node} spawned"))
            .clone();
        let tick = Tick(0);
        let hit = HitEvent::try_new(
            HitEventId {
                session,
                tick,
                producer: 0,
                sequence: 0,
            },
            None,
            bound.actor,
            key(node),
            DamageChannel::Internal,
            DESIGNED_TARGET_INTEGRITY,
        )
        .expect("a hit event");
        app.world_mut()
            .resource_mut::<SessionDamageResolver>()
            .resolver_mut()
            .resolve(tick, &[hit])
            .expect("the hit resolves");
        app.update();

        let entity = bound.entity;
        let ((shown, hidden, wreck_shown, wreck_hidden), _, destroyed_slots) =
            target_state(&mut app, entity);
        assert_eq!(shown, 0, "{node}: no intact node keeps drawing");
        assert!(hidden > 0, "{node}: the intact nodes are hidden");
        if destroyed_slots.is_empty() {
            assert_eq!(
                (wreck_shown, wreck_hidden),
                (0, 0),
                "{node} authors no destroyed layout: nothing is drawn instead of being invented"
            );
        } else {
            assert!(
                wreck_shown > 0 && wreck_hidden == 0,
                "{node}: the wreck layout is fully drawn"
            );
        }
    }

    // The still-standing trucks keep their intact presentation.
    for node in ["t_truck02", "t_truck03"] {
        let entity = app
            .world()
            .resource::<MissionGroundTargetReport>()
            .target(node)
            .unwrap_or_else(|| panic!("{node} spawned"))
            .entity;
        let ((shown, hidden, _, _), _, _) = target_state(&mut app, entity);
        assert!(
            shown > 0 && hidden == 0,
            "{node} still stands: {shown} shown, {hidden} hidden"
        );
    }

    teardown(&mut app);
}

/// Parks the player `STANDOFF_M` short of `point`, nose on it, at rest —
/// the same shape the convoy-kill smoke places.
fn aim_player_at(app: &mut App, point: Vec3, from_side: Vec3) {
    let player = app
        .world()
        .resource::<cs_app::mission_session::MissionComposition>()
        .player;
    let eye = point + from_side.normalize() * STANDOFF_M;
    let look = Transform::from_translation(eye).looking_at(point, Vec3::Y);
    let mut entity = app.world_mut().entity_mut(player);
    entity.insert((
        look,
        GlobalTransform::from(look),
        Position(eye),
        Rotation(look.rotation),
        LinearVelocity(Vec3::ZERO),
        AngularVelocity(Vec3::ZERO),
    ));
}

fn fire(app: &mut App) {
    use cs_app::mission_session::MissionCockpitActions;
    use cs_types::input::{Action, FlightCommand};
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: vec![Action::Flight(FlightCommand::FirePrimary)],
    });
    app.update();
}

fn idle(app: &mut App) {
    use cs_app::mission_session::MissionCockpitActions;
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: Vec::new(),
    });
    app.update();
}

/// **On the composed retail stage the fire key's own destruction draws the
/// wreck** — the delivery path the smoke measures, driven end to end.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_destroyed_layout_retail_the_fire_key_swaps_the_drawn_layout() {
    let root = game_dir();
    let plan =
        plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("M01's launch plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    idle(&mut app);

    let bound = app
        .world()
        .resource::<MissionGroundTargetReport>()
        .target("t_truck01")
        .expect("t_truck01 spawned")
        .clone();
    let (sweep, box_node) = {
        let e = app.world().entity(bound.entity);
        (
            e.get::<MissionGroundTargetSweep>()
                .expect("the sweep record")
                .centre_m,
            e.get::<PartSweptBox>().expect("the swept box").node.clone(),
        )
    };
    assert_eq!(box_node, key("t_truck01"));

    let point = Vec3::new(sweep[0] as f32, sweep[1] as f32, sweep[2] as f32);
    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        aim_player_at(&mut app, point, Vec3::new(1.0, 0.15, 0.3));
        fire(&mut app);
        if app
            .world()
            .get_resource::<DamageScheduleReport>()
            .is_some_and(|report| report.destroyed.contains(&bound.actor))
        {
            killed = true;
            break;
        }
    }
    assert!(killed, "the fire key did not destroy t_truck01");

    app.update();
    let ((shown, hidden, wreck_shown, wreck_hidden), _, _) = target_state(&mut app, bound.entity);
    assert_eq!(shown, 0, "no intact node keeps drawing");
    assert!(hidden > 0, "the intact nodes are hidden");
    assert!(
        wreck_shown > 0 && wreck_hidden == 0,
        "the wreck layout is fully drawn"
    );

    teardown(&mut app);
}
