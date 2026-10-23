//! The leave-the-flight stage of the owner-command start-flow smoke
//! (`--smoke-start-flow`, task #1832 `PARITY5-START-FLOW.04`).
//!
//! Each mission leg of `cs --cs-path <retail> --smoke-seconds <n>` goes
//! through the briefing, the Flight Check and the launch cinematic like the
//! scripted mode, flies a short measured flight, opens the Esc pause map and
//! presses **Quit** through the map's own click path. The first leg — the
//! one handed over with `--from-start-flow` — reports `abandoned` so the
//! chain's next leg resumes on the cabin; the replay leg's Quit ends the
//! process. A screen the leg never reaches is a failed check naming it, and
//! a framebuffer that never arrives lands in `missing_shots` by its label.
//! Nothing here is `verified_original`; the frames are for comparing with
//! the owner's captures by eye.

use super::briefing::evaluate_briefing;
use super::{
    Check, MAX_CINEMATIC_DRIFT_M, MAX_CINEMATIC_TURN_DEG, SmokeMeasurements,
    evaluate_first_control, evaluate_flight_check, judge_cinematic_frames,
};

/// What the leave-the-flight stage measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StartFlowMetrics {
    /// The mission was handed over by the start flow (`--from-start-flow`),
    /// so its Quit returns to the cabin rather than ending the process.
    pub return_to_front_end: bool,
    /// The Esc pause map drew when the script pressed it.
    pub map_shown: bool,
    /// The map's own click path accepted a **Quit** press.
    pub quit_clicked: bool,
    /// How the flight reported ending to the front end (`key()` of the
    /// `MissionEnd`): present on the `--from-start-flow` leg only — the
    /// chain's proof the quit reached the cabin.
    pub mission_left: Option<String>,
}

/// The stage's checks: the briefing and Flight Check the leg drives, the
/// launch cinematic it sits through, the controlled flight, the pause map's
/// own screen and the Quit its click path delivers.
pub fn evaluate(m: &SmokeMeasurements) -> Vec<Check> {
    let Some(s) = &m.start_flow else {
        return vec![Check {
            id: "start_flow_pause_map_opened".to_owned(),
            applies: true,
            passed: false,
            detail: "the run was not a start-flow mission leg".to_owned(),
        }];
    };
    let mut checks = evaluate_briefing(m);
    checks.extend(evaluate_flight_check(m));

    // The launch cinematic, as the scripted mode judges it.
    match &m.cinematic {
        Some(c) => checks.push(Check {
            id: "launch_cinematic_plays".to_owned(),
            applies: true,
            passed: c.handed_over
                && c.played_ticks >= c.total_ticks
                && c.max_drift_m <= MAX_CINEMATIC_DRIFT_M
                && c.max_turn_deg <= MAX_CINEMATIC_TURN_DEG
                && c.bars_on_while_playing
                && c.bars_off_after
                && judge_cinematic_frames(&c.frames).is_ok(),
            detail: format!(
                "{} of {} ticks played, handed over: {}, body drift {:.3} m (ceiling {}), \
                 turn {:.3} deg (ceiling {}), bars on while playing: {}, off after: {}; \
                 pixels: {}",
                c.played_ticks,
                c.total_ticks,
                c.handed_over,
                c.max_drift_m,
                MAX_CINEMATIC_DRIFT_M,
                c.max_turn_deg,
                MAX_CINEMATIC_TURN_DEG,
                c.bars_on_while_playing,
                c.bars_off_after,
                judge_cinematic_frames(&c.frames)
                    .map_or_else(|why| why, |()| "as expected".to_owned())
            ),
        }),
        None => checks.push(Check {
            id: "launch_cinematic_plays".to_owned(),
            applies: true,
            passed: false,
            detail: "the launch cinematic never ran".to_owned(),
        }),
    }
    if let Some(c) = &m.camera {
        let camera = &c.camera;
        checks.push(Check {
            id: "launch_cinematic_camera_played".to_owned(),
            applies: true,
            passed: camera.frames_driven > 0
                && camera.frames_without_frame_node == 0
                && camera.scenes_seen == camera.scenes_total
                && c.shots > 0,
            detail: format!(
                "the measured camera script drove {} frames, {} of {} scenes were on screen, \
                 {} frames had no Pandora entity to put the pose into, {} scene screenshots",
                camera.frames_driven,
                camera.scenes_seen,
                camera.scenes_total,
                camera.frames_without_frame_node,
                c.shots
            ),
        });
    }

    checks.push(evaluate_first_control(m));

    checks.push(Check {
        id: "start_flow_pause_map_opened".to_owned(),
        applies: true,
        passed: s.map_shown && !m.missing_shots.iter().any(|shot| shot == "pause_map"),
        detail: if s.map_shown {
            "the Esc pause map drew and its frame was requested".to_owned()
        } else {
            "the Esc pause map never drew — the press produced no screen".to_owned()
        },
    });
    checks.push(Check {
        id: "start_flow_quit_clicked".to_owned(),
        applies: true,
        passed: s.quit_clicked,
        detail: if s.quit_clicked {
            "the pause map's own click path accepted the Quit press".to_owned()
        } else {
            "the pause map's Quit never took a press (the click hovered nothing \
                 the map calls a button)"
                .to_owned()
        },
    });
    checks.push(Check {
        id: "start_flow_mission_left".to_owned(),
        applies: s.return_to_front_end,
        passed: s.mission_left.as_deref() == Some("abandoned"),
        detail: if s.return_to_front_end {
            format!(
                "the handed-over flight reported its end as {:?}; the chain resumes the \
                 cabin on `abandoned`",
                s.mission_left
            )
        } else {
            "the replay leg was not handed over by the start flow, so its Quit ends the \
                 process"
                .to_owned()
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
                "framebuffer screenshots never arrived for {:?}",
                m.missing_shots
            )
        },
    });
    checks
}
