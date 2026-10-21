//! The windowed composition's **capture path** (VS-M01-RT-EVIDENCE-A, Rally
//! #1312).
//!
//! While [`super::run_windowed`] flies a mission, an evidence run writes two
//! things into a capture directory and nothing else:
//!
//! * `frame_<n>_player.png` — a real framebuffer screenshot of the primary
//!   window, requested through the same Bevy [`Screenshot`] mechanism the
//!   playtest smoke uses, only once the player aircraft exists and the
//!   mission host has stepped ticks;
//! * `trace.jsonl` — one JSON object per line: a `header`, a `tick` sample
//!   every [`SAMPLE_EVERY_FRAMES`] rendered frames, a `camera` record for the
//!   chase camera's resolved frame at each sample, an `input` record whenever
//!   the commanded flight input changes, the `screenshot` measurements, and a
//!   final `terminal` record naming whether the mission host settled and, if
//!   so, its outcome and exit.
//!
//! The run drives one fixed input script (hold `S`, pitch up, for one second)
//! so the trace carries real input changes; it is an automated run, never
//! `human_play`.
//!
//! # Refusal
//!
//! Capture is requested with `CS_MISSION_CAPTURE=1`. It needs the `gpu`
//! capability in `CS_CAPABILITIES` and a `CS_EVIDENCE_DIR`; either missing is
//! a [`CaptureRefusal`] that names it, raised before any content is read —
//! never a silent skip and never a run without a picture. A run that ends
//! without a delivered, non-empty frame or without a stepped tick is refused
//! the same way ([`finish`]).
//!
//! Nothing captured is committed: the directory is private evidence output.

use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use avian3d::prelude::Position;
use bevy::app::{AppExit, FixedPostUpdate};
use bevy::input::keyboard::KeyCode;
use bevy::input::{ButtonInput, InputSystems};
use bevy::prelude::{
    App, ClearColor, Commands, DetectChanges, Entity, IntoScheduleConfigs, Last, MessageWriter, On,
    Plugin, PreUpdate, Query, Res, ResMut, Resource, Update, Visibility, With,
};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

use super::host::{MissionHost, mission_host_tick};
use super::scenario::{Scenario, TriggerWatch, watch_trigger};
use crate::audio::device::SampleLibrary;
use crate::input::platform::PlatformInput;
use crate::physics::PhysicsTickLedger;
use crate::playtest::scene::PlaytestAircraft;
use crate::playtest::{PlaytestCamera, PlaytestState};
use crate::render::horizon::{HorizonDome, HorizonSea};

/// The environment variable that asks a `--mission` run to capture.
pub const CAPTURE_ENV: &str = "CS_MISSION_CAPTURE";
/// The environment variable naming the capture directory.
pub const EVIDENCE_DIR_ENV: &str = "CS_EVIDENCE_DIR";
/// The environment variable listing the capabilities the machine provides.
pub const CAPABILITIES_ENV: &str = "CS_CAPABILITIES";

/// Rendered frames between two `tick` samples.
pub const SAMPLE_EVERY_FRAMES: u32 = 15;
/// The rendered frame the scripted pitch-up starts on.
const INPUT_FROM_FRAME: u32 = 90;
/// The rendered frame the scripted pitch-up ends on.
const INPUT_TO_FRAME: u32 = 150;
/// The rendered frame the player screenshot is requested on, after the input
/// has moved the aircraft.
const SHOT_FRAME: u32 = 180;
/// Frames to wait for the renderer to deliver the screenshot before the run
/// ends without one (which [`finish`] then refuses).
const SHOT_GRACE_FRAMES: u32 = 600;
/// Name of the trace file inside the capture directory.
pub const TRACE_FILE: &str = "trace.jsonl";

// --------------------------------------------------------- the world probe --
//
// The frame around the spawn must hold the world's own pixels
// (M01-PLAY-WORLD-VISIBLE, #1433). The probe measures them without a second
// spawn and without freezing the camera: it hides what is **not** the world —
// the horizon's dome and sea (#1434) and the player's own aircraft — for a
// few frames, so the frame's **background** is the flat clear colour and
// every pixel off it is geometry the world load drew. A frame with no world
// pixels is the pre-#1433 defect — the world's meshes spawned with no
// material, so nothing drew — and a frame whose horizon was never hidden
// would count *its* pixels instead, which is why the probe records which
// entities it hid and [`judge_world_probe`] refuses a class it found nothing
// of.
//
// The pixels are counted against the window's **declared clear colour**
// (`ClearColor`, what the render pass actually clears to), not against the
// frame's own modal colour: the count then cannot be flipped by the world
// covering more of the frame than the background does. The surface's channel
// order and encoding vary by backend (`Bgra8UnormSrgb` on the pinned Metal
// one), so both the sRGB and the linear 8-bit encodings of the clear colour
// are counted and the **stricter** of the two off-colour counts is the
// world's, with the more permissive one supplying `clear_pixels` — whichever
// encoding the surface really stores, one of the two is the frame's own
// background and the other is not, so the min/max pair cannot pass a frame
// whose background merely happens to be a different encoding.

/// The rendered frame the world probe starts hiding the horizon and the
/// player on.
const WORLD_PROBE_HIDE_FRAME: u32 = 30;
/// The rendered frame the world probe's screenshot is requested on, with the
/// horizon and the player hidden and the world drawn.
const WORLD_PROBE_FRAME: u32 = 32;
/// The rendered frame the probe restores the horizon and the player on.
const WORLD_PROBE_RESTORE_FRAME: u32 = 34;
/// How far a channel may differ from a reference colour and still be called
/// that colour: the renderer's clear colour is uniform, and a small tolerance
/// absorbs any dither the post chain adds.
const BACKGROUND_CHANNEL_TOLERANCE: u8 = 6;
/// The least fraction of the probe frame that must be world pixels, in parts
/// per thousand. A frame that drew no world geometry is a single flat colour
/// (zero), and a frame that drew the world around a spawn near a cliff and a
/// valley covers far more than this.
pub const MIN_WORLD_COVERAGE_PERMILLE: u32 = 20;
/// The least fraction of the probe frame that must be the window's clear
/// colour, in parts per thousand. The clear colour is only visible where
/// neither the world nor the hidden horizon and player draw, so a probe
/// frame holding almost none of it did **not** hide the horizon: its dome
/// and sea are geometry, they fill the frame, and counting them as the world
/// would pass a window with no terrain at all.
pub const MIN_CLEAR_COVERAGE_PERMILLE: u32 = 20;

/// Why a capture could not be set up or did not produce its evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureRefusal {
    /// A capability the capture needs (`gpu` for the picture, `audio` for
    /// the playback record) is not listed in `CS_CAPABILITIES`.
    MissingCapability {
        /// The capability that is missing.
        capability: &'static str,
        /// What `CS_CAPABILITIES` held, `None` when unset.
        declared: Option<String>,
    },
    /// `CS_EVIDENCE_DIR` is unset or empty.
    MissingDirectory,
    /// The capture directory could not be created or written.
    Io(String),
    /// The run ended without the evidence it was asked for.
    Incomplete(String),
    /// The scenario variables (start override, trigger watch) are
    /// incomplete, malformed, or were asked of a run that is not a capture.
    Scenario(String),
}

impl fmt::Display for CaptureRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCapability {
                capability,
                declared,
            } => write!(
                f,
                "mission capture needs the `{capability}` capability, but {CAPABILITIES_ENV} is {}",
                declared
                    .as_deref()
                    .map_or_else(|| "unset".to_owned(), |value| format!("{value:?}"))
            ),
            Self::MissingDirectory => write!(
                f,
                "mission capture needs {EVIDENCE_DIR_ENV} to name the capture directory"
            ),
            Self::Io(reason) => write!(f, "mission capture cannot write its evidence: {reason}"),
            Self::Incomplete(reason) => write!(f, "mission capture is incomplete: {reason}"),
            Self::Scenario(reason) => write!(f, "mission capture scenario refused: {reason}"),
        }
    }
}

impl std::error::Error for CaptureRefusal {}

/// A capture that has been checked to be possible.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureSettings {
    /// Where the PNG and `trace.jsonl` are written.
    pub dir: PathBuf,
    /// The start override and trigger watch the run was asked for.
    pub scenario: Scenario,
}

impl CaptureSettings {
    /// Resolves a capture from the two environment values.
    ///
    /// # Errors
    ///
    /// [`CaptureRefusal::MissingCapability`] when `capabilities` does not list
    /// `gpu`; [`CaptureRefusal::MissingDirectory`] when `dir` is unset or
    /// empty. The capability is judged first.
    pub fn resolve(capabilities: Option<&str>, dir: Option<&str>) -> Result<Self, CaptureRefusal> {
        for capability in ["gpu", "audio"] {
            let has = capabilities.is_some_and(|list| {
                list.split(',')
                    .any(|declared| declared.trim() == capability)
            });
            if !has {
                return Err(CaptureRefusal::MissingCapability {
                    capability,
                    declared: capabilities.map(str::to_owned),
                });
            }
        }
        match dir {
            Some(dir) if !dir.is_empty() => Ok(Self {
                dir: PathBuf::from(dir),
                scenario: Scenario::default(),
            }),
            _ => Err(CaptureRefusal::MissingDirectory),
        }
    }

    /// The capture the process environment asks for: `None` when
    /// `CS_MISSION_CAPTURE` is not `1` (an ordinary run), otherwise the
    /// resolved settings or the refusal.
    ///
    /// # Errors
    ///
    /// The [`CaptureRefusal`] of [`Self::resolve`].
    pub fn from_env() -> Result<Option<Self>, CaptureRefusal> {
        Self::gate(
            std::env::var(CAPTURE_ENV).as_deref() == Ok("1"),
            Scenario::from_env()?,
            std::env::var(CAPABILITIES_ENV).ok().as_deref(),
            std::env::var(EVIDENCE_DIR_ENV).ok().as_deref(),
        )
    }

    /// [`Self::from_env`] over explicit values: a scenario is honoured only
    /// by a capture, and asking an ordinary run for one is refused rather
    /// than ignored.
    ///
    /// # Errors
    ///
    /// [`CaptureRefusal::Scenario`] for a non-empty `scenario` without
    /// `requested`; otherwise the refusal of [`Self::resolve`].
    pub fn gate(
        requested: bool,
        scenario: Scenario,
        capabilities: Option<&str>,
        dir: Option<&str>,
    ) -> Result<Option<Self>, CaptureRefusal> {
        if !requested {
            if !scenario.is_empty() {
                return Err(CaptureRefusal::Scenario(format!(
                    "a start override or trigger watch needs {CAPTURE_ENV}=1: an ordinary run \
                     starts at the measured start and never waits"
                )));
            }
            return Ok(None);
        }
        let mut settings = Self::resolve(capabilities, dir)?;
        settings.scenario = scenario;
        Ok(Some(settings))
    }
}

/// What one delivered screenshot measured.
#[derive(Clone, Debug, PartialEq)]
struct Shot {
    /// Which frame this is: `world` (the probe) or `player`.
    label: &'static str,
    path: PathBuf,
    width: u32,
    height: u32,
    distinct_luminance: usize,
    mean_luminance: f32,
    /// The world's own pixels, when this is the probe frame: the stricter of
    /// the two off-clear-colour counts (see the world-probe note above).
    world_pixels: Option<usize>,
    /// The pixels that are the window's clear colour, when this is the probe
    /// frame: the more permissive of the two near-clear counts. It is what
    /// proves the probe hid the horizon and the player.
    clear_pixels: Option<usize>,
    /// The frame's pixel count.
    total_pixels: usize,
    /// The window's clear colour the probe counted against, its sRGB 8-bit
    /// encoding, when the window declared one.
    clear_colour: Option<[u8; 3]>,
    /// The modal colour the frame drew most of, recorded as a diagnostic.
    background: Option<[u8; 3]>,
}

impl Shot {
    /// The world's pixels over the frame's, in parts per thousand.
    #[must_use]
    fn covered_permille(&self) -> u32 {
        permille(self.world_pixels.unwrap_or(0), self.total_pixels)
    }

    /// The clear colour's pixels over the frame's, in parts per thousand.
    #[must_use]
    fn clear_permille(&self) -> u32 {
        permille(self.clear_pixels.unwrap_or(0), self.total_pixels)
    }
}

/// `part` over `whole` in parts per thousand, `0` for an empty frame.
fn permille(part: usize, whole: usize) -> u32 {
    if whole == 0 {
        return 0;
    }
    u32::try_from(part as u64 * 1000 / whole as u64).unwrap_or(u32::MAX)
}

/// Whether the world probe is currently hiding the horizon and the player:
/// the capture sets it for the probe's few frames and restores it after, and
/// [`apply_world_probe`] is the one system that writes it to the entities.
#[derive(Resource, Clone, Copy, Debug, Default)]
struct WorldProbeState {
    hidden: bool,
}

/// What the run collected. It lives behind a [`CaptureHandle`] because
/// `App::run` takes the world with it: the handle is what [`finish`] reads
/// once the window has closed.
#[derive(Default)]
pub(super) struct CaptureData {
    pub(super) dir: PathBuf,
    label: String,
    pub(super) frame: u32,
    pub(super) lines: Vec<String>,
    last_input_changes: u64,
    shot_requested: bool,
    /// The block the run waits for, when it was asked to.
    pub(super) watch: Option<TriggerWatch>,
    /// The watched block completed (the `trigger` record is in `lines`).
    pub(super) trigger_fired: bool,
    world_shot_requested: bool,
    /// The probe was skipped because the player's aircraft had not spawned by
    /// the probe's frame: [`finish`] names that instead of the renderer when
    /// no world frame was taken for that reason.
    world_probe_skipped: bool,
    /// The window's clear colour, both 8-bit encodings, as it was when the
    /// probe frame was requested (`None` while the probe has not run): what
    /// the world pixels are counted against.
    clear_colours: Option<[[u8; 3]; 2]>,
    /// What the probe hid on the frame it requested its screenshot on —
    /// horizon domes, horizon sea planes and player aircraft — read back from
    /// [`WorldProbeHidden`] so a class the probe found nothing of is refused
    /// rather than silently counted as the world.
    probe_hidden: Option<[usize; 3]>,
    io_error: Option<String>,
    shots: Vec<Shot>,
    /// The last host tick stepped, when one was.
    last_host_tick: Option<u64>,
    /// The `terminal` record, once the host settled.
    terminal: Option<String>,
}

/// The shared slot the capture fills while the window runs.
#[derive(Resource, Clone, Default)]
pub struct CaptureHandle(Arc<Mutex<CaptureData>>);

impl CaptureHandle {
    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, CaptureData> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The capture directory.
    #[must_use]
    pub fn dir(&self) -> Option<PathBuf> {
        Some(self.lock().dir.clone())
    }
}

/// The capture as a plugin; add it to the windowed composition.
pub struct MissionCapturePlugin {
    handle: CaptureHandle,
}

impl MissionCapturePlugin {
    /// A capture of the composition labelled `label` into `settings.dir`.
    #[must_use]
    pub fn new(settings: CaptureSettings, label: &str) -> Self {
        let handle = CaptureHandle::default();
        {
            let mut data = handle.lock();
            data.dir = settings.dir;
            data.watch = settings.scenario.watch;
            data.label = label.to_owned();
            data.lines.push(format!(
                "{{\"kind\":\"header\",\"trace\":\"mission-evidence-trace\",\"mission\":{},\
                 \"human_play\":false,\"verified_original\":false,\"frame_hz\":60,\
                 \"sample_every_frames\":{SAMPLE_EVERY_FRAMES}}}",
                json_string(label)
            ));
            if !settings.scenario.is_empty() {
                data.lines.push(settings.scenario.trace_line());
            }
        }
        Self { handle }
    }

    /// The slot [`finish`] reads after the window closes.
    #[must_use]
    pub fn handle(&self) -> CaptureHandle {
        self.handle.clone()
    }
}

impl Plugin for MissionCapturePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.handle.clone())
            .init_resource::<WorldProbeState>()
            .init_resource::<WorldProbeHidden>()
            // After Bevy's keyboard system and before the F22 pump, as the
            // smoke script: the held key is the keyboard state the pump reads.
            .add_systems(
                PreUpdate,
                drive_input
                    .in_set(InputSystems)
                    .after(bevy::input::keyboard::keyboard_input_system),
            )
            .add_systems(FixedPostUpdate, watch_trigger.after(mission_host_tick))
            .add_systems(Update, (keep_focus, apply_world_probe))
            .add_systems(Last, capture_step);
    }
}

/// What the probe's classes hold right now — how many horizon domes, horizon
/// sea planes and player aircraft the hide transition covers. It is read back
/// when the probe frame is requested, so the judge refuses a probe that found
/// nothing of a class: its pixels were never hidden and would be counted as
/// the world's.
#[derive(Resource, Clone, Copy, Debug, Default)]
struct WorldProbeHidden {
    /// `crate::render::horizon`'s dome entities present.
    domes: usize,
    /// `crate::render::horizon`'s sea entities present.
    seas: usize,
    /// The player's aircraft body entities present.
    players: usize,
}

/// Writes the probe's hide flag to the three classes of entity it covers —
/// the horizon's dome and sea (#1434) and the player's aircraft — so the
/// probe frame's background is the flat clear colour and every pixel off it
/// is the world's own geometry. The flag changes twice in a run, so the
/// components are written only on those frames; the counts are refreshed
/// every frame so [`judge_world_probe`] reads what the frame really hid.
fn apply_world_probe(
    probe: Res<WorldProbeState>,
    mut hidden: ResMut<WorldProbeHidden>,
    mut commands: Commands,
    domes: Query<Entity, With<HorizonDome>>,
    seas: Query<Entity, With<HorizonSea>>,
    players: Query<Entity, With<PlaytestAircraft>>,
) {
    hidden.domes = domes.iter().count();
    hidden.seas = seas.iter().count();
    hidden.players = players.iter().count();
    if !probe.is_changed() {
        return;
    }
    let value = if probe.hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for entity in &domes {
        commands.entity(entity).insert(value);
    }
    for entity in &seas {
        commands.entity(entity).insert(value);
    }
    for entity in &players {
        commands.entity(entity).insert(value);
    }
}

fn drive_input(capture: Res<CaptureHandle>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    let capture = capture.lock();
    // A watched run flies the production body untouched: the scripted
    // pitch-up would only move the approach it is measuring.
    if capture.watch.is_some() {
        return;
    }
    let frame = capture.frame;
    if frame == INPUT_FROM_FRAME {
        keys.press(KeyCode::ArrowDown);
    }
    if frame == INPUT_TO_FRAME {
        keys.release(KeyCode::ArrowDown);
    }
}

/// An unfocused window would pause the run through the real focus policy; an
/// evidence run is automated, so the capture keeps the session focused.
fn keep_focus(mut platform: ResMut<PlatformInput>) {
    if !platform.session().is_focused() {
        platform.session_mut().set_focus(true);
    }
}

#[allow(clippy::too_many_arguments)]
fn capture_step(
    handle: Res<CaptureHandle>,
    state: Res<PlaytestState>,
    ledger: Res<PhysicsTickLedger>,
    camera: Res<PlaytestCamera>,
    host: Option<Res<MissionHost>>,
    aircraft: Query<&Position, With<PlaytestAircraft>>,
    clear: Option<Res<ClearColor>>,
    hidden: Res<WorldProbeHidden>,
    mut probe: ResMut<WorldProbeState>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let mut capture = handle.lock();
    capture.frame += 1;
    let frame = capture.frame;
    let player = aircraft.single().ok().copied();
    if player.is_some() && frame.is_multiple_of(SAMPLE_EVERY_FRAMES) {
        let t = state.telemetry;
        let line = format!(
            "{{\"kind\":\"tick\",\"frame\":{frame},\"tick\":{},\"host_tick\":{},\
             \"position_m\":[{},{},{}],\"speed_m_s\":{},\"paused\":{}}}",
            ledger.ticks,
            host.as_ref()
                .and_then(|host| host.last_tick())
                .map_or_else(|| "null".to_owned(), |tick| tick.0.to_string()),
            json_f32(t.position_m[0]),
            json_f32(t.position_m[1]),
            json_f32(t.position_m[2]),
            json_f32(t.speed_m_s),
            state.paused,
        );
        capture.lines.push(line);
        if let Some(rig) = &camera.frame {
            let position = rig.pose.position().to_array();
            let line = format!(
                "{{\"kind\":\"camera\",\"frame\":{frame},\"tick\":{},\"rig\":{},\"mode\":{},\
                 \"position_m\":[{},{},{}]}}",
                rig.at.0,
                json_string(rig.rig.label()),
                json_string(&format!("{:?}", rig.mode)),
                position[0],
                position[1],
                position[2],
            );
            capture.lines.push(line);
        }
    }
    capture.last_host_tick = host
        .as_ref()
        .and_then(|host| host.last_tick())
        .map(|tick| tick.0);
    if capture.terminal.is_none()
        && let Some(terminal) = host.as_ref().and_then(|host| host.terminal())
    {
        capture.terminal = Some(format!(
            "{{\"kind\":\"terminal\",\"settled\":true,\"outcome\":{},\"source\":{},\
             \"tick\":{},\"session\":{},\"exit_code\":{},\"debrief\":{},\"report_line\":{}}}",
            json_string(terminal.outcome.label()),
            json_string(terminal.source.label()),
            terminal.tick.0,
            terminal.session.0,
            terminal.exit.code(),
            terminal.debrief.as_ref().map_or_else(
                || "null".to_owned(),
                |debrief| {
                    format!(
                        "{{\"run_time\":{},\"shots\":{},\"hits\":{},\"cash_earned\":null,\
                         \"planes_downed\":{}}}",
                        json_string(&debrief.run_time()),
                        debrief.shots,
                        debrief.hits,
                        debrief.planes_downed,
                    )
                },
            ),
            json_string(&terminal.report_line),
        ));
    }
    if state.input_changes != capture.last_input_changes {
        capture.last_input_changes = state.input_changes;
        let command = state.command;
        let line = format!(
            "{{\"kind\":\"input\",\"frame\":{frame},\"tick\":{},\"input_changes\":{},\
             \"command\":[{},{},{},{}]}}",
            ledger.ticks,
            state.input_changes,
            command.pitch,
            command.roll,
            command.yaw,
            command.throttle,
        );
        capture.lines.push(line);
    }

    if frame == 1
        && let Err(error) = std::fs::create_dir_all(&capture.dir)
    {
        capture.io_error = Some(format!("cannot create {}: {error}", capture.dir.display()));
    }
    // The world probe (M01-PLAY-WORLD-VISIBLE, #1433): hide the horizon and
    // the player for a few frames, take one frame of the world alone, then
    // restore. The probe frame's background is the flat clear colour, so
    // every pixel off it is geometry the world load drew.
    if frame == WORLD_PROBE_HIDE_FRAME {
        probe.hidden = true;
    }
    if frame == WORLD_PROBE_FRAME && !capture.world_shot_requested {
        if player.is_some() {
            capture.world_shot_requested = true;
            capture.clear_colours = clear.as_deref().map(clear_colour_encodings);
            capture.probe_hidden = Some([hidden.domes, hidden.seas, hidden.players]);
            let path = capture.dir.join(format!("frame_{frame:05}_world.png"));
            request_screenshot(&mut commands, path, "world");
        } else {
            // The player had not spawned on the probe's own frame, so no
            // world-alone frame can be taken: the probe is skipped by name
            // instead of leaving the run waiting for a request that will
            // never come. `finish` refuses the capture for it.
            capture.world_probe_skipped = true;
        }
    }
    if frame == WORLD_PROBE_RESTORE_FRAME {
        probe.hidden = false;
    }
    if !capture.shot_requested && frame >= SHOT_FRAME && player.is_some() {
        capture.shot_requested = true;
        let path = capture.dir.join(format!("frame_{frame:05}_player.png"));
        request_screenshot(&mut commands, path, "player");
    }
    let delivered = capture.shots.len();
    let requested = usize::from(capture.world_shot_requested) + usize::from(capture.shot_requested);
    // A skipped probe still counts towards what the run asked for, and the
    // grace frame bounds every capture: a run whose frames never arrive still
    // ends, and `finish` says what is missing rather than the window never
    // closing.
    let wanted = requested + usize::from(capture.world_probe_skipped);
    let done = match capture.watch {
        // A watched run ends once its block completed and the frame of the
        // trigger was delivered, or when its frame budget is spent.
        Some(watch) => {
            let trigger_delivered = capture.shots.iter().any(|shot| shot.label == "trigger");
            (capture.trigger_fired && trigger_delivered && delivered >= requested)
                || frame >= watch.max_frames + SHOT_GRACE_FRAMES
        }
        None => (wanted >= 2 && delivered >= requested) || frame >= SHOT_FRAME + SHOT_GRACE_FRAMES,
    };
    if done {
        exit.write(AppExit::Success);
    }
}

pub(super) fn request_screenshot(commands: &mut Commands, path: PathBuf, label: &'static str) {
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, handle: Res<CaptureHandle>| {
            let clear_colours = handle.lock().clear_colours;
            let shot = measure(&captured.image, path.clone(), label, clear_colours);
            let delivered = captured.image.data.is_some();
            save_to_disk(path.clone())(captured);
            if delivered {
                handle.lock().shots.push(shot);
            }
        },
    );
}

/// The window's clear colour in the two 8-bit encodings the probe counts
/// against: the sRGB-encoded value an sRGB surface stores, and the linear one
/// a plain surface stores.
pub(super) fn clear_colour_encodings(clear: &ClearColor) -> [[u8; 3]; 2] {
    fn encode(red: f32, green: f32, blue: f32) -> [u8; 3] {
        [
            (red * 255.0).round() as u8,
            (green * 255.0).round() as u8,
            (blue * 255.0).round() as u8,
        ]
    }
    let srgb = clear.0.to_srgba();
    let linear = bevy::color::LinearRgba::from(srgb);
    [
        encode(srgb.red, srgb.green, srgb.blue),
        encode(linear.red, linear.green, linear.blue),
    ]
}

fn measure(
    image: &bevy::image::Image,
    path: PathBuf,
    label: &'static str,
    clear_colours: Option<[[u8; 3]; 2]>,
) -> Shot {
    let mut levels = std::collections::BTreeSet::new();
    let mut sum = 0_u64;
    let mut count = 0_u64;
    if let Some(data) = image.data.as_ref() {
        for pixel in data.as_chunks::<4>().0 {
            let luminance =
                ((29 * u32::from(pixel[0]) + 150 * u32::from(pixel[1]) + 77 * u32::from(pixel[2]))
                    >> 8) as u8;
            levels.insert(luminance);
            sum += u64::from(luminance);
            count += 1;
        }
    }
    let counted = if label == "world" {
        let data = image.data.as_deref().unwrap_or(&[]);
        Some(count_world_pixels(
            data,
            image.texture_descriptor.format,
            clear_colours,
        ))
    } else {
        None
    };
    Shot {
        label,
        path,
        width: image.width(),
        height: image.height(),
        distinct_luminance: levels.len(),
        mean_luminance: if count == 0 {
            0.0
        } else {
            sum as f32 / count as f32
        },
        world_pixels: counted.map(|counted| counted.world),
        clear_pixels: counted.map(|counted| counted.clear),
        total_pixels: count as usize,
        clear_colour: clear_colours.map(|colours| colours[0]),
        background: counted.map(|counted| counted.modal),
    }
}

/// What the probe frame measured against the window's clear colour.
///
/// `pub(super)`: the owner-command smoke ([`super::smoke`]) counts its probes
/// against the same declared clear colour through this same function, so the
/// two paths cannot drift apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct WorldPixelCount {
    /// Pixels off the clear colour: the world's own geometry.
    pub(super) world: usize,
    /// Pixels that are the clear colour: the background the probe left.
    pub(super) clear: usize,
    /// The frame's modal colour, recorded as a diagnostic only — the counts
    /// above are taken against the declared clear colour, never against it.
    pub(super) modal: [u8; 3],
}

/// Counts the frame's world pixels against the window's **declared clear
/// colour** (`clear_colours`, the sRGB and the linear 8-bit encodings): every
/// pixel further from both encodings than [`BACKGROUND_CHANNEL_TOLERANCE`] on
/// any channel is the world's, and every pixel within tolerance of one of
/// them is the background's.
///
/// The stricter of the two off-colour counts is taken as the world's and the
/// more permissive near-colour count as the background's, so the surface's
/// encoding — `Bgra8UnormSrgb` on the pinned Metal backend, a plain
/// `*Unorm` elsewhere — cannot turn the background into "world" or the world
/// into "background". The window screenshot's bytes are in the surface's own
/// channel order, so `format` decides how the first three bytes are read; the
/// counts are order-independent, only the recorded colours are not.
///
/// With no declared clear colour the frame's own **modal** colour stands in
/// for both encodings, so the measurement is still a measurement;
/// [`finish`] refuses the probe for the missing declaration regardless, since
/// a modal reference cannot prove the horizon was hidden.
pub(super) fn count_world_pixels(
    data: &[u8],
    format: bevy::render::render_resource::TextureFormat,
    clear_colours: Option<[[u8; 3]; 2]>,
) -> WorldPixelCount {
    let rgb = |pixel: &[u8; 4]| match format {
        bevy::render::render_resource::TextureFormat::Bgra8Unorm
        | bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb => {
            [pixel[2], pixel[1], pixel[0]]
        }
        _ => [pixel[0], pixel[1], pixel[2]],
    };
    let pixels = data.as_chunks::<4>().0;
    let mut counts: std::collections::HashMap<[u8; 3], usize> = std::collections::HashMap::new();
    for pixel in pixels {
        *counts.entry(rgb(pixel)).or_default() += 1;
    }
    let modal = counts
        .iter()
        .max_by_key(|(_, count)| **count)
        .map_or([0, 0, 0], |(colour, _)| *colour);
    let references = clear_colours.unwrap_or([modal, modal]);
    let mut off = [0_usize; 2];
    let mut near = [0_usize; 2];
    for pixel in pixels {
        let colour = rgb(pixel);
        for (index, reference) in references.iter().enumerate() {
            if same_colour(colour, *reference) {
                near[index] += 1;
            } else {
                off[index] += 1;
            }
        }
    }
    WorldPixelCount {
        world: off[0].min(off[1]),
        clear: near[0].max(near[1]),
        modal,
    }
}

/// Whether two colours are the same within [`BACKGROUND_CHANNEL_TOLERANCE`] on
/// every channel.
pub(super) fn same_colour(left: [u8; 3], right: [u8; 3]) -> bool {
    (0..3).all(|channel| left[channel].abs_diff(right[channel]) <= BACKGROUND_CHANNEL_TOLERANCE)
}

/// Writes `trace.jsonl` after the window has closed and judges the run.
///
/// The `terminal` record says whether the mission host settled; `settled` is
/// `false` when the capture ended the run before the mission did.
///
/// # Errors
///
/// [`CaptureRefusal`] when the directory cannot be written, or when the run
/// ended with no delivered non-empty screenshot or without having stepped a
/// mission host tick. The trace is written first either way, so the
/// diagnostics survive.
pub fn finish(handle: &CaptureHandle) -> Result<(), CaptureRefusal> {
    let capture = handle.lock();
    let stepped = capture.last_host_tick.is_some();
    let mut lines = capture.lines.clone();
    for shot in &capture.shots {
        lines.push(format!(
            "{{\"kind\":\"screenshot\",\"label\":{},\"path\":{},\"width\":{},\
             \"height\":{},\"distinct_luminance\":{},\"mean_luminance\":{}}}",
            json_string(shot.label),
            json_string(&shot.path.display().to_string()),
            shot.width,
            shot.height,
            shot.distinct_luminance,
            json_f32(shot.mean_luminance),
        ));
    }
    // The world probe's own record: the pixels the world load drew in the
    // frame around the spawn, counted against the window's clear colour
    // (M01-PLAY-WORLD-VISIBLE, #1433).
    if let Some(shot) = capture.shots.iter().find(|shot| shot.label == "world") {
        let background = shot.background.unwrap_or([0, 0, 0]);
        let clear_colour = shot.clear_colour.map_or_else(
            || "null".to_owned(),
            |clear| format!("[{},{},{}]", clear[0], clear[1], clear[2]),
        );
        lines.push(format!(
            "{{\"kind\":\"world\",\"label\":\"world\",\"path\":{},\"width\":{},\"height\":{},\
             \"world_pixels\":{},\"total_pixels\":{},\"covered_permille\":{},\
             \"clear_pixels\":{},\"clear_permille\":{},\
             \"background\":[{},{},{}],\"clear_colour\":{clear_colour},\
             \"min_covered_permille\":{MIN_WORLD_COVERAGE_PERMILLE},\
             \"min_clear_permille\":{MIN_CLEAR_COVERAGE_PERMILLE}}}",
            json_string(&shot.path.display().to_string()),
            shot.width,
            shot.height,
            shot.world_pixels.unwrap_or(0),
            shot.total_pixels,
            shot.covered_permille(),
            shot.clear_pixels.unwrap_or(0),
            shot.clear_permille(),
            background[0],
            background[1],
            background[2],
        ));
    }
    lines.push(capture.terminal.clone().unwrap_or_else(|| {
        format!(
            "{{\"kind\":\"terminal\",\"settled\":false,\"frames\":{},\"last_host_tick\":{}}}",
            capture.frame,
            capture
                .last_host_tick
                .map_or_else(|| "null".to_owned(), |tick| tick.to_string()),
        )
    }));

    if let Some(error) = &capture.io_error {
        return Err(CaptureRefusal::Io(error.clone()));
    }
    write_trace(&capture.dir, &lines)?;

    if !stepped {
        return Err(CaptureRefusal::Incomplete(format!(
            "{} stepped no mission host tick",
            capture.label
        )));
    }
    if let Some(watch) = capture.watch
        && !capture.trigger_fired
    {
        return Err(CaptureRefusal::Incomplete(format!(
            "{} never completed block {} within {} frames",
            capture.label, watch.block, watch.max_frames
        )));
    }
    // The world probe is the check this capture exists for: the frame around
    // the spawn must hold the world's own pixels, and the horizon the probe
    // hid must not be among them.
    judge_world_probe(
        capture.shots.iter().find(|shot| shot.label == "world"),
        capture.clear_colours,
        capture.world_probe_skipped,
        capture.probe_hidden,
    )
}

/// Names the probe's three hidden classes for its refusals.
const PROBE_CLASSES: [&str; 3] = ["horizon dome", "horizon sea", "player aircraft"];

/// Judges the probe frame — the check the mission capture exists for.
///
/// The frame around the spawn must hold the world's own pixels over
/// [`MIN_WORLD_COVERAGE_PERMILLE`], counted against the window's **declared**
/// clear colour, and that clear colour must itself still be visible over
/// [`MIN_CLEAR_COVERAGE_PERMILLE`]: a probe frame the clear colour has
/// vanished from did not hide the horizon, whose dome and sea are geometry
/// that would otherwise be counted as terrain and pass a worldless window.
///
/// # Errors
///
/// [`CaptureRefusal::Incomplete`], naming the first refusal in that order:
/// no probe frame (and that the probe was skipped when the player had not
/// spawned), an empty one, a window that declares no clear colour, a class of
/// entity the probe found nothing of to hide, a frame under the world floor,
/// a frame under the clear floor.
fn judge_world_probe(
    shot: Option<&Shot>,
    clear_colours: Option<[[u8; 3]; 2]>,
    probe_skipped: bool,
    probe_hidden: Option<[usize; 3]>,
) -> Result<(), CaptureRefusal> {
    let Some(shot) = shot else {
        return Err(CaptureRefusal::Incomplete(if probe_skipped {
            "the world probe never ran: the player's aircraft had not spawned by the probe's \
             frame, so no frame of the world alone was taken"
                .to_owned()
        } else {
            "the renderer delivered no world-probe framebuffer screenshot".to_owned()
        }));
    };
    if shot.width == 0 || shot.height == 0 {
        return Err(CaptureRefusal::Incomplete(format!(
            "the world probe's frame is empty ({}x{}: {})",
            shot.width,
            shot.height,
            shot.path.display()
        )));
    }
    if clear_colours.is_none() {
        return Err(CaptureRefusal::Incomplete(
            "the window declares no clear colour, so the probe cannot tell the world's pixels \
             from the background they were drawn over"
                .to_owned(),
        ));
    }
    let Some(probe_hidden) = probe_hidden else {
        return Err(CaptureRefusal::Incomplete(
            "the probe did not record what it hid, so its frame cannot be judged".to_owned(),
        ));
    };
    for (found, class) in probe_hidden.iter().zip(PROBE_CLASSES) {
        if *found == 0 {
            return Err(CaptureRefusal::Incomplete(format!(
                "the probe found no {class} to hide, so the frame it took still draws one and \
                 its pixels would be counted as the world's"
            )));
        }
    }
    if shot.covered_permille() < MIN_WORLD_COVERAGE_PERMILLE {
        return Err(CaptureRefusal::Incomplete(format!(
            "the frame around the spawn drew no world: {} holds {} of {} pixels off the \
             window's clear colour ({} per mille, less than the {} required), so the world's \
             geometry is not being drawn",
            shot.path.display(),
            shot.world_pixels.unwrap_or(0),
            shot.total_pixels,
            shot.covered_permille(),
            MIN_WORLD_COVERAGE_PERMILLE,
        )));
    }
    if shot.clear_permille() < MIN_CLEAR_COVERAGE_PERMILLE {
        return Err(CaptureRefusal::Incomplete(format!(
            "the probe frame {} holds only {} of {} pixels of the window's clear colour ({} \
             per mille, less than the {} required): the horizon and the player were not \
             hidden, so their own pixels would be counted as the world's",
            shot.path.display(),
            shot.clear_pixels.unwrap_or(0),
            shot.total_pixels,
            shot.clear_permille(),
            MIN_CLEAR_COVERAGE_PERMILLE,
        )));
    }
    Ok(())
}

fn write_trace(dir: &Path, lines: &[String]) -> Result<(), CaptureRefusal> {
    std::fs::create_dir_all(dir)
        .map_err(|error| CaptureRefusal::Io(format!("cannot create {}: {error}", dir.display())))?;
    let path = dir.join(TRACE_FILE);
    let mut body = lines.join("\n");
    body.push('\n');
    std::fs::write(&path, body)
        .map_err(|error| CaptureRefusal::Io(format!("cannot write {}: {error}", path.display())))
}

pub(super) fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_f32(value: f32) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "null".to_owned()
    }
}

/// Name of the audio playback record inside the capture directory.
pub const PLAYBACK_FILE: &str = "playback.json";

/// Writes the launch's audio playback record into `dir`
/// (VS-M01-RT-EVIDENCE, Rally #1218): which original archive member the
/// device was handed and what its output stream pulled from it.
///
/// This is the **source boundary** — frames the output stream consumed —
/// never a capture of a speaker; that stays the owner's `human_review`.
///
/// # Errors
///
/// [`CaptureRefusal::Incomplete`] when the audible backend was refused, the
/// library never received the member, or the stream pulled nothing or only
/// silence; [`CaptureRefusal::Io`] when the record cannot be written.
pub fn write_playback(
    dir: &Path,
    launch: &super::LaunchSound,
    probe: &super::LaunchAudioProbe,
) -> Result<(), CaptureRefusal> {
    if let Some(refusal) = &probe.refusal {
        return Err(CaptureRefusal::Incomplete(format!(
            "the audible audio backend was refused: {refusal}"
        )));
    }
    let pcm = probe.library.pcm(&launch.content).ok_or_else(|| {
        CaptureRefusal::Incomplete(format!(
            "the device library never received {}",
            launch.content
        ))
    })?;
    let pulled = probe.probe.pulled();
    let peak = probe.probe.peak();
    if pulled == 0 || peak <= 0.0 {
        return Err(CaptureRefusal::Incomplete(format!(
            "the output stream pulled nothing audible from {} ({pulled} samples, peak {peak})",
            launch.member
        )));
    }
    let record = format!(
        "{{\n  \"boundary\": \"sample_probe_source_pull\",\n  \"asset\": {},\n  \
         \"airframe_record\": {},\n  \"sound_group\": {},\n  \"archive\": {},\n  \
         \"archive_sha256\": \"{}\",\n  \"container_choice\": \"labelled choice: which sound archive the original opens is unmeasured\",\n  \"member\": {},\n  \"channels\": {},\n  \
         \"rate_hz\": {},\n  \"member_frames\": {},\n  \"member_duration_s\": {:.4},\n  \
         \"sample_values_pulled\": {pulled},\n  \"frames_pulled\": {},\n  \"peak\": {},\n  \
         \"backend\": \"audible\",\n  \"verified_original\": false,\n  \
         \"human_review\": false\n}}\n",
        json_string(&launch.content.to_string()),
        json_string(&launch.airframe),
        json_string(&launch.group),
        json_string(&launch.container),
        launch.container_sha256.to_hex(),
        json_string(&launch.member),
        pcm.channels(),
        pcm.rate_hz(),
        pcm.frames(),
        pcm.duration().as_secs_f64(),
        pulled / u64::from(pcm.channels().max(1)),
        json_f32(peak),
    );
    let path = dir.join(PLAYBACK_FILE);
    std::fs::write(&path, record)
        .map_err(|error| CaptureRefusal::Io(format!("cannot write {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use bevy::render::render_resource::TextureFormat;

    use super::*;

    /// The window's clear colour as the probe counts it: `[61,112,199]`
    /// sRGB, `[12,41,145]` linear.
    const CLEAR: [[u8; 3]; 2] = [[61, 112, 199], [12, 41, 145]];
    /// A world colour the clear colour is nowhere near.
    const TERRAIN: [u8; 3] = [120, 96, 60];

    /// A frame of `clear` background pixels and `world` terrain pixels, in the
    /// byte order `format` reads.
    fn frame(
        format: TextureFormat,
        clear: [u8; 3],
        world: [u8; 3],
        clear_n: usize,
        world_n: usize,
    ) -> Vec<u8> {
        let bgra = matches!(
            format,
            TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
        );
        let mut bytes = Vec::new();
        for colour in std::iter::repeat_n(clear, clear_n).chain(std::iter::repeat_n(world, world_n))
        {
            if bgra {
                bytes.extend_from_slice(&[colour[2], colour[1], colour[0], 255]);
            } else {
                bytes.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
            }
        }
        bytes
    }

    /// **The probe counts the world's pixels off the declared clear colour,
    /// whatever the surface's channel order or encoding.**
    ///
    /// A frame of 30 clear and 10 terrain pixels must count exactly the 10 as
    /// the world's and the 30 as the background's on an RGBA surface, a BGRA
    /// one, and a surface that stores the **linear** encoding of the same
    /// clear colour — the stricter off-colour count and the more permissive
    /// near-colour count are what make the encoding irrelevant.
    #[test]
    fn accept_m01_play_world_the_probe_counts_world_pixels_off_the_declared_clear_colour() {
        for format in [
            TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Bgra8UnormSrgb,
            TextureFormat::Bgra8Unorm,
        ] {
            let bytes = frame(format, CLEAR[0], TERRAIN, 30, 10);
            let counted = count_world_pixels(&bytes, format, Some(CLEAR));
            assert_eq!(
                counted,
                WorldPixelCount {
                    world: 10,
                    clear: 30,
                    modal: CLEAR[0],
                },
                "{format:?} reads the same frame the same way"
            );
        }
        // The same frame as the *linear* surface stores it: the linear
        // encoding is the second reference, so the counts do not change.
        let linear = frame(TextureFormat::Rgba8Unorm, CLEAR[1], TERRAIN, 30, 10);
        assert_eq!(
            count_world_pixels(&linear, TextureFormat::Rgba8Unorm, Some(CLEAR)),
            WorldPixelCount {
                world: 10,
                clear: 30,
                modal: CLEAR[1],
            }
        );
        // With no declared clear colour the frame's own modal colour stands
        // in: the measurement is still a measurement.
        let undeclared = frame(TextureFormat::Rgba8Unorm, CLEAR[0], TERRAIN, 30, 10);
        assert_eq!(
            count_world_pixels(&undeclared, TextureFormat::Rgba8Unorm, None).world,
            10
        );
    }

    /// **The judge refuses every frame the probe cannot prove: no frame, an
    /// empty one, a worldless one, one whose horizon was never hidden, and
    /// one the horizon was not hidden from.**
    ///
    /// The clear-colour floor is for the second: the horizon's dome and sea
    /// are geometry, and a probe frame they fill counts no clear colour at
    /// all — which must refuse, not pass. The hidden-class floor is for a
    /// probe that found nothing of a class to hide in the first place.
    #[test]
    fn accept_m01_play_world_the_probe_judges_only_a_frame_that_holds_the_world() {
        let shot = |world: usize, clear: usize, total: usize, size: (u32, u32)| Shot {
            label: "world",
            path: PathBuf::from("frame_00032_world.png"),
            width: size.0,
            height: size.1,
            distinct_luminance: 2,
            mean_luminance: 0.0,
            world_pixels: Some(world),
            total_pixels: total,
            clear_pixels: Some(clear),
            clear_colour: Some(CLEAR[0]),
            background: Some(CLEAR[0]),
        };
        let total = 1_000_000;
        let hid = Some([1_usize, 1, 1]);
        assert!(
            judge_world_probe(
                Some(&shot(634_000, 366_000, total, (2560, 1440))),
                Some(CLEAR),
                false,
                hid,
            )
            .is_ok(),
            "the measured M01 probe frame is accepted"
        );
        let worldless = shot(0, total, total, (2560, 1440));
        let error = judge_world_probe(Some(&worldless), Some(CLEAR), false, hid)
            .expect_err("a flat frame drew no world");
        assert!(error.to_string().contains("drew no world"), "{error}");
        // The horizon was not hidden: no clear colour is left in the frame.
        let unhidden = shot(total, 0, total, (2560, 1440));
        let error = judge_world_probe(Some(&unhidden), Some(CLEAR), false, hid)
            .expect_err("the horizon's own pixels are not the world's");
        assert!(error.to_string().contains("not hidden"), "{error}");
        // Nothing of one class to hide: its pixels would be counted instead.
        let nothing_to_hide = judge_world_probe(
            Some(&shot(634_000, 366_000, total, (2560, 1440))),
            Some(CLEAR),
            false,
            Some([0, 1, 1]),
        )
        .expect_err("a probe that found no dome is refused by name");
        assert!(
            nothing_to_hide.to_string().contains("horizon dome"),
            "{nothing_to_hide}"
        );
        let unrecorded = judge_world_probe(
            Some(&shot(634_000, 366_000, total, (2560, 1440))),
            Some(CLEAR),
            false,
            None,
        )
        .expect_err("a probe that recorded nothing is refused by name");
        assert!(
            unrecorded.to_string().contains("did not record"),
            "{unrecorded}"
        );
        let empty = shot(0, 0, 0, (0, 0));
        assert!(
            judge_world_probe(Some(&empty), Some(CLEAR), false, hid)
                .expect_err("an empty frame is not evidence")
                .to_string()
                .contains("empty"),
            "an empty frame is refused by name"
        );
        let skipped = judge_world_probe(None, Some(CLEAR), true, hid)
            .expect_err("a probe that never ran is refused by name");
        assert!(skipped.to_string().contains("never ran"), "{skipped}");
        let undeclared = judge_world_probe(
            Some(&shot(634_000, 366_000, total, (2560, 1440))),
            None,
            false,
            hid,
        )
        .expect_err("an undeclared clear colour is refused by name");
        assert!(
            undeclared.to_string().contains("declares no clear colour"),
            "{undeclared}"
        );
    }

    /// **The clear colour's two encodings are the values the render pass
    /// clears to** — the sRGB one an sRGB surface stores and the linear one a
    /// plain surface stores — measured, never remembered.
    #[test]
    fn accept_m01_play_world_the_probe_reads_the_window_s_clear_colour() {
        let clear = ClearColor(bevy::prelude::Color::srgb(0.24, 0.44, 0.78));
        assert_eq!(
            clear_colour_encodings(&clear),
            [[61, 112, 199], [12, 41, 145]]
        );
    }
}
