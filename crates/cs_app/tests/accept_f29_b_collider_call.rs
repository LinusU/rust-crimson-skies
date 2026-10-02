//! Acceptance scenarios for the F29 damage → collider call: a destroyed damage
//! zone loses its collider, a repair gives it back, and the animation side
//! cannot take the decision back.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stages
//! `### F29-B`/`### F29-C`. Task: "F29 damage zones decide and call
//! `remove_collider_for_damage` / `restore_collider_after_repair`" (#511).
//! Task test prefix: `accept_f29_b_collider_`.
//!
//! # What these tests observe
//!
//! The damage zone is resolved by the **real** session resolver
//! ([`DamageResolver`] with the declared synthetic airframe graph), the
//! transition it emits is handed to F29's own entry
//! ([`apply_damage_events`] / [`repair_damage_zone`]), and the result is read
//! off the **collision** channel: the production contact reporter
//! ([`ContactReports`]) and Avian's own `ColliderDisabled` marker. A zone whose
//! collider left the simulation lets a body fly through it; a zone whose
//! collider returned stops one.
//!
//! Nothing here writes the collision record by hand: the test never calls
//! `remove_collider_for_damage` directly, so a bridge that decided nothing — or
//! decided from the wrong record — fails at the contact assertion.
//!
//! # Designed rule, synthetic data
//!
//! Whether an original destroyed part loses its collider is **unmeasured**
//! (F29-D/F20-D keep the gate). The rule under test is this engine's designed
//! behavior, recorded in
//! `docs/findings/2026-10-02-f29-damage-zone-collider-call.md`. Every value is
//! newly authored fixture content.

use std::time::Duration;

use avian3d::prelude::{ColliderDisabled, Gravity, Position};
use bevy::prelude::App;
use bevy::time::{Real, Time, TimeUpdateStrategy};
use cs_app::animation::{
    AnimationInstance, AnimationPlayback, AnimationSchedulePlugin, CommittedSessionTick,
    bind_animated_node,
};
use cs_app::asset_stack::headless_app;
use cs_app::damage::{
    DamageColliderEvent, DamageColliderLog, DamageColliderReport, DamageZoneBinding,
    ZoneColliderDecision, apply_damage_events, repair_damage_zone, zone_collider_decision,
};
use cs_app::physics::{
    BASELINE_FIXED_HZ, BodyMode, BodySpec, ColliderPresencePlugin, ContactReports,
    NodeColliderPresence, PhysicsAdapterPlugin, PhysicsBodiesPlugin, spawn_body,
};
use cs_app::scene::SceneGeneration;
use cs_content::animation::{
    SYNTHETIC_BREAKABLE_DURATION, SYNTHETIC_BREAKABLE_HIDDEN_TICK, SYNTHETIC_BREAKABLE_NODE,
    SYNTHETIC_BREAKABLE_SHOWN_TICK, declared_synthetic_breakable_clip,
};
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageEvent, DamageEventKind, DamageNodeKey,
    DamagePolicy, DamageResolver, HitEvent, HitEventId, PartState, SYNTHETIC_ENGINE_INTEGRITY,
    SYNTHETIC_ENGINE_NODE, SYNTHETIC_MOUNT_INTEGRITY, SYNTHETIC_MOUNT_NODE,
    synthetic_airframe_graph,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

// --------------------------------------------------------------- constants ---

/// The session the resolver is bound to.
const SESSION: u64 = 41;
/// The zone's serial inside that session.
const ACTOR_SERIAL: u64 = 1;

/// The node under test: a static solid 1 m box at the origin on the
/// `static_world` layer, a real obstacle for a `projectile`-layer probe.
const NODE_HALF_M: f32 = 0.5;
/// The probe that flies at the node: 0.2 m across, 40 m/s.
const PROBE_HALF_M: f32 = 0.1;
const PROBE_SPEED_M_S: f32 = 40.0;
/// Where the probe starts, 3 m short of the node's near face.
const PROBE_START_X_M: f32 = -3.0;
/// Fixed ticks the probe is given to arrive.
const APPROACH_TICKS: u64 = 40;
/// The `x` past the node's far face: proof it went through.
const THROUGH_X_M: f32 = 1.5;

// ----------------------------------------------------------------- helpers ---

fn actor() -> ActorId {
    ActorId {
        session: session(SESSION),
        serial: ACTOR_SERIAL,
    }
}

fn mount() -> DamageNodeKey {
    key(SYNTHETIC_MOUNT_NODE)
}

fn engine() -> DamageNodeKey {
    key(SYNTHETIC_ENGINE_NODE)
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("fixture node keys are valid")
}

fn scene_node(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::SceneNode, key).expect("a valid node id")
}

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn instance() -> AnimationInstance {
    AnimationInstance::new(1).expect("a nonzero instance identity")
}

fn generation() -> SceneGeneration {
    SceneGeneration::default().next()
}

/// A resolver with the synthetic airframe registered under the declared rule.
fn resolver() -> DamageResolver {
    let mut resolver = DamageResolver::new(session(SESSION), 1);
    resolver
        .register_actor(
            actor(),
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the actor registers under its own session");
    resolver
}

/// Resolves one internal hit that exactly destroys `node`, returning the
/// resolution's events.
fn resolve_destroyed(
    resolver: &mut DamageResolver,
    node: &DamageNodeKey,
    integrity: f64,
) -> Vec<DamageEvent> {
    resolver
        .resolve(Tick(0), &[hit(node, integrity)])
        .expect("the resolver accepts the batch")
        .events
}

fn hit(node: &DamageNodeKey, damage: f64) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: session(SESSION),
            tick: Tick(0),
            producer: 1,
            sequence: 0,
        },
        None,
        actor(),
        node.clone(),
        DamageChannel::Internal,
        damage,
    )
    .expect("a finite, non-negative hit")
}

/// The static node the damage zone's collider lives on.
fn node_spec(x_m: f32) -> BodySpec {
    BodySpec {
        layer: CollisionLayer::StaticWorld,
        shape: ShapeClass::Solid,
        mode: BodyMode::Static,
        mass_kg: 0.0,
        half_extents_m: [NODE_HALF_M; 3],
        position_m: [x_m, 0.0, 0.0],
        linear_velocity_m_s: [0.0; 3],
    }
}

/// The probe: a dynamic `projectile`-layer body flying at the node.
fn probe_spec() -> BodySpec {
    BodySpec {
        layer: CollisionLayer::Projectile,
        shape: ShapeClass::Solid,
        mode: BodyMode::Dynamic,
        mass_kg: 1.0,
        half_extents_m: [PROBE_HALF_M; 3],
        position_m: [PROBE_START_X_M, 0.0, 0.0],
        linear_velocity_m_s: [PROBE_SPEED_M_S, 0.0, 0.0],
    }
}

/// The production fixed loop: real Avian, the physics adapters, the animation
/// schedule and the collision-presence pass, on a manual one-tick clock.
fn world_with_the_real_loop() -> App {
    let mut app = headless_app();
    app.add_plugins(PhysicsAdapterPlugin::new(BASELINE_FIXED_HZ));
    app.add_plugins(PhysicsBodiesPlugin);
    app.add_plugins(AnimationSchedulePlugin);
    app.add_plugins(ColliderPresencePlugin);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / BASELINE_FIXED_HZ as f64,
    )));
    app.insert_resource(Gravity::ZERO);
    let startup = app.world().resource::<Time<Real>>().startup();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .update_with_instant(startup);
    app.insert_resource(AnimationPlayback::new(session(SESSION)));
    app.finish();
    app.cleanup();
    app
}

/// Spawns the managed node: a real collider under the collision policy and the
/// damage zone its collider answers to.
///
/// When `animated`, the same entity is bound to the declared breakable clip on
/// its scene node, through the production spawn-side entry
/// [`bind_animated_node`], so the clip really drives its visibility.
fn spawn_zone_node(app: &mut App, at: SceneGeneration, animated: bool) -> bevy::prelude::Entity {
    let world = app.world_mut();
    let entity = spawn_body(world, &node_spec(0.0)).expect("a valid static body spec");
    world.entity_mut(entity).insert((
        NodeColliderPresence::Live,
        DamageZoneBinding::new(actor(), mount()),
    ));
    if animated {
        bind_animated_node(
            world,
            &declared_synthetic_breakable_clip(),
            entity,
            &scene_node(SYNTHETIC_BREAKABLE_NODE),
            instance(),
            at,
            Tick(0),
        )
        .expect("the declared clip drives the node");
    }
    entity
}

/// Spawns a real collider the spawn path never put under the collision policy,
/// but which carries a zone binding: the wiring-gap case.
fn spawn_unmanaged(app: &mut App) -> bevy::prelude::Entity {
    let world = app.world_mut();
    let entity = spawn_body(world, &node_spec(40.0)).expect("a valid static body spec");
    world
        .entity_mut(entity)
        .insert(DamageZoneBinding::new(actor(), mount()));
    entity
}

fn spawn_probe(app: &mut App) -> bevy::prelude::Entity {
    spawn_body(app.world_mut(), &probe_spec()).expect("a valid dynamic body spec")
}

fn retire(app: &mut App, probe: bevy::prelude::Entity) {
    app.world_mut().entity_mut(probe).despawn();
}

fn commit_and_step(app: &mut App, tick: u64) {
    app.world_mut()
        .insert_resource(CommittedSessionTick::new(Tick(tick)));
    app.update();
}

fn presence(app: &App, entity: bevy::prelude::Entity) -> Option<NodeColliderPresence> {
    app.world().get::<NodeColliderPresence>(entity).copied()
}

fn engine_disabled(app: &App, entity: bevy::prelude::Entity) -> bool {
    app.world().get::<ColliderDisabled>(entity).is_some()
}

fn position_x(app: &App, entity: bevy::prelude::Entity) -> f32 {
    app.world()
        .get::<Position>(entity)
        .expect("a spawned body carries a Position")
        .0
        .x
}

fn contact_this_tick(app: &App, probe: bevy::prelude::Entity, node: bevy::prelude::Entity) -> bool {
    app.world()
        .resource::<ContactReports>()
        .reports()
        .iter()
        .any(|report| report.bodies.contains(&probe) && report.bodies.contains(&node))
}

/// Holds the committed tick and runs `ticks` fixed steps; reports whether the
/// probe and the node were ever in contact.
fn run_ticks(
    app: &mut App,
    tick: u64,
    ticks: u64,
    probe: bevy::prelude::Entity,
    node: bevy::prelude::Entity,
) -> bool {
    let mut contacted = false;
    for _ in 0..ticks {
        commit_and_step(app, tick);
        contacted |= contact_this_tick(app, probe, node);
    }
    contacted
}

fn log(app: &App) -> DamageColliderLog {
    app.world()
        .get_resource::<DamageColliderLog>()
        .cloned()
        .unwrap_or_default()
}

fn last_log(app: &App) -> Option<DamageColliderEvent> {
    log(app).last().cloned()
}

// ---------------------------------------------- the destruction and repair ---

/// The minimum scenario on the collision channel: the real resolver destroys a
/// zone, F29's bridge decides the collider goes, and a repair brings it back.
///
/// The bridge is exercised through [`apply_damage_events`], never the seam
/// directly, so a bridge that resolved the wrong entity or decided nothing
/// fails at the contact assertion.
#[test]
fn accept_f29_b_collider_a_destroyed_zone_removes_the_collider_and_a_repair_restores_it() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let node = spawn_zone_node(&mut app, at, false);

    // The authored collider is live and stops a probe.
    commit_and_step(&mut app, 0);
    assert_eq!(presence(&app, node), Some(NodeColliderPresence::Live));
    assert!(!engine_disabled(&app, node));
    let live_probe = spawn_probe(&mut app);
    assert!(
        run_ticks(&mut app, 0, APPROACH_TICKS, live_probe, node),
        "the authored collider stops a body"
    );
    assert!(position_x(&app, live_probe) < THROUGH_X_M);
    retire(&mut app, live_probe);

    // The real resolver destroys the zone.
    let mut resolver = resolver();
    let events = resolve_destroyed(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    assert!(
        events.iter().any(|event| matches!(
            &event.kind,
            DamageEventKind::PartTransition {
                node,
                from: PartState::Intact,
                to: PartState::Destroyed,
            } if node == &mount()
        )),
        "the resolver emitted the zone's destruction: {events:?}"
    );
    let report = apply_damage_events(app.world_mut(), actor(), &events);
    assert_eq!(
        report,
        DamageColliderReport {
            removed: 1,
            ..Default::default()
        },
        "the bridge applied exactly the zone's removal"
    );
    assert_eq!(
        last_log(&app),
        Some(DamageColliderEvent::Removed {
            actor: actor(),
            node: mount(),
            entity: node,
        })
    );
    assert_eq!(
        presence(&app, node),
        Some(NodeColliderPresence::RemovedByDamage),
        "the seam records damage as the owner of the removal"
    );
    assert!(
        engine_disabled(&app, node),
        "Avian's own marker takes the collider out of the broad phase"
    );

    // A probe flies through: the removal is real collision state.
    let removed_probe = spawn_probe(&mut app);
    assert!(
        !run_ticks(&mut app, 0, APPROACH_TICKS, removed_probe, node),
        "a zone whose collider damage removed does not stop a body"
    );
    assert!(position_x(&app, removed_probe) > THROUGH_X_M);
    retire(&mut app, removed_probe);

    // The repair path, through F29's own entry.
    let repair = repair_damage_zone(app.world_mut(), actor(), &mount());
    assert_eq!(
        repair,
        DamageColliderReport {
            restored: 1,
            ..Default::default()
        }
    );
    assert_eq!(presence(&app, node), Some(NodeColliderPresence::Live));
    assert!(!engine_disabled(&app, node));
    let repaired_probe = spawn_probe(&mut app);
    assert!(
        run_ticks(&mut app, 0, APPROACH_TICKS, repaired_probe, node),
        "the repaired zone is an obstacle again"
    );
    assert!(position_x(&app, repaired_probe) < THROUGH_X_M);
}

/// The animation side cannot take the damage decision back: a clip that loops
/// and re-shows the node leaves the collider off.
///
/// This is the direction F20 non-negotiable behavior 3 protects, asserted from
/// F29's own tests rather than only through the seam.
#[test]
fn accept_f29_b_collider_a_looping_clip_cannot_restore_a_destroyed_zones_collider() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let node = spawn_zone_node(&mut app, at, true);

    // Drive the real clip: it shows the node first.
    commit_and_step(&mut app, 0);
    assert_eq!(presence(&app, node), Some(NodeColliderPresence::Live));

    let mut resolver = resolver();
    let events = resolve_destroyed(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    assert_eq!(
        apply_damage_events(app.world_mut(), actor(), &events).removed,
        1
    );

    // Two full clip passes. The loop really re-shows and re-hides the node, so
    // the collider staying off is the terminal damage removal, not an idle clip.
    let mut re_shown = 0u32;
    let mut re_hidden = 0u32;
    for tick in 1..=SYNTHETIC_BREAKABLE_DURATION * 2 {
        commit_and_step(&mut app, tick);
        let visibility = app
            .world()
            .get::<cs_app::animation::NodeAnimatedVisibility>(node)
            .map(cs_app::animation::NodeAnimatedVisibility::visibility);
        match visibility {
            Some(cs_sim::animated_object::Visibility::Visible) => re_shown += 1,
            Some(cs_sim::animated_object::Visibility::Hidden) => re_hidden += 1,
            None => {}
        }
        assert_eq!(
            presence(&app, node),
            Some(NodeColliderPresence::RemovedByDamage),
            "session tick {tick}: the damage removal is terminal for the clip"
        );
        assert!(engine_disabled(&app, node));
    }
    assert!(re_shown >= 2, "the clip really re-showed the node");
    assert!(re_hidden >= 2, "and really re-hid it");

    let probe = spawn_probe(&mut app);
    assert!(
        !run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_DURATION * 2,
            APPROACH_TICKS,
            probe,
            node
        ),
        "a destroyed zone's collider stays gone through the loop"
    );
    assert!(position_x(&app, probe) > THROUGH_X_M);
}

/// A repair under a clip that still hides the node does not expose it.
///
/// The other half of the merge: the repair hands the clip's current verdict to
/// the seam, so the collider stays off until the clip shows the node.
#[test]
fn accept_f29_b_collider_a_repair_under_a_still_hiding_clip_does_not_expose_the_node() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let node = spawn_zone_node(&mut app, at, true);

    // The clip hides the node at its authored tick.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        presence(&app, node),
        Some(NodeColliderPresence::HiddenByAnimation)
    );
    assert!(engine_disabled(&app, node));

    let mut resolver = resolver();
    let events = resolve_destroyed(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    assert_eq!(
        apply_damage_events(app.world_mut(), actor(), &events).removed,
        1
    );
    assert_eq!(
        presence(&app, node),
        Some(NodeColliderPresence::RemovedByDamage)
    );

    // Repair: the clip still hides the node, so the collider stays off.
    let repair = repair_damage_zone(app.world_mut(), actor(), &mount());
    assert_eq!(
        repair,
        DamageColliderReport {
            restored: 1,
            ..Default::default()
        },
        "the damage side lifted its own removal"
    );
    assert_eq!(
        presence(&app, node),
        Some(NodeColliderPresence::HiddenByAnimation),
        "the clip's own removal is what stands after the repair"
    );
    assert!(
        engine_disabled(&app, node),
        "a repair never exposes a hidden node"
    );

    let probe = spawn_probe(&mut app);
    assert!(
        !run_ticks(
            &mut app,
            SYNTHETIC_BREAKABLE_HIDDEN_TICK,
            APPROACH_TICKS,
            probe,
            node
        ),
        "still no contact: the hidden node is not an obstacle after a repair"
    );
    assert!(position_x(&app, probe) > THROUGH_X_M);
    retire(&mut app, probe);

    // The clip's show tick restores it, with no damage record in force.
    commit_and_step(&mut app, SYNTHETIC_BREAKABLE_SHOWN_TICK);
    assert_eq!(presence(&app, node), Some(NodeColliderPresence::Live));
    assert!(!engine_disabled(&app, node));
}

/// Applying the same transition twice changes nothing on the physics side.
#[test]
fn accept_f29_b_collider_applying_the_same_transition_twice_changes_nothing() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let node = spawn_zone_node(&mut app, at, false);
    commit_and_step(&mut app, 0);

    let mut resolver = resolver();
    let events = resolve_destroyed(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    assert_eq!(
        apply_damage_events(app.world_mut(), actor(), &events).removed,
        1
    );
    let after_removal = log(&app).len();
    let second = apply_damage_events(app.world_mut(), actor(), &events);
    assert!(
        second.is_noop(),
        "a repeated destruction is a no-op: {second:?}"
    );
    assert_eq!(
        log(&app).len(),
        after_removal,
        "and logs nothing: only a real state change is an event"
    );

    assert_eq!(
        repair_damage_zone(app.world_mut(), actor(), &mount()).restored,
        1
    );
    let after_repair = log(&app).len();
    let repeat = repair_damage_zone(app.world_mut(), actor(), &mount());
    assert!(repeat.is_noop(), "a repeated repair is a no-op: {repeat:?}");
    assert_eq!(log(&app).len(), after_repair);
    assert_eq!(presence(&app, node), Some(NodeColliderPresence::Live));
}

// ------------------------------------------------------ refusals and gaps ---

/// A zone the spawn path never bound, and a zone bound to an entity the
/// collision policy never managed, are both reported rather than swallowed.
#[test]
fn accept_f29_b_collider_an_unmanaged_zone_is_reported_not_swallowed() {
    let mut app = world_with_the_real_loop();
    let unmanaged = spawn_unmanaged(&mut app);

    // A hit on a zone no entity is bound to: the unbound case.
    let mut resolver = resolver();
    let unbound_events = resolve_destroyed(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let unbound = apply_damage_events(app.world_mut(), actor(), &unbound_events);
    assert_eq!(
        unbound,
        DamageColliderReport {
            unbound: 1,
            ..Default::default()
        }
    );
    assert_eq!(
        last_log(&app),
        Some(DamageColliderEvent::UnboundZone {
            actor: actor(),
            node: engine(),
        }),
        "a zone with no bound entity is reported, not dropped"
    );

    // A zone bound to a body outside the collision policy: the wiring gap.
    let events = resolve_destroyed(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    let report = apply_damage_events(app.world_mut(), actor(), &events);
    assert_eq!(
        report,
        DamageColliderReport {
            unmanaged: 1,
            ..Default::default()
        }
    );
    assert_eq!(
        last_log(&app),
        Some(DamageColliderEvent::UnmanagedNode {
            actor: actor(),
            node: mount(),
            entity: unmanaged,
        }),
        "an unmanaged node is reported through F29's own log"
    );
    assert!(
        app.world()
            .get::<avian3d::prelude::Collider>(unmanaged)
            .is_some(),
        "the refused decision did not touch the unmanaged collider"
    );
    assert!(
        !engine_disabled(&app, unmanaged),
        "and never wrote the engine's marker on a node it does not manage"
    );
}

/// The rule itself: only a transition into or out of destruction moves a
/// collider, and the report agrees with the log.
#[test]
fn accept_f29_b_collider_the_rule_ignores_non_destroying_transitions() {
    use ZoneColliderDecision::{Remove, Restore};

    assert_eq!(
        zone_collider_decision(PartState::Intact, PartState::Destroyed),
        Some(Remove)
    );
    assert_eq!(
        zone_collider_decision(PartState::Damaged, PartState::Destroyed),
        Some(Remove)
    );
    assert_eq!(
        zone_collider_decision(PartState::Destroyed, PartState::Intact),
        Some(Restore)
    );
    assert_eq!(
        zone_collider_decision(PartState::Destroyed, PartState::Damaged),
        Some(Restore)
    );
    for (from, to) in [
        (PartState::Intact, PartState::Damaged),
        (PartState::Damaged, PartState::Intact),
        (PartState::Unknown, PartState::Destroyed),
        (PartState::Destroyed, PartState::Unknown),
        (PartState::Destroyed, PartState::Destroyed),
        (PartState::Intact, PartState::Intact),
    ] {
        assert_eq!(
            zone_collider_decision(from, to),
            None,
            "{from:?} -> {to:?} must not move a collider"
        );
    }
}

/// A transition that moves nothing is reported as a no-op, quietly and
/// consistently across the log and the counters.
#[test]
fn accept_f29_b_collider_a_damaged_but_intact_zone_keeps_its_collider() {
    let at = generation();
    let mut app = world_with_the_real_loop();
    let node = spawn_zone_node(&mut app, at, false);
    commit_and_step(&mut app, 0);

    // Scratch the zone: a `Damaged` transition that is not a destruction.
    let mut resolver = resolver();
    let events = resolve_destroyed(&mut resolver, &mount(), 1.0);
    assert!(
        events.iter().any(|event| matches!(
            &event.kind,
            DamageEventKind::PartTransition {
                from: PartState::Intact,
                to: PartState::Damaged,
                ..
            }
        )),
        "the resolver reports the scratch: {events:?}"
    );
    let report = apply_damage_events(app.world_mut(), actor(), &events);
    assert!(
        report.is_noop(),
        "a zone that is only scratched keeps its collider: {report:?}"
    );
    assert!(log(&app).is_empty(), "and nothing is logged");
    assert_eq!(presence(&app, node), Some(NodeColliderPresence::Live));
    assert!(!engine_disabled(&app, node));

    let probe = spawn_probe(&mut app);
    assert!(
        run_ticks(&mut app, 0, APPROACH_TICKS, probe, node),
        "a damaged zone is still an obstacle"
    );
    assert!(position_x(&app, probe) < THROUGH_X_M);
}
