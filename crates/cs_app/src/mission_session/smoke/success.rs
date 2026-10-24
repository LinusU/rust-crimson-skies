//! The success stage of the owner-command smoke (task #2081, part A of
//! `--smoke-success`).
//!
//! M01 ("The Lost Treasure", `ZBD/C3/M01`) is won by a chain of objective
//! blocks (`docs/findings/2026-10-24-m01-c3-win-route-census.md`):
//! 12 -> 27 -> 21 -> 22 -> 23 -> 13 -> 14 -> 15. This part covers the first
//! beat, driven only through the player's own flight:
//!
//! the **proximity beat** — once the player has control for a moment it is
//! **placed** (a test placement, labelled in `report.json`, re-applied every
//! [`PLACE_EVERY_FRAMES`] frames) a fixed stand-off from the `shipwreck` site,
//! well inside `OBJECTIVE12`'s 200 m radius, nose on it. The run then records
//! whether the host's lifecycle report listed `OBJECTIVE12` and, two seconds
//! later, `OBJECTIVE27` (which marks the site) as completed.
//!
//! Beats after it (the drop-off landing, the Medusa wave, the hydrogen fire,
//! the docking) belong to #2082; the census records what they still need.
//!
//! What the mission host reports as completed is accumulated every fixed tick
//! by [`record_success_completions`] (the host's per-tick report is replaced by
//! the next tick). A run is never `human_play`, and nothing it measures is
//! `verified_original`.

use bevy::ecs::system::SystemParam;
use bevy::prelude::{ClearColor, Commands, Query, Res, ResMut, Resource, Vec3, With, World};

use super::super::compose::MissionPlayerBody;
use super::super::host::MissionHostReport;
use super::{
    Check, FlightCheckFrames, PendingPlacement, Phase, Placement, SHOT_GRACE_FRAMES, SmokeConfig,
    SmokeData, SmokeMeasurements, SmokeShots, clear_colour_encodings, json_string,
    non_black_permille, request_screenshot, yaw_toward,
};
use crate::mission_actors::MissionGroundTarget;

/// The objective site the proximity trigger names (`OBJECTIVE12`'s node).
pub const ANCHOR: &str = "shipwreck";
/// `OBJECTIVE12` — the `TRAVELERS` proximity site (block index 11).
pub const PROXIMITY_BLOCK: u32 = 11;
/// `OBJECTIVE27` — woken two seconds after block 12 completes; it marks the
/// site (block index 26).
pub const MARK_BLOCK: u32 = 26;
/// `OBJECTIVE12`'s radius, metres (the record's own).
pub const PROXIMITY_RADIUS_M: f32 = 200.0;
/// Frames after the player gains control that the stage starts.
pub const START_AFTER_CONTROL_FRAMES: u32 = 60;
/// Where the player stands off from the site, metres. **Designed:** well
/// inside the radius.
pub const STANDOFF_M: f32 = 120.0;
/// The direction from the site to the player, before normalising (east, up,
/// south). **Designed.**
pub const FROM_SIDE: [f32; 3] = [1.0, 0.3, 0.3];
/// The player is put back on its mark this often. **Designed.**
pub const PLACE_EVERY_FRAMES: u32 = 6;
/// Frames after the stage began that the near frame is taken.
const NEAR_SHOT_OFFSET: u32 = 30;
/// Frames the stage waits for the site to appear. **Designed.**
const FIND_BUDGET_FRAMES: u32 = 600;
/// Frames after the stage began that the run waits for both blocks to
/// complete (block 27 runs 2 s after block 12) before photographing the site
/// anyway. **Designed.**
pub const SETTLE_FRAMES: u32 = 900;
/// The least share of a frame that must be off black, in parts per thousand.
/// **Designed:** the world fills the window.
pub const MIN_DRAWN_PERMILLE: u32 = 300;

/// What the success stage measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SuccessMetrics {
    /// The script frame the stage began in.
    pub start_frame: Option<u32>,
    /// The site's measured position, metres.
    pub centre: Option<[f32; 3]>,
    /// Placements made.
    pub placements: u32,
    /// Every block the host's lifecycle report listed as completed, in order
    /// of first report, with the script frame it was first seen in.
    pub completed: Vec<(u32, u32)>,
    /// Whether both framebuffers arrived.
    pub frames_delivered: bool,
    /// The share of the near frame that is off black, per mille.
    pub near_permille: u32,
    /// The share of the marked frame that is off black, per mille.
    pub marked_permille: u32,
    /// The script frame the marked frame was asked for in.
    pub marked_shot_frame: Option<u32>,
    /// The stage never found the site and stopped waiting.
    pub site_never_found: bool,
}

impl SuccessMetrics {
    /// Whether the host reported `block` completed.
    #[must_use]
    pub fn has_completed(&self, block: u32) -> bool {
        self.completed.iter().any(|(b, _)| *b == block)
    }
}

/// Resource: every block the host's lifecycle report listed as completed since
/// the run began.
#[derive(Resource, Default, Debug)]
pub struct SuccessCompletions(pub Vec<u32>);

/// Accumulates the host's completed blocks after each fixed host tick: the
/// host's report is replaced by the next tick, so a `Last` system alone would
/// miss the ticks between two frames.
pub fn record_success_completions(world: &mut World) {
    if world
        .get_resource::<SmokeConfig>()
        .is_none_or(|config| config.mode != super::MissionSmokeMode::Success)
    {
        return;
    }
    let done = match world.get_resource::<MissionHostReport>() {
        Some(MissionHostReport {
            answer: Ok(answer), ..
        }) => answer.lifecycles.completed.clone(),
        _ => return,
    };
    if done.is_empty() {
        return;
    }
    let mut recorded = world.get_resource_or_insert_with(SuccessCompletions::default);
    for block in done {
        if !recorded.0.contains(&block) {
            recorded.0.push(block);
        }
    }
}

/// The stage's read-only world queries.
#[derive(SystemParam)]
pub struct SuccessWorld<'w, 's> {
    player: Query<'w, 's, (), With<MissionPlayerBody>>,
    grounds: Query<'w, 's, &'static MissionGroundTarget>,
    completions: Option<Res<'w, SuccessCompletions>>,
}

/// The stage's system: places the player, photographs.
#[allow(clippy::too_many_arguments)]
pub fn smoke_success(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    mut pending: ResMut<PendingPlacement>,
    shots: Res<SmokeShots>,
    frames: Res<FlightCheckFrames>,
    mut commands: Commands,
    clear: Option<Res<ClearColor>>,
    world: SuccessWorld,
) {
    if config.mode != super::MissionSmokeMode::Success
        || matches!(data.phase, Phase::Final | Phase::Ended)
    {
        return;
    }
    let frame = data.frame;
    if frame == 0 || world.player.is_empty() {
        return;
    }
    let Some(control_frame) = data
        .measurements
        .first_control
        .as_ref()
        .map(|control| control.frame)
    else {
        return;
    };
    if frame < control_frame + START_AFTER_CONTROL_FRAMES {
        return;
    }
    let mut metrics = data.measurements.success.take().unwrap_or_default();
    if metrics.start_frame.is_none() {
        let site = world
            .grounds
            .iter()
            .find(|ground| ground.node == ANCHOR)
            .map(|ground| Vec3::from_array(ground.position_m.map(|v| v as f32)));
        let Some(site) = site else {
            // The site never bound: wait a while, then record that and stop.
            if frame >= control_frame + START_AFTER_CONTROL_FRAMES + FIND_BUDGET_FRAMES {
                metrics.site_never_found = true;
                data.measurements.success = Some(metrics);
                data.phase = Phase::Final;
                data.phase_start = frame;
                return;
            }
            data.measurements.success = Some(metrics);
            return;
        };
        metrics.centre = Some(site.to_array());
        metrics.start_frame = Some(frame);
    }
    if let Some(done) = world.completions.as_deref() {
        for block in &done.0 {
            if !metrics.has_completed(*block) {
                metrics.completed.push((*block, frame));
            }
        }
    }
    let result = step(&mut metrics, &mut pending, frame);

    let clear_colours = clear.as_deref().map(clear_colour_encodings);
    for label in &result.shots {
        data.shots_requested += 1;
        data.requested.push((*label).to_owned());
        request_screenshot(
            &mut commands,
            shots.clone(),
            config
                .request
                .capture_dir
                .join(format!("frame_sc{frame:05}_{label}.png")),
            (*label).to_owned(),
            clear_colours,
            None,
            Some(frames.clone()),
        );
    }
    let mut phase_end = result.finished;
    if phase_end {
        let raw = frames
            .0
            .lock()
            .map(|frames| frames.clone())
            .unwrap_or_default();
        let find = |label: &str| raw.iter().find(|frame| frame.label == label);
        let (near, marked) = (find("success_near"), find("success_marked"));
        let asked = metrics.marked_shot_frame.unwrap_or(frame);
        if near.is_none() || marked.is_none() {
            phase_end = frame >= asked + SHOT_GRACE_FRAMES;
        }
        if phase_end {
            let permille = |shot: Option<&_>| {
                shot.map_or(0, |f: &super::RawFrame| {
                    non_black_permille(&f.data, f.width, f.height)
                })
            };
            metrics.frames_delivered = near.is_some() && marked.is_some();
            metrics.near_permille = permille(near);
            metrics.marked_permille = permille(marked);
        }
    }
    data.measurements.success = Some(metrics);
    if phase_end {
        data.phase = Phase::Final;
        data.phase_start = frame;
    }
}

/// What one frame of the stage asks for.
struct Step {
    /// Framebuffers to photograph this frame.
    shots: Vec<&'static str>,
    /// The stage has nothing more to do.
    finished: bool,
}

/// The mark: a stand-off from the site, nose on it.
fn mark(centre: [f32; 3]) -> Placement {
    let offset = Vec3::from_array(FROM_SIDE).normalize() * STANDOFF_M;
    Placement {
        name: format!("{ANCHOR} mark (test placement)"),
        position_m: (Vec3::from_array(centre) + offset).to_array(),
        yaw: yaw_toward(-offset),
        pitch: -offset.y.atan2(offset.x.hypot(offset.z)),
        speed_m_s: 0.0,
        distance_m: f64::from(STANDOFF_M),
    }
}

/// The stage's state machine, apart from the Bevy plumbing.
fn step(metrics: &mut SuccessMetrics, pending: &mut PendingPlacement, frame: u32) -> Step {
    let mut step = Step {
        shots: Vec::new(),
        finished: false,
    };
    let (Some(start), Some(centre)) = (metrics.start_frame, metrics.centre) else {
        step.finished = true;
        return step;
    };
    let at = frame - start;
    // The player stays on its mark for the whole stage.
    if at.is_multiple_of(PLACE_EVERY_FRAMES) {
        pending.0 = Some(mark(centre));
        metrics.placements += 1;
    }
    if at == NEAR_SHOT_OFFSET {
        step.shots.push("success_near");
    }
    // Photograph the marked site once both blocks completed (or time ran out),
    // never before the near frame has been asked for.
    let done = metrics.has_completed(PROXIMITY_BLOCK) && metrics.has_completed(MARK_BLOCK);
    if metrics.marked_shot_frame.is_none() && at > NEAR_SHOT_OFFSET && (done || at >= SETTLE_FRAMES)
    {
        step.shots.push("success_marked");
        metrics.marked_shot_frame = Some(frame);
    }
    step.finished = metrics.marked_shot_frame.is_some();
    step
}

/// The success-stage checks: the host reported the proximity block and the
/// marking block completed after the placement, and the frames were drawn.
pub fn evaluate(m: &SmokeMeasurements) -> Vec<Check> {
    let Some(c) = &m.success else {
        return vec![Check {
            id: "success_proximity_block_completed".to_owned(),
            applies: true,
            passed: false,
            detail: "the run never reached the success stage".to_owned(),
        }];
    };
    let at = |block: u32| {
        c.completed
            .iter()
            .find(|(b, _)| *b == block)
            .map(|(_, frame)| *frame)
    };
    vec![
        Check {
            id: "success_proximity_block_completed".to_owned(),
            applies: true,
            passed: c.has_completed(PROXIMITY_BLOCK) && c.placements > 0,
            detail: format!(
                "the player was placed {STANDOFF_M} m from {ANCHOR} ({} test placements, radius \
                 {PROXIMITY_RADIUS_M} m) and the host reported block {PROXIMITY_BLOCK} completed \
                 at {:?}; site never found: {}",
                c.placements,
                at(PROXIMITY_BLOCK),
                c.site_never_found
            ),
        },
        Check {
            id: "success_mark_block_completed".to_owned(),
            applies: true,
            passed: c.has_completed(MARK_BLOCK),
            detail: format!(
                "the host's lifecycle report listed block {MARK_BLOCK} (woken 2 s after block \
                 {PROXIMITY_BLOCK}) completed at {:?}",
                at(MARK_BLOCK)
            ),
        },
        Check {
            id: "success_frames_drawn".to_owned(),
            applies: true,
            passed: c.frames_delivered
                && c.near_permille >= MIN_DRAWN_PERMILLE
                && c.marked_permille >= MIN_DRAWN_PERMILLE,
            detail: if c.frames_delivered {
                format!(
                    "{} per mille of the near frame and {} per mille of the marked frame are off \
                     black (floor {MIN_DRAWN_PERMILLE})",
                    c.near_permille, c.marked_permille
                )
            } else {
                "the near or marked framebuffer never arrived".to_owned()
            },
        },
        Check {
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
        },
    ]
}

/// The success measurements as the JSON object `report.json` carries.
pub fn success_json(metrics: Option<&SuccessMetrics>) -> String {
    let Some(c) = metrics else {
        return "null".to_owned();
    };
    let option_u32 =
        |value: Option<u32>| value.map_or_else(|| "null".to_owned(), |v| v.to_string());
    let completed = c
        .completed
        .iter()
        .map(|(block, frame)| format!("{{\"block\":{block},\"frame\":{frame}}}"))
        .collect::<Vec<_>>()
        .join(",");
    let centre = c.centre.map_or_else(
        || "null".to_owned(),
        |p| format!("[{},{},{}]", p[0], p[1], p[2]),
    );
    format!(
        "{{\"start_frame\":{},\"anchor\":{},\"site_never_found\":{},\"centre_m\":{centre},\
         \"standoff_m\":{STANDOFF_M},\"placements\":{},\"completed\":[{completed}],\
         \"frames_delivered\":{},\"near_permille\":{},\"marked_permille\":{}}}",
        option_u32(c.start_frame),
        json_string(ANCHOR),
        c.site_never_found,
        c.placements,
        c.frames_delivered,
        c.near_permille,
        c.marked_permille,
    )
}
