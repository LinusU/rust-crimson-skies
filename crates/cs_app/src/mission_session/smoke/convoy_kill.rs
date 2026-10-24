//! The convoy-kill stage of the owner-command smoke (task #2047),
//! `--smoke-convoy-kill`.
//!
//! M01's bridge convoy (`t_truck01`..`t_truck03`, `bridge_truck01`) is shot
//! down with the player's own guns key. The run goes through the briefing, the
//! Flight Check and the launch cinematic like every windowed run, then:
//!
//! 1. once the player has control for a moment it **places the player** a
//!    fixed stand-off from the first truck still standing, nose on it (a test
//!    placement, labelled in `report.json`, re-applied every
//!    [`PLACE_EVERY_FRAMES`] frames), and photographs the intact convoy;
//! 2. it **presses the guns key** ([`Action::Flight`] `FirePrimary`, through
//!    the production cockpit-action queue) every frame. It never writes
//!    damage: a truck is destroyed only when the session's damage authority
//!    says so, and the mission's observation row for it must turn
//!    `no_longer_in_play`;
//! 3. when every truck is destroyed it places the player on a vantage over the
//!    whole convoy, waits [`SETTLE_FRAMES`] for the destroyed layout and any
//!    debris and photographs it.
//!
//! The floors are designed and named next to their constants. A run is never
//! `human_play`, and nothing it measures is `verified_original`; whether the
//! authored `destroyed` layout or a debris wreck is *drawn* is for the frames
//! to show by eye next to the Drydock originals.

use bevy::ecs::system::SystemParam;
use bevy::prelude::{ClearColor, Commands, Query, Res, ResMut, Resource, Vec3, With, World};
use cs_sim::damage::ActorId;
use cs_types::input::{Action, FlightCommand};

use super::super::compose::MissionPlayerBody;
use super::super::host::MissionWorldObservation;
use super::super::observation::mission_world_observation;
use super::super::weapons::MissionCockpitActions;
use super::{
    Check, FlightCheckFrames, PendingPlacement, Phase, Placement, SHOT_GRACE_FRAMES, SmokeConfig,
    SmokeData, SmokeMeasurements, SmokeShots, clear_colour_encodings, json_string,
    non_black_permille, request_screenshot, yaw_toward,
};
use crate::damage::DamageScheduleReport;
use crate::mission_actors::{MissionGroundTargetReport, MissionGroundTargetSweep};
use crate::world_facts::member_chain;

/// The convoy, by world node name: the three trucks OBJECTIVE6 wakes and the
/// bridge truck. The order is the order they are shot down in.
pub const TRUCKS: [&str; 4] = ["t_truck01", "t_truck02", "t_truck03", "bridge_truck01"];
/// Frames after the player gains control that the stage starts.
pub const START_AFTER_CONTROL_FRAMES: u32 = 60;
/// Where the player stands off from a truck, metres. **Designed:** the
/// distance the headless gun-hit tests fire from.
pub const STANDOFF_M: f32 = 80.0;
/// The direction from the truck to the player, before normalising (east, up,
/// south). **Designed:** the headless tests' side.
pub const FROM_SIDE: [f32; 3] = [1.0, 0.15, 0.3];
/// The stand-off over the whole convoy for the final frame, metres.
/// **Designed.**
pub const VANTAGE_M: f32 = 160.0;
/// The player is put back on its mark this often. **Designed.**
pub const PLACE_EVERY_FRAMES: u32 = 6;
/// Frames after the stage began that the "intact" frame is taken.
const INTACT_SHOT_OFFSET: u32 = 30;
/// Frames after the intact frame is asked for that the guns key goes down.
const FIRE_AFTER_SHOT_FRAMES: u32 = 20;
/// Frames after the last truck died that the "destroyed" frame is taken.
/// **Designed:** time for the destroyed layout and debris to appear.
pub const SETTLE_FRAMES: u32 = 90;
/// Frames the run may spend on one truck before giving up.
pub const PER_TRUCK_BUDGET_FRAMES: u32 = 1_200;
/// The least share of a convoy frame that must be off black, in parts per
/// thousand. **Designed:** the world fills the window.
pub const MIN_CONVOY_DRAWN_PERMILLE: u32 = 300;

/// What the convoy-kill stage measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ConvoyKillMetrics {
    /// The script frame the stage began in.
    pub start_frame: Option<u32>,
    /// Each truck's bound actor, found when the stage began.
    pub actors: Vec<(String, ActorId)>,
    /// Each truck's measured box centre, metres.
    pub centres: Vec<(String, [f32; 3])>,
    /// The observation rows of the trucks when the stage began, `in_play`.
    pub in_play_before: Vec<(String, Option<bool>)>,
    /// Gun presses queued through the cockpit action queue.
    pub fire_presses: u64,
    /// `(truck, frame)` the damage authority first listed as destroyed.
    pub destroyed: Vec<(String, u32)>,
    /// The observation rows of the trucks at the end, `in_play`.
    pub in_play_after: Vec<(String, Option<bool>)>,
    /// The script frame the last truck was destroyed in.
    pub last_destroyed_frame: Option<u32>,
    /// Placements made.
    pub placements: u32,
    /// Whether both framebuffers arrived.
    pub frames_delivered: bool,
    /// The share of the intact frame that is off black, per mille.
    pub intact_permille: u32,
    /// The share of the destroyed frame that is off black, per mille.
    pub destroyed_permille: u32,
    /// The stage gave up on a truck.
    pub gave_up_on: Option<String>,
}

/// The stage's read-only world queries.
#[derive(SystemParam)]
pub struct ConvoyWorld<'w, 's> {
    player: Query<'w, 's, (), With<MissionPlayerBody>>,
    targets: Option<Res<'w, MissionGroundTargetReport>>,
    sweeps: Query<'w, 's, &'static MissionGroundTargetSweep>,
    damage: Option<Res<'w, DamageScheduleReport>>,
    observation: Option<Res<'w, ConvoyObservationRows>>,
}

/// The convoy's observation rows as the production writer built them last
/// frame: `(node, in_play)`, `None` for a node the writer gave no row.
#[derive(Resource, Default)]
pub struct ConvoyObservationRows(pub Vec<(String, Option<bool>)>);

/// Reads the convoy's observation rows through the production writer.
///
/// The mission host consumes [`MissionWorldObservation`] inside the fixed tick
/// that wrote it, so a `Last` system never finds it. This runs the very same
/// [`mission_world_observation`] writer, copies the convoy's rows and puts
/// the world back as it found it (the next fixed tick replaces the port
/// before it is consumed anyway).
pub fn sample_convoy_observation(world: &mut World) {
    if world
        .get_resource::<SmokeConfig>()
        .is_none_or(|config| config.mode != super::MissionSmokeMode::ConvoyKill)
    {
        return;
    }
    let prior = world.remove_resource::<MissionWorldObservation>();
    mission_world_observation(world);
    let written = world.remove_resource::<MissionWorldObservation>();
    if let Some(prior) = prior {
        world.insert_resource(prior);
    }
    let Some(written) = written else {
        return;
    };
    let rows = TRUCKS
        .iter()
        .map(|node| {
            let row = written
                .observation
                .members()
                .get(&member_chain([*node]))
                .map(|row| row.in_play);
            ((*node).to_owned(), row)
        })
        .collect();
    world.insert_resource(ConvoyObservationRows(rows));
}

/// The stage's system: places the player, fires, photographs.
#[allow(clippy::too_many_arguments)]
pub fn smoke_convoy_kill(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    mut pending: ResMut<PendingPlacement>,
    actions: Option<ResMut<MissionCockpitActions>>,
    shots: Res<SmokeShots>,
    frames: Res<FlightCheckFrames>,
    mut commands: Commands,
    clear: Option<Res<ClearColor>>,
    world: ConvoyWorld,
) {
    if config.mode != super::MissionSmokeMode::ConvoyKill
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
    let Some(targets) = world.targets.as_deref() else {
        return;
    };
    let mut metrics = data.measurements.convoy_kill.take().unwrap_or_default();
    let in_play = |node: &str| {
        world
            .observation
            .as_deref()
            .and_then(|rows| rows.0.iter().find(|(name, _)| name == node))
            .and_then(|(_, row)| *row)
    };
    if metrics.start_frame.is_none() {
        for node in TRUCKS {
            let Some(bound) = targets.target(node) else {
                continue;
            };
            let centre = world
                .sweeps
                .get(bound.entity)
                .map_or(bound.position_m, |sweep| sweep.centre_m);
            metrics.actors.push((node.to_owned(), bound.actor));
            metrics
                .centres
                .push((node.to_owned(), centre.map(|c| c as f32)));
            metrics
                .in_play_before
                .push((node.to_owned(), in_play(node)));
        }
        metrics.start_frame = Some(frame);
    }
    let destroyed_now: Vec<String> = metrics
        .actors
        .iter()
        .filter(|(_, actor)| {
            world
                .damage
                .as_deref()
                .is_some_and(|report| report.destroyed.contains(actor))
        })
        .map(|(node, _)| node.clone())
        .collect();
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
    metrics.in_play_after = TRUCKS
        .iter()
        .filter(|node| metrics.actors.iter().any(|(n, _)| n == *node))
        .map(|node| ((*node).to_owned(), in_play(node)))
        .collect();

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
                .join(format!("frame_ck{frame:05}_{label}.png")),
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
        let (intact, destroyed) = (find("convoy_intact"), find("convoy_destroyed"));
        let asked = metrics
            .last_destroyed_frame
            .map_or(frame, |at| at + SETTLE_FRAMES);
        if (intact.is_none() || destroyed.is_none()) && metrics.gave_up_on.is_none() {
            phase_end = frame >= asked + SHOT_GRACE_FRAMES;
        }
        if phase_end {
            let permille = |shot: Option<&_>| {
                shot.map_or(0, |f: &super::RawFrame| {
                    non_black_permille(&f.data, f.width, f.height)
                })
            };
            metrics.frames_delivered = intact.is_some() && destroyed.is_some();
            metrics.intact_permille = permille(intact);
            metrics.destroyed_permille = permille(destroyed);
        }
    }
    data.measurements.convoy_kill = Some(metrics);
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

/// The stage's state machine, apart from the Bevy plumbing.
fn step(
    metrics: &mut ConvoyKillMetrics,
    pending: &mut PendingPlacement,
    actions: &mut MissionCockpitActions,
    frame: u32,
    destroyed_now: &[String],
) -> Step {
    let mut step = Step {
        shots: Vec::new(),
        finished: false,
    };
    let Some(start) = metrics.start_frame else {
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
    let standing: Vec<&(String, [f32; 3])> = metrics
        .centres
        .iter()
        .filter(|(node, _)| !metrics.destroyed.iter().any(|(n, _)| n == node))
        .collect();
    if metrics.centres.is_empty() {
        // The convoy never bound: nothing to shoot.
        step.finished = true;
        return step;
    }
    let side = Vec3::from_array(FROM_SIDE).normalize();
    if let Some((node, centre)) = standing.first() {
        if at > PER_TRUCK_BUDGET_FRAMES * metrics.centres.len() as u32 {
            metrics.gave_up_on = Some(node.clone());
            step.finished = true;
            return step;
        }
        // --- 1. the mark, 2. the guns key -----------------------------------
        if at.is_multiple_of(PLACE_EVERY_FRAMES) {
            let offset = side * STANDOFF_M;
            pending.0 = Some(Placement {
                name: format!("convoy-kill mark at {node} (test placement)"),
                position_m: (Vec3::from_array(*centre) + offset).to_array(),
                yaw: yaw_toward(-offset),
                pitch: -offset.y.atan2(offset.x.hypot(offset.z)),
                speed_m_s: 0.0,
                distance_m: f64::from(STANDOFF_M),
            });
            metrics.placements += 1;
        }
        if at == INTACT_SHOT_OFFSET && metrics.destroyed.is_empty() {
            step.shots.push("convoy_intact");
        }
        // The guns stay quiet until the intact frame has been rendered.
        if at >= INTACT_SHOT_OFFSET + FIRE_AFTER_SHOT_FRAMES {
            actions
                .actions
                .push(Action::Flight(FlightCommand::FirePrimary));
            metrics.fire_presses += 1;
        }
        return step;
    }
    // --- 3. the vantage over the destroyed convoy ----------------------------
    let last = metrics.last_destroyed_frame.unwrap_or(frame);
    let since = frame - last;
    if since == 0 || since.is_multiple_of(PLACE_EVERY_FRAMES) {
        let sum = metrics
            .centres
            .iter()
            .fold(Vec3::ZERO, |acc, (_, c)| acc + Vec3::from_array(*c));
        let middle = sum / metrics.centres.len() as f32;
        let offset = side * VANTAGE_M;
        pending.0 = Some(Placement {
            name: "convoy vantage (test placement)".to_owned(),
            position_m: (middle + offset).to_array(),
            yaw: yaw_toward(-offset),
            pitch: -offset.y.atan2(offset.x.hypot(offset.z)),
            speed_m_s: 0.0,
            distance_m: f64::from(VANTAGE_M),
        });
        metrics.placements += 1;
    }
    if since == SETTLE_FRAMES {
        step.shots.push("convoy_destroyed");
    }
    step.finished = since >= SETTLE_FRAMES;
    step
}

/// The convoy-kill checks: the guns key destroyed every truck through the
/// damage authority, the observation followed, and the frames are drawn.
pub fn evaluate(m: &SmokeMeasurements) -> Vec<Check> {
    let Some(c) = &m.convoy_kill else {
        return vec![Check {
            id: "convoy_destroyed_by_fire_key".to_owned(),
            applies: true,
            passed: false,
            detail: "the run never reached the convoy-kill stage".to_owned(),
        }];
    };
    let all_destroyed = !c.actors.is_empty()
        && c.actors
            .iter()
            .all(|(node, _)| c.destroyed.iter().any(|(n, _)| n == node));
    let held_before =
        !c.in_play_before.is_empty() && c.in_play_before.iter().all(|(_, row)| *row == Some(true));
    let gone_after =
        !c.in_play_after.is_empty() && c.in_play_after.iter().all(|(_, row)| *row == Some(false));
    vec![
        Check {
            id: "convoy_destroyed_by_fire_key".to_owned(),
            applies: true,
            passed: all_destroyed && c.fire_presses > 0 && c.gave_up_on.is_none(),
            detail: format!(
                "{} guns-key press(es) were queued from {:?} m stand-off marks (test \
                 placements, {} made); the damage authority listed {:?} destroyed (the smoke \
                 writes no damage); gave up on {:?}",
                c.fire_presses, STANDOFF_M, c.placements, c.destroyed, c.gave_up_on
            ),
        },
        Check {
            id: "convoy_observation_follows".to_owned(),
            applies: true,
            passed: held_before && gone_after,
            detail: format!(
                "the mission observation rows read {:?} before the first shot and {:?} at the \
                 end (in_play)",
                c.in_play_before, c.in_play_after
            ),
        },
        Check {
            id: "convoy_frames_drawn".to_owned(),
            applies: true,
            passed: c.frames_delivered
                && c.intact_permille >= MIN_CONVOY_DRAWN_PERMILLE
                && c.destroyed_permille >= MIN_CONVOY_DRAWN_PERMILLE,
            detail: if c.frames_delivered {
                format!(
                    "{} per mille of the intact frame and {} per mille of the destroyed frame \
                     are off black (floor {})",
                    c.intact_permille, c.destroyed_permille, MIN_CONVOY_DRAWN_PERMILLE
                )
            } else {
                "the intact or destroyed framebuffer never arrived".to_owned()
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

/// The convoy-kill measurements as the JSON object `report.json` carries.
pub fn convoy_kill_json(metrics: Option<&ConvoyKillMetrics>) -> String {
    let Some(c) = metrics else {
        return "null".to_owned();
    };
    let option_u32 =
        |value: Option<u32>| value.map_or_else(|| "null".to_owned(), |v| v.to_string());
    let rows = |rows: &[(String, Option<bool>)]| {
        rows.iter()
            .map(|(node, row)| {
                let row = row.map_or_else(|| "null".to_owned(), |v| v.to_string());
                format!("{{\"node\":{},\"in_play\":{row}}}", json_string(node))
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let destroyed = c
        .destroyed
        .iter()
        .map(|(node, frame)| format!("{{\"node\":{},\"frame\":{frame}}}", json_string(node)))
        .collect::<Vec<_>>()
        .join(",");
    let gave_up = c
        .gave_up_on
        .as_deref()
        .map_or_else(|| "null".to_owned(), json_string);
    format!(
        "{{\"start_frame\":{},\"trucks\":{},\"in_play_before\":[{}],\"fire_presses\":{},\
         \"destroyed\":[{destroyed}],\"in_play_after\":[{}],\"last_destroyed_frame\":{},\
         \"placements\":{},\"frames_delivered\":{},\"intact_permille\":{},\
         \"destroyed_permille\":{},\"gave_up_on\":{gave_up}}}",
        option_u32(c.start_frame),
        c.actors.len(),
        rows(&c.in_play_before),
        c.fire_presses,
        rows(&c.in_play_after),
        option_u32(c.last_destroyed_frame),
        c.placements,
        c.frames_delivered,
        c.intact_permille,
        c.destroyed_permille,
    )
}
