//! Acceptance PARITY-M01-PLAYER-VIEW-C (#1668): the wing fire plume driven by
//! the damage model, end to end. Task test prefix:
//! `accept_parity_m01_player_view_c_`.
//!
//! The slice's two halves merged under it: C1's region hook
//! ([`apply_region_damage_state`] copies the session [`DamageResolver`]'s
//! [`PartState`] verbatim) and C2's measured `wing_flare` quads
//! ([`update_wing_flares`] gates them on the published record). These tests
//! drive the whole production chain — spawn shape, scheduled driver,
//! resolver hits — and prove the plume answers to *each wing's own* damage
//! node, never to a parallel state. The wing-node graph is declared
//! synthetic data: it proves the wiring, never the original's thresholds
//! (unmeasured, recorded in
//! `docs/findings/2026-10-19-parity-m01-player-view-c-wing-fire-driven.md`
//! with `orig70.png` as the target).

use std::path::PathBuf;

use bevy::prelude::{Entity, Visibility, World};
use cs_app::airframe_damage::{
    AirframeDamageRegion, RegionDamageBinding, RegionDamagePresentation, WingFlare,
    apply_region_damage_state, drive_airframe_damage_visuals, release_region_damage,
    update_wing_flares,
};
use cs_app::damage::{lower_graph, lower_policy};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::SessionDamageResolver;
use cs_content::damage::{
    DamageNodeKind, DeclaredDamageGraph, DeclaredDamageNode, declared_synthetic_airframe_damage,
};
use cs_sim::damage::{
    ActorId, DamageChannel, DamageNodeKey, DamageResolver, HitEvent, HitEventId, PartState,
};
use cs_types::Tick;
use cs_types::content::{Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
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

/// A declared airframe graph whose nodes carry the airframe's four measured
/// region names (`docs/findings/2026-10-07-f29-d1-airframe-damage-vocabulary.md`),
/// so the production binding's own `region.node_name()` meets a node the
/// resolver describes. Designed integrities; synthetic data proves the
/// wiring only.
fn declared_wing_damage() -> DeclaredDamageGraph {
    let fixture = declared_synthetic_airframe_damage();
    let node = |name: &str| DeclaredDamageNode {
        key: key(name),
        kind: DamageNodeKind::InternalStructure,
        scene_binding: None,
        integrity: Resolved::Known(Known::new(
            40.0,
            Provenance::designed(
                ClaimId::new("parity-m01-view-c.test.wings").expect("test claim id is valid"),
            ),
        )),
        lethal: false,
        disables: None,
        guarded_by: None,
        overflow: None,
    };
    DeclaredDamageGraph::try_new(
        fixture.subject().clone(),
        fixture.origin().clone(),
        fixture.subject_kind(),
        cs_content::damage::GraphRules {
            lethal_attribution: fixture.rules().lethal_attribution.clone(),
        },
        AirframeDamageRegion::ALL
            .iter()
            .map(|region| node(region.node_name()))
            .collect(),
        fixture.provenance().clone(),
    )
    .expect("the four-region graph validates")
}

fn registered() -> DamageResolver {
    let declared = declared_wing_damage();
    let graph = lower_graph(&declared).expect("the declared graph lowers");
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    let mut resolver = DamageResolver::new(session(), PRODUCER);
    resolver
        .register_actor(actor(), graph, policy)
        .expect("the lowered actor registers");
    resolver
}

fn hit(resolver: &mut DamageResolver, target: ActorId, node: &DamageNodeKey, damage: f64) {
    let event = HitEvent::try_new(
        HitEventId {
            session: session(),
            tick: Tick(0),
            producer: PRODUCER,
            sequence: 0,
        },
        None,
        target,
        node.clone(),
        DamageChannel::Internal,
        damage,
    )
    .expect("a finite, non-negative hit");
    resolver
        .resolve(Tick(0), &[event])
        .expect("the resolver accepts the batch");
}

fn visibility(world: &World, entity: Entity) -> Visibility {
    *world.get::<Visibility>(entity).expect("a visibility")
}

fn state(world: &World, entity: Entity) -> Option<PartState> {
    world
        .get::<RegionDamagePresentation>(entity)
        .map(|r| r.state)
}

/// The task's sentence end to end: a hit on the **port** wing's damage node
/// draws the port quad only; the starboard quad answers to its own node, not
/// to a session-wide "damaged" flag. Driven through
/// [`drive_airframe_damage_visuals`] — the scheduled production pass — with
/// the flares spawned the way the player body spawns them (`WingFlare`,
/// hidden, no binding: the driver declares the measured names).
#[test]
fn accept_parity_m01_player_view_c_each_wings_plume_answers_to_its_own_damage_node() {
    use cs_app::mission_session::open_mission_weapons;
    use cs_sim::time::TickRate;

    let mut world = World::new();
    let generation = SceneGeneration::default().next();
    let sessions = open_mission_weapons(
        session(),
        TickRate::new(60).expect("a nonzero tick rate"),
        generation,
    )
    .expect("the mission weapon session opens");
    let player = sessions.mission.player();
    sessions.install(&mut world).expect("the sessions install");

    // The session resolver serves the player against the four-region graph.
    let declared = declared_wing_damage();
    let graph = lower_graph(&declared).expect("the declared graph lowers");
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    world
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .register_actor(player, graph, policy)
        .expect("the player registers");

    let port = world
        .spawn((
            WingFlare {
                region: AirframeDamageRegion::LeftWing,
            },
            Visibility::Hidden,
        ))
        .id();
    let starboard = world
        .spawn((
            WingFlare {
                region: AirframeDamageRegion::RightWing,
            },
            Visibility::Hidden,
        ))
        .id();

    drive_airframe_damage_visuals(&mut world);
    // The driver bound each quad to its measured region node on the player
    // actor and published `Intact` for both — the orig25 presentation.
    for (entity, region) in [
        (port, AirframeDamageRegion::LeftWing),
        (starboard, AirframeDamageRegion::RightWing),
    ] {
        let binding = world
            .get::<RegionDamageBinding>(entity)
            .expect("the driver declared the binding");
        assert_eq!(binding.actor, player);
        assert_eq!(binding.region, region);
        assert_eq!(binding.node, key(region.node_name()));
        assert_eq!(state(&world, entity), Some(PartState::Intact));
        assert_eq!(visibility(&world, entity), Visibility::Hidden);
    }

    // A hit on the port wing's node draws the port plume; the starboard side
    // reads its own node and stays dark.
    hit(
        world.resource_mut::<SessionDamageResolver>().resolver_mut(),
        player,
        &key("leftwingdamage"),
        10.0,
    );
    drive_airframe_damage_visuals(&mut world);
    assert_eq!(state(&world, port), Some(PartState::Damaged));
    assert_eq!(
        visibility(&world, port),
        Visibility::Inherited,
        "the damaged port wing draws its fire quad"
    );
    assert_eq!(state(&world, starboard), Some(PartState::Intact));
    assert_eq!(
        visibility(&world, starboard),
        Visibility::Hidden,
        "an intact starboard wing draws nothing"
    );

    // A hit on the starboard wing's node draws its own quad too — per-part
    // state, never a shared flag.
    hit(
        world.resource_mut::<SessionDamageResolver>().resolver_mut(),
        player,
        &key("rightwingdamage"),
        10.0,
    );
    drive_airframe_damage_visuals(&mut world);
    assert_eq!(state(&world, starboard), Some(PartState::Damaged));
    assert_eq!(visibility(&world, starboard), Visibility::Inherited);
    assert_eq!(visibility(&world, port), Visibility::Inherited);
}

/// Repair / reload / scene release: [`release_region_damage`] drops the
/// records and the next gate pass re-hides the quads rather than leaving a
/// stale plume burning.
#[test]
fn accept_parity_m01_player_view_c_release_rehides_the_quads() {
    let mut world = World::new();
    let mut resolver = registered();
    let generation = SceneGeneration::default().next();
    let quad = world
        .spawn((
            WingFlare {
                region: AirframeDamageRegion::LeftWing,
            },
            Visibility::Hidden,
            RegionDamageBinding {
                actor: actor(),
                region: AirframeDamageRegion::LeftWing,
                node: key("leftwingdamage"),
                generation,
            },
        ))
        .id();

    hit(&mut resolver, actor(), &key("leftwingdamage"), 10.0);
    let _ = apply_region_damage_state(&mut world, &resolver, actor());
    let _ = update_wing_flares(&mut world);
    assert_eq!(visibility(&world, quad), Visibility::Inherited);

    assert_eq!(release_region_damage(&mut world, actor()), 1);
    assert_eq!(state(&world, quad), None, "the record is gone");
    let changed = update_wing_flares(&mut world);
    assert_eq!(changed, 1);
    assert_eq!(
        visibility(&world, quad),
        Visibility::Hidden,
        "a released region draws no plume"
    );
    assert_eq!(release_region_damage(&mut world, actor()), 0);
}

/// **M01's composed player carries the slice's whole wiring**: the two
/// measured fire quads, bound by the scheduled driver to the measured
/// `leftwingdamage`/`rightwingdamage` nodes of the player actor, each with a
/// C1 record. The player's damage graph (#1699) registers the regions, so
/// the records read `Intact` and both quads stay `Hidden` —
/// exactly the intact presentation `orig25.png` shows.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_view_c_retail_m01_player_carries_the_driven_plume() {
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::{MissionPlayerBody, build_headless, stage_for, teardown};
    use cs_content::campaign_bindings::MissionLabel;

    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let label = MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();

    let player = app
        .world()
        .resource::<cs_app::mission_session::MissionWeapons>()
        .player();
    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, bevy::prelude::With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");

    // Exactly two flares exist in the whole composition, both on the player.
    let mut flares: Vec<(
        AirframeDamageRegion,
        DamageNodeKey,
        PartState,
        Visibility,
        ActorId,
    )> = world
        .query_filtered::<(
            &WingFlare,
            &RegionDamageBinding,
            &RegionDamagePresentation,
            &Visibility,
            &bevy::prelude::ChildOf,
        ), bevy::prelude::With<WingFlare>>()
        .iter(world)
        .filter(|(_, _, _, _, child)| child.parent() == body)
        .map(|(flare, binding, record, visibility, _)| {
            (
                flare.region,
                binding.node.clone(),
                record.state,
                *visibility,
                binding.actor,
            )
        })
        .collect();
    flares.sort_by_key(|flare| flare.0);
    assert_eq!(
        flares.len(),
        2,
        "exactly the two measured fire quads on the player"
    );
    let regions: Vec<AirframeDamageRegion> = flares.iter().map(|flare| flare.0).collect();
    assert_eq!(
        regions,
        vec![
            AirframeDamageRegion::LeftWing,
            AirframeDamageRegion::RightWing
        ]
    );
    for (region, node, record_state, quad_visibility, bound_actor) in &flares {
        assert_eq!(bound_actor, &player, "the quad binds the player actor");
        assert_eq!(
            *node,
            key(region.node_name()),
            "the driver bound the measured {region:?} node"
        );
        assert_eq!(
            *record_state,
            PartState::Intact,
            "the player's damage graph (#1699) registers the region: Intact at spawn"
        );
        assert_eq!(
            *quad_visibility,
            Visibility::Hidden,
            "the intact presentation orig25.png shows: no plume"
        );
    }

    // The scheduled driver is convergent: a second frame publishes nothing
    // new and moves no quad.
    app.update();
    assert_eq!(update_wing_flares(app.world_mut()), 0);

    teardown(&mut app);
}
