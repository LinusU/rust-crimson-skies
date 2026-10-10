//! Acceptance stage VS-M01-RT-WINDOW: the windowed mission composition and
//! its no-window face (Rally #1215).
//!
//! [`cs_app::mission_session::build_headless`] is the composition this suite
//! drives: the same plugins and systems the `--mission` window runs, without
//! a window, a GPU or real time. Two things have to be proved about it:
//!
//! * **The composition attaches and tears down.** A synthetic stage — the
//!   production harbor world through `load_world`, a declared flight model
//!   and one announced load item — composes, steps, and leaves nothing
//!   behind after [`teardown`]: no [`WorldResidency`], no player body, no
//!   bound item entity. A second build in the same process starts from
//!   nothing. The synthetic members run in CI.
//! * **The retail plan reaches it.** Against the owner's installation, M01's
//!   measured plan is launchable and its stage composes headlessly — never
//!   `Blocked`, never `NoRuntime` (that arm no longer exists). The retail
//!   member is `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`, so
//!   CI skips it and the implementing and reviewing agents run it with
//!   `--include-ignored`.
//!
//! What is **not** claimed here: the player's airframe mesh is not drawn
//! (VS-M01-RT-PLAYER-AIRFRAME-VISUAL, #1216), and what the mission host
//! produces per tick (VS-M01-RT-MISSION-HOST, #1217) is measured by
//! `vs_m01_rt_host.rs` over the very stage this file builds — the synthetic
//! stage also carries the [`MissionHostSeed`](cs_app::mission_session::MissionHostSeed)
//! the composed entry drives, so one stage serves both suites. The flight law
//! is the statically recovered one — never `verified_original`, never
//! calibrated against an original run (#358).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::prelude::{Entity, Quat, With};
use cs_app::environment::{EnvironmentSession, RunSeeds};
use cs_app::loading::LoadTarget;
use cs_app::mission_launch::{MissionLaunchPlan, plan_mission_launch};
use cs_app::mission_markers::MissionMarkerBindings;
use cs_app::mission_session::{
    MissionContent, MissionFlight, MissionHostRefusal, MissionHostSeed, MissionPlayerBody,
    MissionPlayerStart, MissionStage, StageItem, StageMount, build_headless,
    no_declared_objectives, stage_for, teardown,
};
use cs_app::mission_start::StartPose;
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_app::playtest::{AircraftSpawner, fixed_ticks};
use cs_app::world::{HARBOR_OBJECT_HANGAR, harbor_meshes, harbor_world, residency, world_instance};
use cs_app::world_actors::LoweredWorldActors;
use cs_app::world_facts::MemberResolver;
use cs_content::world::WorldDefinition;
use cs_script::ir::{Condition, IR_VERSION, MissionProgram, Objective, SymbolId};
use cs_sim::flight::original::PROVENANCE_LABEL;
use cs_sim::flight::{OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_types::asset_id::{PrecedenceClass, WorldGroup};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

use crate::common::{cid, label};

/// The original installation, as the environment declares it.
pub(crate) fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: VS-M01-RT-WINDOW needs the retail capability; run this \
             suite with `--include-ignored` and CS_GAME_DIR pointing at the read-only \
             installation"
        )
    }))
}

/// A scratch tree that removes itself on drop — every synthetic member's
/// fixture installation, container file and private cache live under one
/// directory and vanish with it.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs_vs_m01_rt_window_{label}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch root is created");
        Self(root)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The declared flight model the synthetic stage flies: the recovered
/// original law's own parameter tables, authored here as synthetic values
/// and imported through the production `from_values` boundaries. These are
/// the same field tables `cs_sim`'s own fixtures declare; they prove the
/// composition path, nothing about retail parameters.
fn synthetic_flight() -> MissionFlight {
    let airframe = OriginalAirframe::from_values(&[
        ("roll_torque", 7.5),
        ("pitch_torque", 3.3),
        ("rudder_torque", 2.0),
        ("level_off_rate", 1.0),
        ("return_rate", 3.0),
        ("ang_momentum_damp", 5.0),
        ("rec_moments_inertia_x", 1.18),
        ("rec_moments_inertia_y", 1.0),
        ("rec_moments_inertia_z", 1.1),
        ("fd_speed", 135.0),
        ("engine_factor", 0.62),
        ("drag_factor", 0.37),
        ("veh_weight", 1900.0),
        ("ref_area", 330.0),
        ("gravity", 20.0),
    ])
    .expect("the declared airframe fields are complete");
    let globals = OriginalGlobals::from_values(&[
        ("nom_gravity", 20.0),
        ("lift_aoa_0_deg", 5.0),
        ("lift_aoa_1_deg", 9.0),
        ("max_aoa_deg", 46.0),
        ("high_g_0", 9.0),
        ("high_g_1", 15.0),
        ("low_g_0", -6.0),
        ("low_g_1", -9.0),
        ("lift_accel_rate", 0.75),
        ("stall_mag", 1.25),
        ("turn_fade_in_mph", 10.0),
        ("turn_fade_out_mph", 50.0),
        ("yaw_low_speed", 0.0625),
        ("yaw_high_speed", 0.17),
        ("yaw_fade_in_mph", 10.0),
        ("yaw_max_mph", 50.0),
        ("yaw_fade_out_mph", 400.0),
    ])
    .expect("the declared globals are complete");
    MissionFlight {
        model: OriginalFlightModel::full(airframe, globals),
        record: "synthetic".to_owned(),
        fuel: 1.0,
        inheritance_chain: Vec::new(),
        provenance: PROVENANCE_LABEL,
    }
}

/// The synthetic stage's environment: the fixture clear-sky definition on the
/// composition's own fixed timeline, seeded from a run parameter this file
/// declares. Nothing about it is original weather (F19's fixture is labelled
/// designed).
fn synthetic_environment() -> EnvironmentSession {
    let definition = cs_app::environment::fixture::clear_sky_environment()
        .expect("the fixture sky is well formed");
    EnvironmentSession::new(
        definition,
        MissionContent::tick_rate(),
        RunSeeds::from_root(1),
    )
    .expect("the fixture environment clock starts")
}

/// The synthetic stage's declared control program: one objective whose
/// condition is `true`, so the composed entry's script half completes it on
/// its first tick and the answer a test reads back can only come from the
/// program itself. Authored here as synthetic content — never a reading of
/// any original mission's record.
fn synthetic_control_program() -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-mission-host"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(ContentKind::Objective, "synthetic-mission-host-objective"),
            condition: Condition::Const(true),
            actions: Vec::new(),
            span: None,
        }],
    }
}

/// The synthetic stage's world-actor program: the harbor scope declares no
/// zeppelin, route or gate record, so the program is real but carries no
/// actors. It launches the production session — which then steps to the host
/// tick through [`cs_app::world_actors::WorldActorSession::step`] — and says
/// nothing about retail content.
fn synthetic_world_actors() -> LoweredWorldActors {
    LoweredWorldActors {
        ticks_per_second: cs_app::mission_world_actors::SESSION_TICKS_PER_SECOND,
        actors: Vec::new(),
        subjects: BTreeMap::new(),
        sockets: BTreeMap::new(),
        support: Vec::new(),
        pickups: Vec::new(),
        transitions: Vec::new(),
    }
}

/// The records the synthetic stage hands the mission host: the fixture
/// environment, the empty-but-real world-actor program, no animation join
/// (the fixture installation holds no `startanims.zrd`), the synthetic
/// control program, no objective declarations and no cue table.
fn synthetic_host_seed() -> MissionHostSeed {
    MissionHostSeed {
        environment: synthetic_environment(),
        world_actors: Some(synthetic_world_actors()),
        animation: None,
        control: synthetic_control_program(),
        sound_archives: Vec::new(),
        objectives: no_declared_objectives(),
        markers: MissionMarkerBindings::default(),
        resolver: MemberResolver::from_hierarchy([("harbor".to_owned(), None)]),
        refusals: vec![MissionHostRefusal::ObjectiveDeclarations {
            detail: "this synthetic stage declares no objective records of its own, so the F39 \
                     recovery is never asked for one"
                .to_owned(),
        }],
    }
}

/// A synthetic stage over the production harbor world: one announced load
/// item whose host bytes are a scratch file, so the whole LoadingSession
/// announce/attach path runs exactly as the retail composition runs it.
pub(crate) fn synthetic_stage(scratch: &Path) -> MissionStage {
    let definition: WorldDefinition =
        harbor_world().expect("the synthetic harbor world is well formed");
    let instance = world_instance(&definition, None, &[HARBOR_OBJECT_HANGAR], &[])
        .expect("a load record with one object is valid");
    let world_group =
        WorldGroup::new("zbd/synthetic").expect("the synthetic world group spelling is valid");
    // The mount host is the fixture installation; the private cache sits
    // beside it — `CacheDirectory` refuses a cache root inside the
    // installation, exactly as it refuses one inside `$CS_GAME_DIR`.
    let install = scratch.join("install");
    std::fs::create_dir_all(&install).expect("the fixture installation exists");
    let container = install.join("world.zbd");
    std::fs::write(&container, b"synthetic-mission-window-container")
        .expect("the scratch container is written");
    let bytes = std::fs::metadata(&container)
        .expect("the scratch container exists")
        .len();
    MissionStage {
        target: LoadTarget::world(world_group.clone()),
        mount: StageMount {
            id: "synthetic-mission-window",
            namespace: "world",
            precedence: PrecedenceClass::MissionWorld,
            directory: "zbd/synthetic".to_owned(),
            host: install,
            world_group,
            mission: None,
        },
        items: vec![StageItem {
            path: "world.zbd".to_owned(),
            content: ContentId::from_source(ContentKind::SceneNode, "container.synthetic.window")
                .expect("the synthetic content id is valid"),
            criticality: cs_app::loading::Criticality::GameplayCritical,
            bytes,
        }],
        world: definition,
        meshes: harbor_meshes(),
        instance,
        start: StartPose {
            position: [120.0, 340.0, -560.0],
            heading: 1.0,
        },
        flight: synthetic_flight(),
        label: "SYNTHETIC MISSION COMPOSITION".to_owned(),
        cache_root: scratch.join("cache"),
        installation: ContentHash::from_hex(&"5a".repeat(32))
            .expect("the synthetic installation hash is valid"),
        host: synthetic_host_seed(),
    }
}

/// Every entity that still carries a mission-composition marker, counted by
/// kind — the teardown's observable, read through the markers the
/// composition stamps.
fn mission_leftovers(app: &mut bevy::prelude::App) -> (usize, usize, usize) {
    let world = app.world_mut();
    let players = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .iter(world)
        .count();
    let flights = world
        .query_filtered::<Entity, With<PlaytestOriginalFlight>>()
        .iter(world)
        .count();
    let bound = world
        .query_filtered::<Entity, With<cs_app::loading::LoadedItemBinding>>()
        .iter(world)
        .count();
    (players, flights, bound)
}

/// **The synthetic composition builds, steps with the world resident and the
/// player body at the mission pose, and teardown leaves nothing behind.**
#[test]
fn accept_vs_m01_runtime_window_a_synthetic_world_composes_attaches_and_tears_down() {
    let scratch = Scratch::new("compose");
    let stage = synthetic_stage(scratch.path());
    let pose = stage.start;
    let mut app = build_headless(&stage).expect("the synthetic stage composes");

    // One step runs the Startup (player spawn) and the fixed loop.
    app.update();

    assert!(
        residency(app.world()).is_some(),
        "the stage's world must be resident after the first step"
    );
    assert!(
        fixed_ticks(&app) >= 1,
        "the composed app must run its fixed loop"
    );

    let world = app.world_mut();
    // The spawn recipe is the measured pose, exactly.
    let recipe = world.resource::<MissionPlayerStart>();
    assert_eq!(
        recipe.position_m, pose.position,
        "the player's spawn recipe must be the mission's measured start pose"
    );
    let mut players = world.query_filtered::<Entity, With<MissionPlayerBody>>();
    let player = players
        .iter(world)
        .next()
        .expect("the player body exists after Startup");
    let position = world
        .get::<Position>(player)
        .expect("the body has a position");
    // One frame of flight at the declared start speed is under a metre, so
    // the body has been at the measured pose this frame; the exact pose is
    // the recipe asserted above.
    for axis in 0..3 {
        assert!(
            (position.0.to_array()[axis] - pose.position[axis]).abs() < 2.0,
            "the player body must sit at the mission pose {:?}, got {:?}",
            pose.position,
            position.0.to_array()
        );
    }
    let rotation = world
        .get::<Rotation>(player)
        .expect("the body has a rotation");
    let expected = Quat::from_rotation_y(pose.heading);
    assert!(
        rotation.0.angle_between(expected) < 1.0e-2,
        "the player body must face the mission heading {} (diff {} rad, got {:?})",
        pose.heading,
        rotation.0.angle_between(expected),
        rotation.0
    );
    assert!(
        world.get::<PlaytestOriginalFlight>(player).is_some(),
        "the player body flies the original-law record, not the synthetic wing"
    );
    assert!(
        world.contains_resource::<AircraftSpawner>(),
        "the composition replaces the playtest's aircraft spawner"
    );
    let bound: usize = {
        let mut query = world.query_filtered::<Entity, With<cs_app::loading::LoadedItemBinding>>();
        query.iter(world).count()
    };
    assert_eq!(
        bound, 1,
        "the one announced load item must have attached through the ready bundle"
    );
    let velocity = world
        .get::<LinearVelocity>(player)
        .expect("the body has a velocity");
    assert!(
        velocity.0.length() > 0.0,
        "the player body must launch with the declared start speed"
    );

    teardown(&mut app);
    assert!(
        residency(app.world()).is_none(),
        "teardown must leave no WorldResidency"
    );
    let (players, flights, bound) = mission_leftovers(&mut app);
    assert_eq!(
        (players, flights, bound),
        (0, 0, 0),
        "teardown must leave no player body, no original-law record and no bound item entity"
    );
}

/// **A second composition in the same process starts from nothing: the
/// previous session's residency, entities and load identity never carry
/// over.**
#[test]
fn accept_vs_m01_runtime_window_a_second_composition_starts_from_nothing() {
    let scratch = Scratch::new("repeat");
    let stage = synthetic_stage(scratch.path());

    let mut first = build_headless(&stage).expect("the first composition builds");
    first.update();
    let first_session = {
        let world = first.world_mut();
        let mut query = world.query_filtered::<Entity, With<cs_app::loading::LoadedItemBinding>>();
        let entity = query
            .iter(world)
            .next()
            .expect("the first load attached an entity");
        world
            .get::<cs_app::loading::LoadedItemBinding>(entity)
            .expect("the binding exists")
            .load
    };
    teardown(&mut first);
    drop(first);

    let mut second = build_headless(&stage).expect("the second composition builds cleanly");
    second.update();
    assert!(
        residency(second.world()).is_some(),
        "the second composition's world is resident"
    );
    let (players, flights, bound) = mission_leftovers(&mut second);
    assert_eq!(
        (players, flights, bound),
        (1, 1, 1),
        "the second composition spawns exactly one player body and one bound item"
    );
    let second_session = {
        let world = second.world_mut();
        let mut query = world.query_filtered::<Entity, With<cs_app::loading::LoadedItemBinding>>();
        let entity = query
            .iter(world)
            .next()
            .expect("the second load attached an entity");
        world
            .get::<cs_app::loading::LoadedItemBinding>(entity)
            .expect("the binding exists")
            .load
    };
    assert_ne!(
        first_session.session, second_session.session,
        "each composition announces its own content-session generation: \
         a previous session's identity is never reused"
    );
    teardown(&mut second);
    assert!(residency(second.world()).is_none());
}

/// **Against the owner's installation, M01's measured plan is launchable and
/// its stage reaches the composition: never `Blocked`, never `NoRuntime`.**
///
/// `NoRuntime` no longer exists (#1215 removed it with the seam it named),
/// and the plan's closure is satisfied on current main — #1220 re-measured
/// `world_geometry` off #771's residual counters, so the task premise "all
/// 11 surfaces measure Satisfied" holds (this member ran against the
/// pre-#1220 base mid-task and saw the old `fvol*` reading; the correction
/// is recorded in
/// `docs/findings/2026-10-10-vs-m01-rt-window-composition.md`). The
/// windowed half is the reviewer's `gpu`-capability run of
/// `cs --cs-path <install> --mission M01`; a test can never open the real
/// window, so this member drives the very same stage through
/// [`build_headless`].
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_window_retail_m01_reaches_the_headless_composition() {
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    assert!(
        plan.launchable(),
        "every launch surface must be satisfied before the composition runs: {}",
        plan.gaps()
            .map(|gap| format!("{}: {}", gap.surface.label(), gap.verdict.describe()))
            .collect::<Vec<_>>()
            .join("; ")
    );

    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    let pose = stage.start;
    let mission_dir = plan.mission_dir.clone();
    drop(plan);
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    assert!(
        residency(app.world()).is_some(),
        "M01's own world must be resident in the composition"
    );
    let world = app.world_mut();
    let recipe = world.resource::<MissionPlayerStart>();
    assert_eq!(
        recipe.position_m, pose.position,
        "M01's spawn recipe must be the measured start pose of {mission_dir}"
    );
    let mut query = world.query_filtered::<Entity, With<MissionPlayerBody>>();
    let player = query
        .iter(world)
        .next()
        .expect("M01's player body exists at the mission pose");
    let position = world
        .get::<Position>(player)
        .expect("the body has a position");
    for axis in 0..3 {
        assert!(
            (position.0.to_array()[axis] - pose.position[axis]).abs() < 2.0,
            "M01's player body must sit at the measured start pose {:?}, got {:?} \
             ({mission_dir})",
            pose.position,
            position.0.to_array()
        );
    }
    assert!(
        world.get::<PlaytestOriginalFlight>(player).is_some(),
        "M01's player body flies the pdevastator law through the original-law driver"
    );

    teardown(&mut app);
    assert!(
        residency(app.world()).is_none(),
        "teardown must leave no WorldResidency"
    );
    let (players, flights, bound) = mission_leftovers(&mut app);
    assert_eq!(
        (players, flights, bound),
        (0, 0, 0),
        "teardown must leave nothing of {mission_dir} behind"
    );
}
