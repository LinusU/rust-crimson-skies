//! Acceptance scenarios F41-B follow-up (#531): the **spawn/bind path**
//! produces the emitter bindings the F41 consumer stack keys off, so a real
//! spawn over the real [`AudioPlugin`] sounds instead of mixing silence.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`; non-negotiable behaviors 1 and 3, and the F41-B minimum
//! scenario (AC02). Task test prefix: `accept_f41_b_`. Finding this closes:
//! `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md`.
//!
//! **No fixture below spawns an `AudioEmitterBinding` or an
//! `EngineVoiceFollow`.** Every binding asserted here is produced by
//! [`cs_app::audio::bind_spawned_emitters`] from what the production F15
//! handoff delivered:
//!
//! * the aircraft enters through the production [`spawn_flight_body`] path
//!   (the same call the scene and the mission's player spawn use), or through
//!   the production `spawn_body` path with only the original law on it (the
//!   shape the mission's `spawn_player` produces), and
//! * the world's emitter is the entity the production `ReadyBundle::attach`
//!   handoff spawned for the delivered environment loop.
//!
//! Everything else is production: the real plugin and its registered
//! schedule, the real `LoadTransaction`/`ExpectedLoad` handoff, the real F24-B
//! flight driver under a real fixed clock, and the recording device the mixer
//! mixes to. Every value is newly authored synthetic fixture content: no test
//! reads `CS_GAME_DIR`, and no original audio was involved.
//!
//! Sensitivity — each one measured by breaking the implementation, rerunning
//! the suite and reverting:
//!
//! * unregistering `bind_spawned_emitters` fails every scenario below that
//!   asserts a binding — 8 of the 9, all but the role-vocabulary one — and
//!   the sharpened assertion in `accept_f41_b_audio_wiring.rs`;
//! * dropping `EngineVoiceFollow` from the aircraft binding fails the three
//!   scenarios that need it: the throttle follow and the two that assert the
//!   marker is attached;
//! * resolving the aircraft's asset from the declared catalog alone (ignoring
//!   what the load delivered) names the earlier declared record and fails
//!   `accept_f41_b_spawn_path_prefers_the_delivered_engine_record`, whose
//!   catalog is built so the two rules name different records;
//! * reading `FlightAircraft` alone as the engine authority — the shape
//!   `smooth_engine_voices` had before this task — creates no voice for the
//!   original-law body, so its level stays at the `VoiceLevel::UNITY`
//!   fallback and
//!   `accept_f41_b_spawn_path_binds_a_body_flying_the_original_law` fails.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use cs_app::audio::{
    AudioBindLog, AudioBindRecord, AudioBindRefusal, AudioEmitterBinding, AudioEmitterRole,
    AudioPlugin, AudioSession, AudioSpatial, EngineVoiceFollow, EngineVoices, LoopRefusal,
    lower_bus, lower_mode,
};
use cs_app::loading::{
    CompletionVerdict, Criticality, ExpectedLoad, IoOutcome, LoadItem, LoadRequest, LoadState,
    LoadTarget, LoadTransaction, LoadedItemBinding,
};
use cs_app::physics::{
    BodyMode, BodySpec, FixtureBodySpec, FlightAircraft, FlightForcesPlugin, FlightSpawnSpec,
    PhysicsFixture, spawn_body, spawn_flight_body,
};
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_app::scene::{SceneGeneration, SceneGenerations};
use cs_content::audio::{
    AudioAssetRecord, AudioCatalog, AudioDraft, AudioLevel, AudioPlayback,
    declared_synthetic_audio_catalog,
};
use cs_sim::audio_events::{
    AudioBus, DeviceCommand, EmitterStopReason, Listener, PlaybackMode, RecordingAudioDevice,
    SpatialPolicy, VoiceLevel,
};
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::flight::original::FORCE_TO_ACCEL;
use cs_sim::flight::{
    EngineState, FlightInput, FlightModel, OriginalAirframe, OriginalFlightModel, OriginalGlobals,
    OriginalState, synthetic_fixed_wing,
};
use cs_types::asset_id::{AssetKey, ResolveContext, WorldGroup};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ContentHash};
use cs_types::net::SessionId;
use cs_types::space::Quaternion;

/// The fixed rate of the fixture worlds: one [`PhysicsFixture::step`] is one
/// fixed tick and one frame, so the smoothing below is driven by ticks.
const FIXED_HZ: u32 = 60;

/// The engine loop the synthetic catalog declares, on the engine bus.
const ENGINE_KEY: &str = "synthetic.engine.loop";

/// The environment loop the synthetic catalog declares, on the environment
/// bus: the world's ambient emitter.
const WIND_KEY: &str = "synthetic.environment.wind";

fn engine_asset() -> ContentId {
    ContentId::from_source(ContentKind::Sound, ENGINE_KEY).expect("a valid fixture id")
}

fn wind_asset() -> ContentId {
    ContentId::from_source(ContentKind::Sound, WIND_KEY).expect("a valid fixture id")
}

/// A second engine loop the **test-local** catalog declares, sorting before
/// [`ENGINE_KEY`]: no load ever delivers it, so a binding that names it was
/// resolved from the declared catalog alone. That is what makes the
/// delivered-first scenario below able to tell the two rules apart instead of
/// agreeing with both.
const EARLIER_ENGINE_KEY: &str = "synthetic.engine.alt";

fn earlier_engine_asset() -> ContentId {
    ContentId::from_source(ContentKind::Sound, EARLIER_ENGINE_KEY).expect("a valid fixture id")
}

/// A declared loop record for a test-local catalog: bus and mode known — the
/// only two fields a role reads — and the rest unmeasured, exactly as a
/// fixture may be. Nothing here reaches a session: a record the load did not
/// deliver is declared content the binding must *not* pick when the delivered
/// one exists.
fn declared_loop(
    key: &str,
    bus: cs_content::audio::AudioBus,
    mode: cs_content::audio::PlaybackMode,
) -> AudioAssetRecord {
    let claim = |field: &str| {
        ClaimId::new(&format!("f41b2.spawn.{key}.{field}")).expect("a valid claim id")
    };
    AudioAssetRecord::try_new(AudioDraft {
        id: ContentId::from_source(ContentKind::Sound, key).expect("a valid fixture id"),
        origin: Origin::SyntheticFixture,
        playback: AudioPlayback {
            bus: Resolved::Known(Known::new(bus, Provenance::designed(claim("bus")))),
            level: Resolved::Known(Known::new(
                AudioLevel::UNITY,
                Provenance::designed(claim("level")),
            )),
            mode: Resolved::Known(Known::new(mode, Provenance::designed(claim("mode")))),
        },
        decoded: Resolved::unknown(claim("decoded"), "not decoded").expect("a reason is present"),
        provenance: Provenance::designed(claim("record")),
    })
    .expect("the declared loop record is valid")
}

/// The declared original law this fixture's second body flies: the recovered
/// parameter tables authored here as **synthetic** values and imported through
/// the production `from_values` boundaries — the same tables `cs_app`'s
/// campaign fixture declares. Nothing in it is a retail parameter and the
/// scenario below claims nothing about the law; it needs a body whose engine
/// authority is `PlaytestOriginalFlight`, because that is the record the
/// mission's `spawn_player` inserts and no `FlightAircraft` record accompanies.
fn synthetic_original_law() -> OriginalFlightModel {
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
    OriginalFlightModel::full(airframe, globals)
}

/// The fixture spatial configuration, the same designed values the F41-B
/// wiring scenarios use. Nothing here measures an original attenuation curve.
fn spatial() -> AudioSpatial {
    AudioSpatial::new(
        SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy"),
        Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener"),
    )
}

/// A content session with nothing mounted: the generation a load runs under is
/// the only thing the audio handoff reads from it.
fn content_session() -> cs_assets::vfs::ContentSession {
    cs_assets::vfs::SessionBuilder::new(ResolveContext::new(ContentHash::from_bytes([0xF4; 32])))
        .open()
}

/// A digest for a delivered payload; only its identity matters to the load.
fn digest(byte: u8) -> ContentHash {
    ContentHash::from_bytes([byte; 32])
}

/// A load item for one declared audio record of the synthetic catalog.
fn sound_item(key: &str) -> LoadItem {
    LoadItem::new(
        AssetKey::from_spelling("synthetic", &format!("{key}.zbd"), "default")
            .expect("a valid fixture key"),
        ContentId::from_source(ContentKind::Sound, key).expect("a valid fixture id"),
        Criticality::GameplayCritical,
        64,
    )
    .expect("nonzero work units")
}

/// Runs one load over `items` to `Ready` and attaches its bundle to `world`
/// through the production [`ExpectedLoad`] handoff.
fn deliver(world: &mut World, items: Vec<LoadItem>) -> SessionId {
    let session = content_session().generation();
    let mut transaction = LoadTransaction::issue(LoadRequest {
        session,
        target: LoadTarget::world(WorldGroup::new("zbd/c1").expect("a valid world group")),
        items,
    });
    transaction.begin().expect("the load begins");
    for index in 0..transaction.items().len() {
        let ticket = transaction.issue_io(index).expect("the read is issued");
        let verdict = transaction.accept(ticket.complete(IoOutcome::Read {
            payload_sha256: digest(index as u8),
        }));
        assert_eq!(
            verdict,
            CompletionVerdict::Accepted,
            "item {index} settles as accepted"
        );
    }
    assert_eq!(
        transaction.state(),
        LoadState::Validating,
        "a fully settled load validates itself"
    );
    transaction.validate().expect("the load validates");
    let bundle = transaction.ready_bundle().expect("a ready bundle");
    let identity = bundle.identity();
    world.insert_resource(ExpectedLoad(identity));
    bundle
        .attach(world, identity)
        .expect("the announced bundle attaches");
    world.remove_resource::<ExpectedLoad>();
    SessionId::new(identity.session.get()).expect("a content session generation is nonzero")
}

/// A physics world running the production [`AudioPlugin`] over a recording
/// device, whose load delivered `items` through the production handoff.
struct SpawnWorld {
    fixture: PhysicsFixture,
    device: RecordingAudioDevice,
}

impl SpawnWorld {
    /// Builds the world over the declared synthetic catalog and installs the
    /// load's session: one step runs the `PreUpdate` handoff before anything
    /// below looks at it.
    fn new(items: Vec<LoadItem>) -> Self {
        Self::with_catalog(items, declared_synthetic_audio_catalog())
    }

    /// The same world over `catalog` instead of the shared synthetic one.
    ///
    /// The delivered closure is unchanged — only the records the handoff may
    /// lower from and the bind path may resolve against — which is what lets a
    /// scenario declare content no load delivers and watch which rule picks.
    fn with_catalog(items: Vec<LoadItem>, catalog: AudioCatalog) -> Self {
        let device = RecordingAudioDevice::new();
        let mut fixture = PhysicsFixture::builder(FixtureBodySpec::at_origin(1.0))
            .fixed_hz(FIXED_HZ)
            .configure({
                let device = device.clone();
                move |app| {
                    app.add_plugins(FlightForcesPlugin);
                    app.add_plugins(
                        AudioPlugin::new(catalog, spatial()).with_device(Box::new(device.handle())),
                    );
                    app.init_resource::<SceneGenerations>();
                    // The scene load path consumes a generation before its
                    // load; the fixture consumes one the same way, and the
                    // handoff stamps the session and every binding the spawn
                    // path produces with it.
                    app.world_mut()
                        .resource_mut::<SceneGenerations>()
                        .take_next();
                    deliver(app.world_mut(), items);
                }
            })
            .build()
            .expect("the fixture spec is valid");
        // One step: `insert_audio_session` runs in `PreUpdate` of it.
        fixture.step(1);
        Self { fixture, device }
    }

    /// Advances the world by `ticks` fixed ticks (each one frame).
    fn step(&mut self, ticks: u64) {
        self.fixture.step(ticks);
    }

    fn world(&self) -> &World {
        self.fixture.world()
    }

    fn world_mut(&mut self) -> &mut World {
        self.fixture.world_mut()
    }

    /// The session the handoff installed.
    fn session(&self) -> &AudioSession {
        self.world().resource::<AudioSession>()
    }

    /// The scene generation every binding this path produces carries.
    fn generation(&self) -> SceneGeneration {
        self.world().resource::<SceneGenerations>().latest()
    }

    /// What the spawn/bind path produced.
    fn bind_log(&self) -> &AudioBindLog {
        self.world().resource::<AudioBindLog>()
    }

    /// The spawn path's binding on `entity`, if it attached one.
    fn binding_of(&self, entity: Entity) -> Option<&AudioEmitterBinding> {
        self.world().entity(entity).get::<AudioEmitterBinding>()
    }

    /// Spawns an aircraft through the production flight path at `position_m`
    /// with `throttle` commanded and its spool already there.
    fn spawn_aircraft(&mut self, position_m: [f32; 3], throttle: f64) -> Entity {
        let mut spec = FlightSpawnSpec::level_at(position_m, [0.0; 3]);
        spec.engine = EngineState::direct(throttle);
        spec.command =
            FlightInput::try_new(0.0, 0.0, 0.0, throttle, false).expect("the throttle is in range");
        spawn_flight_body(
            self.world_mut(),
            FlightModel::new(synthetic_fixed_wing()),
            &spec,
        )
        .expect("the spawn spec is valid")
    }

    /// Spawns a body carrying **only** the recovered original law: the shape
    /// `spawn_original_flight_body` and the mission's `spawn_player` produce —
    /// one production [`spawn_body`] call with the record inserted on it — and
    /// no `FlightAircraft` record anywhere on it.
    ///
    /// `throttle` is the law's own actual throttle, which is this body's only
    /// engine authority: the designed F24 record the other scenarios read is
    /// exactly what this body must not carry.
    fn spawn_original_aircraft(&mut self, position_m: [f32; 3], throttle: f64) -> Entity {
        let model = synthetic_original_law();
        let spec = BodySpec {
            layer: CollisionLayer::Aircraft,
            shape: ShapeClass::Solid,
            mode: BodyMode::Dynamic,
            mass_kg: (model.airframe.veh_weight / FORCE_TO_ACCEL) as f32,
            half_extents_m: [6.0, 1.0, 5.0],
            position_m,
            linear_velocity_m_s: [0.0; 3],
        };
        let entity = spawn_body(self.world_mut(), &spec).expect("the body spawns");
        let record = PlaytestOriginalFlight {
            model,
            state: OriginalState {
                position_m: position_m.map(f64::from),
                velocity_mps: [0.0; 3],
                orientation: Quaternion::IDENTITY,
                angular_momentum_world: [0.0; 3],
                throttle,
                fuel: 1.0,
                level_off: false,
            },
            command: FlightInput {
                throttle,
                ..FlightInput::NEUTRAL
            },
            hold_attitude: false,
            provenance: "synthetic fixture: a body over the original law, for the bind path",
        };
        self.world_mut().entity_mut(entity).insert(record);
        entity
    }

    /// The delivered item entity presenting `content`, as the handoff spawned
    /// it.
    fn delivered_item(&mut self, content: &ContentId) -> Entity {
        let world = self.world_mut();
        let mut query = world.query::<(Entity, &LoadedItemBinding)>();
        query
            .iter(world)
            .find(|(_, item)| &item.content == content)
            .map(|(entity, _)| entity)
            .expect("the load delivered this content")
    }

    /// The level the engine voice of `entity`'s binding has reached.
    fn engine_level(&self, entity: Entity) -> VoiceLevel {
        let binding = self.binding_of(entity).expect("the spawn path bound it");
        self.world()
            .resource::<EngineVoices>()
            .level(&binding.emitter)
    }
}

/// The spawn path is the producer: an aircraft spawned through the production
/// flight path gets its binding, its engine voice and a loop on the device in
/// the one frame that spawns it — with nothing in the fixture inserting a
/// binding by hand.
#[test]
fn accept_f41_b_spawn_path_binds_the_aircraft_engine_loop() {
    let mut world = SpawnWorld::new(vec![sound_item(ENGINE_KEY)]);
    let aircraft = world.spawn_aircraft([0.0, 250.0, 0.0], 1.0);
    world.step(1);

    let binding = world
        .binding_of(aircraft)
        .expect("the spawn path attached the aircraft's binding");
    assert_eq!(binding.bus, AudioBus::Engine);
    assert_eq!(
        binding.asset,
        engine_asset(),
        "the binding names the declared engine record"
    );
    assert_eq!(
        binding.generation,
        world.generation(),
        "the binding is stamped with the load's scene generation"
    );
    assert!(
        world
            .world()
            .entity(aircraft)
            .get::<EngineVoiceFollow>()
            .is_some(),
        "an aircraft's engine loop follows the throttle"
    );
    assert!(
        world
            .session()
            .router
            .active_loop(&binding.emitter)
            .is_some(),
        "the loop system bound the spawn path's emitter"
    );
    assert_eq!(
        world.device.sounding(),
        1,
        "the loop reached the device in the spawning frame: {:?}",
        world.device.commands()
    );

    // Delivered first: the load delivered this engine loop, so the binding
    // names the delivered record rather than any other declared engine record.
    let session_id = world.session().router.session();
    assert_eq!(
        binding.emitter.session, session_id,
        "the emitter id is qualified by the installed session"
    );
    let installed = world
        .world()
        .resource::<cs_app::audio::AudioHandoffLog>()
        .installed
        .clone()
        .expect("the load installed a session");
    assert_eq!(installed.specs, 1, "exactly the delivered record lowered");

    // And the log says what was produced, by role.
    let record: &AudioBindRecord = world
        .bind_log()
        .bound
        .iter()
        .find(|record| record.entity == aircraft)
        .expect("the bind log records the aircraft");
    assert_eq!(record.role, AudioEmitterRole::Engine);
    assert_eq!(record.emitter, binding.emitter);
    assert_eq!(record.asset, binding.asset);
    assert!(
        world.bind_log().refusals.is_empty(),
        "nothing was refused: {:?}",
        world.bind_log().refusals
    );
    assert_eq!(
        world
            .world()
            .resource::<cs_app::audio::AudioEmitterIds>()
            .next_serial(),
        2,
        "one serial was issued, for one emitter entity"
    );
}

/// Delivered first, and **provably** so: the test-local catalog also declares
/// an engine loop that sorts before the delivered one and that no load ever
/// delivers, so the two resolution rules name different records and only one
/// of them is right. The binding must take the record the installed session
/// actually lowered, not the catalog's own lowest-id candidate.
#[test]
fn accept_f41_b_spawn_path_prefers_the_delivered_engine_record() {
    let mut catalog = declared_synthetic_audio_catalog();
    catalog
        .insert(declared_loop(
            EARLIER_ENGINE_KEY,
            cs_content::audio::AudioBus::Engine,
            cs_content::audio::PlaybackMode::Loop,
        ))
        .expect("the earlier engine loop's id is unique");

    // The scenario distinguishes the two rules only while the declared-first
    // candidate is the record no load delivers. Asserting that here means a
    // future catalog change that made the two rules agree again fails this
    // test instead of letting it pass for the wrong reason.
    let declared_first = catalog
        .iter()
        .filter(|record| {
            let Some(bus) = record.playback().bus.clone().known() else {
                return false;
            };
            let Some(mode) = record.playback().mode.clone().known() else {
                return false;
            };
            lower_bus(bus) == AudioBus::Engine && lower_mode(mode) == PlaybackMode::Loop
        })
        .map(|record| record.id().clone())
        .min();
    assert_eq!(
        declared_first,
        Some(earlier_engine_asset()),
        "the declared-first candidate is the record no load delivers"
    );
    assert_ne!(
        declared_first,
        Some(engine_asset()),
        "so the two resolution rules really do name different records"
    );

    let mut world = SpawnWorld::with_catalog(vec![sound_item(ENGINE_KEY)], catalog);
    let aircraft = world.spawn_aircraft([0.0, 250.0, 0.0], 1.0);
    world.step(1);

    let installed = world
        .world()
        .resource::<cs_app::audio::AudioHandoffLog>()
        .installed
        .clone()
        .expect("the load installed a session");
    assert_eq!(
        installed.specs, 1,
        "only the delivered engine record lowered; the earlier declared one stayed declared"
    );
    let binding = world
        .binding_of(aircraft)
        .expect("the aircraft still binds")
        .clone();
    assert_eq!(
        binding.asset,
        engine_asset(),
        "the delivered record wins over the earlier declared one"
    );
    let voices: Vec<ContentId> = world
        .device
        .voices()
        .iter()
        .map(|(_, start)| start.asset.clone())
        .collect();
    assert!(
        voices.contains(&engine_asset()) && !voices.contains(&earlier_engine_asset()),
        "the device plays the delivered record and never the declared-only one: {voices:?}"
    );
}

/// The engine voice follows the aircraft's throttle: an idle throttle holds
/// the idle level, opening it moves the voice up, and the level lives on the
/// emitter the spawn path named.
#[test]
fn accept_f41_b_spawn_path_engine_voice_follows_the_aircraft_throttle() {
    let mut world = SpawnWorld::new(vec![sound_item(ENGINE_KEY)]);
    let aircraft = world.spawn_aircraft([0.0, 250.0, 0.0], 0.0);
    world.step(12);

    let idle = world.engine_level(aircraft);
    let law = cs_sim::audio_events::EngineSmoothing::DESIGNED_DEFAULT;
    assert!(
        (idle.gain - law.target(true, 0.0).gain).abs() < 1e-9,
        "an idle throttle holds the designed idle level: {idle:?}"
    );

    // Open the throttle: the authority the F24-B driver holds (its spool) and
    // the command that holds it both move to full, and the voice follows that
    // measured engine state over fixed ticks rather than over frames.
    {
        let mut record = world
            .world_mut()
            .get_mut::<FlightAircraft>(aircraft)
            .expect("the spawned aircraft carries FlightAircraft");
        record
            .set_engine(EngineState::direct(1.0))
            .expect("full throttle is valid");
        record
            .set_command(FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("a valid command"))
            .expect("a valid command");
    }
    world.step(12);

    let open = world.engine_level(aircraft);
    assert!(
        open.gain > idle.gain && open.pitch > idle.pitch,
        "0.2 s of full throttle raises the voice above idle: {idle:?} -> {open:?}"
    );
    assert!(
        open.gain < law.target(true, 1.0).gain,
        "and has ramped part of the way rather than snapping to the target: {open:?}"
    );
}

/// The mission's player body carries **no** `FlightAircraft` record: only the
/// recovered original law, exactly as `spawn_original_flight_body` and the
/// mission's `spawn_player` insert it. That is the second engine authority the
/// bind path and `smooth_engine_voices` claim to read, so this body must bind,
/// sound, and follow the law's own actual throttle — with no designed-law
/// record on it to fall back to.
#[test]
fn accept_f41_b_spawn_path_binds_a_body_flying_the_original_law() {
    let mut world = SpawnWorld::new(vec![sound_item(ENGINE_KEY)]);
    let aircraft = world.spawn_original_aircraft([0.0, 250.0, 0.0], 0.0);
    world.step(12);

    assert!(
        world
            .world()
            .entity(aircraft)
            .get::<FlightAircraft>()
            .is_none(),
        "the original law is this body's only engine authority"
    );
    let binding = world
        .binding_of(aircraft)
        .expect("a body over the original law binds too")
        .clone();
    assert_eq!(binding.bus, AudioBus::Engine);
    assert_eq!(binding.asset, engine_asset());
    assert_eq!(binding.generation, world.generation());
    assert!(
        world
            .world()
            .entity(aircraft)
            .get::<EngineVoiceFollow>()
            .is_some(),
        "its engine loop follows the law's own throttle"
    );
    assert!(
        world
            .session()
            .router
            .active_loop(&binding.emitter)
            .is_some(),
        "the loop system bound it"
    );
    assert_eq!(
        world.device.sounding(),
        1,
        "the original-law body's loop reached the device: {:?}",
        world.device.commands()
    );

    let law = cs_sim::audio_events::EngineSmoothing::DESIGNED_DEFAULT;
    let idle = world.engine_level(aircraft);
    assert!(
        (idle.gain - law.target(true, 0.0).gain).abs() < 1e-9,
        "an idle law throttle holds the designed idle level: {idle:?}"
    );

    // The record's own actual throttle is the only thing that can move this
    // voice: there is no `FlightAircraft` spool on the body to read instead.
    world
        .world_mut()
        .get_mut::<PlaytestOriginalFlight>(aircraft)
        .expect("the body carries the original law")
        .state
        .throttle = 1.0;
    world.step(12);

    let open = world.engine_level(aircraft);
    assert!(
        open.gain > idle.gain && open.pitch > idle.pitch,
        "0.2 s of full throttle on the original law raises the voice: {idle:?} -> {open:?}"
    );
    assert!(
        open.gain < law.target(true, 1.0).gain,
        "and ramps part of the way rather than snapping: {open:?}"
    );
}

/// F41-B's minimum scenario through the spawn path: destroying the aircraft
/// ends its loop on the device, and the replacement spawn binds a fresh
/// emitter of its own.
#[test]
fn accept_f41_b_destroyed_aircraft_ends_loop_and_replacement_binds() {
    let mut world = SpawnWorld::new(vec![sound_item(ENGINE_KEY)]);
    let first = world.spawn_aircraft([0.0, 250.0, 0.0], 1.0);
    world.step(1);
    let first_binding = world.binding_of(first).expect("it bound").clone();
    assert_eq!(world.device.sounding(), 1);

    world.world_mut().entity_mut(first).despawn();
    world.step(1);
    assert!(
        world
            .session()
            .router
            .active_loop(&first_binding.emitter)
            .is_none(),
        "the destroyed aircraft's loop is gone"
    );
    assert!(
        world.device.commands().iter().any(|command| matches!(
            command,
            DeviceCommand::Stopped {
                reason: Some(EmitterStopReason::Despawned),
                ..
            }
        )),
        "the despawn reached the device: {:?}",
        world.device.commands()
    );
    assert_eq!(world.device.sounding(), 0, "and nothing is sounding");

    let replacement = world.spawn_aircraft([0.0, 250.0, -50.0], 1.0);
    world.step(1);
    let second_binding = world.binding_of(replacement).expect("it bound").clone();
    assert_ne!(
        first_binding.emitter, second_binding.emitter,
        "the replacement is a new emitter entity, so it takes a new serial"
    );
    assert_eq!(
        first_binding.emitter.session, second_binding.emitter.session,
        "both belong to the same session's serial space"
    );
    assert!(
        world
            .session()
            .router
            .active_loop(&second_binding.emitter)
            .is_some(),
        "the replacement's loop bound"
    );
    assert_eq!(
        world.device.sounding(),
        1,
        "the replacement reached the device: {:?}",
        world.device.commands()
    );
}

/// The world's emitter is produced the same way: the handoff's own delivered
/// entity for the environment loop carries the binding, loops to the device,
/// and stops when that entity leaves the world.
#[test]
fn accept_f41_b_spawn_path_binds_the_delivered_environment_loop() {
    let mut world = SpawnWorld::new(vec![sound_item(WIND_KEY)]);
    world.step(1);

    let item = world.delivered_item(&wind_asset());
    let binding = world
        .binding_of(item)
        .expect("the delivered environment loop is the world's emitter")
        .clone();
    assert_eq!(binding.bus, AudioBus::Environment);
    assert_eq!(binding.asset, wind_asset());
    assert_eq!(binding.generation, world.generation());
    assert!(
        world
            .world()
            .entity(item)
            .get::<EngineVoiceFollow>()
            .is_none(),
        "a world emitter's level is the loop's own, not an engine's"
    );
    assert!(
        world
            .session()
            .router
            .active_loop(&binding.emitter)
            .is_some(),
        "the loop system bound the world's emitter"
    );
    assert_eq!(
        world.device.sounding(),
        1,
        "the world's loop reached the device: {:?}",
        world.device.commands()
    );
    assert!(
        world
            .bind_log()
            .bound
            .iter()
            .any(|record| record.entity == item && record.role == AudioEmitterRole::Environment)
    );

    // The world's emitter lives and dies with the entity the load spawned.
    world.world_mut().entity_mut(item).despawn();
    world.step(1);
    assert!(
        world
            .session()
            .router
            .active_loop(&binding.emitter)
            .is_none()
    );
    assert_eq!(world.device.sounding(), 0);
}

/// Content the load did not deliver is still refused by name, now through the
/// production spawn path: the aircraft binds the declared engine record and
/// the session refuses that exact asset as `UnknownAsset`.
#[test]
fn accept_f41_b_spawn_path_refuses_the_undelivered_engine_asset_by_name() {
    let mut world = SpawnWorld::new(vec![sound_item(WIND_KEY)]);
    let aircraft = world.spawn_aircraft([0.0, 250.0, 0.0], 1.0);
    world.step(1);

    let binding = world
        .binding_of(aircraft)
        .expect("the spawn path still names the aircraft's engine loop");
    assert_eq!(
        binding.asset,
        engine_asset(),
        "the declared engine record is what the aircraft needs"
    );
    assert!(
        world.session().refusals.iter().any(|refusal| matches!(
            refusal,
            LoopRefusal::UnknownAsset { emitter, asset }
                if *emitter == binding.emitter && *asset == engine_asset()
        )),
        "the undelivered asset was refused by name: {:?}",
        world.session().refusals
    );
    assert!(
        !world
            .device
            .voices()
            .iter()
            .any(|(_, start)| start.asset == engine_asset()),
        "the refused engine loop reached no device voice: {:?}",
        world.device.voices()
    );
    // The delivered environment loop still plays: delivery, not the refusal,
    // decides what sounds.
    assert_eq!(
        world.device.sounding(),
        1,
        "only the delivered environment loop is sounding: {:?}",
        world.device.voices()
    );
}

/// A world whose catalog states no record for a role refuses by name instead
/// of inventing content: nothing to name means nothing to bind, and the gap
/// is recorded as the role's.
#[test]
fn accept_f41_b_spawn_path_records_a_role_with_no_declared_record() {
    // A catalog with only a one-shot: neither role has a loop to name, so no
    // binding may be produced at all.
    let mut catalog = cs_content::audio::AudioCatalog::new();
    catalog
        .insert(
            cs_content::audio::AudioAssetRecord::try_new(cs_content::audio::AudioDraft {
                id: ContentId::from_source(ContentKind::Sound, "synthetic.ui.click")
                    .expect("a valid id"),
                origin: cs_types::content::Origin::SyntheticFixture,
                playback: cs_content::audio::AudioPlayback {
                    bus: cs_types::content::Resolved::Known(cs_types::content::Known::new(
                        cs_content::audio::AudioBus::Ui,
                        cs_types::content::Provenance::designed(
                            cs_types::evidence::ClaimId::new("f41b2.spawn.no-role")
                                .expect("a valid claim"),
                        ),
                    )),
                    level: cs_types::content::Resolved::Known(cs_types::content::Known::new(
                        cs_content::audio::AudioLevel::UNITY,
                        cs_types::content::Provenance::designed(
                            cs_types::evidence::ClaimId::new("f41b2.spawn.no-role-level")
                                .expect("a valid claim"),
                        ),
                    )),
                    mode: cs_types::content::Resolved::Known(cs_types::content::Known::new(
                        cs_content::audio::PlaybackMode::OneShot,
                        cs_types::content::Provenance::designed(
                            cs_types::evidence::ClaimId::new("f41b2.spawn.no-role-mode")
                                .expect("a valid claim"),
                        ),
                    )),
                },
                decoded: cs_types::content::Resolved::unknown(
                    cs_types::evidence::ClaimId::new("f41b2.spawn.no-role-decoded")
                        .expect("a valid claim"),
                    "not decoded",
                )
                .expect("a reason is present"),
                provenance: cs_types::content::Provenance::designed(
                    cs_types::evidence::ClaimId::new("f41b2.spawn.no-role-record")
                        .expect("a valid claim"),
                ),
            })
            .expect("the record is valid"),
        )
        .expect("the id is unique");

    let device = RecordingAudioDevice::new();
    let mut fixture = PhysicsFixture::builder(FixtureBodySpec::at_origin(1.0))
        .fixed_hz(FIXED_HZ)
        .configure({
            let device = device.clone();
            move |app| {
                app.add_plugins(FlightForcesPlugin);
                app.add_plugins(
                    AudioPlugin::new(catalog, spatial()).with_device(Box::new(device.handle())),
                );
                app.init_resource::<SceneGenerations>();
                app.world_mut()
                    .resource_mut::<SceneGenerations>()
                    .take_next();
                deliver(app.world_mut(), vec![sound_item("synthetic.ui.click")]);
            }
        })
        .build()
        .expect("the fixture spec is valid");
    fixture.step(1);

    let mut world = SpawnWorld {
        fixture,
        device: device.handle(),
    };
    let aircraft = world.spawn_aircraft([0.0, 250.0, 0.0], 1.0);
    world.step(1);

    assert!(
        world.binding_of(aircraft).is_none(),
        "an aircraft with no declared engine loop gets no binding rather than a guessed one"
    );
    assert_eq!(world.device.sounding(), 0);
    let refusals = &world.bind_log().refusals;
    assert!(
        refusals.contains(&AudioBindRefusal::NoRoleAsset {
            role: AudioEmitterRole::Engine,
        }),
        "the missing role is named: {refusals:?}"
    );
    // The refusal is recorded once, not once per frame: five more frames of
    // the same gap must not grow the log.
    let before = refusals.len();
    world.step(5);
    assert_eq!(
        world.bind_log().refusals.len(),
        before,
        "a per-frame refusal would grow without bound"
    );
}

/// The role vocabulary is the closed one this path binds on, and its bus is
/// the runtime bus the loop system routes by.
#[test]
fn accept_f41_b_emitter_roles_name_the_buses_they_bind() {
    assert_eq!(AudioEmitterRole::ALL.len(), 2);
    assert_eq!(AudioEmitterRole::Engine.bus(), AudioBus::Engine);
    assert_eq!(AudioEmitterRole::Environment.bus(), AudioBus::Environment);
    assert_eq!(AudioEmitterRole::Engine.label(), "engine");
    assert_eq!(AudioEmitterRole::Environment.label(), "environment");

    // The declared record a role resolves to lowers onto that same bus and
    // mode, which is what makes "delivered first" a match on the runtime
    // values rather than on a label.
    let catalog = declared_synthetic_audio_catalog();
    let engine = catalog
        .get(&engine_asset())
        .expect("the fixture declares an engine loop");
    assert_eq!(
        lower_bus(engine.playback().bus.clone().known().expect("a known bus")),
        AudioBus::Engine
    );
    assert_eq!(
        lower_mode(
            engine
                .playback()
                .mode
                .clone()
                .known()
                .expect("a known mode")
        ),
        cs_sim::audio_events::PlaybackMode::Loop
    );
}
