//! Acceptance PARITY-M01-PLAYER-VIEW-C2 (#1683): the wing fire quad, gated on
//! the C1 region presentation records. Task test prefix:
//! `accept_parity_m01_player_view_c2_`.
//!
//! The measured source data is recorded in
//! `docs/findings/2026-10-19-parity-m01-player-view-c2-wing-fire-smoke-source.md`:
//! `wing_flare1`/`wing_flare2` are single-quad `oil_liteflare.tif` meshes at
//! the wingtips, damage-gated in the original (absent in orig25.png, burning
//! at the damaged left wingtip in orig70.png). The synthetic scenarios drive
//! the real [`DamageResolver`] through the production lowering and read the
//! visibility [`update_wing_flares`] publishes. Synthetic data proves the
//! gate, never the original's behaviour.

use std::path::PathBuf;

use bevy::prelude::{Entity, Visibility, World};
use cs_app::airframe_damage::{
    AirframeDamageRegion, RegionDamageBinding, RegionDamagePresentation, WingFlare,
    apply_region_damage_state, update_wing_flares,
};
use cs_app::damage::{lower_graph, lower_policy};
use cs_app::scene::SceneGeneration;
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_sim::damage::{
    ActorId, DamageChannel, DamageNodeKey, DamageResolver, HitEvent, HitEventId, PartState,
    SYNTHETIC_ENGINE_INTEGRITY, SYNTHETIC_ENGINE_NODE,
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

fn hit(resolver: &mut DamageResolver, node: &DamageNodeKey, damage: f64) {
    let event = HitEvent::try_new(
        HitEventId {
            session: session(),
            tick: Tick(0),
            producer: PRODUCER,
            sequence: 0,
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

fn flare(world: &mut World, region: AirframeDamageRegion, node: &str) -> Entity {
    world
        .spawn((
            WingFlare { region },
            Visibility::Hidden,
            RegionDamageBinding {
                actor: actor(),
                region,
                node: key(node),
                generation: SceneGeneration::default().next(),
            },
        ))
        .id()
}

fn visibility(world: &World, entity: Entity) -> Visibility {
    *world.get::<Visibility>(entity).expect("a visibility")
}

/// The gate the task names: the quad shows only while the region's published
/// state is `Damaged` or `Destroyed`; `Intact` and `Unknown` stay hidden.
#[test]
fn accept_parity_m01_player_view_c2_flare_shows_only_when_damaged_or_destroyed() {
    let mut world = World::new();
    let mut resolver = registered();
    let quad = flare(
        &mut world,
        AirframeDamageRegion::LeftWing,
        SYNTHETIC_ENGINE_NODE,
    );

    let _ = apply_region_damage_state(&mut world, &resolver, actor());
    let _ = update_wing_flares(&mut world);
    assert_eq!(
        visibility(&world, quad),
        Visibility::Hidden,
        "an intact wing draws no fire"
    );

    hit(
        &mut resolver,
        &key(SYNTHETIC_ENGINE_NODE),
        SYNTHETIC_ENGINE_INTEGRITY / 4.0,
    );
    let _ = apply_region_damage_state(&mut world, &resolver, actor());
    let changed = update_wing_flares(&mut world);
    assert_eq!(changed, 1);
    assert_eq!(
        visibility(&world, quad),
        Visibility::Inherited,
        "a damaged wing draws the fire quad"
    );

    hit(
        &mut resolver,
        &key(SYNTHETIC_ENGINE_NODE),
        SYNTHETIC_ENGINE_INTEGRITY,
    );
    let _ = apply_region_damage_state(&mut world, &resolver, actor());
    let _ = update_wing_flares(&mut world);
    assert_eq!(
        world.get::<RegionDamagePresentation>(quad).map(|r| r.state),
        Some(PartState::Destroyed)
    );
    assert_eq!(
        visibility(&world, quad),
        Visibility::Inherited,
        "a destroyed wing still draws the fire"
    );
}

/// A flare whose node the resolver does not describe publishes `Unknown` —
/// and `Unknown` is never fire.
#[test]
fn accept_parity_m01_player_view_c2_unknown_state_never_shows_the_flare() {
    let mut world = World::new();
    let resolver = registered();
    let quad = flare(
        &mut world,
        AirframeDamageRegion::RightWing,
        "not-in-the-graph",
    );
    let outcome = apply_region_damage_state(&mut world, &resolver, actor());
    assert_eq!(outcome.refusals.len(), 1);
    let _ = update_wing_flares(&mut world);
    assert_eq!(
        visibility(&world, quad),
        Visibility::Hidden,
        "Unknown never shows the flare"
    );
}

/// A flare with no published record yet stays hidden rather than guessing.
#[test]
fn accept_parity_m01_player_view_c2_unpublished_flare_stays_hidden() {
    let mut world = World::new();
    let quad = world
        .spawn((
            WingFlare {
                region: AirframeDamageRegion::Tail,
            },
            Visibility::Hidden,
        ))
        .id();
    assert_eq!(update_wing_flares(&mut world), 0);
    assert_eq!(visibility(&world, quad), Visibility::Hidden);
}

/// **M01's player carries the two measured fire quads**, hidden until their
/// regions' C1 records report damage — measured `wing_flare1`/`wing_flare2`
/// under `dontmove`, textured `oil_liteflare.tif`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_view_c2_retail_player_carries_the_measured_flare_quads() {
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::{
        MISSION_PLAYER_FLARE_LEFT_NODE_SLOT, MISSION_PLAYER_FLARE_RIGHT_NODE_SLOT,
        MissionPlayerBody, build_headless, stage_for, teardown,
    };
    use cs_app::playtest_retail::AircraftPart;
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
    let resolver = app
        .world()
        .resource::<cs_app::weapons::SessionDamageResolver>()
        .resolver()
        .clone();
    let world = app.world_mut();
    // The player body spawns before `MissionWeapons` is installed, so the
    // flare pass declares each quad's region binding on its first run.
    let _ = update_wing_flares(world);
    let body = world
        .query_filtered::<Entity, bevy::prelude::With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    let mut flares: Vec<(Entity, u32, AirframeDamageRegion, Visibility)> = world
        .query_filtered::<(
            Entity,
            &AircraftPart,
            &RegionDamageBinding,
            &Visibility,
            &bevy::prelude::ChildOf,
        ), bevy::prelude::With<WingFlare>>()
        .iter(world)
        .filter(|(_, _, _, _, child)| child.parent() == body)
        .map(|(entity, part, binding, visibility, _)| {
            (entity, part.node_slot, binding.region, *visibility)
        })
        .collect();
    flares.sort_by_key(|(_, slot, _, _)| *slot);
    assert_eq!(flares.len(), 2, "exactly the two measured fire quads");
    assert_eq!(
        flares[0].1, MISSION_PLAYER_FLARE_LEFT_NODE_SLOT,
        "slot 208 is wing_flare1"
    );
    assert_eq!(flares[0].2, AirframeDamageRegion::LeftWing);
    assert_eq!(
        flares[1].1, MISSION_PLAYER_FLARE_RIGHT_NODE_SLOT,
        "slot 209 is wing_flare2"
    );
    assert_eq!(flares[1].2, AirframeDamageRegion::RightWing);
    for (_, _, _, visibility) in &flares {
        assert_eq!(
            *visibility,
            Visibility::Hidden,
            "the player starts intact; no fire quad draws"
        );
    }
    // The measured node names are the bindings' damage nodes.
    for expected in ["leftwingdamage", "rightwingdamage"] {
        assert!(
            flares
                .iter()
                .any(|(_, _, region, _)| region.node_name() == expected),
            "a flare binds {expected}"
        );
    }

    // The C1 pass over the real session resolver publishes Unknown for the
    // player (its actor has no damage graph yet), so the flares stay hidden —
    // the same presentation orig25.png captures.
    let _ = apply_region_damage_state(world, &resolver, player);
    assert_eq!(update_wing_flares(world), 0);
    for (_, _, _, visibility) in &flares {
        assert_eq!(*visibility, Visibility::Hidden);
    }

    teardown(&mut app);
}
