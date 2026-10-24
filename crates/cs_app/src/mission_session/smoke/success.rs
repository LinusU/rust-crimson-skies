//! The success stage of the owner-command smoke (task #2081, part A of
//! `--smoke-success`).
//!
//! M01 is won by a chain of objective blocks. This part of the scripted win
//! covers the first two beats, driven only through the player's own inputs:
//!
//! 1. **the proximity beat** — once the player has control for a moment it is
//!    **placed** (a test placement, labelled in `report.json`, re-applied every
//!    [`PLACE_EVERY_FRAMES`] frames) a fixed stand-off from
//!    `workersvoyagezep`, well inside `OBJECTIVE3`'s 700 m radius, nose on it;
//! 2. **the engine-part beat** — it **presses the guns key**
//!    ([`Action::Flight`] `FirePrimary`, through the production cockpit-action
//!    queue) every frame until the damage authority lists all eighteen
//!    `reng<n>`/`leng<n>` parts of the zeppelin out of play. It never writes
//!    damage.
//!
//! What the mission host reports as completed is accumulated every fixed tick
//! by [`record_success_completions`] (the host's per-tick report is replaced by
//! the next tick) and the run records whether blocks 2, 5, 7 and 8
//! (`OBJECTIVE3`, `6`, `8`, `9`) completed. A run is never `human_play`, and
//! nothing it measures is `verified_original`.

use bevy::ecs::system::SystemParam;
use bevy::prelude::{ClearColor, Commands, Query, Res, ResMut, Resource, Vec3, With, World};
use cs_sim::damage::{ActorId, PartState};
use cs_types::input::{Action, FlightCommand};

use super::super::compose::MissionPlayerBody;
use super::super::host::MissionHostReport;
use super::super::parts::MissionPartReport;
use super::super::weapons::MissionCockpitActions;
use super::{
    Check, FlightCheckFrames, PendingPlacement, Phase, Placement, SHOT_GRACE_FRAMES, SmokeConfig,
    SmokeData, SmokeMeasurements, SmokeShots, clear_colour_encodings, json_string,
    non_black_permille, request_screenshot, yaw_toward,
};
use crate::scene::NodeVisualTransform;
use crate::weapons::SessionDamageResolver;

/// The zeppelin the proximity trigger and the engine ladders name.
pub const ANCHOR: &str = "workersvoyagezep";
/// How many engine parts the zeppelin's record declares (and the three
/// ladders count together).
pub const ENGINE_PARTS: usize = 18;
/// `OBJECTIVE3` — the `TRAVELERS` proximity site.
pub const PROXIMITY_BLOCK: u32 = 2;
/// `OBJECTIVE6`, `OBJECTIVE8` and `OBJECTIVE9` — the three engine ladders.
pub const ENGINE_LADDER_BLOCKS: [u32; 3] = [5, 7, 8];
/// `OBJECTIVE3`'s radius, metres (the record's own).
pub const PROXIMITY_RADIUS_M: f32 = 700.0;
/// Frames after the player gains control that the stage starts.
pub const START_AFTER_CONTROL_FRAMES: u32 = 60;
/// Where the player stands off from the zeppelin, metres. **Designed:** the
/// stand-off the convoy-kill stage and the headless gun-hit tests fire from.
pub const STANDOFF_M: f32 = 80.0;
/// The direction from the zeppelin to the player, before normalising (east,
/// up, south). **Designed:** the headless tests' side.
pub const FROM_SIDE: [f32; 3] = [1.0, 0.15, 0.3];
/// The player is put back on its mark this often. **Designed.**
pub const PLACE_EVERY_FRAMES: u32 = 6;
/// Frames after the stage began that the proximity frame is taken.
const NEAR_SHOT_OFFSET: u32 = 30;
/// Frames after the proximity frame is asked for that the guns key goes down.
const FIRE_AFTER_SHOT_FRAMES: u32 = 20;
/// Frames the stage waits for the engine parts to appear. **Designed.**
const FIND_BUDGET_FRAMES: u32 = 600;
/// Frames the run may spend bringing the engines out of play before giving
/// up. **Designed.**
pub const ENGINE_BUDGET_FRAMES: u32 = 3_600;
/// Frames after the last engine left play that the run waits for the three
/// ladders to complete (each is woken by the one before) and photographs the
/// zeppelin. **Designed.**
pub const SETTLE_FRAMES: u32 = 600;
/// The least share of a frame that must be off black, in parts per thousand.
/// **Designed:** the world fills the window.
pub const MIN_DRAWN_PERMILLE: u32 = 300;

/// What the success stage measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SuccessMetrics {
    /// The script frame the stage began in.
    pub start_frame: Option<u32>,
    /// The zeppelin's measured centre (its parts' bind pose), metres.
    pub centre: Option<[f32; 3]>,
    /// Each engine part's node name, found when the stage began.
    pub parts: Vec<String>,
    /// Placements made.
    pub placements: u32,
    /// Gun presses queued through the cockpit action queue.
    pub fire_presses: u64,
    /// `(part, frame)` the damage authority first listed as destroyed.
    pub destroyed: Vec<(String, u32)>,
    /// The script frame the last engine part was destroyed in.
    pub last_destroyed_frame: Option<u32>,
    /// Every block the host's lifecycle report listed as completed, in order
    /// of first report, with the script frame it was first seen in.
    pub completed: Vec<(u32, u32)>,
    /// Whether both framebuffers arrived.
    pub frames_delivered: bool,
    /// The share of the proximity frame that is off black, per mille.
    pub near_permille: u32,
    /// The share of the engines-out frame that is off black, per mille.
    pub out_permille: u32,
    /// The script frame the engines-out frame was asked for in.
    pub out_shot_frame: Option<u32>,
    /// Whether the composition installed a part report at all.
    pub part_report_present: bool,
    /// How many parts of the report name the anchor's engines.
    pub engine_chains_reported: usize,
    /// How many parts of any actor the report bound.
    pub parts_reported: usize,
    /// The report's refusals, verbatim (first few).
    pub part_refusals: Vec<String>,
    /// The stage never found the engine parts and stopped waiting.
    pub parts_never_found: bool,
    /// The stage gave up bringing the engines out of play.
    pub gave_up: bool,
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
    parts: Option<Res<'w, MissionPartReport>>,
    poses: Query<'w, 's, &'static NodeVisualTransform>,
    resolver: Option<Res<'w, SessionDamageResolver>>,
    completions: Option<Res<'w, SuccessCompletions>>,
}

/// Whether a part chain is one of the anchor's engines.
fn is_engine_chain(chain: &[String]) -> bool {
    chain.first().map(String::as_str) == Some(ANCHOR)
        && chain.get(2).map(String::as_str) == Some("healthy")
        && chain
            .get(1)
            .is_some_and(|node| node.starts_with("reng") || node.starts_with("leng"))
}

/// The zeppelin's engine parts: `(node, actor, entity centre)`.
fn engine_parts(world: &SuccessWorld) -> Vec<(String, ActorId, [f32; 3])> {
    let Some(report) = world.parts.as_deref() else {
        return Vec::new();
    };
    report
        .parts
        .iter()
        .filter(|part| is_engine_chain(part.chain.as_ref()))
        .filter_map(|part| {
            let pose = world.poses.get(part.entity).ok()?;
            let at = pose.global().translation();
            Some((part.chain.get(1)?.clone(), part.actor, [at.x, at.y, at.z]))
        })
        .collect()
}

/// The stage's system: places the player, fires, photographs.
#[allow(clippy::too_many_arguments)]
pub fn smoke_success(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    mut pending: ResMut<PendingPlacement>,
    actions: Option<ResMut<MissionCockpitActions>>,
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
    let found = engine_parts(&world);
    let mut actors: Vec<(String, ActorId)> = Vec::new();
    if metrics.start_frame.is_none() {
        metrics.part_report_present = world.parts.is_some();
        metrics.engine_chains_reported = world.parts.as_deref().map_or(0, |report| {
            report
                .parts
                .iter()
                .filter(|part| is_engine_chain(part.chain.as_ref()))
                .count()
        });
        metrics.parts_reported = world.parts.as_deref().map_or(0, |r| r.parts.len());
        metrics.part_refusals = world
            .parts
            .as_deref()
            .map_or_else(Vec::new, |r| r.refusals.iter().take(5).cloned().collect());
        if found.is_empty() {
            // The parts never bound: wait a while, then record that and stop.
            if frame >= control_frame + START_AFTER_CONTROL_FRAMES + FIND_BUDGET_FRAMES {
                metrics.parts_never_found = true;
                data.measurements.success = Some(metrics);
                data.phase = Phase::Final;
                data.phase_start = frame;
                return;
            }
            data.measurements.success = Some(metrics);
            return;
        }
        let sum = found
            .iter()
            .fold(Vec3::ZERO, |acc, (_, _, c)| acc + Vec3::from_array(*c));
        metrics.centre = Some((sum / found.len() as f32).to_array());
        metrics.parts = found.iter().map(|(node, _, _)| node.clone()).collect();
        metrics.start_frame = Some(frame);
    }
    actors.extend(found.iter().map(|(node, actor, _)| (node.clone(), *actor)));
    // The authority's recorded part state, not a one-tick publication.
    let destroyed_now: Vec<String> = actors
        .iter()
        .filter(|(node, actor)| {
            cs_sim::damage::DamageNodeKey::new(node).is_ok_and(|key| {
                world.resolver.as_deref().is_some_and(|resolver| {
                    resolver.resolver().part_state(actor, &key) == Some(PartState::Destroyed)
                })
            })
        })
        .map(|(node, _)| node.clone())
        .collect();
    if let Some(done) = world.completions.as_deref() {
        for block in &done.0 {
            if !metrics.has_completed(*block) {
                metrics.completed.push((*block, frame));
            }
        }
    }
    let mut queue = MissionCockpitActions::default();
    let result = step(
        &mut metrics,
        &mut pending,
        &mut queue,
        frame,
        &destroyed_now,
    );
    if let Some(mut actions) = actions {
        actions.actions.append(&mut queue.actions);
    }

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
        let (near, out) = (find("success_near"), find("success_engines_out"));
        let asked = metrics.out_shot_frame.unwrap_or(frame);
        if (near.is_none() || out.is_none()) && !metrics.gave_up {
            phase_end = frame >= asked + SHOT_GRACE_FRAMES;
        }
        if phase_end {
            let permille = |shot: Option<&_>| {
                shot.map_or(0, |f: &super::RawFrame| {
                    non_black_permille(&f.data, f.width, f.height)
                })
            };
            metrics.frames_delivered = near.is_some() && out.is_some();
            metrics.near_permille = permille(near);
            metrics.out_permille = permille(out);
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

/// The mark: a stand-off from the zeppelin, nose on it.
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
fn step(
    metrics: &mut SuccessMetrics,
    pending: &mut PendingPlacement,
    actions: &mut MissionCockpitActions,
    frame: u32,
    destroyed_now: &[String],
) -> Step {
    let mut step = Step {
        shots: Vec::new(),
        finished: false,
    };
    let (Some(start), Some(centre)) = (metrics.start_frame, metrics.centre) else {
        step.finished = true;
        return step;
    };
    let at = frame - start;
    for node in destroyed_now {
        if !metrics.destroyed.iter().any(|(n, _)| n == node) {
            metrics.destroyed.push((node.clone(), frame));
            metrics.last_destroyed_frame = Some(frame);
        }
    }
    // The player stays on its mark for the whole stage.
    if at.is_multiple_of(PLACE_EVERY_FRAMES) {
        pending.0 = Some(mark(centre));
        metrics.placements += 1;
    }
    let remaining = metrics.parts.len().saturating_sub(metrics.destroyed.len());
    if remaining > 0 {
        if at > ENGINE_BUDGET_FRAMES {
            metrics.gave_up = true;
            step.finished = true;
            return step;
        }
        if at == NEAR_SHOT_OFFSET {
            step.shots.push("success_near");
        }
        // The guns stay quiet until the proximity frame has been rendered.
        if at >= NEAR_SHOT_OFFSET + FIRE_AFTER_SHOT_FRAMES {
            actions
                .actions
                .push(Action::Flight(FlightCommand::FirePrimary));
            metrics.fire_presses += 1;
        }
        return step;
    }
    // Every engine is out: wait for the ladders the host folds in turn, then
    // photograph the zeppelin once.
    let since = frame - metrics.last_destroyed_frame.unwrap_or(frame);
    let ladders_done = ENGINE_LADDER_BLOCKS
        .iter()
        .all(|block| metrics.has_completed(*block));
    if metrics.out_shot_frame.is_none() && (ladders_done || since >= SETTLE_FRAMES) {
        step.shots.push("success_engines_out");
        metrics.out_shot_frame = Some(frame);
    }
    step.finished = metrics.out_shot_frame.is_some();
    step
}

/// The success-stage checks: the guns key took the engines out through the
/// damage authority, and the host reported the proximity block and the three
/// engine ladders completed.
pub fn evaluate(m: &SmokeMeasurements) -> Vec<Check> {
    let Some(c) = &m.success else {
        return vec![Check {
            id: "success_engines_out_by_fire_key".to_owned(),
            applies: true,
            passed: false,
            detail: "the run never reached the success stage".to_owned(),
        }];
    };
    let all_destroyed = c.parts.len() == ENGINE_PARTS
        && c.parts
            .iter()
            .all(|node| c.destroyed.iter().any(|(n, _)| n == node));
    let blocks = |blocks: &[u32]| {
        blocks
            .iter()
            .map(|block| {
                let at = c
                    .completed
                    .iter()
                    .find(|(b, _)| b == block)
                    .map(|(_, frame)| *frame);
                format!("{block}@{at:?}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    vec![
        Check {
            id: "success_proximity_block_completed".to_owned(),
            applies: true,
            passed: c.has_completed(PROXIMITY_BLOCK) && c.placements > 0,
            detail: format!(
                "the player was placed {STANDOFF_M} m from {ANCHOR} ({} test placements, radius \
                 {PROXIMITY_RADIUS_M} m) and the host reported block {}",
                c.placements,
                blocks(&[PROXIMITY_BLOCK])
            ),
        },
        Check {
            id: "success_engines_out_by_fire_key".to_owned(),
            applies: true,
            passed: all_destroyed && c.fire_presses > 0 && !c.gave_up,
            detail: format!(
                "{} guns-key press(es) were queued; the damage authority listed {} of {} \
                 engine parts destroyed (the smoke writes no damage); gave up: {}; part report \
                 present: {}, parts reported: {}, engine chains reported: {}, parts never \
                 found: {}, refusals: {:?}",
                c.fire_presses,
                c.destroyed.len(),
                ENGINE_PARTS,
                c.gave_up,
                c.part_report_present,
                c.parts_reported,
                c.engine_chains_reported,
                c.parts_never_found,
                c.part_refusals
            ),
        },
        Check {
            id: "success_engine_ladders_completed".to_owned(),
            applies: true,
            passed: ENGINE_LADDER_BLOCKS
                .iter()
                .all(|block| c.has_completed(*block)),
            detail: format!(
                "the host's lifecycle report listed completed blocks (frame first seen): {}",
                blocks(&ENGINE_LADDER_BLOCKS)
            ),
        },
        Check {
            id: "success_frames_drawn".to_owned(),
            applies: true,
            passed: c.frames_delivered
                && c.near_permille >= MIN_DRAWN_PERMILLE
                && c.out_permille >= MIN_DRAWN_PERMILLE,
            detail: if c.frames_delivered {
                format!(
                    "{} per mille of the near frame and {} per mille of the engines-out frame \
                     are off black (floor {MIN_DRAWN_PERMILLE})",
                    c.near_permille, c.out_permille
                )
            } else {
                "the near or engines-out framebuffer never arrived".to_owned()
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
    let destroyed = c
        .destroyed
        .iter()
        .map(|(node, frame)| format!("{{\"node\":{},\"frame\":{frame}}}", json_string(node)))
        .collect::<Vec<_>>()
        .join(",");
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
        "{{\"start_frame\":{},\"anchor\":{},\"part_report_present\":{},\
         \"engine_chains_reported\":{},\"parts_never_found\":{},\"centre_m\":{centre},\"engine_parts\":{},\
         \"standoff_m\":{STANDOFF_M},\"placements\":{},\"fire_presses\":{},\
         \"destroyed\":[{destroyed}],\"last_destroyed_frame\":{},\"completed\":[{completed}],\
         \"frames_delivered\":{},\"near_permille\":{},\"out_permille\":{},\
         \"gave_up\":{}}}",
        option_u32(c.start_frame),
        json_string(ANCHOR),
        c.part_report_present,
        c.engine_chains_reported,
        c.parts_never_found,
        c.parts.len(),
        c.placements,
        c.fire_presses,
        option_u32(c.last_destroyed_frame),
        c.frames_delivered,
        c.near_permille,
        c.out_permille,
        c.gave_up,
    )
}
