//! The finite deterministic smoke run (`--playtest --smoke-seconds <n>
//! --capture-dir <dir>`).
//!
//! A scripted key sequence is injected into Bevy's keyboard state, so it goes
//! through the same path as a human's keys: Bevy input → F22 device events →
//! session policy → [`super::command::flight_command`] → the production flight
//! model → Avian → the chase camera. Every rendered frame advances exactly
//! 1/[`SMOKE_FRAME_HZ`] s of simulated time (the app is built with a manual
//! time step), so the same script produces the same inputs on every machine.
//!
//! The run records a trace, optionally takes real framebuffer screenshots from
//! the window, evaluates its own pass/fail checks and exits. **A scripted run
//! is never `human_play`.** An empty or uniform framebuffer, an aircraft that
//! did not respond to the script, a missing collision or a missing reset is a
//! failure, not a pass.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use avian3d::prelude::Position;
use bevy::app::AppExit;
use bevy::input::ButtonInput;
use bevy::input::InputSystems;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::{
    App, Commands, Entity, IntoScheduleConfigs, Last, MessageWriter, On, Plugin, PreUpdate, Query,
    Res, ResMut, Resource, Update, With,
};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::WindowFocused;

use super::retail::{self, RetailContent};
use super::scene::PlaytestAircraft;
use super::{PlaytestCamera, PlaytestError, PlaytestState, SmokeRequest};
use crate::input::platform::PlatformInput;
use crate::physics::PhysicsTickLedger;

/// Rendered frames per simulated second in a smoke run.
pub const SMOKE_FRAME_HZ: f64 = 60.0;

/// The scripted sequence needs this many simulated seconds.
pub const MIN_SMOKE_SECONDS: u32 = 20;

/// Frames between two trace samples.
const SAMPLE_EVERY_FRAMES: u32 = 6;

/// Frames to wait for outstanding screenshots after the script ends.
const SCREENSHOT_GRACE_FRAMES: u32 = 600;

/// A screenshot whose frame holds fewer distinct luminance levels than this is
/// treated as an empty framebuffer.
const MIN_DISTINCT_LUMINANCE: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Press(KeyCode),
    Release(KeyCode),
    Focus(bool),
}

fn frame_of(seconds: f64) -> u32 {
    (seconds * SMOKE_FRAME_HZ).round() as u32
}

/// Simulated seconds one pass of [`script`] takes.
pub const CYCLE_SECONDS: u32 = 20;

/// How many whole passes of the script a run of `seconds` holds: a 2-minute run
/// is six passes, each with its pauses and its two resets.
#[must_use]
pub const fn cycles(seconds: u32) -> u32 {
    seconds / CYCLE_SECONDS
}

/// The scripted sequence as `(frame, step)`, sorted by frame.
///
/// `steer_into_area` holds yaw-right through the second half of every pass, which
/// is how the original-assets run reaches the area's own collider (the aircraft
/// spawns alongside it); the synthetic wall is dead ahead and needs no steering.
///
/// | seconds | action |
/// | --- | --- |
/// | 0 – 1 | neutral, level cruise |
/// | 1 – 3 | hold `S` (pitch up) |
/// | 4 – 5 | hold `E` (roll right) |
/// | 5 – 6 | hold `Q` (roll left) |
/// | 6 – 7 | hold `D` (yaw right) |
/// | 7.0 / 7.2 | tap `Left Shift` (throttle up), tap `F` (throttle down) |
/// | 8.0 / 9.0 | `Esc` pauses, `Esc` resumes |
/// | 9.5 / 10.0 | window focus lost (pauses), regained (resumes) |
/// | 10.5 | `R` reset |
/// | 10.5 – 19.5 | neutral: the aircraft flies into the wall |
/// | 19.5 | `R` reset again |
///
/// The pass repeats every [`CYCLE_SECONDS`] for as long as the run lasts.
fn script(cycles: u32, steer_into_area: bool) -> Vec<(u32, Step)> {
    let mut steps = Vec::new();
    for cycle in 0..cycles.max(1) {
        pass(&mut steps, f64::from(cycle * CYCLE_SECONDS), steer_into_area);
    }
    steps.sort_by_key(|(frame, _)| *frame);
    steps
}

fn pass(steps: &mut Vec<(u32, Step)>, at: f64, steer_into_area: bool) {
    let mut hold = |key, from: f64, to: f64| {
        steps.push((frame_of(at + from), Step::Press(key)));
        steps.push((frame_of(at + to), Step::Release(key)));
    };
    hold(KeyCode::KeyS, 1.0, 3.0);
    hold(KeyCode::KeyE, 4.0, 5.0);
    hold(KeyCode::KeyQ, 5.0, 6.0);
    hold(KeyCode::KeyD, 6.0, 7.0);
    hold(KeyCode::ShiftLeft, 7.0, 7.1);
    hold(KeyCode::KeyF, 7.2, 7.3);
    hold(KeyCode::Escape, 8.0, 8.1);
    hold(KeyCode::Escape, 9.0, 9.1);
    hold(KeyCode::KeyR, 10.5, 10.6);
    hold(KeyCode::KeyR, 19.5, 19.6);
    if steer_into_area {
        hold(KeyCode::KeyD, 10.7, 12.5);
        hold(KeyCode::KeyE, 12.5, 15.0);
    }
    steps.push((frame_of(at + 9.5), Step::Focus(false)));
    steps.push((frame_of(at + 10.0), Step::Focus(true)));
}

/// One trace record.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    /// Rendered frame index.
    pub frame: u32,
    /// Fixed physics ticks so far.
    pub tick: u64,
    /// Aircraft position, meters.
    pub position_m: [f32; 3],
    /// Speed, m/s.
    pub speed_m_s: f32,
    /// Nose pitch, degrees.
    pub pitch_deg: f32,
    /// Bank, degrees.
    pub roll_deg: f32,
    /// Heading, degrees.
    pub heading_deg: f32,
    /// The command held by the aircraft: pitch, roll, yaw, throttle.
    pub command: [f64; 4],
    /// Whether the session was paused.
    pub paused: bool,
    /// Resets so far.
    pub resets: u32,
    /// Obstacle contacts so far.
    pub obstacle_contacts: u64,
    /// Ground contacts so far.
    pub ground_contacts: u64,
}

/// What one captured screenshot measured.
#[derive(Clone, Debug, PartialEq)]
pub struct ShotRecord {
    /// Why the frame was taken.
    pub label: String,
    /// The PNG the renderer wrote.
    pub path: PathBuf,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Distinct luminance levels in the frame.
    pub distinct_luminance: usize,
    /// Mean luminance, 0–255.
    pub mean_luminance: f32,
}

/// Screenshots delivered by the renderer, shared with the observers.
#[derive(Resource, Clone, Default)]
pub struct SmokeShots(Arc<Mutex<Vec<ShotRecord>>>);

/// Everything the run collected.
#[derive(Resource, Debug, Default)]
struct SmokeData {
    samples: Vec<Sample>,
    shots_requested: usize,
    collision_shot_taken: bool,
    paused_seen: bool,
    frames: u32,
    focus_losses: u32,
    /// Frames in which the host window's real focus state disagreed with the
    /// script's and was overridden.
    real_focus_overrides: u32,
    io_error: Option<String>,
}

/// The finished run, for the caller of [`super::run_playtest`].
#[derive(Clone, Debug, PartialEq)]
pub struct SmokeReport {
    /// Rendered frames.
    pub frames: u32,
    /// Fixed physics ticks.
    pub ticks: u64,
    /// Times the commanded input changed.
    pub input_changes: u64,
    /// Resets performed.
    pub resets: u32,
    /// Obstacle contacts.
    pub obstacle_contacts: u64,
    /// Ground contacts.
    pub ground_contacts: u64,
    /// Ticks the fixed clock advanced while paused.
    pub ticks_while_paused: u64,
    /// Frames in which the host window's real focus state was overridden.
    pub real_focus_overrides: u32,
    /// Farthest the aircraft got from its spawn point, meters.
    pub max_distance_m: f32,
    /// The screenshots taken.
    pub shots: Vec<ShotRecord>,
    /// The label the run flew under.
    pub label: &'static str,
    /// The original-assets source manifest (a JSON object), when flown over
    /// original content.
    pub retail: Option<String>,
    /// Every failed check; empty means the run passed.
    pub failures: Vec<String>,
}

/// Shared slot the smoke system fills when the run ends.
#[derive(Clone, Default)]
pub struct SmokeHandle(Arc<Mutex<Option<Result<SmokeReport, String>>>>);

impl SmokeHandle {
    /// The outcome once the run has ended.
    pub fn take(&self) -> Option<Result<SmokeReport, String>> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// The smoke run as a plugin.
#[derive(Clone)]
pub struct SmokePlugin {
    request: SmokeRequest,
    capture: bool,
    handle: SmokeHandle,
}

impl SmokePlugin {
    /// A smoke run that takes real window screenshots into
    /// `request.capture_dir`.
    #[must_use]
    pub fn windowed(request: SmokeRequest) -> Self {
        Self {
            request,
            capture: true,
            handle: SmokeHandle::default(),
        }
    }

    /// A smoke run with no renderer: the same script, trace and checks, no
    /// screenshots. Used by the headless acceptance tests.
    #[must_use]
    pub fn headless(request: SmokeRequest) -> Self {
        Self {
            request,
            capture: false,
            handle: SmokeHandle::default(),
        }
    }

    /// The slot the finished report lands in.
    #[must_use]
    pub fn handle(&self) -> SmokeHandle {
        self.handle.clone()
    }
}

#[derive(Resource, Clone)]
struct SmokeConfig {
    request: SmokeRequest,
    capture: bool,
    handle: SmokeHandle,
    script: Vec<(u32, Step)>,
    shots: Vec<(u32, &'static str)>,
}

impl Plugin for SmokePlugin {
    fn build(&self, app: &mut App) {
        let retail_run = app.world().contains_resource::<RetailContent>();
        let shots = [
            (0.5, "initial"),
            (2.5, "pitch_up"),
            (5.5, "roll"),
            (12.0, "approach"),
        ]
        .into_iter()
        .map(|(seconds, label)| (frame_of(seconds), label))
        .collect();
        app.insert_resource(SmokeConfig {
            request: self.request.clone(),
            capture: self.capture,
            handle: self.handle.clone(),
            script: script(cycles(self.request.seconds), retail_run),
            shots,
        })
        .init_resource::<SmokeShots>()
        .init_resource::<SmokeData>()
        .init_resource::<SmokeScriptState>()
        // After Bevy's own keyboard system and before the F22 pump (which is
        // `.after(InputSystems)`): the script's keys are the keyboard state the
        // pump reads this frame.
        .add_systems(
            PreUpdate,
            drive_script
                .in_set(InputSystems)
                .after(bevy::input::keyboard::keyboard_input_system),
        )
        .add_systems(Update, override_real_focus.before(super::sync_pause))
        .add_systems(Last, smoke_step);
    }
}

#[derive(Resource, Default)]
struct SmokeScriptState {
    frame: u32,
    next: usize,
    /// Whether the script's own focus loss is in effect.
    script_unfocused: bool,
}

/// The smoke ignores the host window's real focus state: a window the
/// operating system left unfocused (or that the operator clicked away from)
/// would otherwise pause the scripted run through the real focus policy. The
/// only focus loss in a smoke run is the scripted one; every disagreement is
/// counted in the report.
fn override_real_focus(
    script: Res<SmokeScriptState>,
    mut platform: ResMut<PlatformInput>,
    mut data: ResMut<SmokeData>,
) {
    if !script.script_unfocused && !platform.session().is_focused() {
        platform.session_mut().set_focus(true);
        data.real_focus_overrides += 1;
    }
}

fn drive_script(
    config: Res<SmokeConfig>,
    mut script: ResMut<SmokeScriptState>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut data: ResMut<SmokeData>,
    mut focus: MessageWriter<WindowFocused>,
) {
    while let Some((frame, step)) = config.script.get(script.next).copied() {
        if frame > script.frame {
            break;
        }
        match step {
            Step::Press(key) => keys.press(key),
            Step::Release(key) => keys.release(key),
            Step::Focus(focused) => {
                script.script_unfocused = !focused;
                if !focused {
                    data.focus_losses += 1;
                }
                focus.write(WindowFocused {
                    window: Entity::PLACEHOLDER,
                    focused,
                });
            }
        }
        script.next += 1;
    }
    script.frame += 1;
}

#[allow(clippy::too_many_arguments)]
fn smoke_step(
    config: Res<SmokeConfig>,
    script: Res<SmokeScriptState>,
    state: Res<PlaytestState>,
    ledger: Res<PhysicsTickLedger>,
    shots: Res<SmokeShots>,
    mut data: ResMut<SmokeData>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    aircraft: Query<&Position, With<PlaytestAircraft>>,
    camera: Res<PlaytestCamera>,
    retail: Option<Res<RetailContent>>,
    area: Query<(), (With<retail::PlaytestAreaBody>, With<avian3d::prelude::Collider>)>,
) {
    let frame = script.frame;
    data.frames = frame;
    if frame == 1
        && config.capture
        && let Err(error) = std::fs::create_dir_all(&config.request.capture_dir)
    {
        data.io_error = Some(format!(
            "cannot create {}: {error}",
            config.request.capture_dir.display()
        ));
    }
    if state.paused {
        data.paused_seen = true;
    }
    if frame.is_multiple_of(SAMPLE_EVERY_FRAMES) && aircraft.single().is_ok() {
        let t = state.telemetry;
        data.samples.push(Sample {
            frame,
            tick: ledger.ticks,
            position_m: t.position_m,
            speed_m_s: t.speed_m_s,
            pitch_deg: t.pitch_deg,
            roll_deg: t.roll_deg,
            heading_deg: t.heading_deg,
            command: [
                state.command.pitch,
                state.command.roll,
                state.command.yaw,
                state.command.throttle,
            ],
            paused: state.paused,
            resets: state.resets,
            obstacle_contacts: state.obstacle_contacts,
            ground_contacts: state.ground_contacts,
        });
    }

    let end_frame = config.request.seconds * SMOKE_FRAME_HZ as u32;
    if config.capture {
        let mut wanted: Vec<(u32, String)> = config
            .shots
            .iter()
            .map(|(at, label)| (*at, (*label).to_owned()))
            .collect();
        wanted.push((end_frame.saturating_sub(12), "final".to_owned()));
        let mut due: Vec<String> = wanted
            .iter()
            .filter(|(at, _)| *at == frame)
            .map(|(_, label)| label.clone())
            .collect();
        if state.first_obstacle_contact_tick.is_some() && !data.collision_shot_taken {
            data.collision_shot_taken = true;
            due.push("collision".to_owned());
        }
        for label in due {
            data.shots_requested += 1;
            take_screenshot(&mut commands, &config.request.capture_dir, frame, &label);
        }
    }

    let delivered = shots.0.lock().map_or(0, |shots| shots.len());
    let finished_capturing = !config.capture || delivered >= data.shots_requested;
    if frame >= end_frame && (finished_capturing || frame >= end_frame + SCREENSHOT_GRACE_FRAMES) {
        let facts = Facts {
            camera_frame: camera.frame.is_some(),
            retail: retail.as_ref().map(|content| content.manifest_json()),
            area_records: retail.as_ref().map(|content| content.area.mesh_records),
            area_colliders: area.iter().count(),
        };
        let report = evaluate(&config, &state, &ledger, &data, &shots, &facts);
        let outcome = write_artifacts(&config.request.capture_dir, &data, &report)
            .map(|()| report.clone())
            .map_err(|(path, error)| format!("cannot write {}: {error}", path.display()));
        if let Ok(mut slot) = config.handle.0.lock() {
            *slot = Some(outcome);
        }
        exit.write(AppExit::Success);
    }
}

fn take_screenshot(commands: &mut Commands, dir: &Path, frame: u32, label: &str) {
    let path = dir.join(format!("frame_{frame:05}_{label}.png"));
    let label = label.to_owned();
    let saved = path.clone();
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, shots: Res<SmokeShots>| {
            let record = measure(&captured.image, label.clone(), saved.clone());
            let delivered = captured.image.data.is_some();
            save_to_disk(saved.clone())(captured);
            if delivered && let Ok(mut shots) = shots.0.lock() {
                shots.push(record);
            }
        },
    );
}

fn measure(image: &bevy::image::Image, label: String, path: PathBuf) -> ShotRecord {
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
    ShotRecord {
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
    }
}

fn sample_at(samples: &[Sample], seconds: f64) -> Option<&Sample> {
    let frame = frame_of(seconds);
    samples
        .iter()
        .min_by_key(|sample| sample.frame.abs_diff(frame))
}

/// What the run read off the world at its end, besides the trace.
struct Facts {
    camera_frame: bool,
    /// The source manifest, when flown over original content.
    retail: Option<String>,
    /// How many mesh records the original area spawned.
    area_records: Option<usize>,
    /// How many area entities carry a derived collider.
    area_colliders: usize,
}

fn evaluate(
    config: &SmokeConfig,
    state: &PlaytestState,
    ledger: &PhysicsTickLedger,
    data: &SmokeData,
    shots: &SmokeShots,
    facts: &Facts,
) -> SmokeReport {
    let shots = shots
        .0
        .lock()
        .map(|shots| shots.clone())
        .unwrap_or_default();
    let mut failures = Vec::new();
    let max_distance_m = data
        .samples
        .iter()
        .map(|sample| {
            (0..3)
                .map(|axis| (sample.position_m[axis] - state.spawn_m[axis]).powi(2))
                .sum::<f32>()
                .sqrt()
        })
        .fold(0.0, f32::max);

    if let Some(error) = &data.io_error {
        failures.push(error.clone());
    }
    if config.capture {
        if shots.len() < data.shots_requested || shots.is_empty() {
            failures.push(format!(
                "only {} of {} requested framebuffer screenshots arrived",
                shots.len(),
                data.shots_requested
            ));
        }
        for shot in &shots {
            if shot.distinct_luminance < MIN_DISTINCT_LUMINANCE || shot.mean_luminance < 4.0 {
                failures.push(format!(
                    "framebuffer {} is empty or uniform ({} luminance levels, mean {:.1})",
                    shot.path.display(),
                    shot.distinct_luminance,
                    shot.mean_luminance
                ));
            }
        }
    }
    if data.samples.is_empty() {
        failures.push("the run never saw a player aircraft".to_owned());
    }
    if let Some(sample) = data.samples.iter().find(|sample| {
        sample
            .position_m
            .iter()
            .chain([&sample.speed_m_s, &sample.pitch_deg, &sample.roll_deg, &sample.heading_deg])
            .any(|value| !value.is_finite())
    }) {
        failures.push(format!(
            "the aircraft pose was not finite at frame {}",
            sample.frame
        ));
    }
    if !facts.camera_frame {
        failures.push("the chase camera never resolved a frame".to_owned());
    }
    if let Some(records) = facts.area_records
        && (records == 0 || facts.area_colliders == 0)
    {
        failures.push(format!(
            "the original area has {records} records and {} derived colliders",
            facts.area_colliders
        ));
    }
    if max_distance_m < 200.0 {
        failures.push(format!(
            "the aircraft never left its spawn point (max distance {max_distance_m:.1} m)"
        ));
    }
    if state.input_changes < 8 {
        failures.push(format!(
            "the scripted keys changed the flight command only {} times",
            state.input_changes
        ));
    }
    let expected_resets = 2 * cycles(config.request.seconds);
    if state.resets != expected_resets {
        failures.push(format!(
            "expected {expected_resets} resets, saw {}",
            state.resets
        ));
    }
    if state.obstacle_contacts == 0 {
        failures.push("the aircraft never collided with the obstacle".to_owned());
    }
    if !data.paused_seen {
        failures.push("the focus loss never paused the session".to_owned());
    }
    if state.ticks_while_paused != 0 {
        failures.push(format!(
            "the fixed clock advanced {} ticks while paused",
            state.ticks_while_paused
        ));
    }
    let pick = |seconds: f64| sample_at(&data.samples, seconds);
    if let (Some(before), Some(after)) = (pick(1.0), pick(3.0))
        && after.pitch_deg - before.pitch_deg < 3.0
    {
        failures.push(format!(
            "holding pitch up changed the nose by only {:.2} deg",
            after.pitch_deg - before.pitch_deg
        ));
    }
    if let (Some(right), Some(left)) = (pick(5.0), pick(6.0)) {
        if right.roll_deg < 3.0 {
            failures.push(format!(
                "holding roll right banked only {:.2} deg",
                right.roll_deg
            ));
        }
        if right.roll_deg - left.roll_deg < 3.0 {
            failures.push(format!(
                "holding roll left removed only {:.2} deg of bank",
                right.roll_deg - left.roll_deg
            ));
        }
    }
    if let (Some(before), Some(after)) = (pick(6.0), pick(7.0))
        && after.heading_deg - before.heading_deg < 0.5
    {
        failures.push(format!(
            "holding yaw right turned the heading by only {:.2} deg",
            after.heading_deg - before.heading_deg
        ));
    }

    SmokeReport {
        frames: data.frames,
        ticks: ledger.ticks,
        input_changes: state.input_changes,
        resets: state.resets,
        obstacle_contacts: state.obstacle_contacts,
        ground_contacts: state.ground_contacts,
        ticks_while_paused: state.ticks_while_paused,
        real_focus_overrides: data.real_focus_overrides,
        max_distance_m,
        shots,
        label: state.label,
        retail: facts.retail.clone(),
        failures,
    }
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

/// Writes `trace.jsonl` and `report.json` into `dir`.
fn write_artifacts(
    dir: &Path,
    data: &SmokeData,
    report: &SmokeReport,
) -> Result<(), (PathBuf, std::io::Error)> {
    std::fs::create_dir_all(dir).map_err(|error| (dir.to_path_buf(), error))?;
    let mut trace = format!(
        "{{\"kind\":\"playtest-smoke-trace\",\"provenance\":{},\"frame_hz\":{}}}\n",
        json_string(report.label),
        SMOKE_FRAME_HZ
    );
    for sample in &data.samples {
        let _ = writeln!(
            trace,
            "{{\"frame\":{},\"tick\":{},\"position_m\":[{},{},{}],\"speed_m_s\":{},\"pitch_deg\":{},\"roll_deg\":{},\"heading_deg\":{},\"command\":[{},{},{},{}],\"paused\":{},\"resets\":{},\"obstacle_contacts\":{},\"ground_contacts\":{}}}",
            sample.frame,
            sample.tick,
            json_f32(sample.position_m[0]),
            json_f32(sample.position_m[1]),
            json_f32(sample.position_m[2]),
            json_f32(sample.speed_m_s),
            json_f32(sample.pitch_deg),
            json_f32(sample.roll_deg),
            json_f32(sample.heading_deg),
            sample.command[0],
            sample.command[1],
            sample.command[2],
            sample.command[3],
            sample.paused,
            sample.resets,
            sample.obstacle_contacts,
            sample.ground_contacts,
        );
    }
    let trace_path = dir.join("trace.jsonl");
    std::fs::write(&trace_path, trace).map_err(|error| (trace_path, error))?;

    let shots = report
        .shots
        .iter()
        .map(|shot| {
            format!(
                "{{\"label\":{},\"path\":{},\"width\":{},\"height\":{},\"distinct_luminance\":{},\"mean_luminance\":{}}}",
                json_string(&shot.label),
                json_string(&shot.path.display().to_string()),
                shot.width,
                shot.height,
                shot.distinct_luminance,
                json_f32(shot.mean_luminance)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let failures = report
        .failures
        .iter()
        .map(|failure| json_string(failure))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        "{{\"kind\":\"playtest-smoke-report\",\"provenance\":{},\"human_play\":false,\"frames\":{},\"ticks\":{},\"input_changes\":{},\"resets\":{},\"obstacle_contacts\":{},\"ground_contacts\":{},\"ticks_while_paused\":{},\"real_focus_overrides\":{},\"max_distance_m\":{},\"retail\":{},\"shots\":[{}],\"failures\":[{}],\"passed\":{}}}\n",
        json_string(report.label),
        report.frames,
        report.ticks,
        report.input_changes,
        report.resets,
        report.obstacle_contacts,
        report.ground_contacts,
        report.ticks_while_paused,
        report.real_focus_overrides,
        json_f32(report.max_distance_m),
        report.retail.as_deref().unwrap_or("null"),
        shots,
        failures,
        report.failures.is_empty(),
    );
    let report_path = dir.join("report.json");
    std::fs::write(&report_path, body).map_err(|error| (report_path, error))
}

/// Turns the finished run into the process result and prints the summary.
///
/// # Errors
///
/// [`PlaytestError::SmokeFailed`] when any check failed,
/// [`PlaytestError::Exit`] when the run produced no report, and
/// [`PlaytestError::Io`] when the artifacts could not be written.
pub fn finish(handle: &SmokeHandle, request: &SmokeRequest) -> Result<SmokeReport, PlaytestError> {
    let outcome = handle
        .take()
        .ok_or_else(|| PlaytestError::Exit("the smoke run ended before it finished".to_owned()))?;
    let report = outcome.map_err(PlaytestError::Exit)?;
    println!(
        "playtest smoke: {}\n  frames {} ticks {} input changes {} resets {} obstacle contacts {} ground contacts {}\n  max distance from spawn {:.0} m, framebuffers {} ({}), artifacts in {}",
        report.label,
        report.frames,
        report.ticks,
        report.input_changes,
        report.resets,
        report.obstacle_contacts,
        report.ground_contacts,
        report.max_distance_m,
        report.shots.len(),
        if report.shots.is_empty() {
            "none"
        } else {
            "real window frames"
        },
        request.capture_dir.display(),
    );
    if report.failures.is_empty() {
        Ok(report)
    } else {
        Err(PlaytestError::SmokeFailed(report.failures))
    }
}
