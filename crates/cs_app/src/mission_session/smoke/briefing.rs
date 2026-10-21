//! The briefing stage of the owner-command smoke (task #1750
//! `PARITY5-FRONTEND-BRIEFING-MAP`).
//!
//! The run opens on the briefing map screen (the Flight Check follows it).
//! While the briefing is the stage this system:
//!
//! 1. puts the briefing's clock on a fixed 1/60 s step that does not follow
//!    the voice (so the shots land on the script's own times), keeping the
//!    voice itself playing;
//! 2. seeks the clock to a plan of times — before the first marker, 3 s after
//!    each marker, and just past the script's end — and at each takes a real
//!    framebuffer, recording the element ids the timeline says are drawn and
//!    the ids the screen's entities show;
//! 3. judges the last frame's three buttons, the voice-over's start and what
//!    the output stream pulled, then presses **Replay Briefing** and reads the
//!    clock and the voice after it;
//! 4. presses **Go to Flight Check**; the Flight Check stage later presses
//!    **Return to Briefing** once ([`round_trip_from_flight_check`]) and this
//!    system presses Go to Flight Check again.
//!
//! The floors are designed and named next to their constants. A run is never
//! `human_play`, and nothing it measures is `verified_original`; the frames
//! are for comparing with the owner's capture by eye.

use bevy::app::AppExit;
use bevy::prelude::{Commands, MessageWriter, Res, ResMut};
use cs_content::briefing::DialogButtonKind;
use cs_content::replay::CapabilityClass;

use super::super::briefing_screen::{BriefingDriver, BriefingProbe, BriefingVoice, expected_ids};
use super::super::flight_check::PreMissionStage;
use super::{
    Check, FlightCheckFrames, FlightCheckPhase, PhaseResponse, RawFrame, SHOT_GRACE_FRAMES,
    SmokeConfig, SmokeData, SmokeMeasurements, SmokeShots, clear_colour_encodings, end_run,
    json_string, non_black_permille, region_difference, request_screenshot,
};
use crate::audio::CapabilityDeclaration;

/// The least share of a briefing frame that must be off black, in parts per
/// thousand. **Designed:** the 800x600 map is aspect-fitted into the window,
/// so a 16:9 window leaves a black pillarbox and a quarter of the frame is
/// outside the map.
pub const MIN_BRIEFING_DRAWN_PERMILLE: u32 = 300;
/// The least pixels two consecutive briefing frames must differ in. **Designed:**
/// each step of the plan is a new picture, pin or line, or an icon moving, each
/// covering far more.
pub const MIN_BRIEFING_CHANGE_PIXELS: usize = 200;
/// The least share of a button's region that must be warm (brass) pixels, in
/// parts per thousand. **Designed:** the map is blue and green; the button art
/// is brass, so red above blue marks it.
pub const MIN_BUTTON_WARM_PERMILLE: u32 = 250;
/// Frames between two steps of the plan: the clock is seeked, the screen
/// settles, the framebuffer is requested, and the shot has time to be taken
/// before the next seek.
const SHOT_SPACING: u32 = 8;
/// The frame within a step the framebuffer is requested.
const SHOT_REQUEST_AT: u32 = 4;
/// Seconds after a marker the plan photographs (every `Fade`, `Spin` and `Move`
/// of the retail script is at most 4.5 s, and the first three seconds hold
/// the arrivals).
const AFTER_MARKER_SECONDS: f64 = 3.0;
/// Frames to wait after a button press before reading what it did.
const PRESS_SETTLE_FRAMES: u32 = 8;

/// One framebuffer of the briefing and the script's state at it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BriefingShotMetrics {
    /// The shot's label.
    pub label: String,
    /// The briefing's clock when the framebuffer was requested, in seconds.
    pub time_s: f64,
    /// The ids the timeline says are drawn then.
    pub expected: Vec<String>,
    /// The ids the screen's entities showed.
    pub drawn: Vec<String>,
    /// Whether the framebuffer arrived.
    pub delivered: bool,
    /// The share of the frame off black, in parts per thousand.
    pub drawn_permille: u32,
    /// The pixels that differ from the previous shot's frame (`None` for the
    /// first).
    pub changed_pixels: Option<usize>,
}

/// One of the three buttons in the last frame.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BriefingButtonMetrics {
    /// The button's key (`REPLAY`, `RETURNTOCABIN`, `FLIGHTCHECK`).
    pub key: String,
    /// Whether the node was visible.
    pub drawn: bool,
    /// The region in framebuffer pixels.
    pub region: [u32; 4],
    /// The warm pixels in the region, in parts per thousand.
    pub warm_permille: u32,
}

/// The voice-over as the run saw it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BriefingVoiceMetrics {
    /// The member played.
    pub member: String,
    /// How often a voice was started.
    pub starts: u32,
    /// Whether the machine declares the `audio` capability.
    pub audio_declared: bool,
    /// The named refusal, when the device would not open or start.
    pub refusal: Option<String>,
    /// The sample frames the output stream pulled.
    pub pulled_frames: u64,
}

/// What Replay Briefing did.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BriefingReplayMetrics {
    /// The phase's answer.
    pub response: String,
    /// The clock before the press, in seconds.
    pub clock_before_s: f64,
    /// The clock a few frames after, in seconds.
    pub clock_after_s: f64,
    /// The restarts the clock counted.
    pub restarts: u32,
    /// How often a voice had been started before the press.
    pub starts_before: u32,
    /// How often after.
    pub starts_after: u32,
    /// The stage after (must still be the briefing).
    pub stage_after: String,
}

/// What the briefing stage measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BriefingMetrics {
    /// The script's key.
    pub script: String,
    /// The frame's pixel size.
    pub frame_size: (u32, u32),
    /// The marker times of the voice-over, in seconds.
    pub markers_s: Vec<f64>,
    /// The script's end, in seconds.
    pub end_s: f64,
    /// The shots, in plan order.
    pub shots: Vec<BriefingShotMetrics>,
    /// The three buttons.
    pub buttons: Vec<BriefingButtonMetrics>,
    /// The voice-over.
    pub voice: BriefingVoiceMetrics,
    /// Replay Briefing.
    pub replay: Option<BriefingReplayMetrics>,
    /// The phase's answer to Return to Briefing from the Flight Check.
    pub return_response: Option<String>,
    /// The stage after Return to Briefing.
    pub return_stage: Option<String>,
    /// The clock restarts after the round trip.
    pub return_restarts: Option<u32>,
    /// The phase's answer to Go to Flight Check.
    pub forward_response: Option<String>,
    /// The stage after Go to Flight Check.
    pub forward_stage: Option<String>,
}

/// The briefing stage's progress.
#[derive(Clone, Debug, Default)]
pub(in super::super) struct BriefingRun {
    /// Frames since the briefing became the stage this visit.
    frame: u32,
    /// Which visit: 1 the first, 2 after Return to Briefing.
    visit: u32,
    step: Step,
    plan: Vec<f64>,
    index: usize,
    wait: u32,
    clock_before: f64,
    starts_before: u32,
    /// Return to Briefing was delivered from the Flight Check.
    returned: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Start,
    Shots,
    Collect,
    ReplayWait,
    ForwardFirst,
    BackWait,
    Done,
}

fn stage_name(stage: PreMissionStage) -> String {
    format!("{stage:?}")
}

fn response_name(response: &PhaseResponse) -> String {
    format!("{response:?}")
}

/// The warm (brass) pixels of `region` in an RGBA frame, in parts per
/// thousand of the region's pixels inside the frame.
fn warm_permille(frame: &RawFrame, region: [u32; 4]) -> u32 {
    let [x, y, w, h] = region;
    let x_end = x.saturating_add(w).min(frame.width);
    let y_end = y.saturating_add(h).min(frame.height);
    let (mut warm, mut total) = (0_u64, 0_u64);
    for row in y.min(frame.height)..y_end {
        for column in x.min(frame.width)..x_end {
            let at = (row as usize * frame.width as usize + column as usize) * 4;
            let Some(pixel) = frame.data.get(at..at + 3) else {
                continue;
            };
            total += 1;
            // The frame may be RGBA or BGRA: brass is whichever of the outer
            // two channels is the strong one, against the map's blue-green.
            let (outer, other) = (u32::from(pixel[0]), u32::from(pixel[2]));
            let (high, low) = (outer.max(other), outer.min(other));
            if high > 100 && high > low + 30 {
                warm += 1;
            }
        }
    }
    (warm * 1000).checked_div(total).unwrap_or(0) as u32
}

/// The plan of clock times to photograph: before the first marker, after each
/// marker and just past the script's end.
fn plan_times(markers: &[f64], end: f64) -> Vec<f64> {
    let mut times = vec![2.0];
    times.extend(markers.iter().map(|marker| marker + AFTER_MARKER_SECONDS));
    times.push(end + 0.5);
    times
}

/// The briefing stage's system.
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn smoke_briefing(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    phase: Option<ResMut<FlightCheckPhase>>,
    mut driver: Option<ResMut<BriefingDriver>>,
    voice: Option<Res<BriefingVoice>>,
    probe: BriefingProbe,
    shots: Res<SmokeShots>,
    frames: Res<FlightCheckFrames>,
    mut commands: Commands,
    clear: Option<Res<bevy::prelude::ClearColor>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut phase) = phase else { return };
    if phase.briefing().is_none() || phase.stage() != PreMissionStage::Briefing {
        return;
    }
    let Some(voice) = voice else { return };
    let clear_colours = clear.as_deref().map(clear_colour_encodings);
    let run = &mut data.briefing;
    run.frame += 1;
    if run.visit == 0 {
        run.visit = 1;
    }
    // The second visit (after Return to Briefing) only waits for the screen to
    // settle and goes on to the Flight Check again.
    if run.visit == 2 {
        if run.frame == PRESS_SETTLE_FRAMES {
            let response = phase.press_briefing_button(DialogButtonKind::FlightCheck);
            let stage = phase.stage();
            if let Some(metrics) = data.measurements.briefing.as_mut() {
                metrics.forward_response = Some(response_name(&response));
                metrics.forward_stage = Some(stage_name(stage));
            }
            data.briefing.step = Step::Done;
        }
        return;
    }
    let capture_dir = config.request.capture_dir.clone();
    match run.step {
        Step::Start => {
            if let Some(driver) = driver.as_mut() {
                // Seeks must hold: the voice keeps playing but does not set the
                // clock, and every frame is a sixtieth of a second.
                driver.follow_voice = false;
                driver.fixed_step = Some(1.0 / super::MISSION_SMOKE_FRAME_HZ);
            }
            let _ = std::fs::create_dir_all(&capture_dir);
            let Some(stage) = phase.briefing() else {
                return;
            };
            let scene = stage.scene();
            let end = scene.timeline.end();
            run.plan = plan_times(&scene.markers, end);
            run.index = 0;
            run.wait = 0;
            run.step = Step::Shots;
            data.measurements.briefing = Some(BriefingMetrics {
                script: scene.script_key.clone(),
                markers_s: scene.markers.clone(),
                end_s: end,
                ..BriefingMetrics::default()
            });
        }
        Step::Shots => {
            let Some(&time) = run.plan.get(run.index) else {
                run.step = Step::Collect;
                run.wait = 0;
                return;
            };
            let at = run.wait;
            run.wait += 1;
            if at == 0 {
                if let Some(briefing) = phase.briefing_mut() {
                    briefing.playback_mut().sync_to_voice(time);
                }
            } else if at == SHOT_REQUEST_AT {
                let (clock, expected) = {
                    let stage = phase.briefing().expect("checked above");
                    let clock = stage.playback().time();
                    (clock, expected_ids(stage.scene(), clock))
                };
                let label = format!("briefing_{:02}_{:03}s", run.index, clock as u32);
                let path = capture_dir.join(format!("frame_br{:02}_{label}.png", run.index));
                data.shots_requested += 1;
                data.requested.push(label.clone());
                request_screenshot(
                    &mut commands,
                    shots.clone(),
                    path,
                    label.clone(),
                    clear_colours,
                    None,
                    Some(frames.clone()),
                );
                if let Some(metrics) = data.measurements.briefing.as_mut() {
                    metrics.shots.push(BriefingShotMetrics {
                        label,
                        time_s: clock,
                        expected,
                        drawn: probe.drawn_ids(),
                        ..BriefingShotMetrics::default()
                    });
                }
            } else if at >= SHOT_SPACING {
                data.briefing.index += 1;
                data.briefing.wait = 0;
            }
        }
        Step::Collect => {
            run.wait += 1;
            let wanted = run.plan.len();
            let raw = frames
                .0
                .lock()
                .map(|frames| frames.clone())
                .unwrap_or_default();
            let arrived = raw
                .iter()
                .filter(|frame| frame.label.starts_with("briefing_"))
                .count();
            if arrived < wanted && run.wait < SHOT_GRACE_FRAMES {
                return;
            }
            let buttons = probe.buttons();
            let mut previous: Option<&RawFrame> = None;
            let mut judged = Vec::new();
            if let Some(metrics) = data.measurements.briefing.as_mut() {
                for shot in &mut metrics.shots {
                    let frame = raw.iter().find(|frame| frame.label == shot.label);
                    shot.delivered = frame.is_some();
                    if let Some(frame) = frame {
                        shot.drawn_permille =
                            non_black_permille(&frame.data, frame.width, frame.height);
                        metrics.frame_size = (frame.width, frame.height);
                        shot.changed_pixels = previous.map(|before| {
                            region_difference(
                                &before.data,
                                &frame.data,
                                frame.width,
                                frame.height,
                                [0, 0, frame.width, frame.height],
                            )
                        });
                        previous = Some(frame);
                    }
                    judged.push(shot.label.clone());
                }
                let last = previous;
                metrics.buttons = buttons
                    .into_iter()
                    .map(|(key, drawn, region)| BriefingButtonMetrics {
                        warm_permille: last.map_or(0, |frame| warm_permille(frame, region)),
                        key,
                        drawn,
                        region,
                    })
                    .collect();
                metrics.voice = BriefingVoiceMetrics {
                    member: voice.asset().member.clone(),
                    starts: voice.starts(),
                    audio_declared: CapabilityDeclaration::from_environment()
                        .contains(CapabilityClass::Audio),
                    refusal: voice
                        .refusal()
                        .map(|error| format!("{}: {}", error.code, error.detail)),
                    pulled_frames: voice.probe().pulled(),
                };
            }
            // Replay Briefing.
            let clock_before = phase
                .briefing()
                .map_or(0.0, |stage| stage.playback().time());
            data.briefing.clock_before = clock_before;
            data.briefing.starts_before = voice.starts();
            let response = phase.press_briefing_button(DialogButtonKind::Replay);
            if let Some(metrics) = data.measurements.briefing.as_mut() {
                metrics.replay = Some(BriefingReplayMetrics {
                    response: response_name(&response),
                    clock_before_s: clock_before,
                    starts_before: voice.starts(),
                    ..BriefingReplayMetrics::default()
                });
            }
            data.briefing.step = Step::ReplayWait;
            data.briefing.wait = 0;
        }
        Step::ReplayWait => {
            run.wait += 1;
            if run.wait < PRESS_SETTLE_FRAMES {
                return;
            }
            let (clock, restarts) = phase.briefing().map_or((0.0, 0), |stage| {
                (stage.playback().time(), stage.playback().restarts())
            });
            let stage = stage_name(phase.stage());
            let starts_after = voice.starts();
            if let Some(replay) = data
                .measurements
                .briefing
                .as_mut()
                .and_then(|metrics| metrics.replay.as_mut())
            {
                replay.clock_after_s = clock;
                replay.restarts = restarts;
                replay.starts_after = starts_after;
                replay.stage_after = stage;
            }
            data.briefing.step = Step::ForwardFirst;
        }
        Step::ForwardFirst => {
            let response = phase.press_briefing_button(DialogButtonKind::FlightCheck);
            let stage = phase.stage();
            if let Some(metrics) = data.measurements.briefing.as_mut() {
                metrics.forward_response = Some(response_name(&response));
                metrics.forward_stage = Some(stage_name(stage));
            }
            data.briefing.step = Step::BackWait;
            if stage != PreMissionStage::FlightCheck {
                // The Flight Check never opened: the mission stays held, so
                // the run ends here with the failing check.
                end_run(&config, &mut data, &shots, &mut exit, 0);
            }
        }
        Step::BackWait | Step::Done => {}
    }
}

/// The Flight Check's first frames press Return to Briefing once, so the run
/// proves the way back. Returns `true` when it consumed the frame.
pub(in super::super) fn round_trip_from_flight_check(
    phase: &mut FlightCheckPhase,
    data: &mut SmokeData,
) -> bool {
    if phase.briefing().is_none() || data.briefing.returned || data.flight_check_frame < 3 {
        return false;
    }
    data.briefing.returned = true;
    let response = phase.press_hotspot("FC_B_RETURNBRIEF");
    let stage = phase.stage();
    let restarts = phase
        .briefing()
        .map_or(0, |stage| stage.playback().restarts());
    if let Some(metrics) = data.measurements.briefing.as_mut() {
        metrics.return_response = Some(response_name(&response));
        metrics.return_stage = Some(stage_name(stage));
        metrics.return_restarts = Some(restarts);
    }
    data.flight_check_frame = 0;
    data.briefing.visit = 2;
    data.briefing.frame = 0;
    true
}

// ---------------------------------------------------------------- checks --

/// The briefing checks, judged before the Flight Check's. Empty when the run
/// had no briefing (a composition that opens on the Flight Check alone).
#[must_use]
pub(super) fn evaluate_briefing(m: &SmokeMeasurements) -> Vec<Check> {
    let Some(b) = &m.briefing else {
        return Vec::new();
    };
    let check = |id: &str, passed: bool, detail: String| Check {
        id: id.to_owned(),
        applies: true,
        passed,
        detail,
    };
    let mut checks = Vec::new();

    let undelivered: Vec<&str> = b
        .shots
        .iter()
        .filter(|shot| !shot.delivered || shot.drawn_permille < MIN_BRIEFING_DRAWN_PERMILLE)
        .map(|shot| shot.label.as_str())
        .collect();
    checks.push(check(
        "briefing_screen_drawn",
        !b.shots.is_empty() && undelivered.is_empty(),
        if b.shots.is_empty() {
            "the run took no briefing frame".to_owned()
        } else if undelivered.is_empty() {
            format!(
                "all {} briefing frames ({}x{}) are at least {MIN_BRIEFING_DRAWN_PERMILLE} per \
                 mille off black",
                b.shots.len(),
                b.frame_size.0,
                b.frame_size.1
            )
        } else {
            format!("frames missing or under the floor: {undelivered:?}")
        },
    ));

    let mismatched: Vec<String> = b
        .shots
        .iter()
        .filter(|shot| shot.expected != shot.drawn)
        .map(|shot| {
            format!(
                "{} at {:.1} s: script {:?}, screen {:?}",
                shot.label, shot.time_s, shot.expected, shot.drawn
            )
        })
        .collect();
    checks.push(check(
        "briefing_elements_match_script",
        !b.shots.is_empty() && mismatched.is_empty(),
        if mismatched.is_empty() {
            format!(
                "at each of {} clock times the screen shows exactly the elements the compiled \
                 script draws",
                b.shots.len()
            )
        } else {
            format!("the screen does not show the script's elements: {mismatched:?}")
        },
    ));

    let flat: Vec<String> = b
        .shots
        .iter()
        .filter_map(|shot| {
            shot.changed_pixels
                .filter(|changed| *changed < MIN_BRIEFING_CHANGE_PIXELS)
                .map(|changed| format!("{} ({changed} px)", shot.label))
        })
        .collect();
    checks.push(check(
        "briefing_animates",
        b.shots.len() > 1 && flat.is_empty(),
        if flat.is_empty() {
            format!(
                "every one of the {} steps changes at least {MIN_BRIEFING_CHANGE_PIXELS} pixels \
                 against the frame before it",
                b.shots.len().saturating_sub(1)
            )
        } else {
            format!("steps that changed nothing visible: {flat:?}")
        },
    ));

    let bad_buttons: Vec<String> = b
        .buttons
        .iter()
        .filter(|button| !button.drawn || button.warm_permille < MIN_BUTTON_WARM_PERMILLE)
        .map(|button| {
            format!(
                "{} (drawn {}, {} per mille warm)",
                button.key, button.drawn, button.warm_permille
            )
        })
        .collect();
    checks.push(check(
        "briefing_buttons_drawn",
        b.buttons.len() == 3 && bad_buttons.is_empty(),
        if b.buttons.len() != 3 {
            format!("the screen drew {} buttons, not three", b.buttons.len())
        } else if bad_buttons.is_empty() {
            format!(
                "Replay Briefing, Return to Cabin and Go to Flight Check are drawn, each at \
                 least {MIN_BUTTON_WARM_PERMILLE} per mille brass in its region"
            )
        } else {
            format!("buttons not drawn: {bad_buttons:?}")
        },
    ));

    checks.push(if b.voice.audio_declared {
        check(
            "briefing_voice_plays",
            b.voice.starts >= 1 && b.voice.refusal.is_none() && b.voice.pulled_frames > 0,
            format!(
                "{}: {} start(s), {} sample frames pulled by the output stream, refusal {:?}",
                b.voice.member, b.voice.starts, b.voice.pulled_frames, b.voice.refusal
            ),
        )
    } else {
        Check {
            id: "briefing_voice_plays".to_owned(),
            applies: false,
            passed: false,
            detail: "the machine does not declare the audio capability, so the voice-over \
                     cannot be judged (never a pass)"
                .to_owned(),
        }
    });

    checks.push(match &b.replay {
        Some(replay) => check(
            "briefing_replay_restarts",
            replay.response == "Replayed"
                && replay.clock_after_s < replay.clock_before_s
                && replay.restarts >= 1
                && replay.stage_after == "Briefing"
                && (!b.voice.audio_declared || replay.starts_after > replay.starts_before),
            format!(
                "Replay Briefing answered {}: the clock went {:.1} s to {:.2} s, {} restart(s), \
                 voice starts {} to {}, stage {}",
                replay.response,
                replay.clock_before_s,
                replay.clock_after_s,
                replay.restarts,
                replay.starts_before,
                replay.starts_after,
                replay.stage_after
            ),
        ),
        None => check(
            "briefing_replay_restarts",
            false,
            "Replay Briefing was never pressed".to_owned(),
        ),
    });

    checks.push(check(
        "briefing_return_trip",
        b.return_response.as_deref() == Some("BriefingOpened")
            && b.return_stage.as_deref() == Some("Briefing")
            && b.return_restarts.is_some_and(|restarts| restarts >= 2),
        format!(
            "Return to Briefing from the Flight Check answered {:?}, stage {:?}, clock restarts {:?}",
            b.return_response, b.return_stage, b.return_restarts
        ),
    ));

    checks.push(check(
        "briefing_go_to_flight_check",
        b.forward_response.as_deref() == Some("FlightCheckOpened")
            && b.forward_stage.as_deref() == Some("FlightCheck"),
        format!(
            "Go to Flight Check answered {:?}, stage {:?}",
            b.forward_response, b.forward_stage
        ),
    ));
    checks
}

/// The briefing measurements as a JSON object (`null` when the run had none).
#[must_use]
pub(super) fn briefing_json(metrics: Option<&BriefingMetrics>) -> String {
    let Some(b) = metrics else {
        return "null".to_owned();
    };
    let strings = |items: &[String]| {
        items
            .iter()
            .map(|item| json_string(item))
            .collect::<Vec<_>>()
            .join(",")
    };
    let shots = b
        .shots
        .iter()
        .map(|shot| {
            format!(
                "{{\"label\":{},\"time_s\":{:.3},\"delivered\":{},\"drawn_permille\":{},\
                 \"changed_pixels\":{},\"expected\":[{}],\"drawn\":[{}]}}",
                json_string(&shot.label),
                shot.time_s,
                shot.delivered,
                shot.drawn_permille,
                shot.changed_pixels
                    .map_or_else(|| "null".to_owned(), |pixels| pixels.to_string()),
                strings(&shot.expected),
                strings(&shot.drawn)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let buttons = b
        .buttons
        .iter()
        .map(|button| {
            format!(
                "{{\"key\":{},\"drawn\":{},\"region\":{:?},\"warm_permille\":{}}}",
                json_string(&button.key),
                button.drawn,
                button.region,
                button.warm_permille
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let optional = |value: &Option<String>| {
        value
            .as_deref()
            .map_or_else(|| "null".to_owned(), json_string)
    };
    let replay = b.replay.as_ref().map_or_else(
        || "null".to_owned(),
        |replay| {
            format!(
                "{{\"response\":{},\"clock_before_s\":{:.3},\"clock_after_s\":{:.3},\
                 \"restarts\":{},\"starts_before\":{},\"starts_after\":{},\"stage_after\":{}}}",
                json_string(&replay.response),
                replay.clock_before_s,
                replay.clock_after_s,
                replay.restarts,
                replay.starts_before,
                replay.starts_after,
                json_string(&replay.stage_after)
            )
        },
    );
    format!(
        "{{\"script\":{},\"frame_width\":{},\"frame_height\":{},\"markers_s\":{:?},\
         \"end_s\":{:.3},\"shots\":[{shots}],\"buttons\":[{buttons}],\
         \"voice\":{{\"member\":{},\"starts\":{},\"audio_declared\":{},\"refusal\":{},\
         \"pulled_frames\":{}}},\"replay\":{replay},\"return_response\":{},\
         \"return_stage\":{},\"forward_response\":{},\"forward_stage\":{}}}",
        json_string(&b.script),
        b.frame_size.0,
        b.frame_size.1,
        b.markers_s,
        b.end_s,
        json_string(&b.voice.member),
        b.voice.starts,
        b.voice.audio_declared,
        optional(&b.voice.refusal),
        b.voice.pulled_frames,
        optional(&b.return_response),
        optional(&b.return_stage),
        optional(&b.forward_response),
        optional(&b.forward_stage),
    )
}
