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
    apply_region_damage_state, drive_airframe_damage_visuals, update_wing_flares,
};
use cs_app::damage::{lower_graph, lower_policy};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::SessionDamageResolver;
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
        actor(),
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
        actor(),
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

/// The scheduled driver — the production Update pass — declares the binding
/// the spawn path could not, publishes the C1 record from the session's own
/// resolver, and gates the quad, all inside one pass and with no test-side
/// publish in between.
#[test]
fn accept_parity_m01_player_view_c2_scheduled_driver_declares_publishes_and_gates() {
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

    // The session resolver serves the player against the synthetic graph.
    let declared = declared_synthetic_airframe_damage();
    let graph = lower_graph(&declared).expect("the declared graph lowers");
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    world
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .register_actor(player, graph, policy)
        .expect("the player registers");

    // One flare arrives the way the player body spawns it: marked `WingFlare`,
    // hidden, with no binding yet. Its region names `leftwingdamage`, which
    // the synthetic graph does not describe — the record must publish
    // `Unknown` and the quad must stay hidden, never guessed `Intact`.
    let unbound = world
        .spawn((
            WingFlare {
                region: AirframeDamageRegion::LeftWing,
            },
            Visibility::Hidden,
        ))
        .id();
    // A second flare carries the binding the spawn path declares when one is
    // known, aimed at the graph's engine node.
    let bound = world
        .spawn((
            WingFlare {
                region: AirframeDamageRegion::RightWing,
            },
            Visibility::Hidden,
            RegionDamageBinding {
                actor: player,
                region: AirframeDamageRegion::RightWing,
                node: key(SYNTHETIC_ENGINE_NODE),
                generation,
            },
        ))
        .id();

    drive_airframe_damage_visuals(&mut world);
    let binding = world
        .get::<RegionDamageBinding>(unbound)
        .expect("the driver declared the binding");
    assert_eq!(binding.actor, player);
    assert_eq!(binding.node, key("leftwingdamage"));
    assert_eq!(binding.generation, generation);
    assert_eq!(
        world
            .get::<RegionDamagePresentation>(unbound)
            .map(|r| r.state),
        Some(PartState::Unknown),
        "an undescribed node publishes Unknown"
    );
    assert_eq!(visibility(&world, unbound), Visibility::Hidden);
    assert_eq!(
        world
            .get::<RegionDamagePresentation>(bound)
            .map(|r| r.state),
        Some(PartState::Intact)
    );
    assert_eq!(visibility(&world, bound), Visibility::Hidden);

    // Damage the graph reports flips the quad on the next scheduled pass —
    // the whole loop inside the driver, no test-side publish between.
    hit(
        world.resource_mut::<SessionDamageResolver>().resolver_mut(),
        player,
        &key(SYNTHETIC_ENGINE_NODE),
        SYNTHETIC_ENGINE_INTEGRITY / 4.0,
    );
    drive_airframe_damage_visuals(&mut world);
    assert_eq!(
        world
            .get::<RegionDamagePresentation>(bound)
            .map(|r| r.state),
        Some(PartState::Damaged)
    );
    assert_eq!(
        visibility(&world, bound),
        Visibility::Inherited,
        "the scheduled pass shows the damaged quad"
    );
    assert_eq!(visibility(&world, unbound), Visibility::Hidden);
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
    let world = app.world_mut();
    // `app.update()` above already ran the scheduled driver: each quad's
    // binding was declared and its C1 record published before this test reads
    // a thing — no pass runs here by hand.
    let body = world
        .query_filtered::<Entity, bevy::prelude::With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    let mut flares: Vec<(
        Entity,
        u32,
        AirframeDamageRegion,
        Visibility,
        PartState,
        ActorId,
    )> = world
        .query_filtered::<(
            Entity,
            &AircraftPart,
            &RegionDamageBinding,
            &RegionDamagePresentation,
            &Visibility,
            &bevy::prelude::ChildOf,
        ), bevy::prelude::With<WingFlare>>()
        .iter(world)
        .filter(|(_, _, _, _, _, child)| child.parent() == body)
        .map(|(entity, part, binding, record, visibility, _)| {
            (
                entity,
                part.node_slot,
                binding.region,
                *visibility,
                record.state,
                binding.actor,
            )
        })
        .collect();
    flares.sort_by_key(|(_, slot, _, _, _, _)| *slot);
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
    for (_, _, _, visibility, state, bound_actor) in &flares {
        assert_eq!(bound_actor, &player, "the quad binds the player actor");
        assert_eq!(
            *state,
            PartState::Unknown,
            "the player actor has no damage graph yet, so the scheduled pass \
             published Unknown — never a guessed Intact"
        );
        assert_eq!(
            *visibility,
            Visibility::Hidden,
            "the player starts undamaged; no fire quad draws"
        );
    }
    // The measured node names are the bindings' damage nodes.
    for expected in ["leftwingdamage", "rightwingdamage"] {
        assert!(
            flares
                .iter()
                .any(|(_, _, region, _, _, _)| region.node_name() == expected),
            "a flare binds {expected}"
        );
    }

    // A second scheduled frame changes nothing: the driver is convergent, so
    // the flare pass reports no visibility changes.
    app.update();
    assert_eq!(
        update_wing_flares(app.world_mut()),
        0,
        "the scheduled driver already settled the quads"
    );

    // The anchor measurement the finding records: the airframe's `l_aileron*`
    // parts sit at negative x and its `r_aileron*` at positive x, so the port
    // wingtip is −x. `wing_flare1` (slot 208) is the port quad and
    // `wing_flare2` (slot 209) the starboard one.
    let report = app
        .world()
        .resource::<cs_app::mission_session::MissionPlayerVisual>()
        .report();
    let at = |name: &str| {
        report
            .parts
            .iter()
            .find(|part| part.node_name == name)
            .unwrap_or_else(|| panic!("{name} is a drawn part"))
            .composed_translation_m
    };
    for port in ["l_aileron1", "l_aileron2", "l_elevator1"] {
        assert!(
            at(port)[0] < 0.0,
            "{port} sits on the port side (x < 0), got {:?}",
            at(port)
        );
    }
    for starboard in ["r_aileron1", "r_aileron2", "r_elevator1"] {
        assert!(
            at(starboard)[0] > 0.0,
            "{starboard} sits on the starboard side (x > 0), got {:?}",
            at(starboard)
        );
    }
    assert_eq!(at("wing_flare1")[0].signum(), -1.0, "slot 208 is port");
    assert_eq!(at("wing_flare2")[0].signum(), 1.0, "slot 209 is starboard");

    teardown(&mut app);
}
