//! The truck stage of the owner-command smoke (task #1926), `--smoke-trucks`.
//!
//! M01's convoy trucks (`t_truck01`..`t_truck03`) start asleep; the mission
//! script's own `WAKEUP_TURRETS` (OBJECTIVE6) wakes them. The run goes through
//! the briefing, the Flight Check and the launch cinematic like every windowed
//! run, then:
//!
//! 1. once the player has control for a moment it **walks the declared
//!    objective route**: it places the player on each declared target in turn
//!    (test placements, labelled in `report.json`) and leaves everything else
//!    to the production host. It never wakes a turret itself: the ledger's
//!    `applied()` count of consumed wake emissions is the evidence that the
//!    script did it;
//! 2. once the host reports all three trucks live it **places the player** in
//!    their range, heading at the first truck, and re-places it every
//!    [`HOLD_EVERY_FRAMES`] frames;
//! 3. it photographs the trucks awake, waits for the turret report to count
//!    fire orders and for the player's hull to drop, and photographs the
//!    trucks firing.
//!
//! The floors are designed and named next to their constants. A run is never
//! `human_play`, and nothing it measures is `verified_original`; the frames are
//! for putting beside the Drydock originals by eye.

use bevy::ecs::system::SystemParam;
use bevy::prelude::{ClearColor, Commands, Query, Res, ResMut, Vec3, With};

use super::super::compose::MissionPlayerBody;
use super::super::host::MissionHost;
use super::super::player_victim::PlayerVictim;
use super::super::turret_fire::MissionTurretFire;
use super::super::weapons::MISSION_TARGET_NODE;
use super::{
    Check, FlightCheckFrames, PendingPlacement, Phase, Placement, SHOT_GRACE_FRAMES, SmokeConfig,
    SmokeData, SmokeMeasurements, SmokeShots, clear_colour_encodings, json_string,
    non_black_permille, request_screenshot, yaw_toward,
};
use crate::mission_session::MissionStage;
use crate::weapons::SessionDamageResolver;

/// The trucks OBJECTIVE6 wakes, by world node name.
pub const TRUCKS: [&str; 3] = ["t_truck01", "t_truck02", "t_truck03"];
/// Frames after the player gains control that the route walk starts.
pub const ROUTE_AFTER_CONTROL_FRAMES: u32 = 60;
/// Frames the player stays on each route stop. **Designed:** enough
/// simulation ticks for the host to observe the stop.
pub const ROUTE_STOP_FRAMES: u32 = 30;
/// Frames the route walk may take before the run gives up on the wake.
pub const ROUTE_BUDGET_FRAMES: u32 = 7_200;
/// Where the player holds, relative to the first truck, metres (east, up).
/// **Designed:** inside any truck's detection range.
pub const HOLD_OFFSET_M: [f32; 2] = [100.0, 40.0];
/// The player is put back on its hold point this often. **Designed.**
pub const HOLD_EVERY_FRAMES: u32 = 20;
/// Frames after the hold began that the "awake" frame is taken.
const AWAKE_SHOT_OFFSET: u32 = 90;
/// Frames after the first fire order that the "firing" frame is taken.
const FIRING_SHOT_OFFSET: u32 = 20;
/// Frames the run waits for fire and a hull drop after the hold began.
const ENGAGE_BUDGET_FRAMES: u32 = 1_800;
/// The least share of a truck frame that must be off black, in parts per
/// thousand. **Designed:** the world fills the window.
pub const MIN_TRUCKS_DRAWN_PERMILLE: u32 = 300;

/// What the truck stage measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrucksMetrics {
    /// The script frame the route walk began in.
    pub route_frame: Option<u32>,
    /// Route stops placed.
    pub stops: u32,
    /// The script frame all three trucks first read live in.
    pub woke_frame: Option<u32>,
    /// Which trucks read live at the end of the stage.
    pub live: Vec<String>,
    /// `WAKEUP_TURRETS` emissions the mission host consumed.
    pub wake_emissions: usize,
    /// The names those wakes matched, verbatim.
    pub wake_names: Vec<String>,
    /// The first truck's composed position, metres.
    pub truck_m: Option<[f32; 3]>,
    /// The script frame the hold began in.
    pub hold_frame: Option<u32>,
    /// Fire orders handed to the weapon publication.
    pub cannon_orders: u64,
    /// Shots the turret report logged.
    pub shots: usize,
    /// Shooters the report named, deduplicated.
    pub shooters: Vec<String>,
    /// The player's hull when the hold began.
    pub start_hull: Option<f64>,
    /// The lowest hull read since.
    pub min_hull: Option<f64>,
    /// The script frame the first fire order was seen in.
    pub fire_frame: Option<u32>,
    /// Whether both framebuffers arrived.
    pub frames_delivered: bool,
    /// The share of the awake frame that is off black, per mille.
    pub awake_permille: u32,
    /// The share of the firing frame that is off black, per mille.
    pub firing_permille: u32,
}

/// The stage's read-only world queries.
#[derive(SystemParam)]
pub struct TrucksWorld<'w, 's> {
    player: Query<'w, 's, (), With<MissionPlayerBody>>,
    victim: Option<Res<'w, PlayerVictim>>,
    resolver: Option<Res<'w, SessionDamageResolver>>,
    stage: Option<Res<'w, MissionStage>>,
    fire: Option<Res<'w, MissionTurretFire>>,
}

/// The stage's system: walks the route, holds in range, photographs.
#[allow(clippy::too_many_arguments)]
pub fn smoke_trucks(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    mut pending: ResMut<PendingPlacement>,
    host: Option<Res<MissionHost>>,
    shots: Res<SmokeShots>,
    frames: Res<FlightCheckFrames>,
    mut commands: Commands,
    clear: Option<Res<ClearColor>>,
    world: TrucksWorld,
) {
    if config.mode != super::MissionSmokeMode::Trucks
        || matches!(data.phase, Phase::Final | Phase::Ended)
    {
        return;
    }
    let frame = data.frame;
    if frame == 0 || world.player.is_empty() {
        return;
    }
    let (Some(host), Some(stage)) = (host.as_deref(), world.stage.as_deref()) else {
        return;
    };
    let Some(control_frame) = data
        .measurements
        .first_control
        .as_ref()
        .map(|control| control.frame)
    else {
        return;
    };
    if frame < control_frame + ROUTE_AFTER_CONTROL_FRAMES {
        return;
    }
    if data.measurements.trucks.is_none() {
        data.measurements.trucks = Some(TrucksMetrics::default());
    }
    let ledger = host.wake_turrets();
    let live: Vec<String> = TRUCKS
        .iter()
        .filter(|name| ledger.entry(name).is_some_and(|entry| entry.live))
        .map(|name| (*name).to_owned())
        .collect();
    let all_live = live.len() == TRUCKS.len();
    let truck_m = stage
        .host
        .turrets
        .entry(TRUCKS[0])
        .and_then(|entry| entry.position_m)
        .map(|p| p.map(|c| c as f32));
    let hull = world
        .victim
        .as_deref()
        .zip(world.resolver.as_deref())
        .and_then(|(victim, resolver)| {
            let key = cs_sim::damage::DamageNodeKey::new(MISSION_TARGET_NODE).ok()?;
            resolver.resolver().remaining_integrity(&victim.actor, &key)
        });
    let report = world.fire.as_deref().map(MissionTurretFire::report);
    let speed = data.speed_m_s;
    let mut metrics = data.measurements.trucks.take().unwrap_or_default();
    metrics.live = live;
    metrics.wake_emissions = ledger.applied();
    metrics.wake_names = ledger.wakes().iter().map(|w| w.name.clone()).collect();
    metrics.truck_m = truck_m;
    if let Some(report) = report {
        metrics.cannon_orders = report.cannon_orders;
        metrics.shots = report.shots.len();
        let mut shooters: Vec<String> = report.shots.iter().map(|s| s.name.clone()).collect();
        shooters.sort();
        shooters.dedup();
        metrics.shooters = shooters;
    }

    let result = step(
        &config,
        frame,
        &mut metrics,
        &mut pending,
        &stage.host.targets,
        speed,
        all_live,
        hull,
    );
    let wanted = result.shots;
    let clear_colours = clear.as_deref().map(clear_colour_encodings);
    let mut phase_end = result.finished;
    for label in &wanted {
        data.shots_requested += 1;
        data.requested.push((*label).to_owned());
        request_screenshot(
            &mut commands,
            shots.clone(),
            config
                .request
                .capture_dir
                .join(format!("frame_tr{frame:05}_{label}.png")),
            (*label).to_owned(),
            clear_colours,
            None,
            Some(frames.clone()),
        );
    }
    if phase_end {
        // The firing frame is the last one asked for; wait for it.
        let raw = frames
            .0
            .lock()
            .map(|frames| frames.clone())
            .unwrap_or_default();
        let find = |label: &str| raw.iter().find(|frame| frame.label == label);
        let (awake, firing) = (find("trucks_awake"), find("trucks_firing"));
        let waited = frame
            >= metrics
                .fire_frame
                .unwrap_or(frame)
                .saturating_add(FIRING_SHOT_OFFSET + SHOT_GRACE_FRAMES);
        if awake.is_none() || firing.is_none() {
            phase_end = waited;
        }
        if phase_end {
            let permille = |shot: Option<&_>| {
                shot.map_or(0, |f: &super::RawFrame| {
                    non_black_permille(&f.data, f.width, f.height)
                })
            };
            metrics.frames_delivered = awake.is_some() && firing.is_some();
            metrics.awake_permille = permille(awake);
            metrics.firing_permille = permille(firing);
        }
    }
    data.measurements.trucks = Some(metrics);
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
#[allow(clippy::too_many_arguments)]
fn step(
    _config: &SmokeConfig,
    frame: u32,
    metrics: &mut TrucksMetrics,
    pending: &mut PendingPlacement,
    targets: &crate::mission_targets::MissionTargetBinding,
    speed_m_s: f32,
    all_live: bool,
    hull: Option<f64>,
) -> Step {
    let mut step = Step {
        shots: Vec::new(),
        finished: false,
    };
    let route_frame = *metrics.route_frame.get_or_insert(frame);
    if metrics.woke_frame.is_none() && all_live {
        metrics.woke_frame = Some(frame);
    }
    let Some(_) = metrics.woke_frame else {
        // --- 1. walking the route -------------------------------------------
        let stops: Vec<([f64; 3], String)> = targets
            .declared_targets()
            .map(|target| (target.position_m, target.node.clone()))
            .collect();
        let since = frame - route_frame;
        if stops.is_empty() || since > ROUTE_BUDGET_FRAMES {
            step.finished = true;
            return step;
        }
        if since.is_multiple_of(ROUTE_STOP_FRAMES) {
            let index = (since / ROUTE_STOP_FRAMES) as usize % stops.len();
            let (position, node) = &stops[index];
            pending.0 = Some(Placement {
                name: format!("route stop {node} (test placement)"),
                position_m: position.map(|c| c as f32),
                yaw: 0.0,
                pitch: 0.0,
                speed_m_s,
                distance_m: 0.0,
            });
            metrics.stops += 1;
        }
        return step;
    };

    // --- 2. holding in range, 3. photographs ---------------------------------
    let Some(truck) = metrics.truck_m else {
        step.finished = true;
        return step;
    };
    let hold_frame = *metrics.hold_frame.get_or_insert(frame);
    let at = frame - hold_frame;
    if at.is_multiple_of(HOLD_EVERY_FRAMES) {
        let offset = Vec3::new(HOLD_OFFSET_M[0], HOLD_OFFSET_M[1], 0.0);
        pending.0 = Some(Placement {
            name: format!("truck hold at {} (test placement)", TRUCKS[0]),
            position_m: (Vec3::from_array(truck) + offset).to_array(),
            yaw: yaw_toward(-offset),
            pitch: (-offset.y).atan2(offset.x),
            speed_m_s,
            distance_m: f64::from(offset.length()),
        });
    }
    if let Some(now) = hull {
        metrics.start_hull.get_or_insert(now);
        metrics.min_hull = Some(metrics.min_hull.map_or(now, |low| low.min(now)));
    }
    if at == AWAKE_SHOT_OFFSET {
        step.shots.push("trucks_awake");
    }
    if metrics.fire_frame.is_none() && metrics.cannon_orders > 0 && at > AWAKE_SHOT_OFFSET {
        metrics.fire_frame = Some(frame);
    }
    if let Some(fired) = metrics.fire_frame {
        if frame == fired + FIRING_SHOT_OFFSET {
            step.shots.push("trucks_firing");
        }
        step.finished = frame >= fired + FIRING_SHOT_OFFSET;
    } else if at > ENGAGE_BUDGET_FRAMES {
        step.finished = true;
    }
    step
}

/// The truck checks: the script woke the trucks, they fired through the
/// turret runtime, the hull dropped, and the frames are drawn.
pub fn evaluate(m: &SmokeMeasurements) -> Vec<Check> {
    let Some(t) = &m.trucks else {
        return vec![Check {
            id: "trucks_woken_by_script".to_owned(),
            applies: true,
            passed: false,
            detail: "the run never reached the truck stage".to_owned(),
        }];
    };
    let hull_dropped = t
        .start_hull
        .zip(t.min_hull)
        .is_some_and(|(start, low)| low < start);
    vec![
        Check {
            id: "trucks_woken_by_script".to_owned(),
            applies: true,
            passed: t.woke_frame.is_some() && t.wake_emissions > 0 && t.live.len() == TRUCKS.len(),
            detail: format!(
                "the player was placed on {} declared route stop(s) from frame {:?} (test \
                 placements; no turret was woken by the smoke); the mission host consumed {} \
                 wake emission(s) matching {:?} and {:?} read live, all three at frame {:?}",
                t.stops, t.route_frame, t.wake_emissions, t.wake_names, t.live, t.woke_frame
            ),
        },
        Check {
            id: "trucks_fire_orders".to_owned(),
            applies: true,
            passed: t.cannon_orders > 0
                && t.shots > 0
                && t.shooters
                    .iter()
                    .all(|name| TRUCKS.contains(&name.as_str())),
            detail: format!(
                "the turret report counted {} cannon order(s) and {} shot(s) from {:?} after \
                 the player was placed {:?} m east/up of {} (a test placement)",
                t.cannon_orders, t.shots, t.shooters, HOLD_OFFSET_M, TRUCKS[0]
            ),
        },
        Check {
            id: "trucks_hull_drops".to_owned(),
            applies: true,
            passed: hull_dropped,
            detail: format!(
                "the player's hull read {:?} when the hold began and {:?} at its lowest",
                t.start_hull, t.min_hull
            ),
        },
        Check {
            id: "trucks_frames_drawn".to_owned(),
            applies: true,
            passed: t.frames_delivered
                && t.awake_permille >= MIN_TRUCKS_DRAWN_PERMILLE
                && t.firing_permille >= MIN_TRUCKS_DRAWN_PERMILLE,
            detail: if t.frames_delivered {
                format!(
                    "{} per mille of the awake frame and {} per mille of the firing frame are \
                     off black (floor {})",
                    t.awake_permille, t.firing_permille, MIN_TRUCKS_DRAWN_PERMILLE
                )
            } else {
                "the awake or firing framebuffer never arrived".to_owned()
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

/// The truck measurements as the JSON object `report.json` carries.
pub fn trucks_json(metrics: Option<&TrucksMetrics>) -> String {
    let Some(t) = metrics else {
        return "null".to_owned();
    };
    let option_u32 =
        |value: Option<u32>| value.map_or_else(|| "null".to_owned(), |v| v.to_string());
    let option_f64 = |value: Option<f64>| {
        value
            .filter(|v| v.is_finite())
            .map_or_else(|| "null".to_owned(), |v| format!("{v:.4}"))
    };
    let strings = |values: &[String]| {
        values
            .iter()
            .map(|v| json_string(v))
            .collect::<Vec<_>>()
            .join(",")
    };
    let truck = t.truck_m.map_or_else(
        || "null".to_owned(),
        |p| format!("[{:.1},{:.1},{:.1}]", p[0], p[1], p[2]),
    );
    format!(
        "{{\"route_frame\":{},\"stops\":{},\"woke_frame\":{},\"live\":[{}],\
         \"wake_emissions\":{},\"wake_names\":[{}],\"truck_m\":{truck},\"hold_frame\":{},\
         \"cannon_orders\":{},\"shots\":{},\"shooters\":[{}],\"start_hull\":{},\
         \"min_hull\":{},\"fire_frame\":{},\"frames_delivered\":{},\"awake_permille\":{},\
         \"firing_permille\":{}}}",
        option_u32(t.route_frame),
        t.stops,
        option_u32(t.woke_frame),
        strings(&t.live),
        t.wake_emissions,
        strings(&t.wake_names),
        option_u32(t.hold_frame),
        t.cannon_orders,
        t.shots,
        strings(&t.shooters),
        option_f64(t.start_hull),
        option_f64(t.min_hull),
        option_u32(t.fire_frame),
        t.frames_delivered,
        t.awake_permille,
        t.firing_permille,
    )
}
