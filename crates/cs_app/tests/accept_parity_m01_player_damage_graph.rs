//! Acceptance PARITY-M01-PLAYER-DAMAGE-GRAPH (#1699): the M01 player's
//! airframe damage graph. Task test prefix: `accept_parity_m01_player_damage_graph_`.
//!
//! The region node names are measured (`ZBD/planes.zbd`); the pools are
//! designed and the original's are recorded unknown in
//! `docs/findings/2026-10-23-parity-m01-player-damage-graph.md`.

use bevy::prelude::{Visibility, World};
use cs_app::airframe_damage::{
    AirframeDamageRegion, RegionDamageBinding, RegionDamagePresentation, WingFlare,
    drive_airframe_damage_visuals,
};
use cs_app::mission_session::{DESIGNED_REGION_INTEGRITY, attach_player_victim};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::SessionDamageResolver;
use cs_sim::damage::{
    ActorId, DamageChannel, DamageNodeKey, DamageNodeKind, HitEvent, HitEventId, PartState,
};
use cs_types::Tick;
use cs_types::net::SessionId;

fn actor() -> ActorId {
    ActorId {
        session: SessionId::new(9).expect("a nonzero session"),
        serial: 1,
    }
}

fn hit(world: &mut World, region: AirframeDamageRegion, damage: f64, sequence: u32) {
    let event = HitEvent::try_new(
        HitEventId {
            session: actor().session,
            tick: Tick(0),
            producer: 999,
            sequence,
        },
        None,
        actor(),
        DamageNodeKey::new(region.node_name()).expect("a measured node name"),
        DamageChannel::Internal,
        damage,
    )
    .expect("a finite hit");
    world
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .resolve(Tick(0), &[event])
        .expect("the batch resolves");
}

fn flare(world: &mut World, region: AirframeDamageRegion) -> bevy::prelude::Entity {
    world
        .spawn((
            WingFlare { region },
            RegionDamageBinding {
                actor: actor(),
                region,
                node: DamageNodeKey::new(region.node_name()).expect("key"),
                generation: SceneGeneration(1),
            },
            Visibility::Hidden,
        ))
        .id()
}

#[test]
fn accept_parity_m01_player_damage_graph_registers_the_four_region_nodes() {
    let mut world = World::new();
    let body = world.spawn_empty().id();
    attach_player_victim(&mut world, body, actor(), SceneGeneration(1)).expect("attaches");
    let resolver = world.resource::<SessionDamageResolver>().resolver();
    let graph = resolver.graph(&actor()).expect("the player is registered");
    for region in AirframeDamageRegion::ALL {
        let node = graph
            .nodes()
            .find(|n| n.key().as_str() == region.node_name())
            .unwrap_or_else(|| panic!("{} is a node", region.node_name()));
        let expected = if region == AirframeDamageRegion::Tail {
            DamageNodeKind::Engine
        } else {
            DamageNodeKind::InternalStructure
        };
        assert_eq!(node.kind(), expected);
        assert!(!node.is_lethal(), "region lethality is unmeasured");
    }
}

#[test]
fn accept_parity_m01_player_damage_graph_hits_drive_the_wing_flares() {
    let mut world = World::new();
    let body = world.spawn_empty().id();
    attach_player_victim(&mut world, body, actor(), SceneGeneration(1)).expect("attaches");
    let left = flare(&mut world, AirframeDamageRegion::LeftWing);
    let right = flare(&mut world, AirframeDamageRegion::RightWing);

    drive_airframe_damage_visuals(&mut world);
    for entity in [left, right] {
        assert_eq!(
            world
                .get::<RegionDamagePresentation>(entity)
                .map(|r| r.state),
            Some(PartState::Intact),
            "intact at spawn, not Unknown"
        );
        assert_eq!(world.get::<Visibility>(entity), Some(&Visibility::Hidden));
    }

    hit(
        &mut world,
        AirframeDamageRegion::LeftWing,
        DESIGNED_REGION_INTEGRITY / 4.0,
        0,
    );
    drive_airframe_damage_visuals(&mut world);
    assert_eq!(
        world.get::<RegionDamagePresentation>(left).map(|r| r.state),
        Some(PartState::Damaged)
    );
    assert_eq!(world.get::<Visibility>(left), Some(&Visibility::Inherited));
    assert_eq!(world.get::<Visibility>(right), Some(&Visibility::Hidden));
}

/// Over real M01 data: Intact at spawn, then a left-wing hit shows the quad.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_damage_graph_retail_m01_player_regions_publish_and_burn() {
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::{MissionWeapons, build_headless, stage_for};
    use cs_content::campaign_bindings::MissionLabel;

    let root =
        std::path::PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let label = MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    app.update();
    let player = app.world().resource::<MissionWeapons>().player();

    let world = app.world_mut();
    let mut flares: Vec<_> = world
        .query::<(
            &WingFlare,
            &RegionDamageBinding,
            &RegionDamagePresentation,
            &Visibility,
        )>()
        .iter(world)
        .map(|(f, b, r, v)| (f.region, b.actor, r.state, *v))
        .collect();
    flares.sort_by_key(|(region, ..)| *region);
    assert_eq!(flares.len(), 2);
    for (_, bound, state, visibility) in &flares {
        assert_eq!(*bound, player);
        assert_eq!(*state, PartState::Intact);
        assert_eq!(*visibility, Visibility::Hidden);
    }
    let resolver = world.resource::<SessionDamageResolver>().resolver();
    let graph = resolver.graph(&player).expect("the player is registered");
    for region in AirframeDamageRegion::ALL {
        assert!(
            graph
                .nodes()
                .any(|n| n.key().as_str() == region.node_name())
        );
    }

    let event = HitEvent::try_new(
        HitEventId {
            session: player.session,
            tick: Tick(0),
            producer: 999,
            sequence: 0,
        },
        None,
        player,
        DamageNodeKey::new("leftwingdamage").expect("key"),
        DamageChannel::Internal,
        DESIGNED_REGION_INTEGRITY / 4.0,
    )
    .expect("a finite hit");
    world
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .resolve(Tick(0), &[event])
        .expect("the hit resolves");
    app.update();
    let world = app.world_mut();
    for (flare, record, visibility) in world
        .query::<(&WingFlare, &RegionDamagePresentation, &Visibility)>()
        .iter(world)
    {
        match flare.region {
            AirframeDamageRegion::LeftWing => {
                assert_eq!(record.state, PartState::Damaged);
                assert_eq!(*visibility, Visibility::Inherited);
            }
            _ => assert_eq!(*visibility, Visibility::Hidden),
        }
    }
}
