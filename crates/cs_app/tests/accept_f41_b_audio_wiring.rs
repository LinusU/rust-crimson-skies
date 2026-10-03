//! Acceptance scenarios F41-B follow-up (#445): the F41 audio stack is wired
//! into a running app — the loading handoff installs the session, the systems
//! run in the schedule without a caller registering them, engine pitch and
//! volume follow the flight model's throttle at a fixed rate, and the mixer
//! carries the session's outcomes and the spatial law to a device while
//! simulation stays independent of that device.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-B`; non-negotiable behaviors 1, 2 and 3. Task test prefix:
//! `accept_f41_b_`. Finding this closes:
//! `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md`.
//!
//! These drive production code only: the real [`AudioPlugin`] over a real Bevy
//! `App` with the real fixed-step schedules, the real F15 [`LoadTransaction`] and
//! its controlled [`ExpectedLoad`] handoff, the production
//! [`cs_sim::audio_events::RecordingAudioDevice`] as the device, and a real
//! flight body whose throttle comes from the production F24-B driver. Every
//! value is newly authored synthetic fixture content: no test reads
//! `CS_GAME_DIR`, and no original audio was involved.
//!
//! The sensitivity of each scenario is direct. Removing `insert_audio_session`
//! from the plugin leaves the world with no session; removing the schedule
//! registration leaves loops unsynced; smoothing on a render delta instead of
//! `Time<Fixed>` changes the engine mix; dropping the outcomes drain leaves the
//! device silent.

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::prelude::{MinimalPlugins, Transform, TransformPlugin};
use cs_app::audio::{
    AudioEmitterBinding, AudioHandoffLog, AudioHandoffRefusal, AudioMixReport, AudioPlugin,
    AudioSession, AudioSpatial, EngineVoiceFollow, LoopRefusal, device_lost, device_restored,
};
use cs_app::loading::{
    CompletionVerdict, Criticality, ExpectedLoad, IoOutcome, LoadItem, LoadRequest, LoadState,
    LoadTarget, LoadTransaction,
};
use cs_app::physics::{
    FixtureBodySpec, FlightAircraft, FlightForcesPlugin, FlightSpawnSpec, PhysicsFixture,
    spawn_flight_body,
};
use cs_app::scene::{SceneGeneration, SceneGenerations};
use cs_content::audio::declared_synthetic_audio_catalog;
use cs_sim::audio_events::{
    AudioBus, AudioDevice, AudioEmitterId, DeviceCommand, EmitterStopReason, Listener,
    RecordingAudioDevice, SpatialPolicy, VoiceLevel,
};
use cs_sim::flight::{EngineState, FlightInput, FlightModel, synthetic_fixed_wing};
use cs_types::asset_id::{AssetKey, ResolveContext, WorldGroup};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::net::SessionId;

/// The fixed rate of the audio-sensitive worlds. The smoothing scenario runs a
/// second world at twice this rate and compares, so no single rate is "the"
/// rate.
const FIXED_HZ: u32 = 60;

/// How close two engine levels must be for the fixed-rate assertion.
///
/// `Time<Fixed>` carries its timestep in `f32` seconds, so a 1/60 s and a
/// 1/120 s step differ by an ulp of that representation before any audio code
/// runs. That is the floor of what the ECS path can be compared at; the
/// smoothing law itself is exact to summation order, which
/// `accept_f41_b_smoothing_is_step_invariant` measures on the `cs_sim` side.
const CLOCK_EPSILON: f64 = 1e-6;

/// The engine loop the synthetic catalog declares, on the engine bus.
const ENGINE_KEY: &str = "synthetic.engine.loop";

fn engine_asset() -> ContentId {
    ContentId::from_source(ContentKind::Sound, ENGINE_KEY).expect("a valid fixture id")
}

/// The fixture spatial configuration: full gain inside 10 m, inverse distance
/// to 100 m, silence beyond, and a listener at the origin whose right axis is
/// canonical `+X`.
///
/// Designed, like every spatial value in the workspace: the original
/// attenuation curve is unmeasured and nothing here claims otherwise.
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
/// through the production [`ExpectedLoad`] handoff, returning the bundle's
/// session generation as the shared `SessionId` the audio session binds to.
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
    let generation = SessionId::new(bundle.identity().session.get())
        .expect("a content session generation is nonzero");
    // The controlled handoff: announce, attach, consume the expectation.
    world.insert_resource(ExpectedLoad(bundle.identity()));
    bundle
        .attach(world, bundle.identity())
        .expect("the announced bundle attaches");
    world.remove_resource::<ExpectedLoad>();
    generation
}

/// A world with the production audio plugin whose load delivered `items`, and
/// a handle onto the recording device the plugin mixed to.
struct AudioWorld {
    app: App,
    /// The session generation the delivered load ran under.
    session: SessionId,
    /// The scene generation the load path consumed.
    generation: SceneGeneration,
    /// A second handle onto the plugin's device.
    device: RecordingAudioDevice,
}

impl AudioWorld {
    /// Builds the world, delivering a load over `items`.
    fn new(items: Vec<LoadItem>) -> Self {
        let device = RecordingAudioDevice::new();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AudioPlugin::new(declared_synthetic_audio_catalog(), spatial())
                .with_device(Box::new(device.handle())),
        ));
        app.init_resource::<SceneGenerations>();
        // The scene load path consumes a generation before its load, and the
        // emitter bindings it spawns carry that number; the audio handoff reads
        // the same counter, so the fixture consumes one the same way.
        let generation = app
            .world_mut()
            .resource_mut::<SceneGenerations>()
            .take_next();
        let session = deliver(app.world_mut(), items);
        Self {
            app,
            session,
            generation,
            device: device.handle(),
        }
    }

    /// The scene generation the installed session is stamped with.
    fn generation(&self) -> SceneGeneration {
        self.generation
    }

    /// The session-qualified emitter `serial` names.
    fn emitter(&self, serial: u64) -> AudioEmitterId {
        AudioEmitterId {
            session: self.session,
            serial,
        }
    }

    /// The session the handoff installed.
    fn session(&self) -> &AudioSession {
        self.app.world().resource::<AudioSession>()
    }

    /// The handoff's own record.
    fn log(&self) -> &AudioHandoffLog {
        self.app.world().resource::<AudioHandoffLog>()
    }

    /// The last mix pass.
    fn mix(&self) -> &AudioMixReport {
        self.app.world().resource::<AudioMixReport>()
    }

    /// Spawns an engine emitter at `position_m`, with or without a pose.
    fn spawn_engine_emitter(&mut self, serial: u64, position_m: Option<[f32; 3]>) -> Entity {
        let binding = AudioEmitterBinding {
            emitter: self.emitter(serial),
            bus: AudioBus::Engine,
            asset: engine_asset(),
            generation: self.generation(),
        };
        match position_m {
            Some(position) => self
                .app
                .world_mut()
                .spawn((binding, Transform::from_translation(position.into())))
                .id(),
            None => self.app.world_mut().spawn(binding).id(),
        }
    }
}

/// The loading handoff owns the session, and the plugin registers the loop
/// systems: nothing below adds a system or inserts a session by hand, so the
/// F41-B minimum scenario runs because of the wiring rather than because a test
/// set it up.
#[test]
fn accept_f41_b_loading_handoff_installs_the_session_and_syncs_loops() {
    let mut world = AudioWorld::new(vec![sound_item(ENGINE_KEY)]);
    world.app.update();

    // One update is enough: `insert_audio_session` runs in `PreUpdate`, before
    // the loop systems of the same frame.
    let installed = world
        .log()
        .installed
        .clone()
        .expect("the delivered load installed an audio session");
    assert_eq!(installed.session, world.session);
    assert_eq!(installed.generation, SceneGeneration(1));
    assert_eq!(installed.specs, 1, "one delivered record lowered");
    assert_eq!(installed.refused, 0);
    assert_eq!(
        world.session().router.session(),
        world.session,
        "the session's router is bound to the delivered session generation"
    );

    let engine = world.spawn_engine_emitter(1, Some([0.0, 0.0, 0.0]));
    world.app.update();
    assert!(
        world
            .session()
            .router
            .active_loop(&world.emitter(1))
            .is_some(),
        "the registered loop system bound the delivered engine loop"
    );
    assert_eq!(
        world.device.sounding(),
        1,
        "the registered mix pass carried the loop to the device: {:?}",
        world.device.commands()
    );

    // Destroying the aircraft ends its loop in the same wiring (the F41-B
    // minimum scenario, now through the schedule rather than a hand-built App).
    world.app.world_mut().entity_mut(engine).despawn();
    world.app.update();
    assert!(
        world
            .session()
            .router
            .active_loop(&world.emitter(1))
            .is_none()
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
    assert_eq!(world.device.sounding(), 0);
}

/// Content the load did not deliver is not in the session, so an emitter naming
/// it is refused by name instead of scheduling an asset that is not there.
#[test]
fn accept_f41_b_undelivered_sound_is_refused_by_name() {
    let mut world = AudioWorld::new(vec![sound_item("synthetic.environment.wind")]);
    world.app.update();
    let installed = world
        .log()
        .installed
        .clone()
        .expect("a session is installed");
    assert_eq!(installed.specs, 1);
    world.spawn_engine_emitter(1, Some([0.0, 0.0, 0.0]));
    world.app.update();
    assert!(
        matches!(
            world.session().refusals[0],
            LoopRefusal::UnknownAsset { .. }
        ),
        "the engine loop was refused: {:?}",
        world.session().refusals
    );
    assert_eq!(world.device.sounding(), 0, "nothing was played");
}

/// A delivered audio content with no declared record is refused by the handoff
/// itself, and named in its log.
#[test]
fn accept_f41_b_undeclared_delivered_sound_is_refused_by_the_handoff() {
    let mut world = AudioWorld::new(vec![sound_item("synthetic.not.declared")]);
    world.app.update();
    let installed = world
        .log()
        .installed
        .clone()
        .expect("a session is installed");
    assert_eq!(installed.specs, 0);
    assert_eq!(installed.refused, 1);
    assert!(
        world
            .log()
            .refusals
            .iter()
            .any(|refusal| matches!(refusal, AudioHandoffRefusal::UndeclaredAsset { .. })),
        "the undeclared record is named: {:?}",
        world.log().refusals
    );
    world.spawn_engine_emitter(1, Some([0.0, 0.0, 0.0]));
    world.app.update();
    assert_eq!(world.device.sounding(), 0);
}

/// A world with no delivered load has no audio session at all, and the systems
/// the plugin registered do nothing rather than panic on a missing resource.
///
/// The device-failure path is total even here: there is no mixer to stop, but
/// the output is still the world's, so losing it closes it and restoring it
/// opens it. A device the caller declared gone must be observably gone in every
/// world, not only in one that happens to have loaded something.
#[test]
fn accept_f41_b_unloaded_world_has_no_session_and_mixes_nothing() {
    let device = RecordingAudioDevice::new();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AudioPlugin::new(declared_synthetic_audio_catalog(), spatial())
            .with_device(Box::new(device.handle())),
    ));
    app.update();
    app.update();
    assert!(
        app.world().get_resource::<AudioSession>().is_none(),
        "an unloaded world owns no audio session"
    );
    assert_eq!(
        app.world().resource::<AudioMixReport>().passes,
        0,
        "and the mixer never ran"
    );

    device_lost(app.world_mut());
    assert!(
        device.commands().contains(&DeviceCommand::Closed),
        "the output closed even with nothing loaded: {:?}",
        device.commands()
    );
    assert!(!device.is_open(), "and it stayed closed");
    device_restored(app.world_mut()).expect("an empty output still opens");
    assert!(
        device.is_open(),
        "a restore works without a mixer: {:?}",
        device.commands()
    );
    assert!(!device.commands().is_empty(), "the device was told");
}

/// The loader and the mixer are one consumer pair: a reload installs a session
/// for the new generation, and the previous load's voices are stopped instead
/// of being left audible.
#[test]
fn accept_f41_b_reload_replaces_the_session_and_stops_the_old_voices() {
    let mut world = AudioWorld::new(vec![sound_item(ENGINE_KEY)]);
    world.app.update();
    world.spawn_engine_emitter(1, Some([0.0, 0.0, 0.0]));
    world.app.update();
    assert_eq!(world.device.sounding(), 1);
    let first = world.session().router.session();

    // A second load in a *new* content session, delivered into the same world.
    let generation = deliver(world.app.world_mut(), vec![sound_item(ENGINE_KEY)]);
    assert_ne!(generation, first, "a reload runs in a new content session");
    world.device.clear();
    world.app.update();
    assert!(
        world
            .log()
            .refusals
            .iter()
            .any(|refusal| matches!(refusal, AudioHandoffRefusal::ReplacedLoads { .. })),
        "the superseded load is named: {:?}",
        world.log().refusals
    );
    assert_eq!(world.session().router.session(), generation);
    assert_eq!(world.log().installs, 2);
    // The load the new session replaced released its voices: the previous load
    // is not left audible with nothing left to stop it.
    assert!(
        world
            .device
            .commands()
            .iter()
            .any(|command| matches!(command, DeviceCommand::Stopped { .. })),
        "the previous load's voice was stopped at the device: {:?}",
        world.device.commands()
    );
    assert_eq!(
        world.device.sounding(),
        0,
        "the previous load is silent: {:?}",
        world.device.commands()
    );
    assert_eq!(
        world.log().released,
        vec![world.emitter(1)],
        "the handoff names the emitters it silenced: {:?}",
        world.log().released
    );
}

/// The spatial law reaches the device: an emitter to the right pans right and
/// attenuates, one beyond the cutoff is silent, and one with no pose in the
/// world is reported as unplaced instead of being assumed to be at the listener.
#[test]
fn accept_f41_b_mixer_carries_the_spatial_law_to_the_device() {
    let mut world = AudioWorld::new(vec![sound_item(ENGINE_KEY)]);
    world.app.update();
    world.spawn_engine_emitter(1, Some([20.0, 0.0, 0.0]));
    world.spawn_engine_emitter(2, Some([0.0, 0.0, 200.0]));
    let unplaced = world.spawn_engine_emitter(3, None);
    // A pose is a pose: an emitter whose pose propagation never ran has none,
    // and the mixer must say so instead of mixing it as if it sat on the
    // listener.
    world
        .app
        .world_mut()
        .entity_mut(unplaced)
        .remove::<bevy::prelude::GlobalTransform>();
    world.app.update();
    // Poses propagate after `Update`, so the frame that binds the loops reads
    // the poses of the previous one; one more frame mixes the real placements.
    world.app.update();
    assert_eq!(world.mix().unplaced, 1, "the pose-less emitter is reported");

    let updates: Vec<cs_sim::audio_events::VoiceUpdate> = world
        .device
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            DeviceCommand::Updated { update, .. } => Some(update),
            _ => None,
        })
        .collect();
    let near = updates
        .iter()
        .find(|update| (update.pan - 1.0).abs() < 1e-12)
        .expect("the emitter to the right pans hard right");
    assert!(
        (near.gain - 0.5).abs() < 1e-12,
        "20 m under a 10 m reference attenuates to half: {near:?}"
    );
    let far = updates
        .iter()
        .find(|update| update.gain == 0.0)
        .expect("the emitter beyond the cutoff is silent");
    assert_eq!(far.pan, 0.0, "silence carries no placement: {far:?}");
    // The pose-less voice is sounding but was never given a placement this pass:
    // it keeps whatever mix it last had.
    assert_eq!(
        world.device.sounding(),
        3,
        "the three loops are all bound: {:?}",
        world.device.commands()
    );
}

/// Losing the device stops every voice and closes it, and the session keeps its
/// loops for retry — while the radio queue keeps completing on simulation ticks,
/// which is F41 non-negotiable behavior 2: mission progress cannot depend on an
/// output device.
#[test]
fn accept_f41_b_device_loss_stops_voices_and_leaves_simulation_running() {
    let mut world = AudioWorld::new(vec![sound_item(ENGINE_KEY)]);
    world.app.update();
    world.spawn_engine_emitter(1, Some([0.0, 0.0, 0.0]));
    world.app.update();
    assert_eq!(world.device.sounding(), 1);

    device_lost(world.app.world_mut());
    assert_eq!(world.device.sounding(), 0, "every voice stopped");
    assert!(
        world.device.commands().contains(&DeviceCommand::Closed),
        "the device closed: {:?}",
        world.device.commands()
    );
    assert!(
        !world.session().device_available(),
        "the session knows the device is gone"
    );
    assert!(
        world
            .session()
            .router
            .active_loop(&world.emitter(1))
            .is_none(),
        "the loop stopped as DeviceLost"
    );

    // A lost device stays lost: the mixer opens a closed device on demand, so
    // without the session's own authority as a gate the next frame would
    // re-open the device nobody restored and the loss would leave no trace in
    // the log but a `Closed` and an `Opened` a frame apart.
    world.device.clear();
    world.app.update();
    world.app.update();
    assert!(
        !world.device.is_open(),
        "no mix pass re-opened a device the caller declared lost: {:?}",
        world.device.commands()
    );
    assert!(
        !world
            .device
            .commands()
            .iter()
            .any(|command| matches!(command, DeviceCommand::Opened)),
        "and nothing asked it to play: {:?}",
        world.device.commands()
    );
    assert_eq!(
        world.mix().passes,
        2,
        "and no mix pass ran either: a world with no usable output records nothing"
    );

    // The device comes back: the session re-binds the loop it remembered, and
    // the next mix pass carries it to the device again.
    let restart = device_restored(world.app.world_mut()).expect("the device opens");
    assert!(restart.is_none(), "no music cue was current");
    world.app.update();
    assert!(
        world
            .session()
            .router
            .active_loop(&world.emitter(1))
            .is_some(),
        "the retry re-bound the live loop"
    );
    assert_eq!(
        world.device.sounding(),
        1,
        "and the device sounds it again: {:?}",
        world.device.commands()
    );
}

/// The engine voice follows the flight model's throttle spool, advanced by the
/// **fixed** clock: the same elapsed time at 60 Hz and at 120 Hz reaches the
/// same level, and the level is the throttle's.
///
/// The spool is seeded *at* the commanded throttle in the invariance half, so
/// the authority holds still and the only thing dividing the elapsed time is
/// the step count. That isolates what the spec asks for — the smoothing is
/// driven by the fixed tick, not by how many frames the renderer drew.
#[test]
fn accept_f41_b_engine_voice_follows_throttle_at_a_fixed_rate() {
    let coarse = smoothed_level(FIXED_HZ, 1.0, 12);
    let fine = smoothed_level(FIXED_HZ * 2, 1.0, 24);
    assert!(
        (coarse.gain - fine.gain).abs() < CLOCK_EPSILON
            && (coarse.pitch - fine.pitch).abs() < CLOCK_EPSILON,
        "12 ticks at 60 Hz and 24 ticks at 120 Hz are the same 0.2 s and must reach \
         the same level: {coarse:?} vs {fine:?}"
    );

    // The throttle is what moves it. A fifth of a second from idle at full
    // throttle is above the idle level and below the full-throttle target, and
    // an idle throttle holds the idle level.
    let law = cs_sim::audio_events::EngineSmoothing::DESIGNED_DEFAULT;
    let idle = law.target(true, 0.0);
    let full = law.target(true, 1.0);
    assert!(
        idle.gain < coarse.gain && coarse.gain < full.gain,
        "0.2 s of full throttle opens the voice part of the way: {coarse:?}"
    );
    assert!(
        idle.pitch < coarse.pitch && coarse.pitch < full.pitch,
        "and sweeps the pitch part of the way: {coarse:?}"
    );
    let held_idle = smoothed_level(FIXED_HZ, 0.0, 12);
    assert!(
        (held_idle.gain - idle.gain).abs() < 1e-12 && (held_idle.pitch - idle.pitch).abs() < 1e-12,
        "an idle throttle holds the idle level: {held_idle:?} vs {idle:?}"
    );

    // A stopped engine asks for silence however open the throttle is, and the
    // voice falls all the way there.
    let silenced = engine_voice_gain_with_stopped_engine();
    assert!(
        silenced < 1e-12,
        "a stopped engine must reach silence, got {silenced}"
    );
}

/// The level an engine voice reaches with its engine stopped and its throttle
/// held open — what a destroyed or out-of-fuel aircraft produces.
fn engine_voice_gain_with_stopped_engine() -> f64 {
    let mut fixture = flight_world(FIXED_HZ);
    fixture.step(1);
    let emitter = AudioEmitterId {
        session: fixture.world().resource::<AudioSession>().router.session(),
        serial: 1,
    };
    let aircraft = engine_aircraft(&mut fixture, emitter, [0.0, 0.0, 0.0], 1.0);
    fixture.step(12);
    fixture
        .world_mut()
        .get_mut::<FlightAircraft>(aircraft)
        .expect("the spawned aircraft carries FlightAircraft")
        .set_engine(EngineState::STOPPED)
        .expect("a stopped engine is valid");
    fixture.step(120);
    fixture
        .world()
        .resource::<cs_app::audio::EngineVoices>()
        .level(&emitter)
        .gain
}

/// Runs a real flight world with the audio plugin, holds the engine's spool at
/// `throttle` (its commanded throttle too, so the authority holds still), and
/// steps `ticks` fixed ticks; returns the engine voice's level after them.
fn smoothed_level(fixed_hz: u32, throttle: f64, ticks: u64) -> VoiceLevel {
    let mut fixture = flight_world(fixed_hz);
    fixture.step(1);
    let emitter = AudioEmitterId {
        session: fixture.world().resource::<AudioSession>().router.session(),
        serial: 1,
    };
    let _ = engine_aircraft(&mut fixture, emitter, [0.0, 0.0, 0.0], throttle);
    fixture.step(ticks);
    fixture
        .world()
        .resource::<cs_app::audio::EngineVoices>()
        .level(&emitter)
}

/// A physics world with the audio plugin and the flight driver at `fixed_hz`,
/// seeded with the real manual clock so one `step` is one fixed tick.
fn flight_world(fixed_hz: u32) -> PhysicsFixture {
    PhysicsFixture::builder(FixtureBodySpec::at_origin(1.0))
        .fixed_hz(fixed_hz)
        .configure(|app| {
            app.add_plugins(FlightForcesPlugin);
            app.add_plugins(AudioPlugin::new(
                declared_synthetic_audio_catalog(),
                spatial(),
            ));
            app.init_resource::<SceneGenerations>();
            // The scene load path consumes a generation before its load, and the
            // emitter bindings it spawns carry that number; the audio handoff
            // reads the same counter, so the fixture consumes one the same way.
            app.world_mut()
                .resource_mut::<SceneGenerations>()
                .take_next();
            let _ = deliver(app.world_mut(), vec![sound_item(ENGINE_KEY)]);
        })
        .build()
        .expect("the fixture spec is valid")
}

/// Spawns a real flight body at `position_m` with `throttle` commanded, marks
/// it as the emitter of an engine loop that follows its throttle, and seeds its
/// spool at that same throttle — so the authority is already where the command
/// holds it and does not move while the smoothing runs.
fn engine_aircraft(
    fixture: &mut PhysicsFixture,
    emitter: AudioEmitterId,
    position_m: [f32; 3],
    throttle: f64,
) -> Entity {
    let spec = FlightSpawnSpec::level_at(position_m, [0.0; 3]);
    let aircraft = spawn_flight_body(
        fixture.world_mut(),
        FlightModel::new(synthetic_fixed_wing()),
        &spec,
    )
    .expect("the spawn spec is valid");
    fixture.world_mut().entity_mut(aircraft).insert((
        AudioEmitterBinding {
            emitter,
            bus: AudioBus::Engine,
            asset: engine_asset(),
            generation: SceneGeneration(1),
        },
        EngineVoiceFollow::default(),
    ));
    let mut record = fixture
        .world_mut()
        .get_mut::<FlightAircraft>(aircraft)
        .expect("a spawned flight body carries FlightAircraft");
    record
        .set_engine(EngineState::direct(throttle))
        .expect("a seeded spool is valid");
    record
        .set_command(FlightInput::try_new(0.0, 0.0, 0.0, throttle, false).expect("a valid command"))
        .expect("a valid command");
    aircraft
}
