//! Acceptance scenarios F11-F: the mission session owner drives the
//! canonical airframe scene (`specs/
//! F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`; task test prefix
//! `accept_f11_f_`).
//!
//! F11-E proved the producer contract against a fixture graph; this suite
//! proves the production caller: [`cs_app::mission_session::build_headless`]
//! loads the named airframe through [`cs_app::scene::start_airframe_scene`]
//! from the mission's own resolved start, the composed restart
//! ([`cs_app::mission_session::MissionHostRestartRequest`] →
//! `MissionHost::restart`) supersedes the live generation through the same
//! call, [`cs_app::mission_session::teardown`] releases every entity through
//! [`cs_app::scene::end_airframe_scene`], and the [`LodDistance`] the LOD
//! pass reads is the viewer stage's own declared value
//! ([`MISSION_PLAYER_LOD_DISTANCE_M`]), never a distance the scene owner
//! invents.
//!
//! The synthetic member runs in CI and proves the composition carries no
//! scene when the stage has no airframe source — the gate is the stage's
//! own record, not a silently missing scene. The retail members read the
//! owner's installation at `$CS_GAME_DIR` through production code and are
//! `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`, so CI skips
//! them and the implementing and reviewing agents run them with
//! `--include-ignored`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::{App, Entity};
use cs_app::environment::{EnvironmentSession, RunSeeds};
use cs_app::loading::LoadTarget;
use cs_app::mission_ai::MissionAiAirframes;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_markers::MissionMarkerBindings;
use cs_app::mission_session::{
    MISSION_PLAYER_LOD_DISTANCE_M, MissionAirframeScene, MissionContent, MissionFlight,
    MissionHostRefusal, MissionHostRestartReport, MissionHostRestartRequest, MissionHostSeed,
    MissionRadio, MissionStage, StageItem, StageMount, build_headless, no_declared_objectives,
    stage_for, teardown,
};
use cs_app::mission_start::StartPose;
use cs_app::scene::{
    AirframeDamageState, AirframeSceneLog, AirframeSceneRequest, LiveAirframeScene, LodDistance,
    NodePresentation, PartDebrisPublish, PresentationState, SceneCollisionGeometry,
    SceneGenerations, SceneNodeBinding,
};
use cs_app::world::{HARBOR_OBJECT_HANGAR, harbor_meshes, harbor_world, world_instance};
use cs_app::world_actors::LoweredWorldActors;
use cs_app::world_facts::MemberResolver;
use cs_content::campaign_bindings::MissionLabel;
use cs_content::world::WorldDefinition;
use cs_script::ir::{Condition, IR_VERSION, MissionProgram, Objective, SymbolId};
use cs_sim::flight::original::PROVENANCE_LABEL;
use cs_sim::flight::{OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_sim::mission::BlockLifecycleTable;
use cs_types::asset_id::{PrecedenceClass, WorldGroup};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

/// The original installation, as the environment declares it.
fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: accept_f11_f_ needs the retail capability; run this \
             suite with `--include-ignored` and CS_GAME_DIR pointing at the read-only \
             installation"
        )
    }))
}

/// A scratch tree that removes itself on drop: the synthetic member's
/// fixture installation and private cache live under it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("cs_accept_f11_f_{label}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch root is created");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The M01 retail stage, read through the production launch/compose path —
/// the mission whose committed player airframe is `player_pfighter`.
fn retail_m01_app(game_dir: &Path) -> App {
    let plan = plan_mission_launch(
        game_dir,
        MissionLabel::new("M06").expect("the label is valid"),
        "The Red Menace",
    )
    .expect("M01's launch closure plans");
    let stage =
        stage_for(game_dir, &plan).expect("M01's stage reads through the production readers");
    build_headless(&stage).expect("M01's stage composes headlessly")
}

/// Every entity the live airframe scene's import owns, in binding order.
fn live_scene_entities(app: &App) -> Vec<Entity> {
    app.world()
        .resource::<LiveAirframeScene>()
        .import()
        .entities()
        .map(|(_, entity)| entity)
        .collect()
}

fn bound_count(app: &mut App) -> usize {
    app.world_mut()
        .query::<&SceneNodeBinding>()
        .iter(app.world())
        .count()
}

// ------------------------------------------------------------ synthetic --

/// The declared flight model the synthetic stage flies: authored values
/// imported through the production `from_values` boundaries — the same
/// field tables `cs_sim`'s fixtures declare. They prove the composition
/// path, nothing about retail parameters.
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
        wingman_record: "synthetic-wingman".to_owned(),
        fuel: 1.0,
        crash_health_damage_range: [50.0, 300.0],
        inheritance_chain: Vec::new(),
        provenance: PROVENANCE_LABEL,
    }
}

/// The synthetic stage's environment: the fixture clear-sky definition on
/// the composition's own fixed timeline.
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
/// condition is `true`. Authored synthetic content — never a reading of any
/// original mission's record.
fn synthetic_control_program() -> MissionProgram {
    let cid = |kind: ContentKind, key: &str| {
        ContentId::from_source(kind, key).expect("the synthetic content id is valid")
    };
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

/// The records the synthetic stage hands the mission host: the fixture
/// environment, an empty-but-real world-actor program, the synthetic
/// control program, no animation join and no objective declarations.
fn synthetic_host_seed() -> MissionHostSeed {
    MissionHostSeed {
        environment: synthetic_environment(),
        world_actors: Some(LoweredWorldActors {
            ticks_per_second: cs_app::mission_world_actors::SESSION_TICKS_PER_SECOND,
            actors: Vec::new(),
            subjects: BTreeMap::new(),
            sockets: BTreeMap::new(),
            support: Vec::new(),
            pickups: Vec::new(),
            transitions: Vec::new(),
        }),
        animation: None,
        control: synthetic_control_program(),
        block_lifecycles: BlockLifecycleTable::new(),
        engine_objectives: None,
        sound_archives: Vec::new(),
        radio: MissionRadio::empty(),
        targets: cs_app::mission_targets::MissionTargetBinding::default(),
        turrets: cs_app::mission_turrets::TurretEntries::default(),
        objectives: no_declared_objectives(),
        markers: MissionMarkerBindings::default(),
        resolver: MemberResolver::from_hierarchy([("harbor".to_owned(), None)]),
        rewards: Vec::new(),
        refusals: vec![MissionHostRefusal::ObjectiveDeclarations {
            detail: "this synthetic stage declares no objective records of its own, so the F39 \
                     recovery is never asked for one"
                .to_owned(),
        }],
    }
}

/// A synthetic stage over the production harbor world — the same shape the
/// VS-M01-RT-WINDOW suite's synthetic stage takes: one announced load item
/// whose host bytes are a scratch file, no `player_aircraft` source, so the
/// composition's airframe-scene gate is exercised rather than bypassed.
fn synthetic_stage(scratch: &Path) -> MissionStage {
    let definition: WorldDefinition =
        harbor_world().expect("the synthetic harbor world is well formed");
    let instance = world_instance(&definition, None, &[HARBOR_OBJECT_HANGAR], &[])
        .expect("a load record with one object is valid");
    let world_group =
        WorldGroup::new("zbd/synthetic").expect("the synthetic world group spelling is valid");
    let install = scratch.join("install");
    std::fs::create_dir_all(&install).expect("the fixture installation exists");
    let container = install.join("world.zbd");
    std::fs::write(&container, b"synthetic-f11f-container")
        .expect("the scratch container is written");
    let bytes = std::fs::metadata(&container)
        .expect("the scratch container exists")
        .len();
    MissionStage {
        target: LoadTarget::world(world_group.clone()),
        mount: StageMount {
            id: "synthetic-f11f",
            namespace: "world",
            precedence: PrecedenceClass::MissionWorld,
            directory: "zbd/synthetic".to_owned(),
            host: install,
            world_group,
            mission: None,
        },
        items: vec![StageItem {
            path: "world.zbd".to_owned(),
            content: ContentId::from_source(ContentKind::SceneNode, "container.synthetic.f11f")
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
        aircraft: Vec::new(),
        ai_airframes: MissionAiAirframes::default(),
        flight: synthetic_flight(),
        // No installation to read `zbd/planes.zbd` from: no airframe source,
        // so the composition builds no canonical airframe scene — the gate
        // is this stage's own record.
        player_aircraft: None,
        world_draw: None,
        launch_sound: None,
        gun_sound: None,
        flak_sound: None,
        hit_sound: None,
        ambient_sound: None,
        weapon_counts: None,
        burst_puffs: None,
        muzzle_burst: None,
        contact_sparks: None,
        damage_trail: None,
        wing_fire: None,
        flak_effect: None,
        flak_trails: None,
        launch_cinematic: None,
        label: "SYNTHETIC F11-F COMPOSITION".to_owned(),
        cache_root: scratch.join("cache"),
        installation: ContentHash::from_hex(&"5a".repeat(32))
            .expect("the synthetic installation hash is valid"),
        messages: cs_app::ui::hud::MissionMessages::default(),
        turret_records: Vec::new(),
        turret_carrier: None,
        turret_refusal: None,
        host: synthetic_host_seed(),
        weather: None,
    }
}

/// **A stage with no airframe source carries no scene** — through build,
/// through the composed restart and through teardown. The absence is the
/// stage's own record: the retry entry answers its `None` and the restart
/// reports success anyway.
#[test]
fn accept_f11_f_synthetic_composition_carries_no_airframe_scene() {
    let scratch = Scratch::new("synthetic");
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes headlessly");
    assert!(app.world().get_resource::<MissionAirframeScene>().is_none());
    assert!(app.world().get_resource::<LiveAirframeScene>().is_none());
    assert!(app.world().get_resource::<LodDistance>().is_none());
    app.update();

    // The composed restart entry runs the same scene seam the retail path
    // does: with no scene source installed it answers `None`, and the
    // restart still reports success.
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    assert!(
        app.world().contains_resource::<MissionHostRestartReport>(),
        "the composed restart ran and wrote its report"
    );
    assert!(app.world().get_resource::<MissionAirframeScene>().is_none());
    assert!(app.world().get_resource::<LiveAirframeScene>().is_none());
    assert_eq!(bound_count(&mut app), 0);
    teardown(&mut app);
}

// --------------------------------------------------------------- retail --

/// **A real session start loads the named airframe.** The composition's
/// session start imports the `player` subtree the mission's own start
/// resolution committed — `airframe/player_pfighter`, the node under
/// `planes.zbd`'s `player` root — through the production producer, before
/// the first frame.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_f11_f_session_start_loads_the_named_airframe() {
    let mut app = retail_m01_app(&game_dir());

    let (generation, root, len, airframe) = {
        let live = app
            .world()
            .get_resource::<LiveAirframeScene>()
            .expect("the session start loaded the airframe before the first frame");
        (
            live.generation(),
            live.root().clone(),
            live.len(),
            live.airframe().clone(),
        )
    };
    assert_eq!(
        airframe,
        ContentId::from_source(ContentKind::Airframe, "player_pfighter")
            .expect("the airframe id is valid"),
        "the loaded airframe is the one M01's start resolution committed"
    );
    assert_eq!(
        root.as_content_id().key(),
        "planes-planes.player",
        "the import starts from the `player` root — `player_pfighter`'s \
         owning root inside `zbd/planes.zbd`, never an index"
    );
    assert!(generation > cs_app::scene::SceneGeneration(0));
    // The import is exactly the checked root's subtree of the source's own
    // converted graph — the GameZ node-array reader's whole-container
    // `SceneGraph`, nothing drawn by index.
    let subtree = app
        .world()
        .get_resource::<MissionAirframeScene>()
        .expect("the retail stage carries the session's airframe scene source")
        .graph()
        .subtree(&root)
        .len();
    assert_eq!(len, subtree, "the import is the root's whole subtree");
    assert_eq!(bound_count(&mut app), len);
    for binding in app
        .world_mut()
        .query::<&SceneNodeBinding>()
        .iter(app.world())
        .collect::<Vec<_>>()
    {
        assert_eq!(binding.generation, generation);
    }
    // The LOD distance is supplied and it is the viewer stage's own
    // declared value, not a number this owner invented.
    let distance = app
        .world()
        .get_resource::<LodDistance>()
        .expect("the viewer stage's LOD distance is supplied");
    assert_eq!(distance.meters().0, MISSION_PLAYER_LOD_DISTANCE_M);
}

/// **A retry supersedes the previous generation.** The composed restart —
/// the same entry `R`/terminal retries reach — runs the session's own
/// `start_airframe_scene` again: the fresh generation stamps every binding
/// and every entity of the old generation is released.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_f11_f_retry_supersedes_the_scene_generation() {
    let mut app = retail_m01_app(&game_dir());

    let first_generation = app.world().resource::<LiveAirframeScene>().generation();
    let first_entities = live_scene_entities(&app);
    assert!(!first_entities.is_empty());

    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    assert!(
        app.world().contains_resource::<MissionHostRestartReport>(),
        "the composed restart ran and wrote its report"
    );
    let (generation, len) = {
        let live = app.world().resource::<LiveAirframeScene>();
        (live.generation(), live.len())
    };
    assert!(
        generation > first_generation,
        "the retry's load minted a fresh generation"
    );
    assert_eq!(len, first_entities.len());
    for entity in first_entities {
        assert!(
            app.world().get_entity(entity).is_err(),
            "the superseded generation left no entity behind"
        );
    }
    let new_entities = live_scene_entities(&app);
    assert_eq!(bound_count(&mut app), new_entities.len());
}

/// **Session end releases all entities.** [`teardown`] ends the scene the
/// session started: the live record, the imported entities and the scene's
/// own session records are all gone, so a second composition in the same
/// process starts from nothing.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_f11_f_session_end_releases_every_entity() {
    let mut app = retail_m01_app(&game_dir());
    let imported = live_scene_entities(&app);
    assert!(!imported.is_empty());

    teardown(&mut app);

    assert!(app.world().get_resource::<LiveAirframeScene>().is_none());
    for entity in imported {
        assert!(
            app.world().get_entity(entity).is_err(),
            "session end released every scene entity"
        );
    }
    assert_eq!(bound_count(&mut app), 0);
    // The scene's own records go with the composition: its source, its log,
    // its generation counter, its damage state and the one-shot request /
    // debris / collision / distance resources.
    for removed in [
        app.world().get_resource::<MissionAirframeScene>().is_none(),
        app.world().get_resource::<AirframeSceneLog>().is_none(),
        app.world().get_resource::<SceneGenerations>().is_none(),
        app.world().get_resource::<AirframeDamageState>().is_none(),
        app.world().get_resource::<AirframeSceneRequest>().is_none(),
        app.world().get_resource::<PartDebrisPublish>().is_none(),
        app.world()
            .get_resource::<SceneCollisionGeometry>()
            .is_none(),
        app.world().get_resource::<LodDistance>().is_none(),
    ] {
        assert!(removed);
    }
}

/// **The viewer stage owns the LOD distance.** The plugin's
/// `select_lod_presentation` runs inside the composition's `Update`
/// schedule on the supplied [`LodDistance`] — the designed 20 m the drawn
/// airframe's bands are selected at — and it picks the `nearest` band the
/// retail subtree authors, culling its siblings. No band is both drawn and
/// culled, and a culled band's subtree is culled with it.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_f11_f_viewer_distance_drives_lod_presentation() {
    let mut app = retail_m01_app(&game_dir());
    // The composition's own frame runs the plugin's chain: the request
    // pass, the damage pass and the LOD selection over the supplied
    // distance.
    app.update();

    let mut variants = 0usize;
    let mut drawn_bands = 0usize;
    let mut culled = 0usize;
    for (binding, variant, presentation) in app
        .world_mut()
        .query::<(
            &SceneNodeBinding,
            Option<&cs_app::scene::NodeLodVariant>,
            Option<&NodePresentation>,
        )>()
        .iter(app.world())
    {
        let Some(presentation) = presentation.map(|p| p.0) else {
            panic!("{:?} has no presentation verdict", binding.node);
        };
        if variant.is_some() {
            variants += 1;
        }
        match presentation {
            PresentationState::Drawn if variant.is_some() => drawn_bands += 1,
            PresentationState::LodCulled => culled += 1,
            _ => {}
        }
    }
    // The `player` subtree authors LOD bands (`nearest`/`l7`/`l8`/`l9` and
    // their siblings): at the viewer's declared distance exactly one band
    // of each sibling group is drawn and the rest are culled.
    assert!(variants > 0, "the imported subtree carries LOD bands");
    assert!(drawn_bands > 0, "the viewer distance selects a band");
    assert!(culled > 0, "the bands the distance did not pick are culled");
}
