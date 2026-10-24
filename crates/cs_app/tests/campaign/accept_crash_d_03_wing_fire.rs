//! PARITY4-CRASH-D.03 (#1946): a wing region whose remaining integrity falls
//! to a measured `pdpanelN` threshold burns on its `pdpN` node, following the
//! airframe; the intact side does not. The threshold direction and the pools
//! are designed; the emitters, ladders and nodes are read from retail.

use std::collections::BTreeMap;

use bevy::prelude::{App, Entity, FixedPostUpdate, GlobalTransform, Handle, Transform, Vec3, With};
use cs_app::airframe_damage::AirframeDamageRegion;
use cs_app::mission_session::{
    DESIGNED_REGION_INTEGRITY, MissionWingFirePlugin, PlayerVictim, TrailEmitter,
    WING_FIRE_MIN_TRAVEL_M_S, WingFireArt, WingFireDefinition, WingFireEffect, WingFireEmitter,
    WingFireNode, WingFirePuff, WingFireStage, WingFireState, attach_player_victim,
    burning_effects, plan_wing_emissions, threshold_reached, wing_node,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::playtest::headless_app_with;
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::scene::SceneGeneration;
use cs_app::weapons::SessionDamageResolver;
use cs_sim::damage::{DamageChannel, HitEvent};
use cs_types::Tick;
use cs_types::net::{ActorId, EventId, SessionId};

const HZ: f32 = 120.0;

fn emitter() -> TrailEmitter {
    TrailEmitter {
        distance_interval: 0.5,
        size: [0.1, 0.15],
        lifetime_s: [0.5, 0.7],
        min_velocity: [0.0; 3],
        max_velocity: [0.0; 3],
        friction: 0.0,
        textures: vec!["smoke101".to_owned()],
        frames: Vec::new(),
        colors: Vec::new(),
        growth_factor: 1.0,
    }
}

fn stage(region: AirframeDamageRegion, threshold: f32, panel: &str, node: &str) -> WingFireStage {
    WingFireStage {
        region,
        threshold,
        panel: panel.to_owned(),
        effects: vec![WingFireEffect {
            effect: "loop_short_firetrail".to_owned(),
            node: node.to_owned(),
            delay_s: 0.0,
            emitters: vec![WingFireEmitter {
                name: "shortpuffer".to_owned(),
                emitter: emitter(),
                off_after_s: None,
            }],
        }],
    }
}

/// The player's ladder shape (port 0.50/0.15, starboard 0.15) with a single
/// emitter per effect; the retail test reads the real records.
fn definition() -> WingFireDefinition {
    WingFireDefinition {
        stages: vec![
            stage(AirframeDamageRegion::LeftWing, 0.5, "pdpanel5", "pdp5"),
            stage(AirframeDamageRegion::LeftWing, 0.15, "pdpanel3", "pdp3"),
            stage(AirframeDamageRegion::RightWing, 0.15, "pdpanel2", "pdp2"),
        ],
    }
}

struct Rig {
    app: App,
    actor: ActorId,
    sequence: u32,
}

fn rig() -> Rig {
    let mut app = headless_app_with(|app| {
        app.add_plugins(MissionWingFirePlugin);
    });
    app.update();
    let aircraft = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("the playtest aircraft");
    let session = SessionId::new(9).expect("a session id");
    let actor = ActorId { session, serial: 1 };
    let victim = attach_player_victim(app.world_mut(), aircraft, actor, SceneGeneration(1))
        .expect("the player becomes a victim");
    app.world_mut().insert_resource(victim);
    app.insert_resource(WingFireArt::from_handles(
        definition(),
        BTreeMap::new(),
        Handle::default(),
    ));
    for (name, x) in [("pdp5", -1.7), ("pdp3", -3.67), ("pdp2", 1.9)] {
        app.world_mut().spawn((
            WingFireNode {
                name: name.to_owned(),
            },
            Transform::from_xyz(x, 10.0, 0.0),
            GlobalTransform::from(Transform::from_xyz(x, 10.0, 0.0)),
        ));
    }
    Rig {
        app,
        actor,
        sequence: 0,
    }
}

impl Rig {
    /// A production damage-resolver hit that leaves `remaining` of the pool.
    fn hit_leaving(&mut self, region: AirframeDamageRegion, remaining: f64) {
        self.sequence += 1;
        let now = self
            .app
            .world()
            .resource::<SessionDamageResolver>()
            .resolver()
            .remaining_integrity(&self.actor, &wing_node(region))
            .expect("the region pool");
        let hit = HitEvent::try_new(
            EventId {
                session: self.actor.session,
                tick: Tick(1),
                producer: 997,
                sequence: self.sequence,
            },
            None,
            self.actor,
            wing_node(region),
            DamageChannel::Internal,
            now - DESIGNED_REGION_INTEGRITY * remaining,
        )
        .expect("a hit");
        self.app
            .world_mut()
            .resource_mut::<SessionDamageResolver>()
            .resolver_mut()
            .resolve(Tick(1), &[hit])
            .expect("the hit applies");
    }

    fn fixed_ticks(&mut self, count: u64) {
        for _ in 0..count {
            {
                let mut ledger = self.app.world_mut().resource_mut::<PhysicsTickLedger>();
                ledger.ticks += 1;
                ledger.timestep_s = 1.0 / HZ;
            }
            self.app.world_mut().run_schedule(FixedPostUpdate);
        }
    }

    fn burning_nodes(&self) -> Vec<String> {
        let state = self.app.world().resource::<WingFireState>();
        let art = self.app.world().resource::<WingFireArt>();
        let mut nodes: Vec<String> = burning_effects(state, art.definition())
            .into_iter()
            .map(|e| e.node)
            .collect();
        nodes.sort();
        nodes
    }

    fn puff_positions(&mut self) -> Vec<Vec3> {
        let world = self.app.world_mut();
        world
            .query::<(&WingFirePuff, &Transform)>()
            .iter(world)
            .map(|(_, t)| t.translation)
            .collect()
    }
}

#[test]
fn accept_crash_d_03_a_wing_past_the_threshold_burns_on_its_pdp_node_only() {
    let mut rig = rig();
    rig.fixed_ticks(5);
    assert!(
        rig.burning_nodes().is_empty(),
        "an intact airframe is not burning"
    );
    assert!(rig.puff_positions().is_empty());

    // 90 % of the port wing gone: both port rungs (0.50, 0.15) are crossed.
    rig.hit_leaving(AirframeDamageRegion::LeftWing, 0.10);
    rig.fixed_ticks(30);

    assert_eq!(rig.burning_nodes(), ["pdp3", "pdp5"]);
    let state = rig.app.world().resource::<WingFireState>();
    assert!(state.spawned_by_node.get("pdp3").copied().unwrap_or(0) > 0);
    assert!(state.spawned_by_node.get("pdp5").copied().unwrap_or(0) > 0);
    assert!(
        !state.spawned_by_node.contains_key("pdp2"),
        "the intact starboard wing does not burn"
    );
    let near_pdp3 = rig
        .puff_positions()
        .iter()
        .filter(|p| p.distance(Vec3::new(-3.67, 10.0, 0.0)) < 0.01)
        .count();
    assert!(near_pdp3 > 0, "puffs are born at the pdp3 node");
}

#[test]
fn accept_crash_d_03_the_threshold_is_the_stored_number_and_a_light_hit_does_not_burn() {
    assert!(threshold_reached(45.0, 300.0, 0.15));
    assert!(threshold_reached(30.0, 200.0, 0.15), "equal counts");
    assert!(!threshold_reached(46.0, 300.0, 0.15));
    assert!(!threshold_reached(1.0, 0.0, 0.15), "no pool, no fire");

    let mut rig = rig();
    rig.hit_leaving(AirframeDamageRegion::LeftWing, 0.70);
    rig.fixed_ticks(30);
    assert!(
        rig.burning_nodes().is_empty(),
        "0.70 remaining is above every rung"
    );
    // The rung at 0.50 starts when a second hit crosses it.
    rig.hit_leaving(AirframeDamageRegion::LeftWing, 0.40);
    rig.fixed_ticks(5);
    assert_eq!(rig.burning_nodes(), ["pdp5"]);
}

#[test]
fn accept_crash_d_03_the_fire_follows_the_moving_node_and_stays_on_a_static_wreck() {
    let mut rig = rig();
    rig.hit_leaving(AirframeDamageRegion::LeftWing, 0.10);
    rig.fixed_ticks(2);
    let before = rig
        .app
        .world()
        .resource::<WingFireState>()
        .spawned_by_node
        .get("pdp3")
        .copied()
        .unwrap_or(0);

    // The airframe moves the node 20 m along +x over 60 ticks.
    for i in 1..=60u32 {
        let x = -3.67 + 20.0 * i as f32 / 60.0;
        let mut query = rig
            .app
            .world_mut()
            .query::<(&WingFireNode, &mut GlobalTransform)>();
        for (node, mut global) in query.iter_mut(rig.app.world_mut()) {
            if node.name == "pdp3" {
                *global = GlobalTransform::from(Transform::from_xyz(x, 10.0, 0.0));
            }
        }
        rig.fixed_ticks(1);
    }
    let moved = rig.app.world().resource::<WingFireState>().spawned_by_node["pdp3"];
    assert!(
        moved > before + 20,
        "20 m of travel at 0.5 m spacing: {before} -> {moved}"
    );
    assert!(
        rig.puff_positions().iter().any(|p| p.x > 10.0),
        "puffs trail the moving node"
    );

    // The wreck stops: no travel, the fire still burns.
    rig.fixed_ticks(120);
    let wreck = rig.app.world().resource::<WingFireState>().spawned_by_node["pdp3"];
    assert!(
        wreck > moved + 4,
        "a static anchor is credited travel: {moved} -> {wreck}"
    );
    assert_eq!(rig.burning_nodes(), ["pdp3", "pdp5"]);
}

#[test]
fn accept_crash_d_03_emission_planning_credits_a_static_anchor() {
    let p = Vec3::new(1.0, 2.0, 3.0);
    let (points, carried) = plan_wing_emissions(p, p, 1.0, 0.0, 2.0);
    assert_eq!(points.len(), (WING_FIRE_MIN_TRAVEL_M_S / 2.0) as usize);
    assert!(points.iter().all(|q| *q == p));
    assert!(carried < 2.0);
    let (points, carried) =
        plan_wing_emissions(Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0), 0.0, 0.5, 1.0);
    assert_eq!(points.len(), 3);
    assert!((points[0].x - 0.5).abs() < 1e-5 && (carried - 0.5).abs() < 1e-5);
}

/// The real ladders, nodes, delays and emitters of `ZBD/zrdr.zbd`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_crash_d_03_retail_ladders_and_pdp_effects_are_read_from_the_archive() {
    let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let def = cs_app::mission_session::read_wing_fire_definition(&root).expect("the ladders");
    let ladder = |region| -> Vec<(f32, &str)> {
        def.stages
            .iter()
            .filter(|s| s.region == region)
            .map(|s| (s.threshold, s.panel.as_str()))
            .collect()
    };
    assert_eq!(
        ladder(AirframeDamageRegion::LeftWing),
        [(0.5, "pdpanel5"), (0.3, "pdpanel4"), (0.15, "pdpanel3")]
    );
    assert_eq!(
        ladder(AirframeDamageRegion::RightWing),
        [(0.4, "pdpanel6"), (0.3, "pdpanel1"), (0.15, "pdpanel2")]
    );
    let effects = |panel: &str| -> Vec<(&str, &str, f32)> {
        def.stages
            .iter()
            .find(|s| s.panel == panel)
            .expect(panel)
            .effects
            .iter()
            .map(|e| (e.effect.as_str(), e.node.as_str(), e.delay_s))
            .collect()
    };
    assert_eq!(
        effects("pdpanel3"),
        [
            ("small_fireball_follow", "pdp3", 0.0),
            ("loop_short_firetrail", "pdp3", 2.0)
        ]
    );
    assert_eq!(
        effects("pdpanel2"),
        [
            ("small_fireball_follow", "pdp2", 0.0),
            ("short_firetrail", "pdp2", 2.01)
        ]
    );
    let panel3 = def.stages.iter().find(|s| s.panel == "pdpanel3").unwrap();
    let fireball = &panel3.effects[0];
    assert_eq!(fireball.emitters.len(), 2);
    assert_eq!(fireball.emitters[0].name, "lgpuffer");
    assert_eq!(fireball.emitters[0].emitter.distance_interval, 1.5);
    assert_eq!(fireball.emitters[0].off_after_s, Some(1.0));
    let looped = &panel3.effects[1].emitters[0];
    assert_eq!(
        (looped.name.as_str(), looped.emitter.distance_interval),
        ("shortpuffer", 0.5)
    );
    assert_eq!(looped.emitter.frames.len(), 6);
    assert!(def.texture_names().contains(&"fire_f01".to_owned()));
}

/// The production composition: a hit on the real M01 player's left wing node
/// lights fire emitters on the drawn airframe's `pdp3` part and none on the
/// starboard side.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_crash_d_03_retail_m01_wing_hit_burns_at_the_drawn_pdp3_part() {
    use cs_app::mission_session::{build_headless, stage_for, teardown};

    let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = cs_app::mission_launch::plan_mission_launch(
        &root,
        crate::campaign::common::label("M01"),
        "The Lost Treasure",
    )
    .expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    assert!(
        stage.wing_fire.is_some(),
        "M01's retail stage carries the fire"
    );
    let mut app = build_headless(&stage).expect("M01 composes");
    for _ in 0..3 {
        app.update();
    }
    // The hand-over is a standstill (#1845) and the fire's puffs are
    // distance-driven, so this test puts the body in flight, as any moving
    // aircraft would be: 55 m/s along the nose, a value of this test only.
    {
        use avian3d::prelude::LinearVelocity;
        use bevy::prelude::Vec3;
        let world = app.world_mut();
        let mut bodies = world.query_filtered::<(
            &mut LinearVelocity,
            &mut cs_app::playtest::scene::PlaytestOriginalFlight,
            &avian3d::prelude::Rotation,
        ), bevy::prelude::With<cs_app::mission_session::MissionPlayerBody>>(
        );
        for (mut v, mut record, rotation) in bodies.iter_mut(world) {
            let nose: Vec3 = rotation.0 * Vec3::NEG_Z * 55.0;
            v.0 = nose;
            record.state.velocity_mps = nose.to_array().map(f64::from);
            record.state.throttle = record.command.throttle;
        }
    }
    let actor = app.world().resource::<PlayerVictim>().actor;
    let nodes: Vec<String> = {
        let world = app.world_mut();
        world
            .query::<&WingFireNode>()
            .iter(world)
            .map(|n| n.name.clone())
            .collect()
    };
    assert!(
        nodes.contains(&"pdp3".to_owned()),
        "pdp3 is a marked part: {nodes:?}"
    );
    assert!(
        app.world()
            .resource::<WingFireState>()
            .burning_stages()
            .next()
            .is_none()
    );

    let hit = HitEvent::try_new(
        EventId {
            session: actor.session,
            tick: Tick(1),
            producer: 997,
            sequence: 1,
        },
        None,
        actor,
        wing_node(AirframeDamageRegion::LeftWing),
        DamageChannel::Internal,
        DESIGNED_REGION_INTEGRITY * 0.9,
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .resolve(Tick(1), &[hit])
        .expect("the wing hit applies");
    for _ in 0..3 {
        {
            let mut ledger = app.world_mut().resource_mut::<PhysicsTickLedger>();
            ledger.ticks += 1;
            ledger.timestep_s = 1.0 / HZ;
        }
        app.world_mut().run_schedule(FixedPostUpdate);
    }
    let state = app.world().resource::<WingFireState>();
    let art = app.world().resource::<WingFireArt>();
    let burning = burning_effects(state, art.definition());
    assert!(
        burning
            .iter()
            .any(|e| e.node == "pdp3" && e.effect == "small_fireball_follow")
    );
    assert!(
        burning
            .iter()
            .all(|e| e.region == AirframeDamageRegion::LeftWing)
    );
    assert!(state.spawned_by_node.get("pdp3").copied().unwrap_or(0) > 0);
    assert!(!state.spawned_by_node.contains_key("pdp2"));
    teardown(&mut app);
}

/// #1970: tearing a composition down takes the wing-fire and damage-trail
/// plugins' resources and their live puffs with it.
#[test]
fn accept_crash_d_03_teardown_removes_particle_state_and_puffs() {
    use cs_app::mission_session::{
        DamageTrailArt, DamageTrailCursor, DamageTrailDefinition, DamageTrailReport,
        DamageTrailSource, DamageTrailSourceResource, TrailKind, TrailPuff, WingFireSource,
        WingFireSourceResource, teardown,
    };

    let mut rig = rig();
    let app = &mut rig.app;
    let puff = TrailPuff {
        kind: TrailKind::Smoke,
        born: 0,
        life_ticks: 100,
        origin: Vec3::ZERO,
        velocity: Vec3::ZERO,
        size: 1.0,
        friction: 0.0,
        cycle_index: 0,
    };
    app.world_mut().spawn(WingFirePuff {
        puff,
        emitter: [0; 3],
    });
    app.world_mut().spawn(puff);
    app.insert_resource(WingFireState::default());
    app.insert_resource(DamageTrailReport::default());
    app.insert_resource(DamageTrailCursor::default());
    app.insert_resource(DamageTrailArt::from_handles(
        DamageTrailDefinition {
            smoke: emitter(),
            fire: emitter(),
        },
        BTreeMap::new(),
        Handle::default(),
    ));
    app.insert_resource(WingFireSourceResource(WingFireSource {
        definition: definition(),
        images: Vec::new(),
        texture_archive: String::new(),
    }));
    app.insert_resource(DamageTrailSourceResource(DamageTrailSource {
        definition: DamageTrailDefinition {
            smoke: emitter(),
            fire: emitter(),
        },
        images: Vec::new(),
        texture_archive: String::new(),
    }));
    assert!(app.world().contains_resource::<PlayerVictim>());
    assert!(app.world().contains_resource::<WingFireArt>());

    teardown(app);

    let world = app.world_mut();
    assert!(!world.contains_resource::<PlayerVictim>());
    assert!(!world.contains_resource::<WingFireState>());
    assert!(!world.contains_resource::<WingFireArt>());
    assert!(!world.contains_resource::<WingFireSourceResource>());
    assert!(!world.contains_resource::<DamageTrailReport>());
    assert!(!world.contains_resource::<DamageTrailCursor>());
    assert!(!world.contains_resource::<DamageTrailArt>());
    assert!(!world.contains_resource::<DamageTrailSourceResource>());
    assert_eq!(world.query::<&WingFirePuff>().iter(world).count(), 0);
    assert_eq!(world.query::<&TrailPuff>().iter(world).count(), 0);
}
