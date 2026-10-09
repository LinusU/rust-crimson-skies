//! The windowed flyable development playtest (`cs --playtest`, task #647).
//!
//! **This is not original Crimson Skies and not mission M01.** It is a
//! deliberately labelled development scene — synthetic geometry, a designed
//! synthetic airframe, designed spawn and lighting, uncalibrated flight — that
//! needs no original installation, so the owner can launch something and fly
//! it now. Nothing in it is `verified_original`.
//!
//! What it composes, and what it does not reimplement:
//!
//! * **Flight** is the production F24 path: [`crate::physics::spawn_flight_body`]
//!   and [`crate::physics::FlightForcesPlugin`] run one fixed tick per
//!   [`BASELINE_FIXED_HZ`] inside the F23 Avian adapter. The aircraft is moved
//!   only by forces; no system here writes its `Transform`. **Over original
//!   content there is one documented exception** (task #797): the retail scene
//!   flies the original game's recovered fixed-wing law
//!   ([`scene::PlaytestOriginalFlight`], provenance `OWNER-STATIC-2026-10-08`,
//!   still uncalibrated against an original run #358), which integrates its own
//!   attitude, so [`drive_original_flight`] submits the step's force *and*
//!   writes the attitude the step integrated while the body integrates no
//!   torque at all. The synthetic scene keeps the synthetic airframe.
//! * **Input** is the F22 session through [`crate::input::platform::BevyInputPlugin`]:
//!   the keyboard is read by Bevy, converted to device events, gated by the
//!   session's focus/pause/context policy and only then read back as a
//!   [`cs_sim::flight::FlightInput`] by [`command::flight_command`].
//! * **Camera** is the F21 [`crate::camera::CameraRig`] in its chase rig,
//!   resolved each frame from the aircraft's authoritative `Position` and
//!   `Rotation`.
//! * **Collision** is Avian contact with a static wall and the ground, reported
//!   through [`crate::physics::ContactReports`].
//!
//! [`PlaytestPlugin`] is the render-independent core (it runs headless in the
//! acceptance tests, through the same systems); [`visuals::PlaytestVisualsPlugin`]
//! adds the meshes, lights, window camera and HUD; [`smoke`] adds the finite
//! deterministic run. [`run_playtest`] is what `cs --playtest` calls.

pub mod command;
pub mod propeller;
pub mod retail;
pub mod scene;
pub mod smoke;
pub mod visuals;

use std::path::PathBuf;

use avian3d::prelude::{
    AngularVelocity, Gravity, LinearVelocity, PhysicsPlugins, Position, Rotation,
};
use bevy::app::{AppExit, RunFixedMainLoop, RunFixedMainLoopSystems};
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::{
    App, Entity, FixedLast, FixedUpdate, IntoScheduleConfigs, Or, Plugin, Quat, Query, Res, ResMut,
    Resource, Startup, Time, Transform, Update, Vec3, With, World,
};
use bevy::time::{Fixed, Real, TimeUpdateStrategy, Virtual};
use cs_content::cameras::{AspectRatio, declared_synthetic_camera_modes};
use cs_sim::collision::CollisionLayer;
use cs_sim::control::LocalSeatId;
use cs_sim::damage::ActorId;
use cs_sim::flight::FlightInput;
use cs_sim::time::TickRate;
use cs_types::Tick;
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, WorldPosition};

use crate::camera::{CameraPose, CameraRig, RigFrame, RigInputs, ViewRig, lower_camera_modes};
use crate::input::platform::{BevyInputPlugin, PlatformInput};
use crate::input::{InputSession, PauseReason, SessionMode};
use crate::origin::OriginChange;
use crate::physics::{
    BASELINE_FIXED_HZ, ContactReports, FlightAircraft, FlightForcesPlugin, ForceRequest,
    ForceRequests, PhysicsAdapterPlugin, PhysicsBodiesPlugin, PhysicsTickLedger,
};

use self::command::{CRUISE_THROTTLE, flight_command, playtest_action_map};
pub use self::retail::RetailRequest;
use self::retail::{PlaytestAreaBody, RetailContent, RetailFlight};
use self::scene::{PlaytestAircraft, PlaytestGround, PlaytestObstacle, PlaytestOriginalFlight};

/// The label shown on screen and in every artifact of the playtest.
pub const PLAYTEST_LABEL: &str = "DEVELOPMENT PLAYTEST / SYNTHETIC SCENE / UNCALIBRATED FLIGHT";

/// What `cs --playtest` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaytestRequest {
    /// `Some` runs the finite deterministic smoke instead of the open-ended
    /// interactive session.
    pub smoke: Option<SmokeRequest>,
    /// `Some` flies over original assets read from the named installation
    /// (task #649) instead of the synthetic scene.
    pub retail: Option<RetailRequest>,
}

/// The finite smoke run (`--smoke-seconds <n> --capture-dir <dir>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmokeRequest {
    /// Simulated seconds the scripted run lasts; at least
    /// [`smoke::MIN_SMOKE_SECONDS`].
    pub seconds: u32,
    /// Where the framebuffer PNGs, the trace and the report are written.
    pub capture_dir: PathBuf,
}

/// The default capture directory (ignored by Git).
pub const DEFAULT_CAPTURE_DIR: &str = "private/playtest";

/// Why the playtest could not run.
#[derive(Debug)]
pub enum PlaytestError {
    /// The scene could not be built.
    Scene(scene::SceneError),
    /// The explicit original installation could not be used. There is no
    /// fallback to the synthetic scene.
    Retail {
        path: PathBuf,
        source: Box<crate::playtest_retail::PlaytestError>,
    },
    /// The original flight parameters could not be imported for the retail
    /// scene. There is no fallback to the synthetic airframe: a scene that
    /// cannot state its flight law must fail, not fly something else.
    Flight {
        /// The installation that refused.
        path: PathBuf,
        /// What could not be read, imported or consumed.
        detail: String,
    },
    /// The smoke run could not write its artifacts.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The smoke run finished but its own checks failed.
    SmokeFailed(Vec<String>),
    /// The app exited with an error.
    Exit(String),
}

impl std::fmt::Display for PlaytestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scene(error) => write!(f, "{error}"),
            Self::Retail { path, source } => write!(
                f,
                "cannot fly the original assets of {} (no fallback to the synthetic scene): {source}",
                path.display()
            ),
            Self::Flight { path, detail } => write!(
                f,
                "cannot fly the original flight law of {} (no fallback to the synthetic \
                 airframe): {detail}",
                path.display()
            ),
            Self::Io { path, source } => write!(f, "cannot write {}: {source}", path.display()),
            Self::SmokeFailed(failures) => {
                write!(f, "playtest smoke failed: {}", failures.join("; "))
            }
            Self::Exit(reason) => write!(f, "playtest exited with an error: {reason}"),
        }
    }
}

impl std::error::Error for PlaytestError {}

/// Requests raised by the meta keys and consumed once per frame.
#[derive(Resource, Debug, Default)]
pub struct PlaytestRequests {
    /// Reset the aircraft to the known flyable state.
    pub reset: bool,
}

/// The aircraft readout the HUD and the traces use, read from the body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Telemetry {
    /// Authoritative position, in meters.
    pub position_m: [f32; 3],
    /// Authoritative linear velocity, in m/s.
    pub velocity_m_s: [f32; 3],
    /// Speed, in m/s.
    pub speed_m_s: f32,
    /// Height above the ground plane, in meters.
    pub altitude_m: f32,
    /// Heading about world up, degrees, 0 at -Z increasing toward +X.
    pub heading_deg: f32,
    /// Nose pitch above the horizon, degrees.
    pub pitch_deg: f32,
    /// Bank, degrees, positive right wing down.
    pub roll_deg: f32,
}

/// The playtest's own bookkeeping: everything the HUD and the smoke report.
#[derive(Resource, Clone, Debug)]
pub struct PlaytestState {
    /// How many resets have been performed.
    pub resets: u32,
    /// Contact episodes between the aircraft and the obstacle wall.
    pub obstacle_contacts: u64,
    /// Contact episodes between the aircraft and the ground.
    pub ground_contacts: u64,
    /// The fixed tick of the first obstacle contact.
    pub first_obstacle_contact_tick: Option<u64>,
    /// Times the commanded flight input changed value.
    pub input_changes: u64,
    /// The command last handed to the aircraft.
    pub command: FlightInput,
    /// Whether the session is paused.
    pub paused: bool,
    /// Why the session is paused, when it is.
    pub pause_reason: Option<PauseReason>,
    /// Ticks the fixed clock advanced while the session was paused.
    pub ticks_while_paused: u64,
    /// Set by the quit key.
    pub quit_requested: bool,
    /// The latest aircraft readout.
    pub telemetry: Telemetry,
    /// Where the aircraft spawns and resets to, metres.
    pub spawn_m: [f32; 3],
    /// The label every surface shows.
    pub label: &'static str,
    /// Fixed ticks on which the original law refused to step (a non-finite
    /// state or an out-of-range timestep). Zero in every run that flies; it is
    /// counted rather than silently skipped.
    pub original_step_errors: u64,
}

impl Default for PlaytestState {
    fn default() -> Self {
        Self {
            resets: 0,
            obstacle_contacts: 0,
            ground_contacts: 0,
            first_obstacle_contact_tick: None,
            input_changes: 0,
            command: FlightInput::NEUTRAL,
            paused: false,
            pause_reason: None,
            ticks_while_paused: 0,
            quit_requested: false,
            telemetry: Telemetry::default(),
            spawn_m: scene::SPAWN_POSITION_M,
            label: PLAYTEST_LABEL,
            original_step_errors: 0,
        }
    }
}

/// The chase camera: the F21 rig and its latest resolved frame.
#[derive(Resource)]
pub struct PlaytestCamera {
    rig: CameraRig,
    /// The frame the rig resolved most recently.
    pub frame: Option<RigFrame>,
    teleport: bool,
    /// The viewport aspect the next frame is framed at.
    pub aspect: AspectRatio,
}

impl PlaytestCamera {
    fn new() -> Self {
        let modes = lower_camera_modes(&declared_synthetic_camera_modes())
            .expect("the synthetic camera modes lower");
        let mut rig = CameraRig::new(modes).expect("the synthetic mode set builds a rig");
        rig.set_rig(ViewRig::Chase)
            .expect("the synthetic mode set declares a chase view");
        Self {
            rig,
            frame: None,
            teleport: true,
            aspect: AspectRatio::SIXTEEN_NINE,
        }
    }
}

/// Marks the one camera entity. The window build adds a `Camera3d` to it.
#[derive(bevy::prelude::Component, Clone, Copy, Debug, Default)]
pub struct PlaytestCameraMarker;

/// The render-independent playtest: physics, flight, input, scene, camera and
/// bookkeeping. Add [`PhysicsPlugins`] and a time source before it.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlaytestPlugin;

impl Plugin for PlaytestPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            PhysicsAdapterPlugin::new(BASELINE_FIXED_HZ),
            PhysicsBodiesPlugin,
            FlightForcesPlugin,
        ));
        // The flight model carries its own gravity; a second one would double it.
        app.insert_resource(Gravity::ZERO);
        let mut session = InputSession::new(
            playtest_action_map(),
            LocalSeatId(0),
            SessionMode::SinglePlayer,
            Tick(0),
        );
        session
            .throttle_mut()
            .set_position(CRUISE_THROTTLE)
            .expect("the cruise throttle is inside the range");
        app.add_plugins(BevyInputPlugin::new(
            session,
            TickRate::new(BASELINE_FIXED_HZ).expect("the baseline rate is nonzero"),
        ));
        app.init_resource::<PlaytestRequests>()
            .init_resource::<PlaytestState>()
            .insert_resource(PlaytestCamera::new())
            .add_systems(Startup, setup_scene)
            // The retail scene's original law runs on the fixed tick, before
            // the F23 adapter drains this tick's force requests.
            .add_systems(FixedUpdate, drive_original_flight)
            .add_systems(
                RunFixedMainLoop,
                apply_flight_command.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
            )
            .add_systems(FixedLast, record_collisions)
            .add_systems(
                Update,
                (
                    meta_controls,
                    perform_reset,
                    sync_pause,
                    // After `sync_pause`: a pause that begins in a frame
                    // freezes the drawn propeller in that same frame.
                    propeller::spin_propellers,
                    update_telemetry,
                    follow_camera,
                )
                    .chain(),
            );
    }
}

/// Builds the app the acceptance tests drive: the same plugins and systems as
/// the window, without a window, a GPU or real time. One `update` is one rendered
/// frame of 1/60 s, which runs the fixed loop twice at the baseline rate.
pub fn headless_app() -> App {
    headless_app_with(|_| {})
}

/// [`headless_app`] with extra plugins added before the app is finalized, the
/// seam the smoke tests use to add [`smoke::SmokePlugin`].
pub fn headless_app_with(configure: impl FnOnce(&mut App)) -> App {
    let mut app = crate::asset_stack::headless_app();
    app.add_plugins(bevy::input::InputPlugin);
    let frame = std::time::Duration::from_secs_f64(1.0 / smoke::SMOKE_FRAME_HZ);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
    app.add_plugins(PlaytestPlugin);
    configure(&mut app);
    let startup = app.world().resource::<Time<Real>>().startup();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .update_with_instant(startup);
    app.finish();
    app.cleanup();
    app
}

fn setup_scene(world: &mut World) {
    // Over original content the area is already spawned (and recorded as
    // `RetailContent`); the synthetic ground and wall exist only without it.
    if !world.contains_resource::<RetailContent>() {
        scene::spawn_world(world).expect("the playtest world is valid");
    }
    scene::spawn_aircraft(world).expect("the playtest aircraft is valid");
    world.spawn((PlaytestCameraMarker, Transform::default()));
}

/// Hands the session's held command to the aircraft once per frame, before the
/// fixed loop runs, so every fixed tick of the frame flies the command the
/// input policy produced.
///
/// The one edge command the playtest consumes is
/// [`FlightCommand::LevelOff`](cs_types::input::FlightCommand::LevelOff): each
/// delivered press toggles the original law's Level-Off assist
/// (`OriginalState.level_off`) once, so two presses in one frame cancel out and
/// a paused or unfocused session — which runs no input boundary and so delivers
/// nothing — cannot flip it.
fn apply_flight_command(
    platform: Res<PlatformInput>,
    mut state: ResMut<PlaytestState>,
    mut aircraft: Query<&mut FlightAircraft, With<PlaytestAircraft>>,
    mut original: Query<&mut PlaytestOriginalFlight, With<PlaytestAircraft>>,
) {
    let command = flight_command(platform.session());
    if command != state.command {
        state.input_changes += 1;
        state.command = command;
    }
    for mut record in &mut aircraft {
        record
            .set_command(command)
            .expect("a clamped command is valid");
    }
    // The retail scene's aircraft carries the original law instead of the F24
    // record: it takes the same held command, unvalidated because the session
    // produced it clamped and the law clamps it again at its own boundary.
    let toggles = platform
        .report()
        .delivered
        .iter()
        .filter(|action| **action == Action::Flight(FlightCommand::LevelOff))
        .count();
    for mut flight in &mut original {
        flight.command = command;
        if toggles % 2 == 1 {
            flight.state.level_off = !flight.state.level_off;
        }
    }
}

/// One fixed tick of the original law that flies the retail playtest (task
/// #797).
///
/// The law integrates pose and velocity itself, so this system does exactly
/// what `docs/findings/2026-10-08-flight-original-fixed-wing-law.md` records as
/// the consumer mapping and nothing more:
///
/// * **seed** the law from the body's authoritative `Position`, `Rotation` and
///   `LinearVelocity`, so a contact the solver resolved is what the next step
///   flies from (the body owns the linear pose: this is the one integrator);
/// * **hand the acceleration back** as this tick's force,
///   `world_force = (W / 9.82) * a`, with the body's mass declared as exactly
///   `W / 9.82` and global gravity `ZERO`: gravity and drag are already inside
///   `a`, so neither is added a second time;
/// * **write the attitude the step integrated** and clear the body's angular
///   velocity. The original rotates by `2 * |omega| * dt`, which no
///   torque-driven rigid body reproduces, so while this law is active the law
///   owns attitude and the body integrates no torque (`world_torque` is
///   reported for instruments only and is never submitted).
///
/// A refused step (a non-finite state) is counted in
/// [`PlaytestState::original_step_errors`] instead of being flown.
fn drive_original_flight(
    time: Res<Time<Fixed>>,
    mut requests: ResMut<ForceRequests>,
    mut state: ResMut<PlaytestState>,
    mut aircraft: Query<(
        Entity,
        &mut PlaytestOriginalFlight,
        &Position,
        &mut Rotation,
        &LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    let dt_s = time.timestep().as_secs_f64();
    for (entity, mut flight, position, mut rotation, velocity, mut angular) in &mut aircraft {
        flight.state.position_m = [
            f64::from(position.0.x),
            f64::from(position.0.y),
            f64::from(position.0.z),
        ];
        flight.state.orientation = to_quaternion(rotation.0);
        flight.state.velocity_mps = velocity.0.to_array().map(f64::from);
        let input = flight.input();
        let step = {
            let record = &mut *flight;
            record.model.step(&mut record.state, input, dt_s)
        };
        let Ok(step) = step else {
            state.original_step_errors += 1;
            continue;
        };
        let Ok(request) =
            ForceRequest::new(entity, step.world_force.map(|value| value as f32), [0.0; 3])
        else {
            state.original_step_errors += 1;
            continue;
        };
        requests.submit(request);
        let [x, y, z, w] = flight.state.orientation.components();
        rotation.0 = Quat::from_xyzw(x as f32, y as f32, z as f32, w as f32).normalize();
        angular.0 = Vec3::ZERO;
    }
}

/// The meta keys: `R` reset, `Esc` pause/resume, `F10` quit. They are read
/// from Bevy's keyboard state directly because they are not flight actions.
fn meta_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut platform: ResMut<PlatformInput>,
    mut requests: ResMut<PlaytestRequests>,
    mut state: ResMut<PlaytestState>,
    mut exit: bevy::prelude::MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::KeyR) {
        requests.reset = true;
    }
    if keys.just_pressed(KeyCode::Escape) {
        let session = platform.session_mut();
        if session.is_paused() {
            session.resume();
        } else {
            session.pause(PauseReason::PlayerRequest);
        }
    }
    if keys.just_pressed(KeyCode::F10) {
        state.quit_requested = true;
        exit.write(AppExit::Success);
    }
}

/// Replaces the aircraft with a fresh one in the known flyable state.
///
/// The old body is despawned (with its visual children) before the new one is
/// spawned, so a reset leaves exactly one player aircraft, one camera and the
/// one fixed clock.
fn perform_reset(world: &mut World) {
    if !std::mem::take(&mut world.resource_mut::<PlaytestRequests>().reset) {
        return;
    }
    let old: Vec<Entity> = world
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .iter(world)
        .collect();
    for entity in old {
        world.despawn(entity);
    }
    scene::spawn_aircraft(world).expect("the playtest aircraft is valid");
    world
        .resource_mut::<PlatformInput>()
        .session_mut()
        .throttle_mut()
        .set_position(CRUISE_THROTTLE)
        .expect("the cruise throttle is inside the range");
    let mut camera = world.resource_mut::<PlaytestCamera>();
    camera.rig.reset();
    camera
        .rig
        .set_rig(ViewRig::Chase)
        .expect("the chase rig exists");
    camera.teleport = true;
    camera.frame = None;
    let mut state = world.resource_mut::<PlaytestState>();
    state.resets += 1;
    state.command = FlightInput::NEUTRAL;
}

/// Freezes and releases the fixed clock with the input session's pause, so the
/// session policy (player request, focus loss) is the one thing that decides.
fn sync_pause(
    mut platform: ResMut<PlatformInput>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut state: ResMut<PlaytestState>,
) {
    // Regaining focus is the caller's decision to resume (the session never
    // resumes itself): a pause that only the focus loss caused ends with it,
    // while a player's own Esc pause stands until Esc.
    if platform.session().pause_reason() == Some(PauseReason::FocusLost)
        && platform.session().is_focused()
    {
        platform.session_mut().resume();
    }
    let paused = platform.session().is_paused();
    state.paused = paused;
    state.pause_reason = platform.session().pause_reason();
    if paused != virtual_time.is_paused() {
        if paused {
            virtual_time.pause();
        } else {
            virtual_time.unpause();
        }
    }
}

/// Counts new contact episodes between the aircraft and the scene.
/// The bodies a contact counts as hitting the obstacle: the synthetic wall or
/// any entity of the original area.
type ObstacleFilter = Or<(With<PlaytestObstacle>, With<PlaytestAreaBody>)>;

fn record_collisions(
    reports: Res<ContactReports>,
    ledger: Res<PhysicsTickLedger>,
    obstacles: Query<(), ObstacleFilter>,
    grounds: Query<(), With<PlaytestGround>>,
    mut state: ResMut<PlaytestState>,
) {
    for report in reports.reports() {
        if report.tick != ledger.ticks || !report.involves(CollisionLayer::Aircraft) {
            continue;
        }
        for body in report.bodies {
            if obstacles.contains(body) {
                state.obstacle_contacts += 1;
                state
                    .first_obstacle_contact_tick
                    .get_or_insert(ledger.ticks);
            } else if grounds.contains(body) {
                state.ground_contacts += 1;
            }
        }
    }
}

fn update_telemetry(
    aircraft: Query<
        (&Position, &Rotation, &LinearVelocity, &AngularVelocity),
        With<PlaytestAircraft>,
    >,
    ledger: Res<PhysicsTickLedger>,
    mut last: bevy::prelude::Local<(u64, bool)>,
    mut state: ResMut<PlaytestState>,
) {
    // A pause takes hold at the next frame's time update, so the frame in
    // which it began has already run its fixed ticks; only ticks after that
    // frame count against the pause.
    if state.paused && last.1 {
        state.ticks_while_paused += ledger.ticks.saturating_sub(last.0);
    }
    *last = (ledger.ticks, state.paused);
    let Ok((position, rotation, velocity, _)) = aircraft.single() else {
        return;
    };
    let forward = rotation.0 * Vec3::NEG_Z;
    let right = rotation.0 * Vec3::X;
    let heading = forward.x.atan2(-forward.z).to_degrees();
    let pitch = forward.y.clamp(-1.0, 1.0).asin().to_degrees();
    let roll = (-right.y).clamp(-1.0, 1.0).asin().to_degrees();
    state.telemetry = Telemetry {
        position_m: position.0.to_array(),
        velocity_m_s: velocity.0.to_array(),
        speed_m_s: velocity.0.length(),
        altitude_m: position.0.y,
        heading_deg: heading,
        pitch_deg: pitch,
        roll_deg: roll,
    };
}

fn to_quaternion(rotation: Quat) -> Quaternion {
    let [x, y, z, w] = rotation.normalize().to_array().map(f64::from);
    let length = (x * x + y * y + z * z + w * w).sqrt();
    Quaternion::try_new([x / length, y / length, z / length, w / length])
        .expect("an Avian rotation is a unit quaternion")
}

/// Resolves the chase rig from the authoritative pose and moves the camera
/// entity to it. The camera's `Transform` is presentation; nothing reads it
/// back as game state.
fn follow_camera(
    aircraft: Query<(&Position, &Rotation), With<PlaytestAircraft>>,
    time: Res<Time<Real>>,
    state: Res<PlaytestState>,
    mut camera: ResMut<PlaytestCamera>,
    mut cameras: Query<&mut Transform, With<PlaytestCameraMarker>>,
) {
    let Ok((position, rotation)) = aircraft.single() else {
        return;
    };
    let pose = CameraPose::new(
        WorldPosition::try_new(position.0.to_array().map(f64::from))
            .expect("an Avian position is finite"),
        to_quaternion(rotation.0),
    );
    let origin_change = if camera.teleport {
        OriginChange::Teleport
    } else {
        OriginChange::Rebase
    };
    let inputs = RigInputs {
        at: Tick(0),
        subject: ActorId {
            session: SessionId::new(1).expect("session generation 1 is valid"),
            serial: u64::from(state.resets) + 1,
        },
        aircraft: pose,
        aspect: camera.aspect,
        elapsed: time.delta(),
        look: None,
        spyglass: None,
        origin_change,
    };
    let Ok(frame) = camera.rig.resolve(&inputs) else {
        return;
    };
    camera.teleport = false;
    camera.frame = Some(frame);
    let eye = frame.pose.position();
    let [x, y, z, w] = frame.pose.rotation().components();
    for mut transform in &mut cameras {
        transform.translation = Vec3::new(eye.x() as f32, eye.y() as f32, eye.z() as f32);
        transform.rotation = Quat::from_xyzw(x as f32, y as f32, z as f32, w as f32);
    }
}

/// Builds the windowed app: the real window, renderer and wall-clock fixed
/// loop, with the playtest on top.
///
/// # Errors
///
/// [`PlaytestError::Retail`] when `retail` names an installation that cannot be
/// read, **before** any window exists.
pub fn windowed_app(
    smoke: Option<&SmokeRequest>,
    retail: Option<&RetailRequest>,
) -> Result<(App, Option<smoke::SmokeHandle>), PlaytestError> {
    use bevy::prelude::{DefaultPlugins, PluginGroup, Window, WindowPlugin};
    let sources = retail.map(retail::read_sources).transpose()?;
    let label = if retail.is_some() {
        retail::RETAIL_LABEL
    } else {
        PLAYTEST_LABEL
    };
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("Crimson Skies — {label}"),
            resolution: (1280, 720).into(),
            ..Window::default()
        }),
        ..WindowPlugin::default()
    }));
    app.add_plugins(PhysicsPlugins::default());
    if smoke.is_some() {
        // A smoke run is deterministic: every rendered frame is exactly 1/60 s
        // of simulated time whatever the machine's frame pacing.
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1.0 / smoke::SMOKE_FRAME_HZ),
        ));
    }
    app.add_plugins(PlaytestPlugin);
    app.add_plugins(visuals::PlaytestVisualsPlugin);
    if let (Some(request), Some(sources)) = (retail, &sources) {
        retail::install(&mut app, sources, request)?;
    }
    let handle = smoke.map(|request| {
        let plugin = smoke::SmokePlugin::windowed(request.clone());
        let handle = plugin.handle();
        app.add_plugins(plugin);
        handle
    });
    Ok((app, handle))
}

/// Runs `cs --playtest`: opens the window and blocks until it closes (or the
/// smoke run ends).
///
/// # Errors
///
/// [`PlaytestError`] when the smoke run fails its own checks or cannot write
/// its artifacts, or the app exits with an error.
pub fn run_playtest(request: &PlaytestRequest) -> Result<(), PlaytestError> {
    let (mut app, handle) = windowed_app(request.smoke.as_ref(), request.retail.as_ref())?;
    if let Some(content) = app.world().get_resource::<RetailContent>() {
        println!("playtest sources: {}", content.manifest_json());
    }
    if let Some(flight) = app.world().get_resource::<RetailFlight>() {
        println!("playtest flight: {}", flight.json());
    }
    let exit = app.run();
    if let (Some(handle), Some(smoke_request)) = (&handle, &request.smoke) {
        smoke::finish(handle, smoke_request)?;
    }
    match exit {
        AppExit::Success => Ok(()),
        AppExit::Error(code) => Err(PlaytestError::Exit(format!("exit code {code}"))),
    }
}

/// Fixed physics ticks the app has run, for the acceptance tests.
pub fn fixed_ticks(app: &App) -> u64 {
    app.world().resource::<PhysicsTickLedger>().ticks
}

/// The fixed timestep in seconds, for tests that convert seconds to ticks.
pub fn fixed_timestep_s(app: &App) -> f64 {
    app.world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f64()
}
