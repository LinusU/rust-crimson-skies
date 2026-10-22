//! The debrief stage of the owner-command smoke (task #1655
//! `PARITY-M01-OUTCOME-C`), `--smoke-failure`.
//!
//! The run goes through the briefing, the Flight Check and the launch
//! cinematic like every windowed run, then:
//!
//! 1. once the player has control for a moment it **destroys the player**
//!    through the session's damage authority — one lethal hit with no attacker
//!    on the hull node, the same record a crash or gunfire writes — which is a
//!    scripted test input, never the original's failure;
//! 2. the mission host settles the failure (`player_destroyed`, #1654) and the
//!    window keeps the run open on the journal's debrief screen
//!    ([`DebriefScreen`]); the stage records whether an `AppExit` was written
//!    meanwhile;
//! 3. it takes the full framebuffer, hides the screen's overlay (buttons,
//!    labels, text rows, the stat card) and takes the bare pane, then judges
//!    both against the scene the screen built.
//!
//! The floors are designed and named next to their constants. A run is never
//! `human_play`, and nothing it measures is `verified_original`; the frames are
//! for comparing with the owner's capture by eye.

use bevy::app::AppExit;
use bevy::camera::visibility::InheritedVisibility;
use bevy::prelude::{ClearColor, Commands, MessageReader, Res, ResMut, Text};
use bevy::ui::{ComputedNode, UiGlobalTransform};
use cs_sim::damage::{DamageChannel, DamageNodeKey, HitEvent};
use cs_types::Tick;
use cs_types::net::EventId;

use super::super::debrief_screen::{DebriefOverlay, DebriefScene, DebriefScreen};
use super::super::host::MissionHost;
use super::super::player_victim::PlayerVictim;
use super::super::weapons::MISSION_TARGET_NODE;
use super::{
    Check, DrawnRow, FlightCheckButtonMetrics, FlightCheckFrames, FlightCheckTextMetrics, Phase,
    ProbeMode, ProbeState, RawFrame, SHOT_GRACE_FRAMES, SmokeConfig, SmokeData, SmokeMeasurements,
    SmokeShots, clear_colour_encodings, json_string, non_black_permille, region_difference,
    request_screenshot,
};
use crate::physics::PhysicsTickLedger;
use crate::weapons::SessionDamageResolver;

/// Frames after the player gains control that the smoke waits before it
/// destroys the player.
pub const KILL_AFTER_CONTROL_FRAMES: u32 = 60;
/// The producer serial of the smoke's lethal hit (the mission's are 201..=206).
pub const SMOKE_KILL_PRODUCER: u32 = 9_100;
/// The hit's damage: far beyond the designed hull pool.
pub const SMOKE_KILL_DAMAGE: f64 = 1.0e6;
/// Frames after the screen appears that the full frame is requested.
const SHOT_OFFSET: u32 = 20;
/// Frames after the screen appears that the overlay is hidden.
const BARE_OFFSET: u32 = 24;
/// Frames after the screen appears that the bare frame is requested.
const BARE_SHOT_OFFSET: u32 = 28;
/// Frames the run waits for the screen after the kill before it gives up.
const SCREEN_WAIT_FRAMES: u32 = 600;
/// The least share of the debrief frame that must be off black, in parts per
/// thousand. **Designed:** the 800x600 journal is aspect-fitted into the
/// window, so a 16:9 window leaves a black pillarbox around it.
pub const MIN_DEBRIEF_DRAWN_PERMILLE: u32 = 300;
/// The least pixels a text row must add inside its own region between the full
/// frame and the bare pane. **Designed:** a short word at 10 px is well over.
pub const MIN_DEBRIEF_TEXT_PIXELS: usize = 24;
/// The least pixels a button or pane must add inside its region. **Designed.**
pub const MIN_DEBRIEF_BUTTON_PIXELS: usize = 32;
/// The text rows the screen must draw.
const REQUIRED_TEXTS: [&str; 5] = [
    "SB_T_COMPLETETITLE",
    "SB_T_NAMEANDAREA",
    "SB_T_TIME",
    "SB_T_HIT",
    "SB_T_PLANES",
];
/// The buttons the screen must draw.
const REQUIRED_BUTTONS: [&str; 3] = ["SB_B_REPLAY", "SB_B_TOC", "SB_B_RETURNPC"];

/// What the debrief stage measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DebriefMetrics {
    /// The script frame the lethal hit was written in.
    pub kill_frame: Option<u32>,
    /// The mission host's outcome label once it had settled.
    pub settled_outcome: Option<String>,
    /// The mission host's terminal source label once it had settled.
    pub settled_source: Option<String>,
    /// The script frame the debrief screen first existed in.
    pub screen_frame: Option<u32>,
    /// `AppExit` messages seen between the kill and the end of the stage.
    pub exits_seen: u32,
    /// Whether both framebuffers arrived (the full frame and the bare pane).
    pub frames_delivered: bool,
    /// The frame's pixel size.
    pub frame_size: (u32, u32),
    /// The share of the full frame that is off black, in parts per thousand.
    pub drawn_permille: u32,
    /// Every text row of the scene, in scene order.
    pub texts: Vec<FlightCheckTextMetrics>,
    /// Every pane of the scene.
    pub panes: Vec<FlightCheckButtonMetrics>,
    /// Every button of the scene.
    pub buttons: Vec<FlightCheckButtonMetrics>,
    /// What the scene lists as unknown or left out.
    pub unknown_rows: Vec<String>,
}

/// The debrief stage's own progress.
#[derive(Clone, Debug, Default)]
pub struct DebriefRun {
    rows: Vec<DrawnRow>,
}

/// Judges the frames against the scene (pure; the system supplies the frames
/// and the nodes it read).
fn measure_screen(
    scene: &DebriefScene,
    rows: &[DrawnRow],
    full: Option<&RawFrame>,
    bare: Option<&RawFrame>,
    metrics: &mut DebriefMetrics,
) {
    let pair = full.zip(bare).filter(|(full, bare)| {
        full.width == bare.width
            && full.height == bare.height
            && full.data.len() == bare.data.len()
            && !full.data.is_empty()
    });
    let pixels = |region: [u32; 4]| {
        pair.map_or(0, |(full, bare)| {
            region_difference(&full.data, &bare.data, full.width, full.height, region)
        })
    };
    let find = |row: &str| {
        rows.iter()
            .find(|drawn| drawn.row.eq_ignore_ascii_case(row))
    };
    metrics.frames_delivered = pair.is_some();
    metrics.frame_size = full.map_or((0, 0), |frame| (frame.width, frame.height));
    metrics.drawn_permille = full.map_or(0, |frame| {
        non_black_permille(&frame.data, frame.width, frame.height)
    });
    metrics.texts = scene
        .texts
        .iter()
        .map(|text| {
            let drawn = find(&text.id);
            let region = drawn.map_or([0; 4], |drawn| drawn.region);
            FlightCheckTextMetrics {
                row: text.id.clone(),
                expected: text.text.clone(),
                drawn: drawn.and_then(|drawn| drawn.text.clone()),
                visible: drawn.is_some_and(|drawn| drawn.visible),
                region,
                pixels: pixels(region),
            }
        })
        .collect();
    let part = |id: &str| {
        let drawn = find(id);
        let region = drawn.map_or([0; 4], |drawn| drawn.region);
        FlightCheckButtonMetrics {
            hotspot: id.to_owned(),
            drawn: drawn.is_some_and(|drawn| drawn.visible),
            region,
            pixels: pixels(region),
        }
    };
    // The stat card is not part of the overlay (the texts are measured against
    // it), so it has no bare-pane difference of its own.
    metrics.panes = Vec::new();
    metrics.buttons = scene
        .buttons
        .iter()
        .map(|button| part(&button.hotspot))
        .collect();
    metrics.unknown_rows = scene
        .unknowns
        .iter()
        .map(|unknown| unknown.row.clone())
        .collect();
}

/// The stage's system: kills the player, waits for the screen, photographs it.
#[allow(clippy::too_many_arguments)]
pub fn smoke_debrief(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    victim: Option<Res<PlayerVictim>>,
    resolver: Option<ResMut<SessionDamageResolver>>,
    ticks: Option<Res<PhysicsTickLedger>>,
    host: Option<Res<MissionHost>>,
    screen: Option<Res<DebriefScreen>>,
    mut exits: MessageReader<AppExit>,
    mut probe: ResMut<ProbeState>,
    shots: Res<SmokeShots>,
    frames: Res<FlightCheckFrames>,
    mut commands: Commands,
    clear: Option<Res<ClearColor>>,
    overlay_nodes: bevy::prelude::Query<(
        &DebriefOverlay,
        Option<&Text>,
        &InheritedVisibility,
        &UiGlobalTransform,
        &ComputedNode,
    )>,
) {
    if config.mode != super::MissionSmokeMode::Failure
        || matches!(data.phase, Phase::Final | Phase::Ended)
    {
        return;
    }
    let frame = data.frame;
    if frame == 0 {
        return;
    }
    if data.measurements.debrief.is_none() {
        data.measurements.debrief = Some(DebriefMetrics::default());
    }
    let seen_exits = u32::try_from(exits.read().count()).unwrap_or(u32::MAX);
    let killed = data
        .measurements
        .debrief
        .as_ref()
        .is_some_and(|metrics| metrics.kill_frame.is_some());
    if killed
        && seen_exits > 0
        && let Some(metrics) = data.measurements.debrief.as_mut()
    {
        metrics.exits_seen += seen_exits;
    }

    // --- 1. the kill, once the player has control ----------------------------
    if !killed {
        let ready = data
            .measurements
            .first_control
            .as_ref()
            .is_some_and(|control| frame >= control.frame + KILL_AFTER_CONTROL_FRAMES);
        if !ready {
            return;
        }
        let (Some(victim), Some(mut resolver), Some(ticks)) = (victim, resolver, ticks) else {
            return;
        };
        let tick = Tick(ticks.ticks);
        let session = resolver.resolver().session();
        let hit = DamageNodeKey::new(MISSION_TARGET_NODE)
            .ok()
            .and_then(|node| {
                HitEvent::try_new(
                    EventId {
                        session,
                        tick,
                        producer: SMOKE_KILL_PRODUCER,
                        sequence: 0,
                    },
                    None,
                    victim.actor,
                    node,
                    DamageChannel::Internal,
                    SMOKE_KILL_DAMAGE,
                )
                .ok()
            });
        if let Some(hit) = hit
            && resolver.resolver_mut().resolve(tick, &[hit]).is_ok()
            && let Some(metrics) = data.measurements.debrief.as_mut()
        {
            metrics.kill_frame = Some(frame);
            data.phase_start = frame;
        }
        return;
    }

    // --- 2. the settle and the screen ----------------------------------------
    if let Some(terminal) = host.as_deref().and_then(MissionHost::terminal)
        && let Some(metrics) = data.measurements.debrief.as_mut()
        && metrics.settled_outcome.is_none()
    {
        metrics.settled_outcome = Some(terminal.outcome.label().to_owned());
        metrics.settled_source = Some(terminal.source.label().to_owned());
    }
    let kill_frame = data
        .measurements
        .debrief
        .as_ref()
        .and_then(|metrics| metrics.kill_frame)
        .unwrap_or(frame);
    let Some(screen) = screen else {
        if frame > kill_frame + SCREEN_WAIT_FRAMES {
            // The screen never came: the run ends with the failing checks.
            data.phase = Phase::Final;
            data.phase_start = frame;
        }
        return;
    };
    let shown_at = {
        let metrics = data.measurements.debrief.as_mut();
        let metrics = metrics.expect("the metrics were created above");
        *metrics.screen_frame.get_or_insert(frame)
    };
    let at = frame - shown_at;

    // --- 3. the frames -------------------------------------------------------
    let clear_colours = clear.as_deref().map(clear_colour_encodings);
    let mut request = |label: &str, data: &mut SmokeData| {
        data.shots_requested += 1;
        data.requested.push(label.to_owned());
        request_screenshot(
            &mut commands,
            shots.clone(),
            config
                .request
                .capture_dir
                .join(format!("frame_db{frame:05}_{label}.png")),
            label.to_owned(),
            clear_colours,
            None,
            Some(frames.clone()),
        );
    };
    if at == SHOT_OFFSET {
        data.debrief_run.rows = overlay_nodes
            .iter()
            .filter_map(|(overlay, text, visible, transform, node)| {
                let center = transform.translation;
                let size = node.size;
                Some(DrawnRow {
                    row: overlay.row.clone()?,
                    text: text.map(|text| text.0.clone()),
                    visible: visible.get(),
                    region: [
                        (center.x - 0.5 * size.x).max(0.0) as u32,
                        (center.y - 0.5 * size.y).max(0.0) as u32,
                        size.x.max(1.0) as u32,
                        size.y.max(1.0) as u32,
                    ],
                })
            })
            .collect();
        request("debrief", &mut data);
    } else if at == BARE_OFFSET {
        probe.0 = ProbeMode::DebriefBare;
    } else if at == BARE_SHOT_OFFSET {
        request("debrief_bare", &mut data);
    }
    if at <= BARE_SHOT_OFFSET {
        return;
    }
    let arrived = frames.0.lock().map_or(0, |frames| {
        frames
            .iter()
            .filter(|frame| frame.label.starts_with("debrief"))
            .count()
    });
    if arrived < 2 && at < BARE_SHOT_OFFSET + SHOT_GRACE_FRAMES {
        return;
    }
    probe.0 = ProbeMode::Restored;
    let raw = frames
        .0
        .lock()
        .map(|frames| frames.clone())
        .unwrap_or_default();
    let rows = data.debrief_run.rows.clone();
    if let Some(metrics) = data.measurements.debrief.as_mut() {
        measure_screen(
            screen.scene(),
            &rows,
            raw.iter().find(|frame| frame.label == "debrief"),
            raw.iter().find(|frame| frame.label == "debrief_bare"),
            metrics,
        );
    }
    data.phase = Phase::Final;
    data.phase_start = frame;
}

/// The debrief checks: the failure the run settles, the screen it opens and
/// that the run stays open on it.
pub fn evaluate(m: &SmokeMeasurements) -> Vec<Check> {
    let Some(d) = &m.debrief else {
        return vec![Check {
            id: "debrief_failure_settled".to_owned(),
            applies: true,
            passed: false,
            detail: "the run never reached the debrief stage".to_owned(),
        }];
    };
    let mut checks = Vec::new();
    checks.push(Check {
        id: "debrief_failure_settled".to_owned(),
        applies: true,
        passed: d.kill_frame.is_some()
            && d.settled_outcome.as_deref() == Some("failure")
            && d.settled_source.as_deref() == Some("player_destroyed"),
        detail: format!(
            "lethal hit written at frame {:?} (a scripted test input); the host settled {:?} from {:?}",
            d.kill_frame, d.settled_outcome, d.settled_source
        ),
    });
    checks.push(Check {
        id: "debrief_screen_shown".to_owned(),
        applies: true,
        passed: d.screen_frame.is_some()
            && d.frames_delivered
            && d.drawn_permille >= MIN_DEBRIEF_DRAWN_PERMILLE,
        detail: if d.screen_frame.is_none() {
            "the debrief screen never appeared after the failure settled".to_owned()
        } else if !d.frames_delivered {
            "the debrief framebuffers never arrived".to_owned()
        } else {
            format!(
                "{} per mille of the {}x{} debrief frame is off black (floor {})",
                d.drawn_permille, d.frame_size.0, d.frame_size.1, MIN_DEBRIEF_DRAWN_PERMILLE
            )
        },
    });
    checks.push(Check {
        id: "debrief_run_stays_open".to_owned(),
        applies: true,
        passed: d.screen_frame.is_some() && d.exits_seen == 0,
        detail: format!(
            "{} AppExit message(s) were written between the failure and the end of the stage",
            d.exits_seen
        ),
    });
    let bad_texts: Vec<String> = d
        .texts
        .iter()
        .filter(|text| {
            text.drawn.as_deref() != Some(text.expected.as_str())
                || !text.visible
                || text.pixels < MIN_DEBRIEF_TEXT_PIXELS
        })
        .map(|text| {
            format!(
                "{} (expected {:?}, drawn {:?}, visible {}, {} px)",
                text.row, text.expected, text.drawn, text.visible, text.pixels
            )
        })
        .collect();
    let absent_texts: Vec<&str> = REQUIRED_TEXTS
        .iter()
        .copied()
        .filter(|row| !d.texts.iter().any(|text| text.row == *row))
        .collect();
    checks.push(Check {
        id: "debrief_text_matches".to_owned(),
        applies: true,
        passed: d.frames_delivered && bad_texts.is_empty() && absent_texts.is_empty(),
        detail: if !d.frames_delivered {
            "the debrief framebuffers never arrived".to_owned()
        } else if !absent_texts.is_empty() {
            format!("the scene has no text row {absent_texts:?}")
        } else if bad_texts.is_empty() {
            format!(
                "all {} text rows are drawn as the scene states them, each adding at least \
                 {MIN_DEBRIEF_TEXT_PIXELS} pixels in its own region",
                d.texts.len()
            )
        } else {
            format!("text rows not drawn as the scene states them: {bad_texts:?}")
        },
    });
    let bad_parts: Vec<String> = d
        .panes
        .iter()
        .chain(&d.buttons)
        .filter(|part| !part.drawn || part.pixels < MIN_DEBRIEF_BUTTON_PIXELS)
        .map(|part| {
            format!(
                "{} (drawn {}, {} px)",
                part.hotspot, part.drawn, part.pixels
            )
        })
        .collect();
    let absent_buttons: Vec<&str> = REQUIRED_BUTTONS
        .iter()
        .copied()
        .filter(|row| !d.buttons.iter().any(|button| button.hotspot == *row))
        .collect();
    checks.push(Check {
        id: "debrief_buttons_drawn".to_owned(),
        applies: true,
        passed: d.frames_delivered && bad_parts.is_empty() && absent_buttons.is_empty(),
        detail: if !d.frames_delivered {
            "the debrief framebuffers never arrived".to_owned()
        } else if !absent_buttons.is_empty() {
            format!("the scene has no button {absent_buttons:?}")
        } else if bad_parts.is_empty() {
            format!(
                "all {} buttons and {} panes add at least {MIN_DEBRIEF_BUTTON_PIXELS} pixels in \
                 their own regions",
                d.buttons.len(),
                d.panes.len()
            )
        } else {
            format!("parts not drawn: {bad_parts:?}")
        },
    });
    checks.push(Check {
        id: "smoke_shots_delivered".to_owned(),
        applies: true,
        passed: m.missing_shots.is_empty(),
        detail: if m.missing_shots.is_empty() {
            "every requested framebuffer screenshot arrived".to_owned()
        } else {
            format!(
                "the renderer never delivered: {}",
                m.missing_shots.join(", ")
            )
        },
    });
    checks
}

/// The debrief measurements as the JSON object `report.json` carries.
pub fn debrief_json(metrics: Option<&DebriefMetrics>) -> String {
    let Some(d) = metrics else {
        return "null".to_owned();
    };
    let option_u32 =
        |value: Option<u32>| value.map_or_else(|| "null".to_owned(), |v| v.to_string());
    let option_string = |value: &Option<String>| {
        value
            .as_deref()
            .map_or_else(|| "null".to_owned(), json_string)
    };
    let texts = d
        .texts
        .iter()
        .map(|text| {
            format!(
                "{{\"row\":{},\"expected\":{},\"drawn\":{},\"visible\":{},\"region\":{:?},\
                 \"pixels\":{}}}",
                json_string(&text.row),
                json_string(&text.expected),
                option_string(&text.drawn),
                text.visible,
                text.region,
                text.pixels
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let parts = |parts: &[FlightCheckButtonMetrics]| {
        parts
            .iter()
            .map(|part| {
                format!(
                    "{{\"hotspot\":{},\"drawn\":{},\"region\":{:?},\"pixels\":{}}}",
                    json_string(&part.hotspot),
                    part.drawn,
                    part.region,
                    part.pixels
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let unknown_rows = d
        .unknown_rows
        .iter()
        .map(|row| json_string(row))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kill_frame\":{},\"settled_outcome\":{},\"settled_source\":{},\"screen_frame\":{},\
         \"exits_seen\":{},\"frames_delivered\":{},\"frame_width\":{},\"frame_height\":{},\
         \"drawn_permille\":{},\"texts\":[{texts}],\"panes\":[{}],\"buttons\":[{}],\
         \"unknown_rows\":[{unknown_rows}]}}",
        option_u32(d.kill_frame),
        option_string(&d.settled_outcome),
        option_string(&d.settled_source),
        option_u32(d.screen_frame),
        d.exits_seen,
        d.frames_delivered,
        d.frame_size.0,
        d.frame_size.1,
        d.drawn_permille,
        parts(&d.panes),
        parts(&d.buttons),
    )
}
