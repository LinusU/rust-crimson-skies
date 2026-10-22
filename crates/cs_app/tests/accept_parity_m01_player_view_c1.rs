//! Acceptance PARITY-M01-PLAYER-VIEW-C1 (#1682): the airframe damage-region
//! presentation hook. Task test prefix: `accept_parity_m01_player_view_c1_`.
//!
//! The synthetic scenarios drive the real [`DamageResolver`] through the
//! production lowering and read the records back off the entities
//! [`apply_region_damage_state`] published. The ignored retail test measures
//! what M01's player actually has in the session resolver; see
//! `docs/findings/2026-10-19-parity-m01-player-view-c1-wing-damage-hook.md`.
//! Synthetic data proves the pass, never the original's behaviour.

use bevy::prelude::{Entity, World};
use cs_app::airframe_damage::{
    AirframeDamageRegion, RegionDamageBinding, RegionDamagePresentation, apply_region_damage_state,
    release_region_damage,
};
use cs_app::damage::{lower_graph, lower_policy};
use cs_app::scene::SceneGeneration;
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_sim::damage::{
    ActorId, DamageChannel, DamageNodeKey, DamageResolver, HitEvent, HitEventId, PartState,
    SYNTHETIC_ENGINE_INTEGRITY, SYNTHETIC_ENGINE_NODE, SYNTHETIC_MOUNT_INTEGRITY,
    SYNTHETIC_MOUNT_NODE,
};
use cs_types::Tick;
use cs_types::net::SessionId;

const SESSION: u64 = 52;
const PRODUCER: u32 = 1;

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn actor() -> ActorId {
    ActorId {
        session: session(),
        serial: 1,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn generation() -> SceneGeneration {
    SceneGeneration::default().next()
}

fn registered() -> DamageResolver {
    let declared = declared_synthetic_airframe_damage();
    let graph = lower_graph(&declared).expect("the declared graph lowers");
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    let mut resolver = DamageResolver::new(session(), PRODUCER);
    resolver
        .register_actor(actor(), graph, policy)
        .expect("the lowered actor registers");
    resolver
}

fn hit(resolver: &mut DamageResolver, node: &DamageNodeKey, damage: f64, sequence: u32) {
    let event = HitEvent::try_new(
        HitEventId {
            session: session(),
            tick: Tick(0),
            producer: PRODUCER,
            sequence,
        },
        None,
        actor(),
        node.clone(),
        DamageChannel::Internal,
        damage,
    )
    .expect("a finite, non-negative hit");
    resolver
        .resolve(Tick(0), &[event])
        .expect("the resolver accepts the batch");
}

fn bind(world: &mut World, region: AirframeDamageRegion, node: &str) -> Entity {
    world
        .spawn(RegionDamageBinding {
            actor: actor(),
            region,
            node: key(node),
            generation: generation(),
        })
        .id()
}

fn record(world: &World, entity: Entity) -> Option<RegionDamagePresentation> {
    world.get::<RegionDamagePresentation>(entity).cloned()
}

#[test]
fn accept_parity_m01_player_view_c1_publishes_the_resolver_state_per_region() {
    let mut world = World::new();
    let mut resolver = registered();
    let left = bind(
        &mut world,
        AirframeDamageRegion::LeftWing,
        SYNTHETIC_ENGINE_NODE,
    );
    let nose = bind(&mut world, AirframeDamageRegion::Nose, SYNTHETIC_MOUNT_NODE);
    let right = bind(
        &mut world,
        AirframeDamageRegion::RightWing,
        "not-in-the-graph",
    );

    let first = apply_region_damage_state(&mut world, &resolver, actor());
    assert_eq!(first.published, 3);
    assert_eq!(
        record(&world, left).map(|r| r.state),
        Some(PartState::Intact)
    );

    hit(
        &mut resolver,
        &key(SYNTHETIC_ENGINE_NODE),
        SYNTHETIC_ENGINE_INTEGRITY,
        0,
    );
    hit(
        &mut resolver,
        &key(SYNTHETIC_MOUNT_NODE),
        SYNTHETIC_MOUNT_INTEGRITY / 4.0,
        1,
    );
    let second = apply_region_damage_state(&mut world, &resolver, actor());
    assert_eq!(second.published, 2, "{second:?}");
    assert_eq!(second.unchanged, 1);
    assert_eq!(
        record(&world, left).map(|r| (r.region, r.state, r.generation)),
        Some((
            AirframeDamageRegion::LeftWing,
            PartState::Destroyed,
            generation()
        ))
    );
    assert_eq!(
        record(&world, nose).map(|r| r.state),
        Some(PartState::Damaged)
    );
    // A node the authority does not describe stays Unknown, with a refusal.
    assert_eq!(
        record(&world, right).map(|r| r.state),
        Some(PartState::Unknown)
    );
    assert_eq!(second.refusals.len(), 1);
}

#[test]
fn accept_parity_m01_player_view_c1_pass_is_idempotent_and_release_drops_records() {
    let mut world = World::new();
    let mut resolver = registered();
    let left = bind(
        &mut world,
        AirframeDamageRegion::LeftWing,
        SYNTHETIC_ENGINE_NODE,
    );
    hit(
        &mut resolver,
        &key(SYNTHETIC_ENGINE_NODE),
        SYNTHETIC_ENGINE_INTEGRITY,
        0,
    );

    let _ = apply_region_damage_state(&mut world, &resolver, actor());
    let again = apply_region_damage_state(&mut world, &resolver, actor());
    assert_eq!((again.published, again.unchanged), (0, 1));

    assert_eq!(release_region_damage(&mut world, actor()), 1);
    assert!(record(&world, left).is_none());
    assert_eq!(release_region_damage(&mut world, actor()), 0);
    assert!(
        world.get::<RegionDamageBinding>(left).is_some(),
        "the binding stays"
    );
}

#[test]
fn accept_parity_m01_player_view_c1_foreign_session_publishes_unknown() {
    let mut world = World::new();
    let resolver = DamageResolver::new(SessionId::new(SESSION + 1).expect("nonzero"), PRODUCER);
    let tail = bind(
        &mut world,
        AirframeDamageRegion::Tail,
        SYNTHETIC_ENGINE_NODE,
    );
    let outcome = apply_region_damage_state(&mut world, &resolver, actor());
    assert_eq!(
        record(&world, tail).map(|r| r.state),
        Some(PartState::Unknown)
    );
    assert_eq!(outcome.refusals.len(), 1);
}

#[test]
fn accept_parity_m01_player_view_c1_region_names_are_the_measured_four() {
    for region in AirframeDamageRegion::ALL {
        assert_eq!(
            AirframeDamageRegion::from_node_name(region.node_name()),
            Some(region)
        );
    }
    assert_eq!(
        AirframeDamageRegion::from_node_name("damageindicator"),
        None
    );
}

/// Measures what M01's player has in the session resolver: since #1699 the
/// four region nodes are registered, so a bound region publishes `Intact`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_view_c1_retail_m01_player_graph_has_region_nodes() {
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::{MissionWeapons, build_headless, stage_for};
    use cs_app::weapons::SessionDamageResolver;

    let root =
        std::path::PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let label = cs_content::campaign_bindings::MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    let player = app.world().resource::<MissionWeapons>().player();
    let resolver = app.world().resource::<SessionDamageResolver>().resolver();
    let regions: Vec<_> = resolver
        .graph(&player)
        .map(|graph| {
            graph
                .nodes()
                .filter_map(|n| AirframeDamageRegion::from_node_name(n.key().as_str()))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(regions.len(), 4, "all four measured regions: {regions:?}");
    let mut world = World::new();
    let region = bind_for(&mut world, player);
    let outcome = apply_region_damage_state(&mut world, resolver, player);
    assert_eq!(
        record(&world, region).map(|r| r.state),
        Some(PartState::Intact)
    );
    assert!(outcome.refusals.is_empty(), "{:?}", outcome.refusals);
}

fn bind_for(world: &mut World, player: ActorId) -> Entity {
    world
        .spawn(RegionDamageBinding {
            actor: player,
            region: AirframeDamageRegion::LeftWing,
            node: key("leftwingdamage"),
            generation: generation(),
        })
        .id()
}
