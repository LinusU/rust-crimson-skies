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
//! Part B (#2082) adds two beats after it:
//!
//! the **landing beat** — the player is placed at the composed `do_approach1`
//! drop-off node (level, at a speed inside the landing rows' `[50, 320]`
//! window, re-applied every [`PLACE_EVERY_FRAMES`] frames) until the host
//! reports `OBJECTIVE21` (`texdrop` EXECUTED) completed; `OBJECTIVE22` and,
//! ten seconds later, `OBJECTIVE23` (which wakes `medkestrel_1..3`) follow;
//!
//! the **Kestrel beat** — the player is placed behind each Kestrel still
//! standing, nose on it, and presses the guns key through the production
//! cockpit-action queue every frame; a Kestrel is down only when the damage
//! authority says so (the smoke writes no damage).
//!
//! Part C (#2083) adds the end of the route:
//!
//! the **tank beat** — the player is placed abeam `hydrogentank1` and presses
//! the guns key until the host reports `OBJECTIVE25` (`hydrogen_fire` RUNNING,
//! started by the tank record's `HEALTH` pool emptying,
//! `2026-10-24-m01-animation-health-consumer.md`) completed; that wakes
//! `OBJECTIVE26`, which `OBJECTIVE13` (group 1 down) needs awake;
//!
//! the **dock beat** — once `OBJECTIVE14` has armed `pz_manual_land` the player
//! is placed on a level pass through it until the host reports `OBJECTIVE14`
//! (`hooked_to_klondike` EXECUTED) completed;
//!
//! the **settle beat** — the run waits for the mission host's own terminal
//! (`OBJECTIVE15`'s `INSTANTWIN`: outcome Success, source ControlProgram) and
//! photographs the screen after it. The smoke writes no flag of its own.
//!
//! What the mission host reports as completed is accumulated every fixed tick
//! by [`record_success_completions`] (the host's per-tick report is replaced by
//! the next tick). A run is never `human_play`, and nothing it measures is
//! `verified_original`.

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::ecs::system::SystemParam;
use bevy::prelude::{ClearColor, Commands, Query, Res, ResMut, Resource, Vec3, With, World};
use cs_sim::damage::ActorId;
use cs_types::input::{Action, FlightCommand};

use super::super::compose::MissionPlayerBody;
use super::super::host::{
    MissionExitHeld, MissionHost, MissionHostReport, MissionWorldObservation,
};
use super::super::observation::mission_world_observation;
use super::super::weapons::MissionCockpitActions;
use super::{
    Check, FlightCheckFrames, PendingPlacement, Phase, Placement, SHOT_GRACE_FRAMES, SmokeConfig,
    SmokeData, SmokeMeasurements, SmokeShots, clear_colour_encodings, json_string,
    non_black_permille, request_screenshot, yaw_toward,
};
use crate::mission_actors::{MissionActorSpawnReport, MissionGroundTarget};
use crate::weapons::SessionDamageResolver;
use crate::world_facts::member_chain;

/// The objective site the proximity trigger names (`OBJECTIVE12`'s node).
pub const ANCHOR: &str = "shipwreck";
/// `OBJECTIVE12` — the `TRAVELERS` proximity site (block index 11).
pub const PROXIMITY_BLOCK: u32 = 11;
/// `OBJECTIVE27` — woken two seconds after block 12 completes; it marks the
/// site (block index 26).
pub const MARK_BLOCK: u32 = 26;
/// `OBJECTIVE21` — `texdrop` EXECUTED, the drop-off landing (block index 20).
pub const LANDING_BLOCK: u32 = 20;
/// `OBJECTIVE23` — woken by block 22 ten seconds after the landing; it wakes
/// the Kestrels (block index 22).
pub const KESTREL_WAKE_BLOCK: u32 = 22;
/// The drop-off node the landing beat flies to. Any of `do_approach1..6`
/// satisfies the rows; the first is used.
pub const LANDING_NODE: &str = "do_approach1";
/// The Kestrels of group 1, by `aiv.zrd` record name, in the order they are
/// shot down.
pub const KESTRELS: [&str; 3] = ["medkestrel_1", "medkestrel_2", "medkestrel_3"];
/// The player's level speed through the landing, metres per second.
/// **Designed:** inside the landing rows' `speed [50, 320]` window.
pub const LANDING_SPEED_M_S: f32 = 100.0;
/// The horizontal distance behind the node the player is placed, metres.
/// **Designed:** inside the landing radius.
pub const LANDING_OFFSET_M: f32 = 60.0;
/// Frames the landing beat waits for `OBJECTIVE21`. **Designed.**
pub const LANDING_BUDGET_FRAMES: u32 = 1_500;
/// Frames after the landing that the run waits for `OBJECTIVE23` (ten
/// seconds of nap, then the tick that runs it). **Designed.**
pub const KESTREL_WAKE_BUDGET_FRAMES: u32 = 1_500;
/// The stand-off behind a Kestrel, metres. **Designed:** the gun-hit
/// distance the convoy stage uses.
pub const KESTREL_STANDOFF_M: f32 = 60.0;
/// The player is put back behind its Kestrel this often. **Designed:** the
/// target flies, so the mark follows it closely.
pub const KESTREL_PLACE_EVERY_FRAMES: u32 = 2;
/// Frames the run may spend on one Kestrel before giving up. **Designed.**
pub const PER_KESTREL_BUDGET_FRAMES: u32 = 1_800;
/// Frames after the Kestrel beat starts that the guns key goes down.
/// **Designed:** the frame of the standing Kestrels is rendered first.
const KESTREL_FIRE_AFTER_FRAMES: u32 = 30;
/// Frames after the last Kestrel died that the closing frame is taken.
/// **Designed:** time for debris to appear.
pub const KESTREL_SETTLE_FRAMES: u32 = 60;
/// Frames after the landing that the "landed" frame is taken. **Designed.**
const LANDED_SHOT_OFFSET: u32 = 60;
/// `OBJECTIVE25` — `hydrogen_fire` RUNNING (block index 24).
pub const HYDROGEN_BLOCK: u32 = 24;
/// `OBJECTIVE13` — all of group 1 down, with `OBJECTIVE26` awake (block index 12).
pub const GROUP_CLEAR_BLOCK: u32 = 12;
/// `OBJECTIVE14` — `hooked_to_klondike` EXECUTED, the docking (block index 13).
pub const DOCK_BLOCK: u32 = 13;
/// `OBJECTIVE15` — the block that carries `INSTANTWIN` (block index 14). The
/// control lowering numbers a block's symbol by its index
/// (`control_lowering.rs`), so the terminal's `requested_by` is this number.
pub const WIN_BLOCK: u32 = 14;
/// The hydrogen tank the stage shoots; the first to go starts all four.
pub const TANK: &str = "hydrogentank1";
/// The docking node the stage flies through (`piratezep`'s manual landing).
pub const DOCK_NODE: &str = "pz_manual_land";
/// The horizontal distance the player stands off from the tank, metres.
/// **Designed:** the gun-hit distance of the Kestrel beat.
pub const TANK_STANDOFF_M: f32 = 60.0;
/// How far below the tank the player stands, metres. **Designed:** under the
/// girders that frame the tanks on the zeppelin's underside.
pub const TANK_BELOW_M: f32 = 45.0;
/// The player is put back abeam the tank this often. **Designed.**
pub const TANK_PLACE_EVERY_FRAMES: u32 = 2;
/// Frames after the tank beat starts that the guns key goes down. **Designed.**
const TANK_FIRE_AFTER_FRAMES: u32 = 30;
/// Frames the tank beat may spend before giving up. **Designed.**
pub const TANK_BUDGET_FRAMES: u32 = 1_800;
/// Frames after the tank beat starts that the tank under fire is photographed.
/// **Designed:** the guns have been down for a second.
const TANK_SHOT_OFFSET: u32 = TANK_FIRE_AFTER_FRAMES + 60;
/// Frames after `OBJECTIVE25` completes that the burning tank is photographed.
/// **Designed:** time for the fire to show.
const HYDROGEN_SHOT_OFFSET: u32 = 90;
/// Frames the dock beat waits for `pz_manual_land` to appear. **Designed:**
/// `OBJECTIVE14` arms it five seconds after `OBJECTIVE13`.
pub const DOCK_FIND_BUDGET_FRAMES: u32 = 2_400;
/// Frames the dock beat may spend flying passes. **Designed.**
pub const DOCK_BUDGET_FRAMES: u32 = 1_800;
/// Frames after the docking that the run waits for the terminal. **Designed.**
pub const TERMINAL_BUDGET_FRAMES: u32 = 9_000;
/// Frames after the terminal that the settled frame is taken. **Designed.**
const SETTLED_SHOT_OFFSET: u32 = 120;
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

/// Which beat the stage is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Beat {
    /// Placed at the shipwreck site (blocks 12 and 27).
    #[default]
    Proximity,
    /// Placed at the drop-off node (block 21, then 22 and 23).
    Landing,
    /// Placed behind each Kestrel, guns key down.
    Kestrels,
    /// Placed abeam the hydrogen tank, guns key down (block 25).
    Tank,
    /// Placed on a level pass through the docking node (block 14).
    Dock,
    /// Waiting for the host's terminal, then photographing it.
    Settle,
}

/// The terminal the mission host settled, as the stage read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuccessTerminal {
    /// The outcome's label.
    pub outcome: String,
    /// The source's label.
    pub source: String,
    /// The objective block (symbol number) that asked for it.
    pub requested_by: Option<u32>,
    /// The host tick it settled on.
    pub tick: u64,
    /// The process exit code the terminal asked for (held by the smoke).
    pub exit_code: i32,
    /// The script frame the stage first saw it in.
    pub frame: u32,
}

/// What the success stage measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SuccessMetrics {
    /// The script frame the stage began in.
    pub start_frame: Option<u32>,
    /// The site's measured position, metres.
    pub centre: Option<[f32; 3]>,
    /// Placements made at the shipwreck site.
    pub placements: u32,
    /// Every block the host's lifecycle report listed as completed, in order
    /// of first report, with the script frame it was first seen in.
    pub completed: Vec<(u32, u32)>,
    /// Whether every framebuffer arrived.
    pub frames_delivered: bool,
    /// The share of the near frame that is off black, per mille.
    pub near_permille: u32,
    /// The share of the marked frame that is off black, per mille.
    pub marked_permille: u32,
    /// The script frame the marked frame was asked for in.
    pub marked_shot_frame: Option<u32>,
    /// The stage never found the site and stopped waiting.
    pub site_never_found: bool,
    /// The beat the stage is in.
    pub beat: Beat,
    /// The script frame the landing beat began in.
    pub landing_start: Option<u32>,
    /// The drop-off node's composed position, metres.
    pub landing_at: Option<[f64; 3]>,
    /// Placements made at the drop-off node.
    pub landing_placements: u32,
    /// The host's landing ledger as last seen during the landing beat.
    pub landing_diag: String,
    /// The landing node never appeared in the observation.
    pub landing_node_never_found: bool,
    /// The script frame the Kestrel beat began in.
    pub kestrel_start: Option<u32>,
    /// Each Kestrel's damage actor, found when the beat began.
    pub kestrel_actors: Vec<(String, ActorId)>,
    /// Guns-key presses queued through the cockpit action queue.
    pub fire_presses: u64,
    /// Placements made behind a Kestrel.
    pub kestrel_placements: u32,
    /// `(Kestrel, frame)` the damage authority first listed as destroyed.
    pub kestrels_destroyed: Vec<(String, u32)>,
    /// The Kestrel the stage gave up on.
    pub gave_up_on: Option<String>,
    /// The beat ended without `OBJECTIVE23` completing.
    pub kestrels_never_woken: bool,
    /// The script frame the "landed" frame was asked for in.
    pub landed_shot_frame: Option<u32>,
    /// The script frame the closing Kestrel frame was asked for in.
    pub kestrels_shot_frame: Option<u32>,
    /// The share of the landed frame that is off black, per mille.
    pub landed_permille: u32,
    /// The share of the closing Kestrel frame that is off black, per mille.
    pub kestrels_permille: u32,
    /// The script frame the tank beat began in.
    pub tank_start: Option<u32>,
    /// The tank's measured position, metres.
    pub tank_at: Option<[f32; 3]>,
    /// Placements made abeam the tank.
    pub tank_placements: u32,
    /// Guns-key presses queued at the tank.
    pub tank_fire_presses: u64,
    /// The host's animation-health ledger as last seen during the tank beat.
    pub tank_diag: String,
    /// The tank never appeared as a ground target.
    pub tank_never_found: bool,
    /// The tank beat ended without `OBJECTIVE25` completing.
    pub tank_gave_up: bool,
    /// The script frame the tank-under-fire frame was asked for in.
    pub tank_shot_frame: Option<u32>,
    /// The share of the tank-under-fire frame that is off black, per mille.
    pub tank_permille: u32,
    /// The script frame the burning-tank frame was asked for in.
    pub hydrogen_shot_frame: Option<u32>,
    /// The share of the burning-tank frame that is off black, per mille.
    pub hydrogen_permille: u32,
    /// The script frame the dock beat began in.
    pub dock_start: Option<u32>,
    /// The docking node's latest composed position, metres.
    pub dock_at: Option<[f64; 3]>,
    /// Placements made at the docking node.
    pub dock_placements: u32,
    /// The host's landing ledger as last seen during the dock beat.
    pub dock_diag: String,
    /// The docking node never appeared in the observation.
    pub dock_node_never_found: bool,
    /// The dock beat ended without `OBJECTIVE14` completing.
    pub dock_gave_up: bool,
    /// The script frame the settle beat began in.
    pub settle_start: Option<u32>,
    /// The terminal the host settled.
    pub terminal: Option<SuccessTerminal>,
    /// The settle beat ended without a terminal.
    pub terminal_never: bool,
    /// The script frame the settled frame was asked for in.
    pub settled_shot_frame: Option<u32>,
    /// The share of the settled frame that is off black, per mille.
    pub settled_permille: u32,
}

impl SuccessMetrics {
    /// Whether the host reported `block` completed.
    #[must_use]
    pub fn has_completed(&self, block: u32) -> bool {
        self.completed.iter().any(|(b, _)| *b == block)
    }

    /// The script frame the host first reported `block` completed in.
    #[must_use]
    pub fn completed_at(&self, block: u32) -> Option<u32> {
        self.completed
            .iter()
            .find(|(b, _)| *b == block)
            .map(|(_, frame)| *frame)
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

/// The drop-off and docking nodes' composed positions as the production
/// observation writer reports them (`None` until the node has an in-play row).
#[derive(Resource, Default)]
pub struct SuccessNodeRow {
    /// `do_approach1`.
    pub landing: Option<[f64; 3]>,
    /// `pz_manual_land`.
    pub dock: Option<[f64; 3]>,
}

/// Reads the drop-off node through the production observation writer — the
/// same seam the convoy stage uses, because the host consumes the port inside
/// the tick that wrote it.
pub fn sample_success_node(world: &mut World) {
    if world
        .get_resource::<SmokeConfig>()
        .is_none_or(|config| config.mode != super::MissionSmokeMode::Success)
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
    let row = |name: &str| {
        written
            .observation
            .members()
            .get(&member_chain([name]))
            .filter(|row| row.in_play)
            .map(|row| row.position)
    };
    world.insert_resource(SuccessNodeRow {
        landing: row(LANDING_NODE),
        dock: row(DOCK_NODE),
    });
}

/// The stage's read-only world queries.
#[derive(SystemParam)]
pub struct SuccessWorld<'w, 's> {
    player: Query<'w, 's, (), With<MissionPlayerBody>>,
    grounds: Query<'w, 's, &'static MissionGroundTarget>,
    completions: Option<Res<'w, SuccessCompletions>>,
    node: Option<Res<'w, SuccessNodeRow>>,
    spawned: Option<Res<'w, MissionActorSpawnReport>>,
    bodies: Query<
        'w,
        's,
        (
            &'static Position,
            &'static Rotation,
            &'static LinearVelocity,
        ),
    >,
    resolver: Option<Res<'w, SessionDamageResolver>>,
    host: Option<Res<'w, MissionHost>>,
}

/// What the host's landing path says: the drop-off nodes armed so far, and
/// the landings and refusals on its ledger.
fn landing_diagnostic(host: &MissionHost, prefix: &str) -> String {
    let armed: Vec<&str> = host
        .armed_nodes()
        .iter()
        .flat_map(|armed| armed.nodes().iter().map(String::as_str))
        .filter(|node| node.starts_with(prefix))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let ledger = host.landings();
    format!(
        "armed {prefix}* nodes {armed:?}; player refusals {:?}; landings {:?}; refusals {:?}",
        host.animation()
            .refused()
            .filter(|record| record.identity().starts_with(prefix))
            .take(1)
            .map(|record| format!("{}: {:?}", record.identity(), record.refusals()))
            .collect::<Vec<_>>(),
        ledger
            .activated()
            .iter()
            .map(|l| format!(
                "{} -> {} at tick {:?}: {:?}, calls {:?}",
                l.node, l.anim, l.at, l.outcome, l.calls
            ))
            .collect::<Vec<_>>(),
        ledger
            .refused()
            .iter()
            .map(|r| format!("{}: {}", r.name, r.reason))
            .collect::<Vec<_>>()
    )
}

/// One Kestrel still standing, as the stage sees it this frame.
struct Standing {
    name: String,
    position: Vec3,
    rotation: bevy::prelude::Quat,
    velocity: Vec3,
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
        // The win's exit would end the run before the settled frame exists.
        commands.insert_resource(MissionExitHeld);
    }
    if let Some(done) = world.completions.as_deref() {
        for block in &done.0 {
            if !metrics.has_completed(*block) {
                metrics.completed.push((*block, frame));
            }
        }
    }
    if metrics.beat == Beat::Landing
        && let Some(host) = world.host.as_deref()
    {
        metrics.landing_diag = landing_diagnostic(host, "do_approach");
    }
    if metrics.beat == Beat::Landing && metrics.landing_at.is_none() {
        metrics.landing_at = world.node.as_deref().and_then(|row| row.landing);
    }
    if metrics.beat == Beat::Dock {
        if let Some(host) = world.host.as_deref() {
            metrics.dock_diag = landing_diagnostic(host, "pz_");
        }
        // The zeppelin moves: the pass follows the node's latest position.
        if let Some(now) = world.node.as_deref().and_then(|row| row.dock) {
            metrics.dock_at = Some(now);
        }
    }
    if metrics.beat == Beat::Tank
        && let Some(ledger) = world
            .host
            .as_deref()
            .and_then(|host| host.animation_health())
    {
        metrics.tank_diag = format!(
            "pools {:?}; starts {:?}",
            ledger
                .pools()
                .iter()
                .map(|pool| format!(
                    "{} on {}: {}/{}",
                    pool.animation(),
                    pool.node(),
                    pool.remaining(),
                    pool.maximum()
                ))
                .collect::<Vec<_>>(),
            ledger
                .starts()
                .iter()
                .map(|start| format!("{} at {:?}: {:?}", start.animation, start.at, start.outcome))
                .collect::<Vec<_>>()
        );
    }
    let tank = world
        .grounds
        .iter()
        .find(|ground| ground.node == TANK)
        .map(|ground| Vec3::from_array(ground.position_m.map(|v| v as f32)));
    let terminal = world
        .host
        .as_deref()
        .and_then(|host| host.terminal())
        .map(|terminal| SuccessTerminal {
            outcome: terminal.outcome.label().to_owned(),
            source: terminal.source.label().to_owned(),
            requested_by: terminal.requested_by.map(|symbol| symbol.0),
            tick: terminal.tick.0,
            exit_code: terminal.exit.code(),
            frame,
        });
    if metrics.beat == Beat::Kestrels
        && metrics.kestrel_actors.is_empty()
        && let Some(report) = world.spawned.as_deref()
    {
        for name in KESTRELS {
            if let Some(aircraft) = report.aircraft.iter().find(|a| a.record == name) {
                metrics
                    .kestrel_actors
                    .push((name.to_owned(), aircraft.actor));
            }
        }
    }
    // The authority's recorded lifecycle, not an observation row: a kill in an
    // earlier tick of the frame must not be lost.
    let destroyed_now: Vec<String> = metrics
        .kestrel_actors
        .iter()
        .filter(|(_, actor)| {
            world
                .resolver
                .as_deref()
                .is_some_and(|resolver| resolver.resolver().is_destroyed(actor))
        })
        .map(|(name, _)| name.clone())
        .collect();
    let standing: Vec<Standing> = metrics
        .kestrel_actors
        .iter()
        .filter(|(name, _)| !destroyed_now.contains(name))
        .filter_map(|(name, _)| {
            let aircraft = world
                .spawned
                .as_deref()?
                .aircraft
                .iter()
                .find(|a| a.record == *name)?;
            let (position, rotation, velocity) = world.bodies.get(aircraft.entity).ok()?;
            Some(Standing {
                name: name.clone(),
                position: position.0,
                rotation: rotation.0,
                velocity: velocity.0,
            })
        })
        .collect();
    let mut queue = MissionCockpitActions::default();
    let result = step(
        &mut metrics,
        &mut pending,
        &mut queue,
        frame,
        &Inputs {
            destroyed_now: &destroyed_now,
            standing: &standing,
            tank,
            terminal: terminal.as_ref(),
        },
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
        let wanted = [
            "success_near",
            "success_marked",
            "success_landed",
            "success_kestrels",
            "success_tank",
            "success_hydrogen",
            "success_settled",
        ];
        let asked = [
            metrics.marked_shot_frame,
            metrics.landed_shot_frame,
            metrics.kestrels_shot_frame,
            metrics.tank_shot_frame,
            metrics.hydrogen_shot_frame,
            metrics.settled_shot_frame,
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(frame);
        // A beat that never ran never asked for its frame; only the asked
        // frames are waited for.
        let required = |label: &str| match label {
            "success_landed" => metrics.landed_shot_frame.is_some(),
            "success_kestrels" => metrics.kestrels_shot_frame.is_some(),
            "success_tank" => metrics.tank_shot_frame.is_some(),
            "success_hydrogen" => metrics.hydrogen_shot_frame.is_some(),
            "success_settled" => metrics.settled_shot_frame.is_some(),
            _ => true,
        };
        let missing = wanted
            .iter()
            .any(|label| required(label) && find(label).is_none());
        if missing {
            phase_end = frame >= asked + SHOT_GRACE_FRAMES;
        }
        if phase_end {
            let permille = |shot: Option<&_>| {
                shot.map_or(0, |f: &super::RawFrame| {
                    non_black_permille(&f.data, f.width, f.height)
                })
            };
            metrics.frames_delivered = wanted.iter().all(|label| find(label).is_some());
            metrics.near_permille = permille(find("success_near"));
            metrics.marked_permille = permille(find("success_marked"));
            metrics.landed_permille = permille(find("success_landed"));
            metrics.kestrels_permille = permille(find("success_kestrels"));
            metrics.tank_permille = permille(find("success_tank"));
            metrics.hydrogen_permille = permille(find("success_hydrogen"));
            metrics.settled_permille = permille(find("success_settled"));
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

/// What the world shows the state machine this frame.
struct Inputs<'a> {
    /// Kestrels the damage authority lists as destroyed.
    destroyed_now: &'a [String],
    /// Kestrels with a live body, in [`KESTRELS`] order.
    standing: &'a [Standing],
    /// The hydrogen tank's ground-target position.
    tank: Option<Vec3>,
    /// The terminal the host has settled, if any.
    terminal: Option<&'a SuccessTerminal>,
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

/// The landing pass: level flight at [`LANDING_SPEED_M_S`] toward the node
/// from [`LANDING_OFFSET_M`] away, so the player is inside the landing radius.
fn landing_pass(node: [f64; 3]) -> Placement {
    pass(node, LANDING_NODE)
}

/// A level pass at [`LANDING_SPEED_M_S`] through `name` at `node`.
fn pass(node: [f64; 3], name: &str) -> Placement {
    let side = Vec3::new(FROM_SIDE[0], 0.0, FROM_SIDE[2]).normalize();
    let offset = side * LANDING_OFFSET_M;
    let target = Vec3::new(node[0] as f32, node[1] as f32, node[2] as f32);
    Placement {
        name: format!("{name} pass (test placement)"),
        position_m: (target + offset).to_array(),
        yaw: yaw_toward(-offset),
        pitch: 0.0,
        speed_m_s: LANDING_SPEED_M_S,
        distance_m: f64::from(LANDING_OFFSET_M),
    }
}

/// The tank mark: [`TANK_STANDOFF_M`] abeam the tank (south of it, level),
/// nose on it.
fn abeam(tank: Vec3) -> Placement {
    let offset = Vec3::new(0.0, -TANK_BELOW_M, TANK_STANDOFF_M);
    Placement {
        name: format!("{TANK} mark (test placement)"),
        position_m: (tank + offset).to_array(),
        yaw: yaw_toward(-offset),
        pitch: -offset.y.atan2(offset.x.hypot(offset.z)),
        speed_m_s: 0.0,
        distance_m: f64::from(TANK_STANDOFF_M),
    }
}

/// The mark behind a Kestrel: [`KESTREL_STANDOFF_M`] astern, nose on it,
/// flying at its speed.
fn behind(target: &Standing) -> Placement {
    let offset = target.rotation * Vec3::Z * KESTREL_STANDOFF_M;
    Placement {
        name: format!("{} mark (test placement)", target.name),
        position_m: (target.position + offset).to_array(),
        yaw: yaw_toward(-offset),
        pitch: -offset.y.atan2(offset.x.hypot(offset.z)),
        speed_m_s: target.velocity.length(),
        distance_m: f64::from(KESTREL_STANDOFF_M),
    }
}

/// The stage's state machine, apart from the Bevy plumbing.
fn step(
    metrics: &mut SuccessMetrics,
    pending: &mut PendingPlacement,
    actions: &mut MissionCockpitActions,
    frame: u32,
    inputs: &Inputs<'_>,
) -> Step {
    let mut step = Step {
        shots: Vec::new(),
        finished: false,
    };
    let (Some(start), Some(centre)) = (metrics.start_frame, metrics.centre) else {
        step.finished = true;
        return step;
    };
    match metrics.beat {
        Beat::Proximity => {
            let at = frame - start;
            // The player stays on its mark for the whole beat.
            if at.is_multiple_of(PLACE_EVERY_FRAMES) {
                pending.0 = Some(mark(centre));
                metrics.placements += 1;
            }
            if at == NEAR_SHOT_OFFSET {
                step.shots.push("success_near");
            }
            // Photograph the marked site once both blocks completed (or time
            // ran out), never before the near frame has been asked for.
            let done = metrics.has_completed(PROXIMITY_BLOCK) && metrics.has_completed(MARK_BLOCK);
            if metrics.marked_shot_frame.is_none()
                && at > NEAR_SHOT_OFFSET
                && (done || at >= SETTLE_FRAMES)
            {
                step.shots.push("success_marked");
                metrics.marked_shot_frame = Some(frame);
                metrics.beat = Beat::Landing;
                metrics.landing_start = Some(frame);
            }
        }
        Beat::Landing => {
            let at = frame - metrics.landing_start.unwrap_or(frame);
            let Some(node) = metrics.landing_at else {
                if at >= FIND_BUDGET_FRAMES {
                    metrics.landing_node_never_found = true;
                    step.finished = true;
                }
                return step;
            };
            if at.is_multiple_of(PLACE_EVERY_FRAMES) {
                pending.0 = Some(landing_pass(node));
                metrics.landing_placements += 1;
            }
            let landed = metrics.completed_at(LANDING_BLOCK);
            if let Some(landed) = landed
                && metrics.landed_shot_frame.is_none()
                && frame >= landed + LANDED_SHOT_OFFSET
            {
                step.shots.push("success_landed");
                metrics.landed_shot_frame = Some(frame);
            }
            if landed.is_none() && at >= LANDING_BUDGET_FRAMES {
                step.finished = true;
                return step;
            }
            // The Kestrels wake ten seconds after the landing; move on then.
            let woken = metrics.has_completed(KESTREL_WAKE_BLOCK);
            let waited = landed.is_some_and(|landed| frame >= landed + KESTREL_WAKE_BUDGET_FRAMES);
            if metrics.landed_shot_frame.is_some() && (woken || waited) {
                metrics.kestrels_never_woken = !woken;
                metrics.beat = Beat::Kestrels;
                metrics.kestrel_start = Some(frame);
            }
        }
        Beat::Kestrels => {
            let begun = metrics.kestrel_start.unwrap_or(frame);
            let at = frame - begun;
            for name in inputs.destroyed_now {
                if !metrics.kestrels_destroyed.iter().any(|(n, _)| n == name) {
                    metrics.kestrels_destroyed.push((name.clone(), frame));
                }
            }
            if metrics.kestrel_actors.is_empty() && at >= FIND_BUDGET_FRAMES {
                // The Kestrels never spawned: nothing to shoot.
                step.finished = true;
                return step;
            }
            if metrics.kestrel_actors.is_empty() {
                return step;
            }
            if let Some(target) = inputs.standing.first() {
                let budget = PER_KESTREL_BUDGET_FRAMES * metrics.kestrel_actors.len() as u32;
                if at > budget {
                    metrics.gave_up_on = Some(target.name.clone());
                    step.finished = true;
                    return step;
                }
                if at.is_multiple_of(KESTREL_PLACE_EVERY_FRAMES) {
                    pending.0 = Some(behind(target));
                    metrics.kestrel_placements += 1;
                }
                if at >= KESTREL_FIRE_AFTER_FRAMES {
                    actions
                        .actions
                        .push(Action::Flight(FlightCommand::FirePrimary));
                    metrics.fire_presses += 1;
                }
                return step;
            }
            // Every Kestrel with a body is down (or none has one): settle,
            // then photograph the sky they fell from.
            let last = metrics
                .kestrels_destroyed
                .iter()
                .map(|(_, frame)| *frame)
                .max()
                .unwrap_or(frame);
            if metrics.kestrels_shot_frame.is_none() && frame >= last + KESTREL_SETTLE_FRAMES {
                step.shots.push("success_kestrels");
                metrics.kestrels_shot_frame = Some(frame);
            }
            if metrics.kestrels_shot_frame.is_some() {
                metrics.beat = Beat::Tank;
                metrics.tank_start = Some(frame);
            }
        }
        Beat::Tank => {
            let at = frame - metrics.tank_start.unwrap_or(frame);
            let Some(tank) = inputs.tank else {
                if at >= FIND_BUDGET_FRAMES {
                    metrics.tank_never_found = true;
                    step.finished = true;
                }
                return step;
            };
            metrics.tank_at = Some(tank.to_array());
            let burning = metrics.completed_at(HYDROGEN_BLOCK);
            if at.is_multiple_of(TANK_PLACE_EVERY_FRAMES) {
                pending.0 = Some(abeam(tank));
                metrics.tank_placements += 1;
            }
            if at == TANK_SHOT_OFFSET {
                step.shots.push("success_tank");
                metrics.tank_shot_frame = Some(frame);
            }
            if burning.is_none() {
                if at > TANK_BUDGET_FRAMES {
                    metrics.tank_gave_up = true;
                    step.finished = true;
                } else if at >= TANK_FIRE_AFTER_FRAMES {
                    actions
                        .actions
                        .push(Action::Flight(FlightCommand::FirePrimary));
                    metrics.tank_fire_presses += 1;
                }
                return step;
            }
            if let Some(burning) = burning
                && metrics.hydrogen_shot_frame.is_none()
                && frame >= burning + HYDROGEN_SHOT_OFFSET
            {
                step.shots.push("success_hydrogen");
                metrics.hydrogen_shot_frame = Some(frame);
                metrics.beat = Beat::Dock;
                metrics.dock_start = Some(frame);
            }
        }
        Beat::Dock => {
            let at = frame - metrics.dock_start.unwrap_or(frame);
            if metrics.has_completed(DOCK_BLOCK) {
                metrics.beat = Beat::Settle;
                metrics.settle_start = Some(frame);
                return step;
            }
            let Some(node) = metrics.dock_at else {
                if at >= DOCK_FIND_BUDGET_FRAMES {
                    metrics.dock_node_never_found = true;
                    step.finished = true;
                }
                return step;
            };
            if at.is_multiple_of(PLACE_EVERY_FRAMES) {
                pending.0 = Some(pass(node, DOCK_NODE));
                metrics.dock_placements += 1;
            }
            if at >= DOCK_FIND_BUDGET_FRAMES + DOCK_BUDGET_FRAMES {
                metrics.dock_gave_up = true;
                step.finished = true;
            }
        }
        Beat::Settle => {
            let at = frame - metrics.settle_start.unwrap_or(frame);
            if metrics.terminal.is_none() {
                metrics.terminal = inputs.terminal.cloned();
            }
            let Some(terminal) = &metrics.terminal else {
                if at >= TERMINAL_BUDGET_FRAMES {
                    metrics.terminal_never = true;
                    step.finished = true;
                }
                return step;
            };
            if metrics.settled_shot_frame.is_none() && frame >= terminal.frame + SETTLED_SHOT_OFFSET
            {
                step.shots.push("success_settled");
                metrics.settled_shot_frame = Some(frame);
            }
            step.finished = metrics.settled_shot_frame.is_some();
        }
    }
    step
}

/// The success-stage checks: the host reported the proximity block and the
/// marking block completed after the placement, the drop-off landing and the
/// Kestrel wake completed, the guns key destroyed every Kestrel through the
/// damage authority, and the frames were drawn.
pub fn evaluate(m: &SmokeMeasurements) -> Vec<Check> {
    let Some(c) = &m.success else {
        return vec![Check {
            id: "success_proximity_block_completed".to_owned(),
            applies: true,
            passed: false,
            detail: "the run never reached the success stage".to_owned(),
        }];
    };
    let at = |block: u32| c.completed_at(block);
    let all_down = !c.kestrel_actors.is_empty()
        && c.kestrel_actors
            .iter()
            .all(|(name, _)| c.kestrels_destroyed.iter().any(|(n, _)| n == name));
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
            id: "success_landing_block_completed".to_owned(),
            applies: true,
            passed: c.has_completed(LANDING_BLOCK) && c.landing_placements > 0,
            detail: format!(
                "the player flew {} level pass(es) at {LANDING_SPEED_M_S} m/s through {LANDING_NODE} \
                 at {:?} (test placements) and the host reported block {LANDING_BLOCK} (texdrop \
                 EXECUTED) completed at {:?}; node never found: {}; host: {}",
                c.landing_placements,
                c.landing_at,
                at(LANDING_BLOCK),
                c.landing_node_never_found,
                c.landing_diag
            ),
        },
        Check {
            id: "success_kestrel_wake_block_completed".to_owned(),
            applies: true,
            passed: c.has_completed(KESTREL_WAKE_BLOCK),
            detail: format!(
                "the host reported block {KESTREL_WAKE_BLOCK} (it wakes the Kestrels, ten seconds \
                 after block 21 wakes block 22) completed at {:?}",
                at(KESTREL_WAKE_BLOCK)
            ),
        },
        Check {
            id: "success_kestrels_destroyed_by_fire_key".to_owned(),
            applies: true,
            passed: all_down && c.fire_presses > 0 && c.gave_up_on.is_none(),
            detail: format!(
                "{} guns-key press(es) were queued from {KESTREL_STANDOFF_M} m marks astern (test \
                 placements, {} made); the damage authority listed {:?} destroyed of {:?} (the \
                 smoke writes no damage); gave up on {:?}",
                c.fire_presses,
                c.kestrel_placements,
                c.kestrels_destroyed,
                c.kestrel_actors.iter().map(|(n, _)| n).collect::<Vec<_>>(),
                c.gave_up_on
            ),
        },
        Check {
            id: "success_hydrogen_fire_block_completed".to_owned(),
            applies: true,
            passed: c.has_completed(HYDROGEN_BLOCK) && c.tank_fire_presses > 0 && !c.tank_gave_up,
            detail: format!(
                "{} guns-key press(es) were queued from {TANK_STANDOFF_M} m abeam and {TANK_BELOW_M} m below {TANK} at {:?} \
                 ({} test placements) and the host reported block {HYDROGEN_BLOCK} \
                 (hydrogen_fire RUNNING) completed at {:?}; tank never found: {}; gave up: {}; host: {}",
                c.tank_fire_presses,
                c.tank_at,
                c.tank_placements,
                at(HYDROGEN_BLOCK),
                c.tank_never_found,
                c.tank_gave_up,
                c.tank_diag
            ),
        },
        Check {
            id: "success_group_clear_block_completed".to_owned(),
            applies: true,
            passed: c.has_completed(GROUP_CLEAR_BLOCK),
            detail: format!(
                "the host reported block {GROUP_CLEAR_BLOCK} (group 1 down, block 26 awake) \
                 completed at {:?}",
                at(GROUP_CLEAR_BLOCK)
            ),
        },
        Check {
            id: "success_dock_block_completed".to_owned(),
            applies: true,
            passed: c.has_completed(DOCK_BLOCK) && c.dock_placements > 0 && !c.dock_gave_up,
            detail: format!(
                "the player flew {} level pass(es) at {LANDING_SPEED_M_S} m/s through {DOCK_NODE} \
                 at {:?} (test placements) and the host reported block {DOCK_BLOCK} \
                 (hooked_to_klondike EXECUTED) completed at {:?}; node never found: {}; gave up: \
                 {}; host: {}",
                c.dock_placements,
                c.dock_at,
                at(DOCK_BLOCK),
                c.dock_node_never_found,
                c.dock_gave_up,
                c.dock_diag
            ),
        },
        Check {
            id: "success_terminal_is_control_program_success".to_owned(),
            applies: true,
            passed: c.terminal.as_ref().is_some_and(|terminal| {
                terminal.outcome == "success"
                    && terminal.source == "control_program"
                    && terminal.requested_by == Some(WIN_BLOCK)
                    && terminal.exit_code == 0
            }),
            detail: format!(
                "the mission host settled {:?} (expected outcome success, source control_program, \
                 requested by block {WIN_BLOCK}, OBJECTIVE15's INSTANTWIN); the smoke wrote no \
                 terminal; terminal never came: {}",
                c.terminal, c.terminal_never
            ),
        },
        Check {
            id: "success_frames_drawn".to_owned(),
            applies: true,
            passed: c.frames_delivered
                && [
                    c.near_permille,
                    c.marked_permille,
                    c.landed_permille,
                    c.kestrels_permille,
                    c.tank_permille,
                    c.hydrogen_permille,
                    c.settled_permille,
                ]
                .iter()
                .all(|permille| *permille >= MIN_DRAWN_PERMILLE),
            detail: if c.frames_delivered {
                format!(
                    "{} / {} / {} / {} / {} / {} per mille of the near, marked, landed, Kestrel, \
                     tank and hydrogen frames are off black ({} for the settled frame; floor \
                     {MIN_DRAWN_PERMILLE})",
                    c.near_permille,
                    c.marked_permille,
                    c.landed_permille,
                    c.kestrels_permille,
                    c.tank_permille,
                    c.hydrogen_permille,
                    c.settled_permille
                )
            } else {
                "a success-stage framebuffer never arrived".to_owned()
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
    let landing_at = c.landing_at.map_or_else(
        || "null".to_owned(),
        |p| format!("[{},{},{}]", p[0], p[1], p[2]),
    );
    let destroyed = c
        .kestrels_destroyed
        .iter()
        .map(|(name, frame)| format!("{{\"node\":{},\"frame\":{frame}}}", json_string(name)))
        .collect::<Vec<_>>()
        .join(",");
    let gave_up = c
        .gave_up_on
        .as_deref()
        .map_or_else(|| "null".to_owned(), json_string);
    let tank_at = c.tank_at.map_or_else(
        || "null".to_owned(),
        |p| format!("[{},{},{}]", p[0], p[1], p[2]),
    );
    let dock_at = c.dock_at.map_or_else(
        || "null".to_owned(),
        |p| format!("[{},{},{}]", p[0], p[1], p[2]),
    );
    let terminal = c.terminal.as_ref().map_or_else(
        || "null".to_owned(),
        |t| {
            format!(
                "{{\"outcome\":{},\"source\":{},\"requested_by_block\":{},\"exit_code\":{},\"tick\":{},\"frame\":{}}}",
                json_string(&t.outcome),
                json_string(&t.source),
                t.requested_by
                    .map_or_else(|| "null".to_owned(), |b| b.to_string()),
                t.exit_code,
                t.tick,
                t.frame
            )
        },
    );
    format!(
        "{{\"start_frame\":{},\"anchor\":{},\"site_never_found\":{},\"centre_m\":{centre},\
         \"standoff_m\":{STANDOFF_M},\"placements\":{},\"completed\":[{completed}],\
         \"frames_delivered\":{},\"near_permille\":{},\"marked_permille\":{},\
         \"landing_node\":{},\"landing_at_m\":{landing_at},\"landing_placements\":{},\
         \"landing_node_never_found\":{},\"landing_diag\":{},\"kestrels\":{},\"fire_presses\":{},\
         \"kestrel_placements\":{},\"kestrels_destroyed\":[{destroyed}],\
         \"kestrels_never_woken\":{},\"gave_up_on\":{gave_up},\"landed_permille\":{},\
         \"kestrels_permille\":{},\"tank\":{},\"tank_at_m\":{tank_at},\"tank_placements\":{},\
         \"tank_fire_presses\":{},\"tank_never_found\":{},\"tank_gave_up\":{},\
         \"tank_permille\":{},\"tank_diag\":{},\"hydrogen_permille\":{},\"dock_node\":{},\"dock_at_m\":{dock_at},\
         \"dock_placements\":{},\"dock_node_never_found\":{},\"dock_gave_up\":{},\
         \"dock_diag\":{},\"terminal\":{terminal},\"terminal_never\":{},\
         \"settled_permille\":{}}}",
        option_u32(c.start_frame),
        json_string(ANCHOR),
        c.site_never_found,
        c.placements,
        c.frames_delivered,
        c.near_permille,
        c.marked_permille,
        json_string(LANDING_NODE),
        c.landing_placements,
        c.landing_node_never_found,
        json_string(&c.landing_diag),
        c.kestrel_actors.len(),
        c.fire_presses,
        c.kestrel_placements,
        c.kestrels_never_woken,
        c.landed_permille,
        c.kestrels_permille,
        json_string(TANK),
        c.tank_placements,
        c.tank_fire_presses,
        c.tank_never_found,
        c.tank_gave_up,
        c.tank_permille,
        json_string(&c.tank_diag),
        c.hydrogen_permille,
        json_string(DOCK_NODE),
        c.dock_placements,
        c.dock_node_never_found,
        c.dock_gave_up,
        json_string(&c.dock_diag),
        c.terminal_never,
        c.settled_permille,
    )
}
