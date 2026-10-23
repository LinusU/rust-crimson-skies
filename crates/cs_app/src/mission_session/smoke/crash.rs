//! The crash stage of the owner-command smoke (task #1857 `PARITY4-CRASH-E`),
//! `--smoke-crash`.
//!
//! The run goes through the briefing, the Flight Check and the launch
//! cinematic like every windowed run, then:
//!
//! 1. once the player has control for a moment it **places the player** in a
//!    dive onto the jungle plateau east of Site #2 (a test placement, labelled
//!    in `report.json`; [`PLATEAU_CRASH_POINT_M`] is the owner's observation of
//!    where the original crashed, and a ray the production physics world
//!    answers says what lies under it) and **scripts one wing hit** so the
//!    damage dial's red pair is on a frame (the production crash flow holds
//!    the gauge cluster back from the first crash frame on) — everything after
//!    the placement is the production flight, the production terrain-contact
//!    rule and the production crash flow;
//! 2. it waits for the terrain contacts to end the flight (a
//!    [`CrashSequence`]), photographs the **contact** frame a few frames
//!    later (sparks, fire, smoke) and the **detached crash camera** with
//!    `Fatal Crash!` once the camera has settled;
//! 3. it **presses a key** — the crash flow's own release — and waits for the
//!    mission host to settle the failure and the journal's Mission Failed
//!    screen to open, then photographs it.
//!
//! The approach is **derived, not measured**: the original's heading into the
//! plateau is unknown, so the dive flies the straight line from the player's
//! position at the placement to [`PLATEAU_CRASH_POINT_M`], at
//! [`DIVE_PITCH_RAD`], from [`DIVE_HEIGHT_M`] above the surface the ray found.
//!
//! The floors are designed and named next to their constants. A run is never
//! `human_play`, and nothing it measures is `verified_original`; the frames are
//! for putting beside the Drydock originals by eye.

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::app::AppExit;
use bevy::ecs::system::SystemParam;
use bevy::prelude::{
    Assets, ClearColor, Commands, Dir3, Entity, GlobalTransform, Has, InheritedVisibility, KeyCode,
    MeshMaterial3d, MessageReader, Query, Res, ResMut, StandardMaterial, Text, Vec3, With,
};

use super::super::compose::MissionPlayerBody;
use super::super::contact_sparks::ContactSpark;
use super::super::crash_flow::CrashMessage;
use super::super::debrief_screen::DebriefScreen;
use super::super::host::MissionHost;
use super::super::hud_art::{HudZonePiece, MissionHudArt, ZonePieceRole};
use super::super::wing_fire::WingFirePuff;
use super::{
    Check, FlightCheckFrames, KeyOp, KeyQueue, PendingPlacement, Phase, Placement,
    SHOT_GRACE_FRAMES, SmokeConfig, SmokeData, SmokeMeasurements, SmokeShots,
    clear_colour_encodings, json_string, non_black_permille, request_screenshot,
};
use crate::airframe_damage::AirframeDamageRegion;
use crate::physics::PhysicsTickLedger;
use crate::playtest::PlaytestCameraMarker;
use crate::playtest::fatal_contact::{
    CrashSequence, FATAL_CRASH_MESSAGE, FailureCause, TerrainContactJudged,
};
use crate::playtest::scene::{PlaytestGround, PlaytestSea};
use crate::weapons::SessionDamageResolver;
use crate::world::WorldColliderInstance;

/// Frames after the player gains control that the smoke waits before it
/// places the dive.
pub const DIVE_AFTER_CONTROL_FRAMES: u32 = 60;
/// Where the original's player was when it crashed in the Drydock capture,
/// metres: the owner's observation recorded in #1755 (task #1755 and
/// `docs/findings/2026-10-22-crash-a-fatal-contact-and-crash-camera.md` §5),
/// on the jungle plateau east of Site #2. An observation, not a measurement
/// of ours; the surface height under it is read from the retail world's
/// colliders at run time and recorded as unknown when no scenery answers.
pub const PLATEAU_CRASH_POINT_M: [f32; 3] = [-5055.0, 324.0, -4718.0];
/// Where the surface ray starts above the crash point, metres. **Designed:**
/// above every authored height.
const SURFACE_RAY_START_M: f32 = 4_000.0;
/// The surface ray's reach, metres.
const SURFACE_RAY_REACH_M: f32 = 8_000.0;
/// The height the dive starts at, metres above the surface under the crash
/// point. **Designed:** high enough that the production flight, not the
/// placement, decides how the aircraft meets the ground.
pub const DIVE_HEIGHT_M: f32 = 400.0;
/// Frames after the dive that the dial frame is taken, so the dial's drive has
/// run on the scripted wing hit. **Designed.**
const DIAL_SHOT_OFFSET: u32 = 20;
/// The wing the scripted hit destroys for the dial frame.
const DIAL_REGION: AirframeDamageRegion = AirframeDamageRegion::LeftWing;
/// The scripted hit's size as a fraction of the designed region pool; above
/// 1 so the region is `Destroyed`.
const DIAL_HIT_FRACTION: f64 = 2.0;
/// The nose attitude of the placement, radians (45 degrees down).
/// **Designed.**
pub const DIVE_PITCH_RAD: f32 = -std::f32::consts::FRAC_PI_4;
/// Frames after the crash sequence began that the contact frame is taken.
const CONTACT_SHOT_OFFSET: u32 = 5;
/// Frames after the crash sequence began that the detached-camera frame is
/// taken.
const CAMERA_SHOT_OFFSET: u32 = 60;
/// Frames after the crash sequence began that the key is pressed.
const KEY_OFFSET: u32 = 90;
/// Frames the run waits for the dive to end the flight before it gives up.
const CRASH_WAIT_FRAMES: u32 = 3_600;
/// Frames the run waits for the debrief screen after the key.
const SCREEN_WAIT_FRAMES: u32 = 600;
/// Frames after the screen appears that its frame is requested.
const SCREEN_SHOT_OFFSET: u32 = 20;
/// The least share of a crash frame that must be off black, in parts per
/// thousand. **Designed:** the world fills the window.
pub const MIN_CRASH_DRAWN_PERMILLE: u32 = 300;
/// The least share of the Mission Failed frame that must be off black, in
/// parts per thousand. **Designed:** as the debrief stage's floor.
pub const MIN_FAILED_DRAWN_PERMILLE: u32 = 300;
/// The farthest the camera may be from the crash stage's camera point, metres.
/// **Designed:** the point is a fixed one; the camera is placed on it.
pub const MAX_CAMERA_ERROR_M: f32 = 1.0;

/// What the crash stage measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CrashMetrics {
    /// The script frame the dive was placed in.
    pub dive_frame: Option<u32>,
    /// Where the dive was placed, metres.
    pub dive_from_m: Option<[f32; 3]>,
    /// The surface the ray found under [`PLATEAU_CRASH_POINT_M`], metres, with
    /// what it is (`scenery`, `sea`, `ground` or `other`); `None`: no collider
    /// answered, the plateau surface there is unknown.
    pub surface_m: Option<([f32; 3], String)>,
    /// What the crash's terrain entity is.
    pub contact_terrain: Option<String>,
    /// The crash position's horizontal distance from [`PLATEAU_CRASH_POINT_M`].
    pub crash_offset_m: Option<f32>,
    /// The script frame the wing hit for the dial frame was applied in.
    pub dial_hit_frame: Option<u32>,
    /// The scripted wing hit's refusal, if the damage authority refused it.
    pub dial_hit_refusal: Option<String>,
    /// Zone pieces of the scripted region drawn with the red texture at the
    /// dial frame, out of those the dial has.
    pub dial_red_pieces: usize,
    /// Zone pieces of the scripted region.
    pub dial_region_pieces: usize,
    /// Zone pieces of the other regions drawn red at the dial frame.
    pub dial_other_red_pieces: usize,
    /// Whether the dial framebuffer arrived.
    pub dial_frame_delivered: bool,
    /// The share of the dial frame that is off black, per mille.
    pub dial_permille: u32,
    /// Terrain contacts the production rule judged before the crash.
    pub contacts_judged: u32,
    /// The player's `[frame, x, y, z]` once a second from the dive on, up to
    /// the crash: how the production flight met the terrain.
    pub track: Vec<[f32; 4]>,
    /// The script frame the crash sequence first existed in.
    pub crash_frame: Option<u32>,
    /// The crash's fixed tick.
    pub crash_tick: Option<u64>,
    /// The crash position, metres.
    pub crash_position_m: Option<[f32; 3]>,
    /// The fatal contact's severity.
    pub severity: Option<f64>,
    /// Most contact spark entities alive in one frame.
    pub max_sparks: usize,
    /// Most wing-fire puff entities alive in one frame.
    pub max_fire_puffs: usize,
    /// Whether the contact and camera framebuffers arrived.
    pub frames_delivered: bool,
    /// The share of the contact frame that is off black, per mille.
    pub contact_permille: u32,
    /// The share of the detached-camera frame that is off black, per mille.
    pub camera_permille: u32,
    /// The camera's distance from the sequence's camera point at the camera
    /// frame, metres.
    pub camera_error_m: Option<f32>,
    /// The `Fatal Crash!` node's text at the camera frame.
    pub message_text: Option<String>,
    /// Whether that node was visible.
    pub message_visible: bool,
    /// The script frame the release key was pressed in.
    pub key_frame: Option<u32>,
    /// The crash sequence was released by the key.
    pub released: bool,
    /// The mission host's outcome label once it had settled.
    pub settled_outcome: Option<String>,
    /// The mission host's terminal source label once it had settled.
    pub settled_source: Option<String>,
    /// The script frame the Mission Failed screen first existed in.
    pub screen_frame: Option<u32>,
    /// `AppExit` messages seen between the crash and the end of the stage.
    pub exits_seen: u32,
    /// Whether the Mission Failed framebuffer arrived.
    pub failed_frame_delivered: bool,
    /// The share of the Mission Failed frame that is off black, per mille.
    pub failed_permille: u32,
}

/// The stage's read-only world queries, grouped so the system stays inside
/// Bevy's parameter limit.
#[derive(SystemParam)]
pub struct CrashWorld<'w, 's> {
    player: Query<
        'w,
        's,
        (
            &'static avian3d::prelude::Position,
            &'static avian3d::prelude::Rotation,
        ),
        With<MissionPlayerBody>,
    >,
    camera: Query<'w, 's, &'static GlobalTransform, With<PlaytestCameraMarker>>,
    message: Query<'w, 's, (&'static Text, &'static InheritedVisibility), With<CrashMessage>>,
    sparks: Query<'w, 's, (), With<ContactSpark>>,
    puffs: Query<'w, 's, (), With<WingFirePuff>>,
    spatial: SpatialQuery<'w, 's>,
    kinds: Query<
        'w,
        's,
        (
            Has<PlaytestSea>,
            Has<PlaytestGround>,
            Option<&'static WorldColliderInstance>,
        ),
    >,
}

/// The damage dial's reads and the scripted wing hit's authorities.
#[derive(SystemParam)]
pub struct CrashDial<'w, 's> {
    damage: Option<ResMut<'w, SessionDamageResolver>>,
    weapons: Option<Res<'w, super::super::MissionWeapons>>,
    ledger: Option<Res<'w, PhysicsTickLedger>>,
    art: Option<Res<'w, MissionHudArt>>,
    materials: Res<'w, Assets<StandardMaterial>>,
    pieces: Query<
        'w,
        's,
        (
            &'static HudZonePiece,
            &'static MeshMaterial3d<StandardMaterial>,
        ),
    >,
}

/// What kind of terrain `entity` is, for the report.
fn terrain_kind(world: &CrashWorld, entity: Entity) -> String {
    match world.kinds.get(entity) {
        Ok((true, _, _)) => "sea",
        Ok((_, true, _)) => "ground",
        Ok((_, _, Some(_))) => "scenery",
        _ => "other",
    }
    .to_owned()
}

/// Counts the zone pieces drawn with the red texture: `(region red, region
/// total, other regions red)`.
fn dial_reading(dial: &CrashDial) -> (usize, usize, usize) {
    let Some(art) = dial.art.as_deref() else {
        return (0, 0, 0);
    };
    let (mut red, mut total, mut other) = (0, 0, 0);
    for (piece, material) in &dial.pieces {
        let name = match piece.role {
            ZonePieceRole::Hilite => "redhilite",
            ZonePieceRole::Hatch => "red_hatchptrn",
        };
        let drawn_red = dial
            .materials
            .get(&material.0)
            .and_then(|m| m.base_color_texture.as_ref())
            .is_some_and(|texture| art.images.get(name) == Some(texture));
        if piece.region == DIAL_REGION {
            total += 1;
            red += usize::from(drawn_red);
        } else {
            other += usize::from(drawn_red);
        }
    }
    (red, total, other)
}

/// The stage's system: dives the player, follows the crash, photographs it.
#[allow(clippy::too_many_arguments)]
pub fn smoke_crash(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    mut pending: ResMut<PendingPlacement>,
    mut queue: ResMut<KeyQueue>,
    crash: Option<Res<CrashSequence>>,
    host: Option<Res<MissionHost>>,
    screen: Option<Res<DebriefScreen>>,
    mut exits: MessageReader<AppExit>,
    mut contacts: Option<MessageReader<TerrainContactJudged>>,
    shots: Res<SmokeShots>,
    frames: Res<FlightCheckFrames>,
    mut commands: Commands,
    clear: Option<Res<ClearColor>>,
    world: CrashWorld,
    mut dial: CrashDial,
) {
    let (player, camera, message, sparks, puffs) = (
        &world.player,
        &world.camera,
        &world.message,
        &world.sparks,
        &world.puffs,
    );
    if config.mode != super::MissionSmokeMode::Crash
        || matches!(data.phase, Phase::Final | Phase::Ended)
    {
        return;
    }
    let frame = data.frame;
    if frame == 0 {
        return;
    }
    if data.measurements.crash.is_none() {
        data.measurements.crash = Some(CrashMetrics::default());
    }
    let dived = data
        .measurements
        .crash
        .as_ref()
        .is_some_and(|metrics| metrics.dive_frame.is_some());
    let seen_exits = u32::try_from(exits.read().count()).unwrap_or(u32::MAX);
    let seen_contacts = contacts.as_mut().map_or(0, |reader| {
        u32::try_from(reader.read().count()).unwrap_or(u32::MAX)
    });
    if dived && let Some(metrics) = data.measurements.crash.as_mut() {
        if metrics.crash_frame.is_some() {
            metrics.exits_seen += seen_exits;
        } else {
            metrics.contacts_judged += seen_contacts;
        }
    }

    // --- 1. the dive, once the player has control ----------------------------
    if !dived {
        let ready = data
            .measurements
            .first_control
            .as_ref()
            .is_some_and(|control| frame >= control.frame + DIVE_AFTER_CONTROL_FRAMES);
        if !ready {
            return;
        }
        let Ok((position, _)) = player.single() else {
            return;
        };
        // Straight down the way the player already flies: same heading, the
        // nose pitched down, [`DIVE_HEIGHT_M`] above the sea ahead.
        // The surface under the owner's crash point, from the production
        // colliders; unknown when nothing answers.
        let crash_point = Vec3::from_array(PLATEAU_CRASH_POINT_M);
        let origin = Vec3::new(crash_point.x, SURFACE_RAY_START_M, crash_point.z);
        let surface = world
            .spatial
            .cast_ray(
                origin,
                Dir3::NEG_Y,
                SURFACE_RAY_REACH_M,
                true,
                &SpatialQueryFilter::default(),
            )
            .map(|hit| {
                (
                    [origin.x, origin.y - hit.distance, origin.z],
                    terrain_kind(&world, hit.entity),
                )
            });
        // An unconfirmed surface falls back to the observed height; the
        // `crash_on_plateau` check then judges where the dive really met the
        // ground.
        let surface_y = surface
            .as_ref()
            .filter(|(_, kind)| kind == "scenery")
            .map_or(crash_point.y, |(point, _)| point[1]);
        // The straight line from where the player is to the crash point; the
        // original's own heading into the plateau is unknown.
        let toward = Vec3::new(
            crash_point.x - position.0.x,
            0.0,
            crash_point.z - position.0.z,
        )
        .normalize_or_zero();
        let toward = if toward == Vec3::ZERO {
            Vec3::NEG_Z
        } else {
            toward
        };
        let yaw = (-toward.x).atan2(-toward.z);
        // 45 degrees down: the horizontal run equals the height.
        let start = Vec3::new(
            crash_point.x - toward.x * DIVE_HEIGHT_M,
            surface_y + DIVE_HEIGHT_M,
            crash_point.z - toward.z * DIVE_HEIGHT_M,
        );
        pending.0 = Some(Placement {
            name: "crash dive onto the jungle plateau east of Site #2".to_owned(),
            position_m: start.to_array(),
            yaw,
            pitch: DIVE_PITCH_RAD,
            speed_m_s: data.speed_m_s,
            distance_m: f64::from(DIVE_HEIGHT_M),
        });
        // The production crash flow holds the gauge cluster back from the
        // first crash frame, so the dial's red pair gets its own frame: one
        // scripted hit through the damage authority destroys a wing zone.
        let hit = super::apply_trail_test_hit(
            dial.damage.as_deref_mut(),
            dial.weapons.as_deref(),
            dial.ledger.as_deref(),
            &[(DIAL_REGION.node_name(), DIAL_HIT_FRACTION)],
        );
        if let Some(metrics) = data.measurements.crash.as_mut() {
            metrics.dive_frame = Some(frame);
            metrics.dive_from_m = Some(start.to_array());
            metrics.surface_m = surface;
            match hit {
                Ok(()) => metrics.dial_hit_frame = Some(frame),
                Err(refusal) => metrics.dial_hit_refusal = Some(refusal),
            }
        }
        // Hold the original's nose-down key (the Up arrow, command 1) for the
        // whole dive: the flight law levels a stickless aircraft off.
        queue.0.push(KeyOp::Press(KeyCode::ArrowUp));
        data.phase_start = frame;
        return;
    }

    if frame.is_multiple_of(60)
        && let Ok((position, _)) = player.single()
        && let Some(metrics) = data.measurements.crash.as_mut()
        && metrics.crash_frame.is_none()
    {
        metrics
            .track
            .push([frame as f32, position.0.x, position.0.y, position.0.z]);
    }

    // The dial frame: the scripted wing hit has been drawn by now.
    let dial_due = data.measurements.crash.as_ref().is_some_and(|metrics| {
        metrics
            .dial_hit_frame
            .is_some_and(|hit| frame == hit + DIAL_SHOT_OFFSET)
            && metrics.crash_frame.is_none()
    });
    if dial_due {
        let (red, total, other) = dial_reading(&dial);
        if let Some(metrics) = data.measurements.crash.as_mut() {
            metrics.dial_red_pieces = red;
            metrics.dial_region_pieces = total;
            metrics.dial_other_red_pieces = other;
        }
        data.shots_requested += 1;
        data.requested.push("crash_dial".to_owned());
        request_screenshot(
            &mut commands,
            shots.clone(),
            config
                .request
                .capture_dir
                .join(format!("frame_cr{frame:05}_crash_dial.png")),
            "crash_dial".to_owned(),
            clear.as_deref().map(clear_colour_encodings),
            None,
            Some(frames.clone()),
        );
    }

    // --- 2. waiting for the flight to end ------------------------------------
    let dive_frame = data
        .measurements
        .crash
        .as_ref()
        .and_then(|metrics| metrics.dive_frame)
        .unwrap_or(frame);
    let Some(crash) = crash else {
        if frame > dive_frame + CRASH_WAIT_FRAMES {
            // The flight never ended: the run ends with the failing checks.
            data.phase = Phase::Final;
            data.phase_start = frame;
        }
        return;
    };
    let crash_frame = {
        let metrics = data
            .measurements
            .crash
            .as_mut()
            .expect("the metrics were created above");
        if metrics.crash_frame.is_none() {
            queue.0.push(KeyOp::Release(KeyCode::ArrowUp));
            metrics.crash_tick = Some(crash.crash.tick);
            metrics.crash_position_m = Some(crash.crash.position_m);
            let FailureCause::TerrainContact {
                severity, terrain, ..
            } = crash.crash.cause;
            metrics.severity = Some(severity);
            metrics.contact_terrain = Some(terrain_kind(&world, terrain));
            let at = crash.crash.position_m;
            let point = PLATEAU_CRASH_POINT_M;
            metrics.crash_offset_m = Some((at[0] - point[0]).hypot(at[2] - point[2]));
        }
        *metrics.crash_frame.get_or_insert(frame)
    };
    let at = frame - crash_frame;
    if let Some(metrics) = data.measurements.crash.as_mut() {
        metrics.max_sparks = metrics.max_sparks.max(sparks.iter().count());
        metrics.max_fire_puffs = metrics.max_fire_puffs.max(puffs.iter().count());
    }

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
                .join(format!("frame_cr{frame:05}_{label}.png")),
            label.to_owned(),
            clear_colours,
            None,
            Some(frames.clone()),
        );
    };
    let pressed = data
        .measurements
        .crash
        .as_ref()
        .and_then(|metrics| metrics.key_frame);
    if pressed.is_none() {
        if at == CONTACT_SHOT_OFFSET {
            request("crash_contact", &mut data);
        }
        if at == CAMERA_SHOT_OFFSET {
            let error = camera
                .single()
                .ok()
                .map(|transform| transform.translation().distance(crash.camera_m));
            let line = message
                .iter()
                .next()
                .map(|(text, visible)| (text.0.clone(), visible.get()));
            if let Some(metrics) = data.measurements.crash.as_mut() {
                metrics.camera_error_m = error;
                metrics.message_text = line.as_ref().map(|(text, _)| text.clone());
                metrics.message_visible = line.is_some_and(|(_, visible)| visible);
            }
            request("crash_camera", &mut data);
        }
        if at == KEY_OFFSET {
            // Any key releases the run; a scripted press, never the player's.
            queue.0.push(KeyOp::Press(KeyCode::Space));
            if let Some(metrics) = data.measurements.crash.as_mut() {
                metrics.key_frame = Some(frame);
            }
            data.phase_start = frame;
        }
        return;
    }
    let key_frame = pressed.unwrap_or(frame);
    if frame == key_frame + 2 {
        queue.0.push(KeyOp::Release(KeyCode::Space));
    }
    if let Some(metrics) = data.measurements.crash.as_mut() {
        metrics.released = crash.released;
    }

    // --- 3. the settle and the Mission Failed screen -------------------------
    if let Some(terminal) = host.as_deref().and_then(MissionHost::terminal)
        && let Some(metrics) = data.measurements.crash.as_mut()
        && metrics.settled_outcome.is_none()
    {
        metrics.settled_outcome = Some(terminal.outcome.label().to_owned());
        metrics.settled_source = Some(terminal.source.label().to_owned());
    }
    if screen.is_none() {
        if frame > key_frame + SCREEN_WAIT_FRAMES {
            data.phase = Phase::Final;
            data.phase_start = frame;
        }
        return;
    }
    let shown_at = {
        let metrics = data
            .measurements
            .crash
            .as_mut()
            .expect("the metrics were created above");
        *metrics.screen_frame.get_or_insert(frame)
    };
    let at = frame - shown_at;
    if at == SCREEN_SHOT_OFFSET {
        request("failed", &mut data);
    }
    if at <= SCREEN_SHOT_OFFSET {
        return;
    }
    let raw = frames
        .0
        .lock()
        .map(|frames| frames.clone())
        .unwrap_or_default();
    let find = |label: &str| raw.iter().find(|frame| frame.label == label);
    let failed = find("failed");
    if failed.is_none() && at < SCREEN_SHOT_OFFSET + SHOT_GRACE_FRAMES {
        return;
    }
    let permille = |label: &str| {
        find(label).map_or(0, |frame| {
            non_black_permille(&frame.data, frame.width, frame.height)
        })
    };
    let (contact, camera_shot) = (permille("crash_contact"), permille("crash_camera"));
    let dial_permille = permille("crash_dial");
    let dial_delivered = find("crash_dial").is_some();
    let failed_permille = failed.map_or(0, |frame| {
        non_black_permille(&frame.data, frame.width, frame.height)
    });
    let delivered = find("crash_contact").is_some() && find("crash_camera").is_some();
    if let Some(metrics) = data.measurements.crash.as_mut() {
        metrics.frames_delivered = delivered;
        metrics.dial_frame_delivered = dial_delivered;
        metrics.dial_permille = dial_permille;
        metrics.contact_permille = contact;
        metrics.camera_permille = camera_shot;
        metrics.failed_frame_delivered = failed.is_some();
        metrics.failed_permille = failed_permille;
    }
    data.phase = Phase::Final;
    data.phase_start = frame;
}

/// The crash checks: the dive ends the flight through the production rule,
/// the crash frames are drawn, the detached camera and `Fatal Crash!` are up,
/// and a key hands the run to the Mission Failed screen.
pub fn evaluate(m: &SmokeMeasurements) -> Vec<Check> {
    let Some(c) = &m.crash else {
        return vec![Check {
            id: "crash_flight_ended".to_owned(),
            applies: true,
            passed: false,
            detail: "the run never reached the crash stage".to_owned(),
        }];
    };
    let mut checks = Vec::new();
    checks.push(Check {
        id: "crash_flight_ended".to_owned(),
        applies: true,
        passed: c.dive_frame.is_some() && c.crash_frame.is_some() && c.crash_tick.is_some(),
        detail: format!(
            "dive placed at frame {:?} from {:?} (a scripted test placement, nose down, Up held); the \
             production terrain-contact rule judged {} contact(s) and ended the flight at frame \
             {:?}, tick {:?}, severity {:?}, position {:?}",
            c.dive_frame,
            c.dive_from_m,
            c.contacts_judged,
            c.crash_frame,
            c.crash_tick,
            c.severity,
            c.crash_position_m
        ),
    });
    checks.push(Check {
        id: "crash_on_plateau".to_owned(),
        applies: true,
        passed: c.contact_terrain.as_deref() == Some("scenery"),
        detail: format!(
            "the fatal contact was with {:?} terrain (world scenery is required, not the sea), \
             {:?} m horizontally from the owner's observed crash point {:?}; the surface the ray \
             found under that point: {}",
            c.contact_terrain,
            c.crash_offset_m,
            PLATEAU_CRASH_POINT_M,
            c.surface_m.as_ref().map_or_else(
                || "unknown, no collider answered".to_owned(),
                |(point, kind)| format!("{kind} at {point:?}")
            )
        ),
    });
    checks.push(Check {
        id: "crash_dial_red_pair".to_owned(),
        applies: true,
        passed: c.dial_hit_frame.is_some()
            && c.dial_region_pieces >= 2
            && c.dial_red_pieces == c.dial_region_pieces
            && c.dial_other_red_pieces == 0
            && c.dial_frame_delivered
            && c.dial_permille >= MIN_CRASH_DRAWN_PERMILLE,
        detail: format!(
            "a scripted wing hit (refusal: {:?}) destroyed the {:?} zone; {} of its {} dial \
             pieces were drawn with the red texture, {} piece(s) of the other zones were red; \
             the dial frame arrived: {}, {} per mille off black (floor {})",
            c.dial_hit_refusal,
            DIAL_REGION,
            c.dial_red_pieces,
            c.dial_region_pieces,
            c.dial_other_red_pieces,
            c.dial_frame_delivered,
            c.dial_permille,
            MIN_CRASH_DRAWN_PERMILLE
        ),
    });
    checks.push(Check {
        id: "crash_frames_drawn".to_owned(),
        applies: true,
        passed: c.frames_delivered
            && c.contact_permille >= MIN_CRASH_DRAWN_PERMILLE
            && c.camera_permille >= MIN_CRASH_DRAWN_PERMILLE,
        detail: if c.frames_delivered {
            format!(
                "{} per mille of the contact frame and {} per mille of the detached-camera frame \
                 are off black (floor {})",
                c.contact_permille, c.camera_permille, MIN_CRASH_DRAWN_PERMILLE
            )
        } else {
            "the contact or detached-camera framebuffer never arrived".to_owned()
        },
    });
    checks.push(Check {
        id: "crash_contact_effects".to_owned(),
        applies: true,
        passed: c.max_sparks > 0 && c.max_fire_puffs > 0,
        detail: format!(
            "at most {} contact spark(s) and {} wing-fire puff(s) were alive in one frame after \
             the crash began",
            c.max_sparks, c.max_fire_puffs
        ),
    });
    checks.push(Check {
        id: "crash_camera_detached".to_owned(),
        applies: true,
        passed: c
            .camera_error_m
            .is_some_and(|error| error <= MAX_CAMERA_ERROR_M),
        detail: format!(
            "the camera was {:?} m from the crash flow's camera point (ceiling {} m)",
            c.camera_error_m, MAX_CAMERA_ERROR_M
        ),
    });
    checks.push(Check {
        id: "crash_message_shown".to_owned(),
        applies: true,
        passed: c.message_visible
            && c.message_text
                .as_deref()
                .is_some_and(|text| !text.is_empty() && text != FATAL_CRASH_MESSAGE),
        detail: format!(
            "the `Fatal Crash!` node read {:?}, visible: {} (the key {FATAL_CRASH_MESSAGE} means \
             no catalog resolved it)",
            c.message_text, c.message_visible
        ),
    });
    checks.push(Check {
        id: "crash_key_hands_over".to_owned(),
        applies: true,
        passed: c.key_frame.is_some()
            && c.released
            && c.settled_outcome.as_deref() == Some("failure")
            && c.settled_source.as_deref() == Some("player_destroyed"),
        detail: format!(
            "a key pressed at frame {:?} (a scripted input) released the sequence: {}; the host \
             settled {:?} from {:?}",
            c.key_frame, c.released, c.settled_outcome, c.settled_source
        ),
    });
    checks.push(Check {
        id: "crash_failed_screen_shown".to_owned(),
        applies: true,
        passed: c.screen_frame.is_some()
            && c.failed_frame_delivered
            && c.failed_permille >= MIN_FAILED_DRAWN_PERMILLE
            && c.exits_seen == 0,
        detail: if c.screen_frame.is_none() {
            "the Mission Failed screen never appeared after the key".to_owned()
        } else {
            format!(
                "{} per mille of the Mission Failed frame is off black (floor {}); {} AppExit \
                 message(s) were written between the crash and the end of the stage",
                c.failed_permille, MIN_FAILED_DRAWN_PERMILLE, c.exits_seen
            )
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

/// The crash measurements as the JSON object `report.json` carries.
pub fn crash_json(metrics: Option<&CrashMetrics>) -> String {
    let Some(c) = metrics else {
        return "null".to_owned();
    };
    let option_u32 =
        |value: Option<u32>| value.map_or_else(|| "null".to_owned(), |v| v.to_string());
    let option_f32 = |value: Option<f32>| {
        value
            .filter(|v| v.is_finite())
            .map_or_else(|| "null".to_owned(), |v| format!("{v:.3}"))
    };
    let option_string = |value: &Option<String>| {
        value
            .as_deref()
            .map_or_else(|| "null".to_owned(), json_string)
    };
    let dive_from = c.dive_from_m.map_or_else(
        || "null".to_owned(),
        |p| format!("[{:.1},{:.1},{:.1}]", p[0], p[1], p[2]),
    );
    let position = c.crash_position_m.map_or_else(
        || "null".to_owned(),
        |p| format!("[{:.2},{:.2},{:.2}]", p[0], p[1], p[2]),
    );
    let surface = c.surface_m.as_ref().map_or_else(
        || "null".to_owned(),
        |(p, kind)| {
            format!(
                "{{\"point\":[{:.1},{:.1},{:.1}],\"kind\":{}}}",
                p[0],
                p[1],
                p[2],
                json_string(kind)
            )
        },
    );
    let track = format!(
        "[{}]",
        c.track
            .iter()
            .map(|p| format!("[{},{:.1},{:.1},{:.1}]", p[0], p[1], p[2], p[3]))
            .collect::<Vec<_>>()
            .join(",")
    );
    format!(
        "{{\"dive_frame\":{},\"dive_from_m\":{dive_from},\"surface\":{surface},\
         \"contact_terrain\":{},\"crash_offset_m\":{},\"dial_hit_frame\":{},\
         \"dial_red_pieces\":{},\"dial_region_pieces\":{},\"dial_other_red_pieces\":{},\
         \"dial_frame_delivered\":{},\"dial_permille\":{},\"contacts_judged\":{},\
         \"track\":{track},\"crash_frame\":{},\"crash_tick\":{},\"crash_position_m\":{position},\"severity\":{},\
         \"max_sparks\":{},\"max_fire_puffs\":{},\"frames_delivered\":{},\
         \"contact_permille\":{},\"camera_permille\":{},\"camera_error_m\":{},\
         \"message_text\":{},\"message_visible\":{},\"key_frame\":{},\"released\":{},\
         \"settled_outcome\":{},\"settled_source\":{},\"screen_frame\":{},\"exits_seen\":{},\
         \"failed_frame_delivered\":{},\"failed_permille\":{}}}",
        option_u32(c.dive_frame),
        option_string(&c.contact_terrain),
        option_f32(c.crash_offset_m),
        option_u32(c.dial_hit_frame),
        c.dial_red_pieces,
        c.dial_region_pieces,
        c.dial_other_red_pieces,
        c.dial_frame_delivered,
        c.dial_permille,
        c.contacts_judged,
        option_u32(c.crash_frame),
        c.crash_tick
            .map_or_else(|| "null".to_owned(), |t| t.to_string()),
        c.severity
            .filter(|v| v.is_finite())
            .map_or_else(|| "null".to_owned(), |v| format!("{v:.4}")),
        c.max_sparks,
        c.max_fire_puffs,
        c.frames_delivered,
        c.contact_permille,
        c.camera_permille,
        option_f32(c.camera_error_m),
        option_string(&c.message_text),
        c.message_visible,
        option_u32(c.key_frame),
        c.released,
        option_string(&c.settled_outcome),
        option_string(&c.settled_source),
        option_u32(c.screen_frame),
        c.exits_seen,
        c.failed_frame_delivered,
        c.failed_permille,
    )
}
