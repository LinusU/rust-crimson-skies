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
use bevy::app::AppExit;
use bevy::input::keyboard::KeyCode;
use bevy::input::{ButtonInput, InputSystems};
use bevy::prelude::{
    App, Commands, DetectChanges, Entity, IntoScheduleConfigs, Last, MessageWriter, On, Or, Plugin,
    PreUpdate, Query, Res, ResMut, Resource, Update, Visibility, With,
};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

use super::host::MissionHost;
use super::sky::DesignedSkyDome;
use crate::audio::device::SampleLibrary;
use crate::input::platform::PlatformInput;
use crate::physics::PhysicsTickLedger;
use crate::playtest::scene::PlaytestAircraft;
use crate::playtest::{PlaytestCamera, PlaytestState};

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
// spawn and without freezing the camera: it hides the designed sky dome and
// the player's own aircraft for a few frames, so the frame's **background** is
// the flat clear colour and every pixel off it is geometry the world load
// drew. A frame with no world pixels is the pre-#1433 defect — the world's
// meshes spawned with no material, so nothing drew.

/// The rendered frame the world probe starts hiding the sky and the player on.
const WORLD_PROBE_HIDE_FRAME: u32 = 30;
/// The rendered frame the world probe's screenshot is requested on, with the
/// sky and the player hidden and the world drawn.
const WORLD_PROBE_FRAME: u32 = 32;
/// The rendered frame the probe restores the sky and the player on.
const WORLD_PROBE_RESTORE_FRAME: u32 = 34;
/// How far a channel may differ from the frame's own modal colour and still be
/// called that colour: the renderer's clear colour is uniform, and a small
/// tolerance absorbs any dither the post chain adds.
const BACKGROUND_CHANNEL_TOLERANCE: u8 = 6;
/// The least fraction of the probe frame that must be world pixels, in parts
/// per thousand. A frame that drew no world geometry is a single flat colour
/// (zero), and a frame that drew the world around a spawn near a cliff and a
/// valley covers far more than this.
pub const MIN_WORLD_COVERAGE_PERMILLE: u32 = 20;

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
        }
    }
}

impl std::error::Error for CaptureRefusal {}

/// A capture that has been checked to be possible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureSettings {
    /// Where the PNG and `trace.jsonl` are written.
    pub dir: PathBuf,
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
        if std::env::var(CAPTURE_ENV).as_deref() != Ok("1") {
            return Ok(None);
        }
        Self::resolve(
            std::env::var(CAPABILITIES_ENV).ok().as_deref(),
            std::env::var(EVIDENCE_DIR_ENV).ok().as_deref(),
        )
        .map(Some)
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
    /// The world's own pixels, when this is the probe frame: every pixel that
    /// differs from the frame's modal colour by more than
    /// [`BACKGROUND_CHANNEL_TOLERANCE`] on any channel.
    world_pixels: Option<usize>,
    /// The frame's pixel count.
    total_pixels: usize,
    /// The modal colour the world pixels were counted against.
    background: Option<[u8; 3]>,
}

impl Shot {
    /// The world's pixels over the frame's, in parts per thousand.
    #[must_use]
    fn covered_permille(&self) -> u32 {
        if self.total_pixels == 0 {
            return 0;
        }
        let permille = self.world_pixels.unwrap_or(0) as u64 * 1000 / self.total_pixels as u64;
        u32::try_from(permille).unwrap_or(u32::MAX)
    }
}

/// Whether the world probe is currently hiding the sky and the player: the
/// capture sets it for the probe's few frames and restores it after, and
/// [`apply_world_probe`] is the one system that writes it to the entities.
#[derive(Resource, Clone, Copy, Debug, Default)]
struct WorldProbeState {
    hidden: bool,
}

/// What the run collected. It lives behind a [`CaptureHandle`] because
/// `App::run` takes the world with it: the handle is what [`finish`] reads
/// once the window has closed.
#[derive(Default)]
struct CaptureData {
    dir: PathBuf,
    label: String,
    frame: u32,
    lines: Vec<String>,
    last_input_changes: u64,
    shot_requested: bool,
    world_shot_requested: bool,
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
    fn lock(&self) -> std::sync::MutexGuard<'_, CaptureData> {
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
            data.label = label.to_owned();
            data.lines.push(format!(
                "{{\"kind\":\"header\",\"trace\":\"mission-evidence-trace\",\"mission\":{},\
                 \"human_play\":false,\"verified_original\":false,\"frame_hz\":60,\
                 \"sample_every_frames\":{SAMPLE_EVERY_FRAMES}}}",
                json_string(label)
            ));
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
            // After Bevy's keyboard system and before the F22 pump, as the
            // smoke script: the held key is the keyboard state the pump reads.
            .add_systems(
                PreUpdate,
                drive_input
                    .in_set(InputSystems)
                    .after(bevy::input::keyboard::keyboard_input_system),
            )
            .add_systems(Update, (keep_focus, apply_world_probe))
            .add_systems(Last, capture_step);
    }
}

/// Writes the probe's hide flag to the two classes of entity it covers — the
/// designed sky dome and the player's aircraft — so the probe frame's
/// background is the flat clear colour and every pixel off it is the world's
/// own geometry. The flag changes twice in a run, so the components are
/// written only on those frames.
#[allow(clippy::type_complexity)]
fn apply_world_probe(
    probe: Res<WorldProbeState>,
    mut commands: Commands,
    targets: Query<Entity, Or<(With<DesignedSkyDome>, With<PlaytestAircraft>)>>,
) {
    if !probe.is_changed() {
        return;
    }
    let value = if probe.hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for entity in &targets {
        commands.entity(entity).insert(value);
    }
}

fn drive_input(capture: Res<CaptureHandle>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    let frame = capture.lock().frame;
    if frame == INPUT_FROM_FRAME {
        keys.press(KeyCode::KeyS);
    }
    if frame == INPUT_TO_FRAME {
        keys.release(KeyCode::KeyS);
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
             \"tick\":{},\"session\":{},\"exit_code\":{},\"report_line\":{}}}",
            json_string(terminal.outcome.label()),
            json_string(terminal.source.label()),
            terminal.tick.0,
            terminal.session.0,
            terminal.exit.code(),
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
    // The world probe (M01-PLAY-WORLD-VISIBLE, #1433): hide the sky and the
    // player for a few frames, take one frame of the world alone, then
    // restore. The probe frame's background is the flat clear colour, so
    // every pixel off it is geometry the world load drew.
    if frame == WORLD_PROBE_HIDE_FRAME {
        probe.hidden = true;
    }
    if frame == WORLD_PROBE_FRAME && player.is_some() && !capture.world_shot_requested {
        capture.world_shot_requested = true;
        let path = capture.dir.join(format!("frame_{frame:05}_world.png"));
        request_screenshot(&mut commands, path, "world");
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
    if requested >= 2 && (delivered >= requested || frame >= SHOT_FRAME + SHOT_GRACE_FRAMES) {
        exit.write(AppExit::Success);
    }
}

fn request_screenshot(commands: &mut Commands, path: PathBuf, label: &'static str) {
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, handle: Res<CaptureHandle>| {
            let shot = measure(&captured.image, path.clone(), label);
            let delivered = captured.image.data.is_some();
            save_to_disk(path.clone())(captured);
            if delivered {
                handle.lock().shots.push(shot);
            }
        },
    );
}

fn measure(image: &bevy::image::Image, path: PathBuf, label: &'static str) -> Shot {
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
    let (world_pixels, background) = if label == "world" {
        let data = image.data.as_deref().unwrap_or(&[]);
        let (pixels, background) = count_world_pixels(data, image.texture_descriptor.format);
        (Some(pixels), Some(background))
    } else {
        (None, None)
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
        world_pixels,
        total_pixels: count as usize,
        background,
    }
}

/// Counts the frame's world pixels: every pixel that differs from the frame's
/// **modal** colour (the background the world was drawn over) by more than
/// [`BACKGROUND_CHANNEL_TOLERANCE`] on any channel, and the modal colour
/// itself, in RGB order.
///
/// The modal colour is read off the frame rather than declared, so the count
/// cannot disagree with what the renderer actually cleared to: a frame that
/// drew no world geometry is one flat colour and counts zero. The window
/// screenshot's bytes are in the surface's own channel order (BGRA on the
/// pinned Metal backend), so `format` decides how the first three bytes are
/// read; the count itself is order-independent, only the recorded colour is
/// not.
fn count_world_pixels(
    data: &[u8],
    format: bevy::render::render_resource::TextureFormat,
) -> (usize, [u8; 3]) {
    let rgb = |pixel: &[u8; 4]| match format {
        bevy::render::render_resource::TextureFormat::Bgra8Unorm
        | bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb => {
            [pixel[2], pixel[1], pixel[0]]
        }
        _ => [pixel[0], pixel[1], pixel[2]],
    };
    let mut counts: std::collections::HashMap<[u8; 3], usize> = std::collections::HashMap::new();
    for pixel in data.as_chunks::<4>().0 {
        *counts.entry(rgb(pixel)).or_default() += 1;
    }
    let Some((&background, _)) = counts.iter().max_by_key(|(_, count)| **count) else {
        return (0, [0, 0, 0]);
    };
    let off_background = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(rgb)
        .filter(|pixel| {
            pixel[0].abs_diff(background[0]) > BACKGROUND_CHANNEL_TOLERANCE
                || pixel[1].abs_diff(background[1]) > BACKGROUND_CHANNEL_TOLERANCE
                || pixel[2].abs_diff(background[2]) > BACKGROUND_CHANNEL_TOLERANCE
        })
        .count();
    (off_background, background)
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
    // frame around the spawn, counted against the frame's own background
    // (M01-PLAY-WORLD-VISIBLE, #1433).
    if let Some(shot) = capture.shots.iter().find(|shot| shot.label == "world") {
        let background = shot.background.unwrap_or([0, 0, 0]);
        lines.push(format!(
            "{{\"kind\":\"world\",\"label\":\"world\",\"path\":{},\"width\":{},\"height\":{},\
             \"world_pixels\":{},\"total_pixels\":{},\"covered_permille\":{},\
             \"background\":[{},{},{}],\"min_covered_permille\":{MIN_WORLD_COVERAGE_PERMILLE}}}",
            json_string(&shot.path.display().to_string()),
            shot.width,
            shot.height,
            shot.world_pixels.unwrap_or(0),
            shot.total_pixels,
            shot.covered_permille(),
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
    // The world probe is the check this capture exists for: the frame around
    // the spawn must hold the world's own pixels, or the mission is drawing
    // only the player against an empty background.
    match capture.shots.iter().find(|shot| shot.label == "world") {
        Some(shot)
            if shot.width > 0
                && shot.height > 0
                && shot.covered_permille() >= MIN_WORLD_COVERAGE_PERMILLE =>
        {
            Ok(())
        }
        Some(shot) => Err(CaptureRefusal::Incomplete(format!(
            "the frame around the spawn drew no world: {} holds {} of {} pixels off the \
             background ({} per mille, less than the {} required), so the world's geometry is \
             not being drawn",
            shot.path.display(),
            shot.world_pixels.unwrap_or(0),
            shot.total_pixels,
            shot.covered_permille(),
            MIN_WORLD_COVERAGE_PERMILLE,
        ))),
        None => Err(CaptureRefusal::Incomplete(
            "the renderer delivered no world-probe framebuffer screenshot".to_owned(),
        )),
    }
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

fn json_string(text: &str) -> String {
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
