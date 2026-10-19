//! The **owner-command smoke** for an original mission (Rally #1535
//! `M01-OWNER-SMOKE`).
//!
//! `--cs-path <dir> --mission M01 --smoke-seconds <n> --capture-dir <dir>`
//! launches the *same windowed composition the owner plays* — the same
//! [`super::compose::build_windowed`], the same input path, the same renderer,
//! the same mission host — and then, exactly like the playtest smoke
//! (`crate::playtest::smoke`), injects keys through Bevy's keyboard state,
//! takes real framebuffer screenshots, writes a `report.json` and **fails the
//! process when a check fails**. A scripted run is never `human_play`, and
//! nothing it measures is `verified_original`.
//!
//! The smoke exists because headless acceptance tests do not prove the window
//! shows anything: in the owner's 2026-10-17 playtest of main `9b75d132` most
//! of the mission was invisible (untextured, unlit terrain, no sea, no
//! objective line, no other aircraft, no visible shots). Every check below is
//! one thing the player must be able to see, hear or do, measured from the
//! live composition:
//!
//! | check id | what it measures |
//! | --- | --- |
//! | `flight_check_screen_drawn` | the framebuffer taken while the Flight Check holds the mission shows the screen (#1672) |
//! | `flight_check_text_matches` | every text row of the Flight Check, the mission title and the objectives among them, is drawn as the `FlightCheckView` states it and adds pixels in its own region |
//! | `flight_check_buttons_drawn` | every button of the Flight Check, Fly Mission among them, adds pixels in its own region |
//! | `flight_check_fly_mission` | Fly Mission, delivered through the front-end machine, released the mission |
//! | `world_terrain_drawn` | the frame around the spawn holds world pixels off the window's clear colour |
//! | `world_terrain_textured` | those pixels hold more than one flat grey (distinct colours) |
//! | `world_terrain_lit` | those pixels carry a luminance spread (shading, not a flat fill) |
//! | `world_sea_visible` | the world's own water tiles (the drawn sea) cover pixels in a differential probe, when the world declares or draws water |
//! | `hud_objective_current` | the recovered lifecycle model marks a current objective during the run |
//! | `hud_objective_matches` | the drawn line equals the read model's current line, and its pixels are on screen |
//! | `actors_project_and_cover` | every spawned actor projects onto the screen and covers more than `MIN_ACTOR_PIXELS` |
//! | `weapon_guns_fire` | holding `Space` for a second sends more than one round through the mission's weapon session |
//! | `weapon_rounds_drawn` | every live round entity carries a mesh, i.e. a tracer the player can see |
//! | `weapon_muzzle_flash_drawn` | every muzzle-effect record seen during the hold carries a drawn flash, and at least one was seen |
//! | `weapon_rocket_launches` | `Left Ctrl` near a declared-hostile actor launches a rocket |
//! | `audio_radio_queued` | a radio line reached the audio queue or the mixer |
//! | `audio_engine_queued` | the player's engine loop is bound and running |
//! | `smoke_shots_delivered` | every screenshot the run asked for arrived |
//!
//! # How the picture is measured
//!
//! Probes hide what is *not* the thing under test and count the frame's pixels
//! against the window's **declared clear colour** — the same rule
//! [`super::evidence`]'s world probe uses, through that module's own
//! `count_world_pixels`. The sea probe is *differential*: one frame with the
//! sea visible and one with it hidden, taken with the session **paused** so
//! nothing moves between them.
//!
//! Placements are test placements, never measurements of where the mission
//! begins: the smoke writes the production player body's pose (the F24 law
//! re-seeds from the body every fixed tick, so the write is coherent) to stand
//! near each spawned actor, the same way `CS_MISSION_START_M` overrides the
//! start for a capture. Every placement is listed in `report.json`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::camera::Camera;
use bevy::camera::visibility::InheritedVisibility;
use bevy::ecs::query::Has;
use bevy::input::keyboard::KeyCode;
use bevy::input::{ButtonInput, InputSystems};
use bevy::prelude::{
    App, AppExit, ChildOf, ClearColor, Commands, DistanceFog, Entity, FogFalloff, GlobalTransform,
    IntoScheduleConfigs, Last, Local, Mesh3d, MessageWriter, On, Plugin, PostUpdate, PreUpdate,
    Quat, Query, Res, ResMut, Resource, Text, Transform, Update, Vec2, Vec3, Visibility, With,
};
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::ui::{ComputedNode, Node, UiGlobalTransform};
use cs_content::world::SurfaceRole;

use super::compose::MissionPlayerBody;
use super::evidence::{CaptureRefusal, clear_colour_encodings, count_world_pixels, same_colour};
use super::flight_check::{FlightCheckPhase, PhaseResponse, ScreenOverlay};
use super::host::MissionHost;
use super::launch_cinematic::LaunchCinematic;
use super::letterbox::LaunchLetterbox;
use super::lifecycles::ObjectiveLifecycles;
use super::weapons::MissionWeaponReport;
use super::world_draw::WorldWaterPart;
use crate::audio::loops::AudioSession;
use crate::audio::mixer::AudioMixReport;
use crate::audio::spawn::{AudioBindLog, AudioEmitterRole};
use crate::input::platform::PlatformInput;
use crate::input::session::PauseReason;
use crate::mission_actors::{MissionActorSpawnReport, MissionZeppelinActor, ScriptedAircraftActor};
use crate::ordnance::OrdnanceItemMirror;
use crate::physics::PhysicsTickLedger;
use crate::playtest::scene::PlaytestAircraft;
use crate::playtest::{PlaytestCamera, PlaytestState};
use crate::render::horizon::{HorizonDome, HorizonSea};
use crate::render::precipitation::PrecipitationLayer;
use crate::render::weather::WeatherLayer;
use crate::ui::hud::{MissionMessages, OBJECTIVE_HUD_CAPTION, ObjectiveHud};
use crate::weapon_effects::MuzzleEffectSpawn;
use crate::weapons::{WeaponRoundMirror, WeaponSession};
use crate::world::WorldResidency;
use crate::world::contacts::WorldVisual;
use crate::world_actors::WorldActorBinding;

/// Rendered frames per simulated second, the playtest smoke's own rate.
pub const MISSION_SMOKE_FRAME_HZ: f64 = 60.0;

/// The shortest scripted mission run: the sequence needs the settle, the
/// probes, one placement segment per spawned actor, the trigger approach, the
/// gun hold and the rocket wait.
pub const MIN_MISSION_SMOKE_SECONDS: u32 = 45;

/// Where a mission smoke writes its artifacts when `--capture-dir` is omitted
/// (ignored by Git).
pub const DEFAULT_MISSION_CAPTURE_DIR: &str = "private/mission-smoke";

/// Name of the smoke's report file inside the capture directory.
pub const SMOKE_REPORT_FILE: &str = "report.json";

/// The run's own kind string in `report.json`.
pub const SMOKE_KIND: &str = "mission-smoke-report";

// ------------------------------------------------------- scripted timeline --
//
// Every frame number below is an offset inside its own phase, so a phase
// always starts at its own frame 0.

/// The first frame of the cinematic phase a screenshot is taken at: the bars
/// and the held HUD are in place from the first fixed tick.
const CINEMATIC_FIRST_SHOT_FRAME: u32 = 5;
/// Fixed ticks before the last one at which the "end" frame is requested, so
/// the frame the renderer captures still has the bars on.
const CINEMATIC_END_MARGIN_TICKS: u64 = 16;
/// Frames after the hand-over at which its screenshot is requested: the HUD
/// and the control are the player's by then.
const CINEMATIC_HANDOVER_SHOT_FRAMES: u32 = 6;
/// Frames past the phase's own length (one frame is at least one tick at the
/// baseline rate) the run waits before it gives the phase up.
const CINEMATIC_GRACE_FRAMES: u32 = 600;
const CINEMATIC_SHOT_START: u8 = 1;
const CINEMATIC_SHOT_MID: u8 = 2;
const CINEMATIC_SHOT_END: u8 = 4;
const CINEMATIC_SHOT_HANDOVER: u8 = 8;

/// The cinematic seconds at which the run also takes a framebuffer screenshot
/// of the camera script: the first scene's opening and middle, its end, the
/// Pandora's doors opening (20.9 s), the second scene, the first-person shot
/// and the drop (`docs/findings/2026-10-18-parity-m01-lc-cinematic-timeline.md`).
pub const CINEMATIC_SHOT_SECONDS: [u32; 7] = [1, 8, 15, 21, 27, 33, 39];

/// Frames the run settles before the first probe.
const SETTLE_FRAMES: u32 = 30;
/// Frames into the world phase at which the probe hides everything but the
/// world.
const WORLD_HIDE_OFFSET: u32 = 0;
/// Frames into the world phase at which the terrain screenshot is requested.
const WORLD_SHOT_OFFSET: u32 = 2;
/// Frames into the world phase at which the probe restores everything.
const WORLD_RESTORE_OFFSET: u32 = 4;
/// Frames into the sea phase at which the session is paused (so nothing moves
/// between its two frames).
const SEA_PAUSE_OFFSET: u32 = 0;
/// Frames into the sea phase at which the *sea visible* screenshot is
/// requested.
const SEA_VISIBLE_SHOT_OFFSET: u32 = 4;
/// Frames into the sea phase at which the sea plane is hidden.
const SEA_HIDE_OFFSET: u32 = 6;
/// Frames into the sea phase at which the *sea absent* screenshot is
/// requested.
const SEA_ABSENT_SHOT_OFFSET: u32 = 8;
/// Frames into the sea phase at which the world's own water tiles are hidden
/// too (the plane is already hidden).
const SEA_TILES_HIDE_OFFSET: u32 = 10;
/// Frames into the sea phase at which the *no sea at all* screenshot is
/// requested.
const SEA_TILES_ABSENT_SHOT_OFFSET: u32 = 12;
/// Frames into the sea phase at which everything is restored.
const SEA_RESTORE_OFFSET: u32 = 14;
/// Frames into the sea phase at which the session resumes.
const SEA_RESUME_OFFSET: u32 = 16;

/// Simulated frames one actor segment takes.
const ACTOR_SEGMENT_FRAMES: u32 = 24;
/// Frames into a segment at which the player is placed (never for the player's
/// own segment).
const ACTOR_PLACE_OFFSET: u32 = 4;
/// Frames into a segment at which the probe hides everything but the actor.
const ACTOR_HIDE_OFFSET: u32 = 12;
/// Frames into a segment at which the probe's screenshot is requested.
const ACTOR_SHOT_OFFSET: u32 = 14;
/// Frames into a segment at which the probe restores everything.
const ACTOR_RESTORE_OFFSET: u32 = 16;

/// How far from the proximity trigger's anchor the approach starts, metres:
/// the measured placement of Rally #1486, outside the record's own radius so
/// the trigger can only fire after the body has really flown in.
const APPROACH_M: f32 = 1300.0;
/// [`APPROACH_M`] as `f64`, for the report.
const APPROACH_M_F64: f64 = APPROACH_M as f64;
/// The frames the approach may spend flying in before the smoke gives up on
/// the wake and fails the HUD check by name.
const APPROACH_BUDGET_FRAMES: u32 = 1080;
/// Frames the approach must have flown before an observed wake counts (so a
/// wake left over from the measured start cannot satisfy it).
const APPROACH_MIN_FRAMES: u32 = 60;
/// Frames into the post-wake phase at which the HUD screenshot is requested.
const HUD_SHOT_OFFSET: u32 = 20;
/// Frames the gun hold lasts: one second of `Space`.
const GUNS_HOLD_FRAMES: u32 = 60;
/// Frames into the gun phase at which the firing screenshot is requested.
const GUNS_SHOT_OFFSET: u32 = 50;
/// Frames the rocket wait gives the ordnance session to launch.
const ROCKET_WAIT_FRAMES: u32 = 90;
/// Frames into the rocket phase at which its screenshot is requested.
const ROCKET_SHOT_OFFSET: u32 = 80;
/// Frames after the last scripted frame the run waits for outstanding
/// screenshots.
const SHOT_GRACE_FRAMES: u32 = 600;

// ----------------------------------------------------------------- floors --

/// The least fraction of the probe frame that must be world pixels, in parts
/// per thousand — the mission capture's own floor.
pub const MIN_WORLD_COVERAGE_PERMILLE: u32 = super::evidence::MIN_WORLD_COVERAGE_PERMILLE;
/// The least fraction of the frame the sea must cover, in parts per thousand,
/// when the world's records declare water. A sea plane seen from altitude
/// fills a band of the frame; a horizon-wide sliver is not a sea a player can
/// see.
pub const MIN_SEA_COVERAGE_PERMILLE: u32 = 100;
/// The least fraction of the world's pixels that may show *local detail* — a
/// colour change of more than 8/255 against a neighbour — for the terrain to
/// be called *textured*, in parts per thousand.
///
/// Measured basis (2026-10-17, this run's own terrain probe,
/// `frame_00033_world.png`): the flat, untextured terrain the owner reported
/// measures **1 per mille** (3 122 of 1 971 178 pixels — antialiasing and the
/// fog band alone), while its 711 distinct colours and its whole-frame
/// luminance spread of 120 come entirely from that smooth fog gradient. Local
/// detail is what separates a texture from a gradient; the floor sits 20×
/// above the flat state.
pub const MIN_TERRAIN_DETAIL_PERMILLE: u32 = 20;
/// The least number of distinct colours the world pixels must hold for the
/// terrain to be called *textured*. A flat grey fill under any light yields
/// one colour; the smallest real texture yields dozens. Measured flat state:
/// 711 (the fog band's gradient), so this floor alone is never the decider —
/// [`Self::MIN_TERRAIN_DETAIL_PERMILLE`] is.
pub const MIN_TERRAIN_DISTINCT_COLOURS: usize = 24;
/// The least luminance spread **in the nearest terrain** (the lowest third of
/// lowest unbroken run of rows the frame's terrain tiles fill, see
/// [`near_terrain_luminance`])
/// for the terrain to be called *lit*.
///
/// Measured basis (same frame): the reported unlit terrain spans **1** there
/// (82..83) — one flat value as far as the eye can see — while the whole
/// frame's 120 spread is the fog band, which is why the near field is what
/// the check reads. A lit surface's facet shading starts at tens.
pub const MIN_TERRAIN_LUMINANCE_SPREAD: u8 = 24;
/// The least screen pixels a spawned actor must cover to count as *seen*.
///
/// Basis (designed): the pinned window renders 1440 physical pixels tall at a
/// 55° vertical FOV, so one metre spans `1440 / (2 · d · tan 27.5°)` pixels —
/// 3.4 px at the smoke's 400 m placement distance. The designed aircraft box
/// (12 m across) therefore covers about 40 px and the airship far more; 8 px
/// excludes stray antialiasing while staying far below any genuinely drawn
/// actor.
pub const MIN_ACTOR_PIXELS: usize = 8;
/// The least screen pixels the objective line's text must cover inside its own
/// UI region when the model marks a current objective.
pub const MIN_OBJECTIVE_TEXT_PIXELS: usize = 64;
/// The least rounds the gun hold must send through the mission's weapon
/// session: more than one accepted shot in the held second.
pub const MIN_GUN_ROUNDS: u64 = 2;

/// The simulated seconds, counted from the first rendered frame, at which the
/// no-teleport mode reads each wingman's distance from the player.
pub const WINGMAN_SAMPLE_SECONDS: [u32; 5] = [1, 5, 10, 20, 30];

/// **Designed** (the original's formation distance is unmeasured): a wingman
/// flying with the player is within this many metres of the body at the last
/// sample, t+30 s. A threshold, not a recovered game rule; the report carries
/// every measured distance so the value can be re-judged.
pub const MAX_WINGMAN_DISTANCE_M: f64 = 500.0;

/// Frames the Flight Check is up before its first framebuffer is taken: the
/// screen is spawned at startup and laid out over the first frames.
const FLIGHT_CHECK_SHOT_OFFSET: u32 = 20;
/// The frame the overlay (buttons, labels, text rows) is hidden for the bare
/// pane.
const FLIGHT_CHECK_BARE_OFFSET: u32 = 24;
/// The frame the bare pane's framebuffer is taken.
const FLIGHT_CHECK_BARE_SHOT_OFFSET: u32 = 28;
/// The least share of the Flight Check frame that must be off black, in parts
/// per thousand. **Designed:** the 800x600 pane is aspect-fitted into the
/// window, so a 16:9 window leaves the pillarbox black — three quarters of the
/// frame is the pane itself, and a held black cover is nowhere near it.
pub const MIN_FLIGHT_CHECK_DRAWN_PERMILLE: u32 = 300;
/// The least pixels a text row must add inside its own region for the row to
/// count as drawn (**designed**: a row of two letters at the smallest drawn
/// size covers several times this).
pub const MIN_FLIGHT_CHECK_TEXT_PIXELS: usize = 24;
/// The least pixels a button must add inside its own region (**designed**).
pub const MIN_FLIGHT_CHECK_BUTTON_PIXELS: usize = 32;
/// The text rows the screen must draw for the check to be about the right
/// screen: the mission title row and the objectives row of the layout.
const FLIGHT_CHECK_REQUIRED_TEXTS: [&str; 2] = ["FC_T_MISSION", "FC_T_OBJECTIVES"];
/// The button the screen must draw.
const FLIGHT_CHECK_REQUIRED_BUTTON: &str = "FC_B_FLYMISSION";
/// A colour channel difference above which a pixel counts as changed between
/// the full frame and the bare pane.
const FLIGHT_CHECK_PIXEL_DELTA: u8 = 16;

/// The frame the no-teleport mode takes its one framebuffer: right after the
/// last wingman sample.
const NO_TELEPORT_SHOT_FRAME: u32 =
    WINGMAN_SAMPLE_SECONDS[WINGMAN_SAMPLE_SECONDS.len() - 1] * 60 + 1;

/// How far the player is stood from a spawned actor for its probe, metres —
/// abeam, so the probe never puts the player in the actor's path.
const PLACEMENT_OFFSET_M: f32 = 400.0;

/// The world actor the M01 proximity trigger watches, as Rally #1480/#1486
/// measured it: the record's radius is 700 m and its anchor is the cargo
/// zeppelin `workersvoyagezep`. A census member the roster makes hostile is
/// preferred when one is found; this name is the measurement standing behind
/// the fallback, never a guess.
const PROXIMITY_ANCHOR_SUBJECT: &str = "workersvoyagezep";

/// The census member the approach flies to: one the roster declares hostile,
/// else the measured proximity anchor.
fn anchor_subject_of(census: &[CensusEntry]) -> Option<String> {
    census
        .iter()
        .find(|entry| entry.hostile)
        .or_else(|| {
            census.iter().find(|entry| {
                entry
                    .subject
                    .as_deref()
                    .is_some_and(|subject| subject.ends_with(PROXIMITY_ANCHOR_SUBJECT))
            })
        })
        .and_then(|entry| entry.subject.clone())
}

// ------------------------------------------------------------- measurements --

/// One census member: a spawned actor the smoke must be able to see.
#[derive(Clone, Debug)]
pub struct CensusEntry {
    /// `player`, `aircraft` or `zeppelin`.
    pub kind: &'static str,
    /// The record or subject name, verbatim.
    pub name: String,
    /// The entity whose pose is read for placement and projection.
    pub pose_entity: Entity,
    /// The entity that draws it, when anything does: each actor's own body or
    /// binding entity, whose drawn parts are children of it (the player's
    /// airframe, a scripted aircraft's `planes.zbd` model and a zeppelin's
    /// world-node visual all hang off their entity, #1538). `None` is the
    /// fail-closed state — a member no drawn entity could be found for, whose
    /// probe then counts the pixels nothing drew.
    pub drawn_entity: Option<Entity>,
    /// Whether the record started asleep (`deactivated`).
    pub asleep: bool,
    /// The subject key the drawn world object carries, for zeppelins.
    pub subject: Option<String>,
    /// Whether the actor's faction is the declared-hostile one.
    pub hostile: bool,
}

/// What one actor probe measured.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorMetrics {
    /// `player`, `aircraft` or `zeppelin`.
    pub kind: String,
    /// The record or subject name.
    pub name: String,
    /// The record started asleep.
    pub asleep: bool,
    /// Whether the smoke placed the player near it (never done for the
    /// player itself).
    pub placed: bool,
    /// The placement distance the smoke used, metres.
    pub placement_m: Option<f64>,
    /// Whether the actor's pose projected inside the viewport at the probe
    /// frame.
    pub on_screen: bool,
    /// The actor's own pixels in its probe frame.
    pub pixels: usize,
    /// The probe frame's total pixels.
    pub total_pixels: usize,
    /// Why no pixel count exists (no probe frame delivered).
    pub note: Option<String>,
}

/// What the terrain probe measured.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainMetrics {
    /// The world's own pixels (off the clear colour).
    pub world_pixels: usize,
    /// The probe frame's pixels.
    pub total_pixels: usize,
    /// The world's coverage, in parts per thousand.
    pub covered_permille: u32,
    /// Distinct colours among the world's pixels.
    pub distinct_colours: usize,
    /// World pixels that differ from a neighbour by more than 8/255, in parts
    /// per thousand: the texture's local detail, not a smooth gradient.
    pub detail_permille: u32,
    /// The smallest luminance among the world's pixels.
    pub luminance_min: u8,
    /// The largest luminance among the world's pixels.
    pub luminance_max: u8,
    /// The smallest luminance in the nearest terrain (see
    /// [`near_terrain_luminance`]; 0 with the maximum when the frame shows no
    /// terrain): what the *lit* check reads.
    pub near_luminance_min: u8,
    /// The largest luminance in the near field.
    pub near_luminance_max: u8,
}

/// What the sea probe measured.
#[derive(Clone, Debug, PartialEq)]
pub struct SeaMetrics {
    /// The sea plane's own pixels: the frame with the sea minus the frame
    /// without it, both taken with the session paused.
    pub sea_pixels: usize,
    /// The probe frame's pixels.
    pub total_pixels: usize,
    /// The sea's coverage, in parts per thousand.
    pub covered_permille: u32,
    /// The world's own water tiles' pixels: the frame without the plane minus
    /// the frame without the plane and without the tiles.
    pub tile_pixels: usize,
    /// [`Self::tile_pixels`] as parts per thousand of the frame.
    pub tile_permille: u32,
    /// How many of the world's object records declare a known water surface.
    pub water_objects: usize,
    /// How many drawn parts of the world bind the water texture
    /// (`WorldWaterPart` children): the tiles this probe hides.
    pub water_tile_parts: usize,
}

/// What the HUD probe measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct HudMetrics {
    /// The text the lifecycle model plus the installation's catalog say is
    /// current, `None` when the model marks none.
    pub model_line: Option<String>,
    /// The text the widget on screen carried at the HUD frame.
    pub drawn_line: Option<String>,
    /// Whether the widget node was visible.
    pub drawn_visible: bool,
    /// The text pixels counted inside the widget's own UI region.
    pub region_text_pixels: Option<usize>,
    /// The developer overlay was hidden (the default): the original cockpit
    /// draws no objective line, so the line must *not* be on screen (#1641).
    pub overlay_hidden: bool,
}

/// One text row of the Flight Check, judged against the scene the
/// `FlightCheckView` built.
#[derive(Clone, Debug, PartialEq)]
pub struct FlightCheckTextMetrics {
    /// The layout row, e.g. `FC_T_OBJECTIVES`.
    pub row: String,
    /// The text the scene (the view's data) states for the row.
    pub expected: String,
    /// The text the screen's node carried, `None` when no node was drawn for
    /// the row.
    pub drawn: Option<String>,
    /// Whether the node was visible.
    pub visible: bool,
    /// The node's region in framebuffer pixels: `[x, y, width, height]`.
    pub region: [u32; 4],
    /// Pixels inside the region that differ between the full frame and the
    /// bare pane (the frame with the overlay hidden).
    pub pixels: usize,
}

/// One button of the Flight Check.
#[derive(Clone, Debug, PartialEq)]
pub struct FlightCheckButtonMetrics {
    /// The hotspot, e.g. `FC_B_FLYMISSION`.
    pub hotspot: String,
    /// Whether a node was drawn for it.
    pub drawn: bool,
    /// The node's region in framebuffer pixels.
    pub region: [u32; 4],
    /// Pixels inside the region that differ between the full frame and the
    /// bare pane.
    pub pixels: usize,
}

/// What the Flight Check frames measured, and what Fly Mission did.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct FlightCheckMetrics {
    /// Whether both framebuffers arrived (the full frame and the bare pane).
    pub frames_delivered: bool,
    /// The frame's pixel size.
    pub frame_size: (u32, u32),
    /// The share of the full frame that is off black, in parts per thousand.
    pub drawn_permille: u32,
    /// Every text row of the scene with text, in scene order.
    pub texts: Vec<FlightCheckTextMetrics>,
    /// Every button of the scene, in scene order.
    pub buttons: Vec<FlightCheckButtonMetrics>,
    /// Fly Mission was delivered through the front-end machine and the machine
    /// accepted it.
    pub launched: bool,
    /// The machine's refusal, when it refused.
    pub refusal: Option<String>,
}

/// What the gun hold measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GunMetrics {
    /// Weapon effects (one per accepted shot) recorded during the hold.
    pub effects: u64,
    /// The most round mirrors alive at once during the hold.
    pub max_live_rounds: u32,
    /// Whether every live round entity carried a mesh at the firing frame —
    /// `None` when no round was live to ask about.
    pub rounds_drawn: Option<bool>,
    /// How many live rounds were examined.
    pub rounds_examined: u32,
    /// Distinct frames of the hold in which a muzzle-effect record was live
    /// (F27-C-FX-RENDER #1595).
    pub flash_frames: u32,
    /// Muzzle-effect records seen during the hold with no drawn flash child.
    pub flashes_undrawn: u32,
}

/// What the rocket tap measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RocketMetrics {
    /// A rocket was live in the world after the tap.
    pub launched: bool,
    /// The most rocket mirrors alive at once after the tap.
    pub max_rockets: u32,
    /// The mission publisher's own rocket refusals, verbatim.
    pub refusals: Vec<String>,
}

/// What the audio probes measured across the run.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AudioMetrics {
    /// A radio line was active or pending, or the mixer started one, in some
    /// sampled frame.
    pub radio_seen: bool,
    /// The player's engine loop was bound and running in some sampled frame.
    pub engine_seen: bool,
    /// How many `Engine` emitter bindings the spawn path attached.
    pub engine_bindings: usize,
    /// How many loops the router held at the last sample.
    pub active_loops: usize,
    /// How many radio lines the mixer started over the run.
    pub radio_started: usize,
}

/// What the trigger approach measured.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ApproachMetrics {
    /// The proximity anchor (the declared-hostile cargo zeppelin) was found
    /// and the player was placed on the line toward the measured start.
    pub placed: bool,
    /// The anchor's subject name.
    pub anchor: Option<String>,
    /// The frames the approach spent flying in.
    pub frames: u32,
    /// The lifecycle model marked a current objective during the run.
    pub objective_woke: bool,
}

/// What the launch cinematic phase measured (Rally #1702): the smoke runs
/// through it, then the scripted checks begin at the hand-over.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CinematicMetrics {
    /// The phase's length in fixed ticks.
    pub total_ticks: u64,
    /// Fixed ticks the phase had run when the smoke left it.
    pub played_ticks: u64,
    /// The farthest the body stood from where the phase found it, metres.
    pub max_drift_m: f32,
    /// The farthest the body turned from how the phase found it, degrees.
    pub max_turn_deg: f32,
    /// The letterbox bars were on in every frame sampled while it played.
    pub bars_on_while_playing: bool,
    /// The letterbox bars were off in every frame sampled after it ended.
    pub bars_off_after: bool,
    /// The phase reached its last tick and handed the player control.
    pub handed_over: bool,
    /// What the phase's framebuffer screenshots show (#1704), by label.
    pub frames: Vec<CinematicFrame>,
}

/// What one cinematic screenshot's pixels show of the bars and the HUD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CinematicFrame {
    /// The screenshot's label (`cinematic_start` ... `cinematic_handover`).
    pub label: String,
    /// Parts per thousand of the letterbox bars' rows that are black.
    pub bars_black_permille: u32,
    /// Pixels of the cockpit HUD's own green in the frame.
    pub hud_green_pixels: usize,
}

/// The least share of the bar rows that must be black in a frame taken while
/// the cinematic plays, in parts per thousand. **Designed:** the bars are
/// opaque black, so anything under this is the picture showing through.
pub const MIN_BARS_BLACK_PERMILLE: u32 = 990;

/// The most of the bar rows that may be black after hand-over, in parts per
/// thousand. **Designed:** the picture fills those rows once the bars are off;
/// a sea-and-sky frame is not black over half its top and bottom rows.
pub const MAX_BARS_BLACK_AFTER_PERMILLE: u32 = 500;

/// The most HUD-green pixels a frame taken while the HUD is held may hold.
/// **Designed:** none is drawn; a few world pixels may match by chance.
pub const MAX_HELD_HUD_GREEN_PIXELS: usize = 20;

/// The least HUD-green pixels the hand-over frame must hold (gauge rings and
/// the reticle are drawn). **Designed.**
pub const MIN_HANDOVER_HUD_GREEN_PIXELS: usize = 200;

/// The screenshot labels the pixel judgement needs, in order.
const CINEMATIC_FRAME_LABELS: [&str; 4] = [
    "cinematic_start",
    "cinematic_mid",
    "cinematic_end",
    "cinematic_handover",
];

/// Judges the cinematic's screenshots: bars drawn and HUD absent while it
/// plays, bars gone and HUD drawn after hand-over. `Err` names the first
/// frame that disagrees, or the missing one.
fn judge_cinematic_frames(frames: &[CinematicFrame]) -> Result<(), String> {
    for label in CINEMATIC_FRAME_LABELS {
        let Some(frame) = frames.iter().find(|frame| frame.label == label) else {
            return Err(format!("no `{label}` frame was measured"));
        };
        let held = label != "cinematic_handover";
        let ok = if held {
            frame.bars_black_permille >= MIN_BARS_BLACK_PERMILLE
                && frame.hud_green_pixels <= MAX_HELD_HUD_GREEN_PIXELS
        } else {
            frame.bars_black_permille <= MAX_BARS_BLACK_AFTER_PERMILLE
                && frame.hud_green_pixels >= MIN_HANDOVER_HUD_GREEN_PIXELS
        };
        if !ok {
            return Err(format!(
                "`{label}` shows {} per mille black in the bar rows and {} HUD-green pixels",
                frame.bars_black_permille, frame.hud_green_pixels
            ));
        }
    }
    Ok(())
}

/// The player's pose in the first control frame (#1697): the first fixed tick
/// after the player gains control — after the launch cinematic's hand-over, or
/// right after Fly Mission when the run has no cinematic — before any scripted
/// move or teleport. Bevy world axes, metres and degrees; nothing here is a
/// measurement of the original.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FirstControlMetrics {
    /// Fixed simulation ticks the physics ledger had counted.
    pub tick: u64,
    /// The script frame the pose was read in.
    pub frame: u32,
    /// The body's position.
    pub position_m: [f64; 3],
    /// The body's yaw, degrees, about world `+Y` (Bevy `YXZ` Euler order).
    pub yaw_deg: f32,
    /// The body's pitch, degrees (Bevy `YXZ` Euler order).
    pub pitch_deg: f32,
    /// The body's roll, degrees (Bevy `YXZ` Euler order).
    pub roll_deg: f32,
    /// The body's nose direction (local `-Z`) in world axes.
    pub forward: [f32; 3],
    /// Placements the script had made or queued by then; zero for a frame
    /// taken before any scripted move.
    pub moves_before: usize,
}

/// What the run saw of the launch cinematic's camera script (#1665).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CameraMetrics {
    /// The camera playback's own counters.
    pub camera: super::launch_camera::LaunchCameraStats,
    /// Scene screenshots taken at [`CINEMATIC_SHOT_SECONDS`].
    pub shots: usize,
}

/// How far the held body may stand from where the cinematic found it,
/// metres. **Designed:** the hold zeroes the velocity every tick, so any
/// drift beyond a numerical hair is the body moving.
pub const MAX_CINEMATIC_DRIFT_M: f32 = 0.5;

/// How far the held body may turn from how the cinematic found it, degrees.
/// **Designed:** the hold keeps the body still; a turn beyond a numerical hair
/// is the body rotating (#1704).
pub const MAX_CINEMATIC_TURN_DEG: f32 = 0.5;

/// How the smoke flies the player.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MissionSmokeMode {
    /// The full script: it places the player near every actor and at the
    /// trigger (a test placement) and judges the whole M01 checklist.
    #[default]
    Scripted,
    /// `--smoke-no-teleport`: the player flies the measured start for the
    /// whole run and is never moved; the one judged check is
    /// `wingmen_near_player`.
    NoTeleport,
}

/// Every wingman's distance from the player at one sample time.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct WingmanSample {
    /// Whole simulated seconds since the first rendered frame.
    pub second: u32,
    /// `(record name, metres from the player)`, in census order.
    pub distances_m: Vec<(String, f64)>,
}

/// Everything the run measured, as [`evaluate`] consumes it.
#[derive(Clone, Debug, Default)]
pub struct SmokeMeasurements {
    /// The mission label, e.g. `M01`.
    pub mission: String,
    /// Rendered frames the run lasted.
    pub frames: u32,
    /// Fixed physics ticks the run lasted.
    pub ticks: u64,
    /// The terrain probe.
    pub terrain: Option<TerrainMetrics>,
    /// The sea probe.
    pub sea: Option<SeaMetrics>,
    /// The HUD probe.
    pub hud: HudMetrics,
    /// The Flight Check frames and Fly Mission (`None`: the run never reached
    /// a Flight Check).
    pub flight_check: Option<FlightCheckMetrics>,
    /// The actor probes, in census order.
    pub actors: Vec<ActorMetrics>,
    /// The gun hold.
    pub guns: GunMetrics,
    /// The rocket tap.
    pub rocket: RocketMetrics,
    /// The audio probes.
    pub audio: AudioMetrics,
    /// The trigger approach.
    pub approach: ApproachMetrics,
    /// Names of shots the run requested but the renderer never delivered.
    pub missing_shots: Vec<String>,
    /// The run never moved the player ([`MissionSmokeMode::NoTeleport`]).
    pub no_teleport: bool,
    /// The wingman distances, one sample per [`WINGMAN_SAMPLE_SECONDS`] entry
    /// reached (no-teleport mode only).
    pub wingmen: Vec<WingmanSample>,
    /// The launch cinematic phase, when the run went through it.
    pub cinematic: Option<CinematicMetrics>,
    /// The launch cinematic's camera script, when the run played it.
    pub camera: Option<CameraMetrics>,
    /// The player's pose at the first control frame (#1697).
    pub first_control: Option<FirstControlMetrics>,
}

/// One pass/fail verdict with its evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    /// The stable check id (`report.json`'s `checks[].id`).
    pub id: String,
    /// Whether the check applies to this run at all. A check that does not
    /// apply is recorded with its reason and never counts as a pass.
    pub applies: bool,
    /// Whether the check passed.
    pub passed: bool,
    /// The evidence, in one line.
    pub detail: String,
}

/// Judges every check from the measurements. Pure, so the acceptance suite
/// drives it without a GPU.
///
/// A check that cannot be measured (`applies == false`) is recorded with the
/// reason rather than silently passed — the world's records declaring no
/// water is the only such case today.
#[must_use]
pub fn evaluate(m: &SmokeMeasurements) -> Vec<Check> {
    if m.no_teleport {
        // The no-teleport run passes through the Flight Check to release the
        // mission and records what it saw, but judges only the wingmen.
        return evaluate_no_teleport(m);
    }
    let mut checks = evaluate_flight_check(m);

    // --- world -------------------------------------------------------------
    match &m.terrain {
        Some(terrain) => {
            checks.push(Check {
                id: "world_terrain_drawn".to_owned(),
                applies: true,
                passed: terrain.covered_permille >= MIN_WORLD_COVERAGE_PERMILLE,
                detail: format!(
                    "{} of {} pixels are off the window's clear colour ({} per mille, floor {})",
                    terrain.world_pixels,
                    terrain.total_pixels,
                    terrain.covered_permille,
                    MIN_WORLD_COVERAGE_PERMILLE
                ),
            });
            checks.push(Check {
                id: "world_terrain_textured".to_owned(),
                applies: true,
                passed: terrain.detail_permille >= MIN_TERRAIN_DETAIL_PERMILLE
                    && terrain.distinct_colours >= MIN_TERRAIN_DISTINCT_COLOURS,
                detail: format!(
                    "{} of the world's pixels ({} per mille, floor {}) differ from a neighbour — \
                     the local detail a texture has and a smooth fog gradient does not; \
                     {} distinct colours (floor {})",
                    (u64::from(terrain.detail_permille) * terrain.world_pixels as u64 / 1000)
                        as usize,
                    terrain.detail_permille,
                    MIN_TERRAIN_DETAIL_PERMILLE,
                    terrain.distinct_colours,
                    MIN_TERRAIN_DISTINCT_COLOURS
                ),
            });
            let spread = terrain
                .near_luminance_max
                .saturating_sub(terrain.near_luminance_min);
            checks.push(Check {
                id: "world_terrain_lit".to_owned(),
                applies: true,
                passed: spread >= MIN_TERRAIN_LUMINANCE_SPREAD,
                detail: format!(
                    "the nearest terrain spans luminance {}..{} (spread {}, floor {}; 0..0 means the \
                     frame shows no terrain); the whole frame spans {}..{}, whose spread is the fog band rather than shading",
                    terrain.near_luminance_min,
                    terrain.near_luminance_max,
                    spread,
                    MIN_TERRAIN_LUMINANCE_SPREAD,
                    terrain.luminance_min,
                    terrain.luminance_max
                ),
            });
        }
        None => {
            for id in [
                "world_terrain_drawn",
                "world_terrain_textured",
                "world_terrain_lit",
            ] {
                checks.push(Check {
                    id: id.to_owned(),
                    applies: true,
                    passed: false,
                    detail: "the renderer delivered no terrain probe frame, so nothing about the \
                             world can be claimed"
                        .to_owned(),
                });
            }
        }
    }
    match &m.sea {
        Some(sea) if sea.water_objects == 0 && sea.water_tile_parts == 0 => checks.push(Check {
            id: "world_sea_visible".to_owned(),
            applies: false,
            passed: false,
            detail: "the world declares no water: no object record with a known water surface and \
                     no drawn part bound to the water texture, so no sea is required here"
                .to_owned(),
        }),
        Some(sea) => checks.push(Check {
            id: "world_sea_visible".to_owned(),
            applies: true,
            // The drawn sea is the world's own water tiles; the horizon plane
            // only continues them. Tiles that draw nothing are not the sea.
            passed: sea.water_tile_parts > 0
                && sea.tile_permille >= MIN_SEA_COVERAGE_PERMILLE
                && sea.covered_permille >= MIN_SEA_COVERAGE_PERMILLE,
            detail: format!(
                "the sea covers {} of {} pixels ({} per mille, floor {}); the world's own water \
                 tiles ({} drawn parts, {} of the world's objects declare water) draw {} of \
                 them ({} per mille, floor {}), the horizon plane the other {}",
                sea.sea_pixels,
                sea.total_pixels,
                sea.covered_permille,
                MIN_SEA_COVERAGE_PERMILLE,
                sea.water_tile_parts,
                sea.water_objects,
                sea.tile_pixels,
                sea.tile_permille,
                MIN_SEA_COVERAGE_PERMILLE,
                sea.sea_pixels.saturating_sub(sea.tile_pixels),
            ),
        }),
        None => checks.push(Check {
            id: "world_sea_visible".to_owned(),
            applies: true,
            passed: false,
            detail: "the renderer delivered no sea probe frames, so the sea cannot be claimed"
                .to_owned(),
        }),
    }

    // --- HUD ---------------------------------------------------------------
    checks.push(Check {
        id: "hud_objective_current".to_owned(),
        applies: true,
        passed: m.approach.objective_woke && m.hud.model_line.is_some(),
        detail: if m.hud.model_line.is_some() {
            format!(
                "the lifecycle model marks a current objective: {:?}",
                m.hud.model_line
            )
        } else if !m.approach.placed {
            "the smoke never reached the proximity trigger's anchor, so no objective could wake"
                .to_owned()
        } else {
            format!(
                "after {} frames flying into the trigger the lifecycle model still marks no \
                 current objective",
                m.approach.frames
            )
        },
    });
    let hud_match = if m.hud.overlay_hidden {
        !m.hud.drawn_visible
    } else {
        m.hud.model_line == m.hud.drawn_line
            && (m.hud.model_line.is_none()
                || (m.hud.drawn_visible
                    && m.hud.region_text_pixels.unwrap_or(0) >= MIN_OBJECTIVE_TEXT_PIXELS))
    };
    checks.push(Check {
        id: "hud_objective_matches".to_owned(),
        applies: true,
        passed: hud_match,
        detail: if m.hud.overlay_hidden {
            format!(
                "developer overlay hidden: the original cockpit draws no objective line, and \
                 the designed caption line is visible {} (must be false)",
                m.hud.drawn_visible
            )
        } else {
            format!(
                "model {:?}, drawn {:?} (visible {}, region text pixels {:?}): the drawn line must \
             equal the read model's and hold at least {MIN_OBJECTIVE_TEXT_PIXELS} pixels of its \
             own text when one is current",
                m.hud.model_line, m.hud.drawn_line, m.hud.drawn_visible, m.hud.region_text_pixels
            )
        },
    });

    // --- actors ------------------------------------------------------------
    let unseen: Vec<String> = m
        .actors
        .iter()
        .filter(|actor| !actor.on_screen || actor.pixels < MIN_ACTOR_PIXELS)
        .map(|actor| {
            format!(
                "{} {} ({} px on screen {})",
                actor.kind, actor.name, actor.pixels, actor.on_screen
            )
        })
        .collect();
    checks.push(Check {
        id: "actors_project_and_cover".to_owned(),
        applies: true,
        passed: !m.actors.is_empty() && unseen.is_empty(),
        detail: if m.actors.is_empty() {
            "the mission spawned no actors at all, so nothing was checked".to_owned()
        } else if unseen.is_empty() {
            format!(
                "all {} spawned actors project onto the screen and cover at least {} pixels",
                m.actors.len(),
                MIN_ACTOR_PIXELS
            )
        } else {
            format!(
                "{} of {} spawned actors are not visible to the player: {}",
                unseen.len(),
                m.actors.len(),
                unseen.join("; ")
            )
        },
    });

    // --- firing ------------------------------------------------------------
    checks.push(Check {
        id: "weapon_guns_fire".to_owned(),
        applies: true,
        passed: m.guns.effects >= MIN_GUN_ROUNDS,
        detail: format!(
            "holding Space for a second put {} rounds through the mission's weapon session \
             ({} live at once), floor {}",
            m.guns.effects, m.guns.max_live_rounds, MIN_GUN_ROUNDS
        ),
    });
    checks.push(Check {
        id: "weapon_rounds_drawn".to_owned(),
        applies: true,
        passed: m.guns.rounds_drawn == Some(true),
        detail: match (m.guns.rounds_drawn, m.guns.rounds_examined) {
            (Some(true), examined) => format!("all {examined} live rounds carry a mesh"),
            (Some(false), examined) => format!(
                "{examined} live round entities carry no mesh, so a shot draws no tracer the \
                 player can see (F27-C-FX-SCHED #1304)"
            ),
            (None, _) => {
                "no round was live at the firing frame, so nothing could be drawn".to_owned()
            }
        },
    });
    checks.push(Check {
        id: "weapon_muzzle_flash_drawn".to_owned(),
        applies: true,
        passed: m.guns.flash_frames > 0 && m.guns.flashes_undrawn == 0,
        detail: if m.guns.flash_frames == 0 {
            "no muzzle-effect record was live during the hold, so no flash was drawn \
             (F27-C-FX-RENDER #1595)"
                .to_owned()
        } else if m.guns.flashes_undrawn > 0 {
            format!(
                "{} muzzle-effect records carried no drawn flash (designed stand-in, claim {})",
                m.guns.flashes_undrawn,
                super::MUZZLE_FLASH_VISUAL_CLAIM
            )
        } else {
            format!(
                "a drawn flash was live in {} frames of the hold (designed stand-in, claim {})",
                m.guns.flash_frames,
                super::MUZZLE_FLASH_VISUAL_CLAIM
            )
        },
    });
    checks.push(Check {
        id: "weapon_rocket_launches".to_owned(),
        applies: true,
        passed: m.rocket.launched,
        detail: if m.rocket.launched {
            format!(
                "Left Ctrl near a declared-hostile actor launched a rocket ({} live)",
                m.rocket.max_rockets
            )
        } else {
            format!(
                "no rocket was live after the tap ({} seen); the mission publisher refused: {}",
                m.rocket.max_rockets,
                if m.rocket.refusals.is_empty() {
                    "nothing recorded".to_owned()
                } else {
                    m.rocket.refusals.join("; ")
                }
            )
        },
    });

    // --- audio -------------------------------------------------------------
    checks.push(Check {
        id: "audio_radio_queued".to_owned(),
        applies: true,
        passed: m.audio.radio_seen,
        detail: format!(
            "{} radio lines reached the mixer; the session's queue held a line in some sampled \
             frame: {}",
            m.audio.radio_started, m.audio.radio_seen
        ),
    });
    checks.push(Check {
        id: "audio_engine_queued".to_owned(),
        applies: true,
        passed: m.audio.engine_seen,
        detail: format!(
            "{} engine emitter bindings, {} loops running at the last sample",
            m.audio.engine_bindings, m.audio.active_loops
        ),
    });

    // --- the launch cinematic ---------------------------------------------
    if let Some(c) = &m.cinematic {
        checks.push(Check {
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
                "{} of {} ticks played, handed over: {}, body drift {:.3} m (ceiling {}), turn {:.3} deg (ceiling {}), \
                 bars on while playing: {}, off after: {}; pixels: {}",
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
        });
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

    // --- the first control frame ------------------------------------------
    checks.push(Check {
        id: "first_control_frame".to_owned(),
        applies: true,
        passed: m.first_control.as_ref().is_some_and(|f| {
            f.moves_before == 0
                && f.position_m.iter().all(|v| v.is_finite())
                && f.yaw_deg.is_finite()
        }) && !m.missing_shots.iter().any(|shot| shot == "first_control"),
        detail: m.first_control.as_ref().map_or_else(
            || "the player never gained control: no first control pose".to_owned(),
            |f| {
                format!(
                    "tick {} (frame {}): position ({:.1}, {:.1}, {:.1}) m, yaw {:.1} deg, \
                     {} scripted moves before the frame",
                    f.tick,
                    f.frame,
                    f.position_m[0],
                    f.position_m[1],
                    f.position_m[2],
                    f.yaw_deg,
                    f.moves_before
                )
            },
        ),
    });

    // --- the picture arrived ----------------------------------------------
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

/// The Flight Check checks: the screen the run opens on, judged before the
/// mission's own checks.
fn evaluate_flight_check(m: &SmokeMeasurements) -> Vec<Check> {
    let missing = |id: &str| Check {
        id: id.to_owned(),
        applies: true,
        passed: false,
        detail: "the run never reached a Flight Check, so nothing was measured".to_owned(),
    };
    let ids = [
        "flight_check_screen_drawn",
        "flight_check_text_matches",
        "flight_check_buttons_drawn",
        "flight_check_fly_mission",
    ];
    let Some(fc) = &m.flight_check else {
        return ids.iter().map(|id| missing(id)).collect();
    };
    let mut checks = Vec::new();
    checks.push(Check {
        id: ids[0].to_owned(),
        applies: true,
        passed: fc.frames_delivered && fc.drawn_permille >= MIN_FLIGHT_CHECK_DRAWN_PERMILLE,
        detail: if fc.frames_delivered {
            format!(
                "{} per mille of the {}x{} Flight Check frame is off black (floor {})",
                fc.drawn_permille,
                fc.frame_size.0,
                fc.frame_size.1,
                MIN_FLIGHT_CHECK_DRAWN_PERMILLE
            )
        } else {
            "the Flight Check framebuffers never arrived".to_owned()
        },
    });
    let bad_texts: Vec<String> = fc
        .texts
        .iter()
        .filter(|text| {
            text.drawn.as_deref() != Some(text.expected.as_str())
                || !text.visible
                || text.pixels < MIN_FLIGHT_CHECK_TEXT_PIXELS
        })
        .map(|text| {
            format!(
                "{} (expected {:?}, drawn {:?}, visible {}, {} px)",
                text.row, text.expected, text.drawn, text.visible, text.pixels
            )
        })
        .collect();
    let absent: Vec<&str> = FLIGHT_CHECK_REQUIRED_TEXTS
        .iter()
        .copied()
        .filter(|row| !fc.texts.iter().any(|text| text.row == *row))
        .collect();
    checks.push(Check {
        id: ids[1].to_owned(),
        applies: true,
        passed: fc.frames_delivered && bad_texts.is_empty() && absent.is_empty(),
        detail: if !fc.frames_delivered {
            "the Flight Check framebuffers never arrived".to_owned()
        } else if !absent.is_empty() {
            format!("the scene has no text row {absent:?}")
        } else if bad_texts.is_empty() {
            format!(
                "all {} text rows are drawn as the view states them, each adding at least \
                 {MIN_FLIGHT_CHECK_TEXT_PIXELS} pixels in its own region",
                fc.texts.len()
            )
        } else {
            format!("text rows not drawn as the view states them: {bad_texts:?}")
        },
    });
    let bad_buttons: Vec<String> = fc
        .buttons
        .iter()
        .filter(|button| !button.drawn || button.pixels < MIN_FLIGHT_CHECK_BUTTON_PIXELS)
        .map(|button| {
            format!(
                "{} (drawn {}, {} px)",
                button.hotspot, button.drawn, button.pixels
            )
        })
        .collect();
    let no_fly = !fc
        .buttons
        .iter()
        .any(|button| button.hotspot == FLIGHT_CHECK_REQUIRED_BUTTON);
    checks.push(Check {
        id: ids[2].to_owned(),
        applies: true,
        passed: fc.frames_delivered && bad_buttons.is_empty() && !no_fly,
        detail: if !fc.frames_delivered {
            "the Flight Check framebuffers never arrived".to_owned()
        } else if no_fly {
            format!("the scene has no {FLIGHT_CHECK_REQUIRED_BUTTON} button")
        } else if bad_buttons.is_empty() {
            format!(
                "all {} buttons add at least {MIN_FLIGHT_CHECK_BUTTON_PIXELS} pixels in their \
                 own regions",
                fc.buttons.len()
            )
        } else {
            format!("buttons not drawn: {bad_buttons:?}")
        },
    });
    checks.push(Check {
        id: ids[3].to_owned(),
        applies: true,
        passed: fc.launched,
        detail: match (&fc.refusal, fc.launched) {
            (_, true) => {
                "Fly Mission went through the front-end machine and released the mission".to_owned()
            }
            (Some(reason), false) => format!("the machine refused Fly Mission: {reason}"),
            (None, false) => "Fly Mission was never delivered".to_owned(),
        },
    });
    checks
}

/// The no-teleport checks: the wingmen's distance from a player nothing moved,
/// and the picture arriving.
fn evaluate_no_teleport(m: &SmokeMeasurements) -> Vec<Check> {
    let rows = m
        .wingmen
        .iter()
        .map(|sample| {
            let distances = sample
                .distances_m
                .iter()
                .map(|(name, metres)| format!("{name} {metres:.0} m"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("t+{}s: {}", sample.second, distances)
        })
        .collect::<Vec<_>>()
        .join("; ");
    let complete = m.wingmen.len() == WINGMAN_SAMPLE_SECONDS.len();
    let last = m.wingmen.last();
    let far: Vec<String> = last
        .into_iter()
        .flat_map(|sample| sample.distances_m.iter())
        .filter(|(_, metres)| !metres.is_finite() || *metres > MAX_WINGMAN_DISTANCE_M)
        .map(|(name, metres)| format!("{name} {metres:.0} m"))
        .collect();
    let any_wingman = last.is_some_and(|sample| !sample.distances_m.is_empty());
    let passed = complete && any_wingman && far.is_empty();
    let verdict = if !complete {
        format!(
            "only {} of {} samples were taken",
            m.wingmen.len(),
            WINGMAN_SAMPLE_SECONDS.len()
        )
    } else if !any_wingman {
        "the mission spawned no wingman, so nothing was measured".to_owned()
    } else if far.is_empty() {
        format!("every wingman is within {MAX_WINGMAN_DISTANCE_M:.0} m at t+30s")
    } else {
        format!(
            "at t+30s these wingmen are beyond {MAX_WINGMAN_DISTANCE_M:.0} m: {}",
            far.join(", ")
        )
    };
    let mut checks = vec![Check {
        id: "wingmen_near_player".to_owned(),
        applies: true,
        passed,
        detail: format!(
            "{verdict}. The player was never moved. The {MAX_WINGMAN_DISTANCE_M:.0} m threshold \
             is designed (the original's distance is unmeasured). Distances: {rows}"
        ),
    }];
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

// ------------------------------------------------------------------ records --

/// One delivered screenshot and what it measured.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRecord {
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
    /// Pixels off the window's clear colour (the mission capture's own rule).
    pub off_clear: usize,
    /// Pixels that are the window's clear colour.
    pub near_clear: usize,
    /// The frame's modal colour.
    pub modal: [u8; 3],
    /// Distinct colours among the pixels off *both* encodings of the clear
    /// colour — the world's own colours, without the background.
    pub off_distinct_colours: usize,
    /// Among those pixels, the fraction that differs from a neighbour by more
    /// than 8/255, in parts per thousand — the texture's local detail.
    pub off_detail_permille: u32,
    /// The smallest luminance among those pixels.
    pub off_luminance_min: u8,
    /// The largest luminance among those pixels.
    pub off_luminance_max: u8,
    /// The smallest luminance in the nearest terrain of this frame.
    pub near_luminance_min: u8,
    /// The largest luminance in the nearest terrain of this frame.
    pub near_luminance_max: u8,
    /// Yellow-ish objective-text pixels counted inside the objective UI region
    /// (`None` for frames the HUD probe did not measure).
    pub text_pixels: Option<usize>,
    /// Parts per thousand of the letterbox bars' rows that are black.
    pub bars_black_permille: u32,
    /// Pixels of the cockpit HUD's own green.
    pub hud_green_pixels: usize,
}

/// The finished run, for the caller of [`super::run_windowed`].
#[derive(Clone, Debug)]
pub struct MissionSmokeReport {
    /// The mission label the run flew under.
    pub mission: String,
    /// Rendered frames.
    pub frames: u32,
    /// Fixed physics ticks.
    pub ticks: u64,
    /// Every check, in order.
    pub checks: Vec<Check>,
    /// The measurements behind the checks.
    pub measurements: SmokeMeasurements,
    /// The screenshots taken, with their measurements.
    pub shots: Vec<FrameRecord>,
    /// The placements the run made, verbatim.
    pub placements: Vec<String>,
    /// Every failing check's id, in order.
    pub failures: Vec<String>,
}

/// Shared slot the screenshot observers fill.
#[derive(Resource, Clone, Default)]
struct SmokeShots(Arc<Mutex<Vec<FrameRecord>>>);

/// One delivered framebuffer's raw pixels, kept for the Flight Check's
/// full-frame / bare-pane comparison.
#[derive(Clone, Debug)]
struct RawFrame {
    label: String,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// The raw Flight Check framebuffers the observers fill.
#[derive(Resource, Clone, Default)]
struct FlightCheckFrames(Arc<Mutex<Vec<RawFrame>>>);

/// What one overlay node of the Flight Check showed at the full frame.
#[derive(Clone, Debug, PartialEq)]
struct DrawnRow {
    row: String,
    text: Option<String>,
    visible: bool,
    region: [u32; 4],
}

/// The shared slot the finished run lands in.
#[derive(Clone, Default)]
pub struct MissionSmokeHandle(Arc<Mutex<Option<Result<MissionSmokeReport, String>>>>);

impl MissionSmokeHandle {
    /// The outcome once the run has ended.
    #[must_use]
    pub fn take(&self) -> Option<Result<MissionSmokeReport, String>> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
}

// ------------------------------------------------------------------- plugin --

/// The smoke as a plugin: added to the windowed mission composition when
/// `--smoke-seconds` was given.
pub struct MissionSmokePlugin {
    request: crate::playtest::SmokeRequest,
    mode: MissionSmokeMode,
    handle: MissionSmokeHandle,
}

impl MissionSmokePlugin {
    /// A smoke run that writes into `request.capture_dir`.
    #[must_use]
    pub fn new(request: crate::playtest::SmokeRequest) -> Self {
        Self {
            request,
            mode: MissionSmokeMode::Scripted,
            handle: MissionSmokeHandle::default(),
        }
    }

    /// The same run in `mode`.
    #[must_use]
    pub fn with_mode(mut self, mode: MissionSmokeMode) -> Self {
        self.mode = mode;
        self
    }

    /// The slot the finished report lands in.
    #[must_use]
    pub fn handle(&self) -> MissionSmokeHandle {
        self.handle.clone()
    }
}

/// What the probe is currently hiding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ProbeMode {
    /// Nothing is hidden.
    #[default]
    Restored,
    /// Hide the horizon, the weather, the precipitation, the player and the
    /// UI: the frame's off-clear pixels are the world's own geometry.
    TerrainOnly,
    /// Hide the horizon's dome, the weather, the precipitation, the player and
    /// the UI: the frame holds the sea plane and the world.
    SeaVisible,
    /// [`Self::SeaVisible`] minus the sea plane: the world's water tiles remain.
    SeaAbsent,
    /// [`Self::SeaAbsent`] minus the world's own water tiles: no sea at all.
    SeaTilesAbsent,
    /// Hide everything drawn except this entity (`None`: the player only).
    Actor(Option<Entity>),
    /// Hide the Flight Check's overlay (buttons, labels, text rows, notice)
    /// and keep its background pane: the bare pane the full frame is compared
    /// with.
    FlightCheckBare,
}

#[derive(Resource, Clone, Copy, Debug, Default)]
struct ProbeState(ProbeMode);

/// A key or pause the state machine wants applied on the next frame, when the
/// F22 pump reads the keyboard state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyOp {
    Press(KeyCode),
    Release(KeyCode),
    Pause,
    Resume,
}

#[derive(Resource, Default)]
struct KeyQueue(Vec<KeyOp>);

/// The pose a placement wants written onto the production player body.
struct Placement {
    /// The census member the placement is for (`None`: the trigger approach).
    name: String,
    position_m: [f32; 3],
    yaw: f32,
    speed_m_s: f32,
    distance_m: f64,
}

#[derive(Resource, Default)]
struct PendingPlacement(Option<Placement>);

#[derive(Resource, Clone)]
struct SmokeConfig {
    request: crate::playtest::SmokeRequest,
    mode: MissionSmokeMode,
    handle: MissionSmokeHandle,
}

/// The run's own state, filled by [`smoke_step`].
#[derive(Resource, Default)]
struct SmokeData {
    /// Frames since Fly Mission: the script's clock, which does not run while
    /// the Flight Check holds the mission.
    frame: u32,
    /// Frames the Flight Check has been up.
    flight_check_frame: u32,
    /// The overlay nodes read at the Flight Check's full frame.
    flight_check_rows: Vec<DrawnRow>,
    phase: Phase,
    phase_start: u32,
    census: Vec<CensusEntry>,
    measured_start_m: Option<[f32; 3]>,
    anchor_position_m: Option<Vec3>,
    water_objects: usize,
    water_tile_parts: usize,
    speed_m_s: f32,
    measurements: SmokeMeasurements,
    placements: Vec<String>,
    requested: Vec<String>,
    shots_requested: usize,
    effects_at_hold_start: Option<u64>,
    hud_region: Option<[u32; 4]>,
    rounds_probe_pending: bool,
    /// Where the launch cinematic's first sampled frame found the body.
    cinematic_start_m: Option<Vec3>,
    /// Where the launch cinematic's first sampled frame found the body's attitude.
    cinematic_start_rotation: Option<Quat>,
    /// The cinematic screenshots already requested.
    cinematic_shots: u8,
    /// The frame the sampler saw the cinematic hand over.
    cinematic_ended_at: Option<u32>,
    /// Scene screenshots requested so far, and the ones waiting for the script.
    camera_shots: usize,
    camera_shots_due: Vec<String>,
    /// The first control frame's screenshot is waiting for the script.
    first_control_due: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Settle,
    /// The launch cinematic's held phase: the run watches it play out and
    /// takes its screenshots before the scripted checks begin.
    Cinematic,
    WorldProbe,
    SeaProbe,
    Actors,
    Approach,
    Hud,
    Guns,
    Rocket,
    Final,
    Ended,
}

impl Plugin for MissionSmokePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SmokeConfig {
            request: self.request.clone(),
            mode: self.mode,
            handle: self.handle.clone(),
        })
        .init_resource::<SmokeShots>()
        .init_resource::<FlightCheckFrames>()
        .init_resource::<SmokeData>()
        .init_resource::<ProbeState>()
        .init_resource::<KeyQueue>()
        .init_resource::<PendingPlacement>()
        // After Bevy's own keyboard system and before the F22 pump: the
        // script's keys are the keyboard state the pump reads this frame.
        .add_systems(
            PreUpdate,
            drive_script
                .in_set(InputSystems)
                .after(bevy::input::keyboard::keyboard_input_system),
        )
        .add_systems(Update, (apply_placement, apply_probe, keep_focus))
        // The probe runs again in `PostUpdate`: a system in `Update` may have
        // respawned an actor label (the #1538 contact list redraws its rows)
        // *after* the first pass, and nothing after `PostUpdate` writes
        // visibility, so this is the state the frame's render sees.
        .add_systems(PostUpdate, apply_probe);
        add_script_systems(app);
    }
}

/// The script's per-frame systems, in the order they must run.
fn add_script_systems(app: &mut App) {
    app.add_systems(
        Last,
        (
            smoke_flight_check,
            smoke_census,
            smoke_sample,
            smoke_wingmen,
            smoke_cinematic,
            smoke_first_control,
            smoke_script,
            smoke_rounds,
            smoke_flashes,
        )
            .chain(),
    );
}

/// An unfocused window would pause the run through the real focus policy; a
/// smoke is automated, so the session stays focused.
fn keep_focus(mut platform: ResMut<PlatformInput>) {
    if !platform.session().is_focused() {
        platform.session_mut().set_focus(true);
    }
}

fn drive_script(
    mut queue: ResMut<KeyQueue>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut platform: ResMut<PlatformInput>,
) {
    for op in std::mem::take(&mut queue.0) {
        match op {
            KeyOp::Press(key) => keys.press(key),
            KeyOp::Release(key) => keys.release(key),
            KeyOp::Pause => {
                platform.session_mut().pause(PauseReason::PlayerRequest);
            }
            KeyOp::Resume => {
                platform.session_mut().resume();
            }
        }
    }
}

/// Writes a placement onto the production player body. The F24 law re-seeds
/// its state from the body every fixed tick (`crate::playtest`'s
/// `drive_original_flight`), so the pose written here is the pose the next
/// tick flies from; the camera is re-seated so the chase rig does not spend
/// the probe swinging across the world.
fn apply_placement(
    mut pending: ResMut<PendingPlacement>,
    mut player: Query<
        (
            &mut Position,
            &mut Rotation,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        With<MissionPlayerBody>,
    >,
    mut camera: ResMut<PlaytestCamera>,
    mut data: ResMut<SmokeData>,
) {
    let Some(placement) = pending.0.take() else {
        return;
    };
    let Ok((mut position, mut rotation, mut velocity, mut angular)) = player.single_mut() else {
        return;
    };
    let heading = Quat::from_rotation_y(placement.yaw);
    position.0 = Vec3::from_array(placement.position_m);
    rotation.0 = heading;
    velocity.0 = heading * Vec3::NEG_Z * placement.speed_m_s;
    angular.0 = Vec3::ZERO;
    camera.reseat();
    data.placements.push(format!(
        "{}: {:.0} m from its target at [{:.1}, {:.1}, {:.1}], heading {:.3} rad, {:.0} m/s",
        placement.name,
        placement.distance_m,
        placement.position_m[0],
        placement.position_m[1],
        placement.position_m[2],
        placement.yaw,
        placement.speed_m_s,
    ));
}

/// Applies the probe's hide mode to every entity class it covers. A value is
/// written only when it changes, so the renderer is not re-extracted every
/// frame. An actor probe then restores exactly the one entity it keeps — the
/// player's own body for the player's segment, the residency's visual for a
/// zeppelin's.
#[allow(clippy::too_many_arguments)]
fn apply_probe(
    mode: Res<ProbeState>,
    mut visibility: Query<&mut Visibility>,
    domes: Query<Entity, With<HorizonDome>>,
    seas: Query<Entity, With<HorizonSea>>,
    water_tiles: Query<Entity, With<WorldWaterPart>>,
    weather: Query<Entity, With<WeatherLayer>>,
    precip: Query<Entity, With<PrecipitationLayer>>,
    players: Query<Entity, With<PlaytestAircraft>>,
    aircraft: Query<Entity, With<ScriptedAircraftActor>>,
    zeppelins: Query<Entity, With<MissionZeppelinActor>>,
    visuals: Query<Entity, With<WorldVisual>>,
    ui: Query<
        (
            Entity,
            Has<crate::playtest::visuals::DeveloperOverlayNode>,
            Has<super::launch_cinematic::CinematicOwnedVisibility>,
        ),
        With<Node>,
    >,
    flight_check_overlay: Query<Entity, With<ScreenOverlay>>,
    overlay: Option<Res<crate::playtest::visuals::DeveloperOverlay>>,
    mut fogs: Query<&mut DistanceFog>,
    mut kept_fog: Local<Option<FogFalloff>>,
) {
    let mode = mode.0;
    let hidden = Visibility::Hidden;
    let shown = Visibility::Inherited;
    let anything_hidden = mode != ProbeMode::Restored;
    // The sea plane is drawn geometry, so every probe that isolates something
    // else hides it — only the sea probe's own "visible" frame keeps it.
    let sea_value = if matches!(mode, ProbeMode::Restored | ProbeMode::SeaVisible) {
        shown
    } else {
        hidden
    };
    // Only an actor probe hides the world: every other probe exists to
    // measure the world (or the sea plane) itself.
    let world_value = if matches!(mode, ProbeMode::Actor(_)) {
        hidden
    } else {
        shown
    };
    // An actor's drawn model hangs off its own body (#1538: the aircraft's
    // `planes.zbd` parts are children of the body, each zeppelin's world-node
    // visual a child of its binding entity), so every probe that measures
    // something else hides every actor body and lets each part inherit it.
    // The probe's own target is restored last, below.
    let body_value = if anything_hidden { hidden } else { shown };
    // The distance fog is the atmosphere's, like the dome: a record that fogs
    // the world to the haze colour within a few kilometres (M01's
    // `FOG_RANGES 1000 4500`, PARITY-M01-ATMO-B) would paint a probe's
    // terrain flat. The falloff is parked far away while a probe isolates
    // something and put back with the restored frame.
    for mut fog in &mut fogs {
        if anything_hidden {
            if kept_fog.is_none() {
                *kept_fog = Some(fog.falloff.clone());
            }
            fog.falloff = FogFalloff::Linear {
                start: 1.0e9,
                end: 2.0e9,
            };
        } else if let Some(falloff) = kept_fog.take() {
            fog.falloff = falloff;
        }
    }
    write_all(&mut visibility, domes.iter(), hidden);
    write_all(&mut visibility, weather.iter(), hidden);
    write_all(&mut visibility, precip.iter(), hidden);
    write_all(&mut visibility, seas.iter(), sea_value);
    // The world's own water tiles are world geometry, so only the probe that
    // measures them hides them; the actor probe hides the whole world anyway.
    let tile_value = if mode == ProbeMode::SeaTilesAbsent {
        hidden
    } else {
        shown
    };
    write_all(&mut visibility, water_tiles.iter(), tile_value);
    write_all(&mut visibility, players.iter(), body_value);
    write_all(&mut visibility, aircraft.iter(), body_value);
    write_all(&mut visibility, zeppelins.iter(), body_value);
    write_all(&mut visibility, visuals.iter(), world_value);
    // Restoring the UI must not re-show what the developer overlay hides.
    let overlay_on = crate::playtest::visuals::overlay_visible(overlay.as_deref());
    for (entity, developer, cinematic_owned) in &ui {
        // The HUD's safe area and the letterbox bars are shown or hidden by
        // the launch cinematic; a restore leaves them to it (#1704).
        if cinematic_owned && matches!(mode, ProbeMode::Restored | ProbeMode::FlightCheckBare) {
            continue;
        }
        let value = if mode == ProbeMode::FlightCheckBare {
            // Only the Flight Check's overlay goes; its background pane and
            // the cover behind it stay, and the mission's own UI is under
            // that cover.
            if flight_check_overlay.contains(entity) {
                hidden
            } else {
                shown
            }
        } else if developer && !overlay_on {
            hidden
        } else {
            body_value
        };
        write_all(&mut visibility, std::iter::once(entity), value);
    }
    if let ProbeMode::Actor(Some(keep)) = mode {
        write_all(&mut visibility, std::iter::once(keep), shown);
    }
}

/// Writes one visibility value onto every entity of a probe class, only where
/// it differs.
fn write_all(
    visibility: &mut Query<&mut Visibility>,
    entities: impl Iterator<Item = Entity>,
    value: Visibility,
) {
    for entity in entities {
        if let Ok(mut slot) = visibility.get_mut(entity)
            && *slot != value
        {
            *slot = value;
        }
    }
}

/// The current objective line the read model reports, through the
/// installation's own catalog.
fn current_objective(
    host: Option<&MissionHost>,
    messages: Option<&MissionMessages>,
) -> Option<String> {
    let host = host?;
    let ObjectiveLifecycles::Engine(engine) = host.lifecycles() else {
        return None;
    };
    ObjectiveHud::from_engine(engine).current_text(messages.and_then(MissionMessages::catalog))
}

/// The query the objective probe reads: UI layout lives in `UiGlobalTransform`
/// (a UI node carries no `GlobalTransform`), and what the player sees is the
/// *inherited* visibility — the probe's own restore writes `Inherited` onto
/// every node, so the node's own `Visibility` says nothing about it.
type ObjectiveNodes<'w, 's> = Query<
    'w,
    's,
    (
        &'static Text,
        &'static InheritedVisibility,
        &'static UiGlobalTransform,
        &'static ComputedNode,
    ),
>;

/// The objective widget's own text on screen and its visibility, when one
/// carries the caption.
fn drawn_objective(nodes: &ObjectiveNodes<'_, '_>) -> Option<(String, bool)> {
    let (text, visibility, _, _) = nodes
        .iter()
        .find(|(text, ..)| text.0.starts_with(OBJECTIVE_HUD_CAPTION))?;
    Some((objective_line_of(&text.0), visibility.get()))
}

/// The objective line a widget text carries: the widget draws its designed
/// caption, a newline, then the line the read model reports, so the caption is
/// not part of what the two must agree on.
#[must_use]
pub fn objective_line_of(widget_text: &str) -> String {
    let rest = widget_text
        .strip_prefix(OBJECTIVE_HUD_CAPTION)
        .unwrap_or(widget_text);
    rest.strip_prefix('\n').unwrap_or(rest).to_owned()
}

/// The objective widget's region in framebuffer pixels, from the node's
/// physical-pixel layout.
fn objective_region(nodes: &ObjectiveNodes<'_, '_>) -> Option<[u32; 4]> {
    let (_, _, transform, node) = nodes
        .iter()
        .find(|(text, ..)| text.0.starts_with(OBJECTIVE_HUD_CAPTION))?;
    let center = transform.translation;
    let size = node.size;
    Some([
        (center.x - 0.5 * size.x).max(0.0) as u32,
        (center.y - 0.5 * size.y).max(0.0) as u32,
        size.x.max(1.0) as u32,
        size.y.max(1.0) as u32,
    ])
}

/// Builds the actor census from the spawn report and the residency: the
/// player, every scripted aircraft the composition spawned, and every
/// zeppelin world actor bound — with each member's drawn entity and the
/// world's own water-object count the sea check gates on.
fn build_census(
    report: Option<&MissionActorSpawnReport>,
    player: Entity,
    zeppelins: &Query<(&MissionZeppelinActor, &WorldActorBinding, &Transform)>,
    residency: Option<&WorldResidency>,
) -> (Vec<CensusEntry>, usize) {
    let water_objects = residency.map_or(0, |residency| {
        residency
            .resident()
            .definition()
            .objects()
            .iter()
            .filter(|object| object.known_surface() == Some(SurfaceRole::Water))
            .count()
    });
    let mut census = vec![CensusEntry {
        kind: "player",
        name: "player".to_owned(),
        pose_entity: player,
        drawn_entity: Some(player),
        asleep: false,
        subject: None,
        hostile: false,
    }];
    let Some(report) = report else {
        return (census, water_objects);
    };
    for spawned in &report.aircraft {
        census.push(CensusEntry {
            kind: "aircraft",
            name: spawned.record.clone(),
            pose_entity: spawned.entity,
            // #1538 draws each scripted aircraft's `planes.zbd` model as
            // children of this body, so the body is what the probe keeps and
            // what hiding takes off the screen.
            drawn_entity: Some(spawned.entity),
            asleep: spawned.asleep,
            subject: None,
            hostile: false,
        });
    }
    for bound in &report.zeppelins {
        let hostile = zeppelins
            .iter()
            .find(|(_, binding, _)| binding.subject == bound.subject)
            .is_some_and(|(actor, _, _)| {
                actor
                    .roster
                    .as_ref()
                    .is_some_and(|roster| roster.faction.key() == "enemy")
            });
        let subject = bound.subject.key().to_owned();
        census.push(CensusEntry {
            kind: "zeppelin",
            name: subject.clone(),
            pose_entity: bound.entity,
            // #1538 parents each zeppelin's drawn world-node visual under this
            // binding entity, so the binding is the probe's keep/hide handle
            // for the airship's whole subtree.
            drawn_entity: Some(bound.entity),
            asleep: false,
            subject: Some(subject),
            hostile,
        });
    }
    (census, water_objects)
}

/// The census: built once, on the first frame, from the spawn report the
/// composition itself produced.
fn smoke_census(
    mut data: ResMut<SmokeData>,
    spawn_report: Option<Res<MissionActorSpawnReport>>,
    zeppelins: Query<(&MissionZeppelinActor, &WorldActorBinding, &Transform)>,
    player: Query<Entity, With<MissionPlayerBody>>,
    residency: Option<Res<WorldResidency>>,
    phase: Option<Res<FlightCheckPhase>>,
) {
    // The script's clock starts at Fly Mission: the 45 s budget is the
    // mission's, not the screen's.
    if phase.is_some_and(|phase| phase.is_pending()) {
        return;
    }
    data.frame += 1;
    let frame = data.frame;
    if frame != 1 {
        return;
    }
    let (census, water_objects) = build_census(
        spawn_report.as_deref(),
        player.single().ok().unwrap_or(Entity::PLACEHOLDER),
        &zeppelins,
        residency.as_deref(),
    );
    data.census = census;
    data.water_objects = water_objects;
    // The proximity anchor's own pose, remembered for the approach: the
    // declared-hostile cargo zeppelin the #1486 trigger measures against.
    let anchor_subject = anchor_subject_of(&data.census);
    if let Some(subject) = anchor_subject {
        data.anchor_position_m = zeppelins
            .iter()
            .find(|(_, binding, _)| binding.subject.key() == subject)
            .map(|(_, _, transform)| transform.translation);
    }
    let census = data.census.clone();
    for entry in &census {
        data.measurements.actors.push(ActorMetrics {
            kind: entry.kind.to_owned(),
            name: entry.name.clone(),
            asleep: entry.asleep,
            placed: false,
            placement_m: None,
            on_screen: false,
            pixels: 0,
            total_pixels: 0,
            // Fail-closed by name: a member the census found no drawn entity
            // for says so, rather than reporting an unexplained zero.
            note: entry
                .drawn_entity
                .is_none()
                .then(|| "no drawn entity was found for this member".to_owned()),
        });
    }
}

/// The live round census during the gun hold: the most rounds alive at once,
/// and whether every round entity a player would see of their own shot
/// carried a mesh in any frame they were alive.
fn smoke_rounds(
    mut data: ResMut<SmokeData>,
    rounds: Query<(Entity, &WeaponRoundMirror)>,
    meshes: Query<(), With<Mesh3d>>,
    child_of: Query<(Entity, &ChildOf)>,
) {
    if data.phase != Phase::Guns && !std::mem::take(&mut data.rounds_probe_pending) {
        return;
    }
    data.rounds_probe_pending = false;
    let live = rounds.iter().count() as u32;
    if live == 0 {
        return;
    }
    let drawn = all_rounds_drawn(&rounds, &meshes, &child_of);
    let guns = &mut data.measurements.guns;
    guns.rounds_examined = guns.rounds_examined.max(live);
    guns.rounds_drawn = Some(guns.rounds_drawn.unwrap_or(true) && drawn);
}

/// The muzzle-flash census during the gun hold: each frame with a live
/// muzzle-effect record counts once, and a record without a drawn child is
/// counted as undrawn.
fn smoke_flashes(
    mut data: ResMut<SmokeData>,
    records: Query<(Option<&bevy::prelude::Children>, &MuzzleEffectSpawn)>,
    meshes: Query<(), With<Mesh3d>>,
) {
    if data.phase != Phase::Guns || records.is_empty() {
        return;
    }
    let undrawn = records
        .iter()
        .filter(|(children, _)| {
            !children.is_some_and(|kids| kids.iter().any(|kid| meshes.get(*kid).is_ok()))
        })
        .count() as u32;
    let guns = &mut data.measurements.guns;
    guns.flash_frames += 1;
    guns.flashes_undrawn += undrawn;
}

/// Samples the live world into the measurements every frame: the audio queue,
/// the weapon session's rounds and the ordnance session's rockets.
#[allow(clippy::too_many_arguments)]
fn smoke_sample(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    state: Res<PlaytestState>,
    ledger: Res<PhysicsTickLedger>,
    weapons: Option<Res<WeaponSession>>,
    rounds: Query<&WeaponRoundMirror>,
    rockets: Query<&OrdnanceItemMirror>,
    rocket_report: Option<Res<MissionWeaponReport>>,
    audio: Option<Res<AudioSession>>,
    mix: Option<Res<AudioMixReport>>,
    bind_log: Option<Res<AudioBindLog>>,
    host: Option<Res<MissionHost>>,
    messages: Option<Res<MissionMessages>>,
) {
    if data.frame == 1 {
        data.measured_start_m = Some(state.spawn_m);
        let speed = state.telemetry.speed_m_s;
        data.speed_m_s = if speed.is_finite() && speed > 1.0 {
            speed
        } else {
            crate::playtest::retail::RETAIL_START_SPEED_M_S as f32
        };
        data.measurements.mission = "M01".to_owned();
        data.measurements.no_teleport = config.mode == MissionSmokeMode::NoTeleport;
    }
    data.measurements.ticks = ledger.ticks;
    sample_audio(
        &mut data,
        audio.as_deref(),
        mix.as_deref(),
        bind_log.as_deref(),
    );
    if data.phase == Phase::Guns {
        if let Some(weapons) = weapons.as_ref() {
            // The count of accepted shots, not the pending log: the production
            // presentation consumer (`WeaponEffectSchedulePlugin`,
            // F27-C-FX-SCHED #1304) drains the log every committed tick, so the
            // pending view is empty by the time this frame's sample runs. The
            // log's own monotone `recorded` count is what "rounds through the
            // mission's weapon session" means here.
            let effects = weapons.effects_recorded();
            let start = *data.effects_at_hold_start.get_or_insert(effects);
            data.measurements.guns.effects = effects.saturating_sub(start);
        }
        let live = rounds.iter().count() as u32;
        data.measurements.guns.max_live_rounds = data.measurements.guns.max_live_rounds.max(live);
    }
    if data.phase == Phase::Rocket {
        let live = rockets.iter().count() as u32;
        data.measurements.rocket.max_rockets = data.measurements.rocket.max_rockets.max(live);
        data.measurements.rocket.launched |= live > 0;
        if let Some(report) = rocket_report.as_ref() {
            data.measurements.rocket.refusals = report.rocket_refusals.clone();
        }
    }
    // The HUD is held back while the cinematic plays; the objective probe is
    // about what the player sees once control is theirs.
    if data.phase != Phase::Cinematic
        && let Some(line) = current_objective(host.as_deref(), messages.as_deref())
    {
        data.measurements.approach.objective_woke = true;
        if data.measurements.hud.model_line.is_none() {
            data.measurements.hud.model_line = Some(line);
        }
    }
}

/// The wingmen's distance from the player at each [`WINGMAN_SAMPLE_SECONDS`]
/// frame, read from the world transforms the mission's own census prints
/// (no-teleport mode only).
fn smoke_wingmen(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    player: Query<&GlobalTransform, With<MissionPlayerBody>>,
    aircraft: Query<(&ScriptedAircraftActor, &GlobalTransform)>,
) {
    if config.mode != MissionSmokeMode::NoTeleport {
        return;
    }
    let frame = data.frame;
    let Some(second) = WINGMAN_SAMPLE_SECONDS
        .iter()
        .copied()
        .find(|second| frame == second * 60)
    else {
        return;
    };
    let Ok(player) = player.single() else { return };
    let player = player.translation();
    let mut distances_m: Vec<(String, f64)> = aircraft
        .iter()
        .filter(|(actor, _)| actor.vehicle_record == "wingman")
        .map(|(actor, transform)| {
            (
                actor.record.name.clone(),
                f64::from(transform.translation().distance(player)),
            )
        })
        .collect();
    distances_m.sort_by(|a, b| a.0.cmp(&b.0));
    data.measurements.wingmen.push(WingmanSample {
        second,
        distances_m,
    });
}

/// Follows the launch cinematic phase into the measurements and moves the
/// script into it on the first frame (scripted mode only: the no-teleport
/// mode measures the flight from the first frame and skips the phase).
fn smoke_cinematic(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    phase: Option<Res<LaunchCinematic>>,
    bars: Option<Res<LaunchLetterbox>>,
    player: Query<(&Position, &Rotation), With<MissionPlayerBody>>,
    playback: Option<Res<super::launch_camera::LaunchCameraPlayback>>,
) {
    let Some(phase) = phase else { return };
    if config.mode != MissionSmokeMode::Scripted {
        return;
    }
    if data.frame == 1 {
        data.phase = Phase::Cinematic;
        data.phase_start = 0;
        data.measurements.cinematic = Some(CinematicMetrics {
            total_ticks: phase.total_ticks(),
            bars_on_while_playing: true,
            bars_off_after: true,
            ..CinematicMetrics::default()
        });
    }
    let Some(mut metrics) = data.measurements.cinematic.take() else {
        return;
    };
    let frame = data.frame;
    let pose = player
        .single()
        .ok()
        .map(|(position, rotation)| (position.0, rotation.0));
    let bars_on = bars.as_deref().is_some_and(LaunchLetterbox::is_active);
    if phase.is_playing() {
        metrics.bars_on_while_playing &= bars_on;
        if let Some((position, rotation)) = pose {
            let start = *data.cinematic_start_m.get_or_insert(position);
            metrics.max_drift_m = metrics.max_drift_m.max(position.distance(start));
            let first = *data.cinematic_start_rotation.get_or_insert(rotation);
            metrics.max_turn_deg = metrics
                .max_turn_deg
                .max(first.angle_between(rotation).to_degrees());
        }
    } else if data.cinematic_ended_at.is_none() {
        data.cinematic_ended_at = Some(frame);
        metrics.handed_over = true;
    } else if frame > data.cinematic_ended_at.unwrap_or(frame) {
        metrics.bars_off_after &= !bars_on;
    }
    metrics.played_ticks = phase.elapsed_ticks();
    data.measurements.cinematic = Some(metrics);
    // The camera script: the scene screenshots wait for the script's own
    // seconds, counted in the phase's fixed ticks.
    if let Some(playback) = playback.as_deref() {
        let hz = u64::from(crate::physics::BASELINE_FIXED_HZ);
        while let Some(seconds) = CINEMATIC_SHOT_SECONDS.get(data.camera_shots) {
            if !phase.is_playing() || phase.elapsed_ticks() < u64::from(*seconds) * hz {
                break;
            }
            data.camera_shots_due.push(format!("scene_{seconds:02}s"));
            data.camera_shots += 1;
        }
        data.measurements.camera = Some(CameraMetrics {
            camera: playback.stats(),
            shots: data.camera_shots,
        });
    }
}

/// Reads the player's pose at the first control frame (#1697) and asks the
/// script for its framebuffer. It runs before the script in the same frame, and
/// the script makes no move before `Phase::Settle` ends, so the pose is the
/// player's own.
fn smoke_first_control(
    mut data: ResMut<SmokeData>,
    phase: Option<Res<LaunchCinematic>>,
    ledger: Res<PhysicsTickLedger>,
    pending: Res<PendingPlacement>,
    player: Query<(&Position, &Rotation), With<MissionPlayerBody>>,
) {
    if data.measurements.first_control.is_some()
        || data.frame == 0
        || phase.is_some_and(|phase| phase.is_playing())
    {
        return;
    }
    let Ok((position, rotation)) = player.single() else {
        return;
    };
    let (yaw, pitch, roll) = rotation.0.to_euler(bevy::math::EulerRot::YXZ);
    let forward = rotation.0 * Vec3::NEG_Z;
    data.measurements.first_control = Some(FirstControlMetrics {
        tick: ledger.ticks,
        frame: data.frame,
        position_m: [
            f64::from(position.0.x),
            f64::from(position.0.y),
            f64::from(position.0.z),
        ],
        yaw_deg: yaw.to_degrees(),
        pitch_deg: pitch.to_degrees(),
        roll_deg: roll.to_degrees(),
        forward: forward.to_array(),
        moves_before: data.placements.len() + usize::from(pending.0.is_some()),
    });
    data.first_control_due = true;
}

#[allow(clippy::too_many_arguments)]
fn smoke_script(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    mut probe: ResMut<ProbeState>,
    mut queue: ResMut<KeyQueue>,
    mut pending: ResMut<PendingPlacement>,
    shots: Res<SmokeShots>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    clear: Option<Res<ClearColor>>,
    objective_nodes: ObjectiveNodes<'_, '_>,
    overlay: Option<Res<crate::playtest::visuals::DeveloperOverlay>>,
    player: Query<&Position, With<MissionPlayerBody>>,
    poses: Query<(Option<&Position>, Option<&Transform>)>,
    camera: Query<(&Camera, &GlobalTransform), With<crate::playtest::PlaytestCameraMarker>>,
    water_tiles: Query<(), With<WorldWaterPart>>,
) {
    let frame = data.frame;
    if frame == 1 {
        // A capture directory that cannot be created surfaces later, when the
        // artifacts are written: the refusal names the path either way.
        let _ = std::fs::create_dir_all(&config.request.capture_dir);
    }

    // --- the script --------------------------------------------------------
    let mut due: Vec<String> = Vec::new();
    if std::mem::take(&mut data.first_control_due) {
        due.push("first_control".to_owned());
    }
    let at = frame.saturating_sub(data.phase_start);
    match data.phase {
        // The no-teleport script has no phases to walk: the player flies the
        // measured start, `PendingPlacement` is never filled, and one frame
        // is taken after the last wingman sample.
        _ if config.mode == MissionSmokeMode::NoTeleport => {
            if data.phase != Phase::Final
                && data.phase != Phase::Ended
                && frame >= NO_TELEPORT_SHOT_FRAME
            {
                due.push("flight".to_owned());
                data.phase = Phase::Final;
                data.phase_start = frame + 1;
            }
        }
        Phase::Settle if at >= SETTLE_FRAMES => {
            data.phase = Phase::WorldProbe;
            data.phase_start = frame + 1;
        }
        Phase::Settle => {}
        Phase::Cinematic => {
            let (played, total) = data
                .measurements
                .cinematic
                .as_ref()
                .map_or((0, 0), |c| (c.played_ticks, c.total_ticks));
            let ended_for = data
                .cinematic_ended_at
                .map(|ended| frame.saturating_sub(ended));
            let wanted = [
                (
                    CINEMATIC_SHOT_START,
                    "cinematic_start",
                    at >= CINEMATIC_FIRST_SHOT_FRAME,
                ),
                (CINEMATIC_SHOT_MID, "cinematic_mid", played >= total / 2),
                (
                    CINEMATIC_SHOT_END,
                    "cinematic_end",
                    played + CINEMATIC_END_MARGIN_TICKS >= total,
                ),
            ];
            due.append(&mut data.camera_shots_due);
            if ended_for.is_none() {
                for (bit, label, ready) in wanted {
                    if ready && data.cinematic_shots & bit == 0 {
                        data.cinematic_shots |= bit;
                        due.push(label.to_owned());
                    }
                }
            } else if ended_for.is_some_and(|frames| frames >= CINEMATIC_HANDOVER_SHOT_FRAMES)
                && data.cinematic_shots & CINEMATIC_SHOT_HANDOVER == 0
            {
                data.cinematic_shots |= CINEMATIC_SHOT_HANDOVER;
                due.push("cinematic_handover".to_owned());
                data.phase = Phase::Settle;
                data.phase_start = frame + 1;
            }
            // A phase that never ends is a failed check, not a hung run.
            if at > u32::try_from(total).unwrap_or(u32::MAX) + CINEMATIC_GRACE_FRAMES {
                data.phase = Phase::Settle;
                data.phase_start = frame + 1;
            }
        }
        Phase::WorldProbe => {
            if at == WORLD_HIDE_OFFSET {
                probe.0 = ProbeMode::TerrainOnly;
            } else if at == WORLD_SHOT_OFFSET {
                due.push("world".to_owned());
            } else if at == WORLD_RESTORE_OFFSET {
                probe.0 = ProbeMode::Restored;
                data.phase = Phase::SeaProbe;
                data.phase_start = frame + 1;
            }
        }
        Phase::SeaProbe => {
            if at == SEA_PAUSE_OFFSET {
                data.water_tile_parts = water_tiles.iter().count();
                queue.0.push(KeyOp::Pause);
                probe.0 = ProbeMode::SeaVisible;
            } else if at == SEA_VISIBLE_SHOT_OFFSET {
                due.push("sea_visible".to_owned());
            } else if at == SEA_HIDE_OFFSET {
                probe.0 = ProbeMode::SeaAbsent;
            } else if at == SEA_ABSENT_SHOT_OFFSET {
                due.push("sea_absent".to_owned());
            } else if at == SEA_TILES_HIDE_OFFSET {
                probe.0 = ProbeMode::SeaTilesAbsent;
            } else if at == SEA_TILES_ABSENT_SHOT_OFFSET {
                due.push("sea_tiles_absent".to_owned());
            } else if at == SEA_RESTORE_OFFSET {
                probe.0 = ProbeMode::Restored;
            } else if at == SEA_RESUME_OFFSET {
                queue.0.push(KeyOp::Resume);
                data.phase = if data.census.len() > 1 {
                    Phase::Actors
                } else {
                    Phase::Approach
                };
                data.phase_start = frame + 1;
            }
        }
        Phase::Actors => {
            let index = (at / ACTOR_SEGMENT_FRAMES) as usize;
            let step = at % ACTOR_SEGMENT_FRAMES;
            if index >= data.census.len() {
                data.phase = Phase::Approach;
                data.phase_start = frame + 1;
            } else {
                let entry = data.census.get(index).cloned();
                let is_player = entry.as_ref().is_some_and(|entry| entry.kind == "player");
                if step == ACTOR_PLACE_OFFSET && !is_player {
                    if let Some(placement) = place_near_actor(&data, index, &player, &poses) {
                        pending.0 = Some(placement);
                        if let Some(metrics) = data.measurements.actors.get_mut(index) {
                            metrics.placed = true;
                            metrics.placement_m = Some(f64::from(PLACEMENT_OFFSET_M));
                        }
                    }
                } else if step == ACTOR_HIDE_OFFSET {
                    probe.0 = ProbeMode::Actor(entry.as_ref().and_then(|entry| entry.drawn_entity));
                } else if step == ACTOR_SHOT_OFFSET {
                    let on_screen = entry.as_ref().is_some_and(|entry| {
                        live_position(entry, &player, &poses)
                            .and_then(|position| project_onto_screen(position, &camera))
                            .is_some()
                    });
                    if let Some(metrics) = data.measurements.actors.get_mut(index) {
                        metrics.on_screen = on_screen;
                    }
                    due.push(format!("actor_{index}"));
                } else if step == ACTOR_RESTORE_OFFSET {
                    probe.0 = ProbeMode::Restored;
                }
            }
        }
        Phase::Approach => {
            if at == 0
                && let Some(placement) = place_for_trigger(&mut data)
            {
                pending.0 = Some(placement);
                data.measurements.approach.placed = true;
            }
            if at > APPROACH_BUDGET_FRAMES
                || (data.measurements.approach.objective_woke && at > APPROACH_MIN_FRAMES)
            {
                data.measurements.approach.frames = at;
                data.phase = Phase::Hud;
                data.phase_start = frame + 1;
            }
        }
        Phase::Hud => {
            if at == 0 {
                queue.0.push(KeyOp::Pause);
            } else if at == HUD_SHOT_OFFSET {
                data.hud_region = objective_region(&objective_nodes);
                data.measurements.hud.overlay_hidden =
                    !crate::playtest::visuals::overlay_visible(overlay.as_deref());
                if let Some((line, visible)) = drawn_objective(&objective_nodes) {
                    data.measurements.hud.drawn_line = Some(line);
                    data.measurements.hud.drawn_visible = visible;
                }
                due.push("hud".to_owned());
            } else if at == HUD_SHOT_OFFSET + 10 {
                queue.0.push(KeyOp::Resume);
                data.phase = Phase::Guns;
                data.phase_start = frame + 1;
                data.effects_at_hold_start = None;
                queue.0.push(KeyOp::Press(KeyCode::Space));
            }
        }
        Phase::Guns => {
            if at == GUNS_SHOT_OFFSET {
                // The round census itself runs in `smoke_rounds`, which walks
                // the hierarchy once instead of every frame.
                data.rounds_probe_pending = true;
                due.push("guns".to_owned());
            } else if at == GUNS_HOLD_FRAMES {
                queue.0.push(KeyOp::Release(KeyCode::Space));
                data.phase = Phase::Rocket;
                data.phase_start = frame + 1;
                queue.0.push(KeyOp::Press(KeyCode::ControlLeft));
            }
        }
        Phase::Rocket => {
            if at == 8 {
                queue.0.push(KeyOp::Release(KeyCode::ControlLeft));
            } else if at == ROCKET_SHOT_OFFSET {
                due.push("rocket".to_owned());
            } else if at == ROCKET_WAIT_FRAMES {
                data.phase = Phase::Final;
                data.phase_start = frame + 1;
            }
        }
        Phase::Final | Phase::Ended => {}
    }

    for label in &due {
        data.shots_requested += 1;
        data.requested.push(label.clone());
    }
    let region = data.hud_region;
    let clear_colours = clear.as_deref().map(clear_colour_encodings);
    for label in &due {
        let path = config
            .request
            .capture_dir
            .join(format!("frame_{frame:05}_{label}.png"));
        let measured = if label == "hud" { region } else { None };
        request_screenshot(
            &mut commands,
            shots.clone(),
            path,
            label.clone(),
            clear_colours,
            measured,
            None,
        );
    }

    // --- the end -----------------------------------------------------------
    let delivered = shots.0.lock().map_or(0, |shots| shots.len());
    let grace_over = data.phase == Phase::Final && frame >= data.phase_start + SHOT_GRACE_FRAMES;
    let script_done = data.phase == Phase::Final && delivered >= data.shots_requested;
    if (script_done || grace_over) && frame > data.phase_start {
        end_run(&config, &mut data, &shots, &mut exit, frame);
    }
}

/// Ends the run: the report and the PNGs are written, the outcome lands in the
/// handle and the app exits.
fn end_run(
    config: &SmokeConfig,
    data: &mut SmokeData,
    shots: &SmokeShots,
    exit: &mut MessageWriter<AppExit>,
    frame: u32,
) {
    let delivered: Vec<FrameRecord> = shots
        .0
        .lock()
        .map(|shots| shots.clone())
        .unwrap_or_default();
    data.measurements.missing_shots = data
        .requested
        .iter()
        .filter(|label| !delivered.iter().any(|shot| &shot.label == *label))
        .cloned()
        .collect();
    data.measurements.frames = frame;
    finish_measurements(data, &delivered);
    let outcome = write_artifacts(&config.request.capture_dir, data, &delivered)
        .map(|()| build_report(data, delivered))
        .map_err(|(path, error)| format!("cannot write {}: {error}", path.display()));
    if let Ok(mut slot) = config.handle.0.lock() {
        *slot = Some(outcome);
    }
    data.phase = Phase::Ended;
    exit.write(AppExit::Success);
}

/// The Flight Check stage of the run (#1672): while the phase holds the
/// mission this system lets the screen settle, takes the full framebuffer,
/// hides the overlay and takes the bare pane, judges both against the scene the
/// `FlightCheckView` built, then delivers Fly Mission through the phase. The
/// script's own clock (`SmokeData::frame`) starts when the phase releases.
#[allow(clippy::too_many_arguments)]
fn smoke_flight_check(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    phase: Option<ResMut<FlightCheckPhase>>,
    mut probe: ResMut<ProbeState>,
    shots: Res<SmokeShots>,
    frames: Res<FlightCheckFrames>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    clear: Option<Res<ClearColor>>,
    overlay_nodes: Query<(
        &ScreenOverlay,
        Option<&Text>,
        &InheritedVisibility,
        &UiGlobalTransform,
        &ComputedNode,
    )>,
) {
    let Some(mut phase) = phase else { return };
    if !phase.is_pending() {
        return;
    }
    data.flight_check_frame += 1;
    let at = data.flight_check_frame;
    if at == 1 {
        let _ = std::fs::create_dir_all(&config.request.capture_dir);
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
                .join(format!("frame_fc{at:05}_{label}.png")),
            label.to_owned(),
            clear_colours,
            None,
            Some(frames.clone()),
        );
    };
    if at == FLIGHT_CHECK_SHOT_OFFSET {
        data.flight_check_rows = overlay_nodes
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
        request("flight_check", &mut data);
    } else if at == FLIGHT_CHECK_BARE_OFFSET {
        probe.0 = ProbeMode::FlightCheckBare;
    } else if at == FLIGHT_CHECK_BARE_SHOT_OFFSET {
        request("flight_check_bare", &mut data);
    }
    if at <= FLIGHT_CHECK_BARE_SHOT_OFFSET {
        return;
    }
    let arrived = frames.0.lock().map_or(0, |frames| frames.len());
    if arrived < 2 && at < FLIGHT_CHECK_BARE_SHOT_OFFSET + SHOT_GRACE_FRAMES {
        return;
    }
    probe.0 = ProbeMode::Restored;
    let raw = frames
        .0
        .lock()
        .map(|frames| frames.clone())
        .unwrap_or_default();
    let mut metrics = flight_check_metrics(
        &phase,
        &data.flight_check_rows,
        raw.iter().find(|frame| frame.label == "flight_check"),
        raw.iter().find(|frame| frame.label == "flight_check_bare"),
    );
    match phase.press_enter() {
        PhaseResponse::Launched => metrics.launched = true,
        PhaseResponse::Refused(reason) => metrics.refusal = Some(reason),
        other => metrics.refusal = Some(format!("{other:?}")),
    }
    let refused = !metrics.launched;
    data.measurements.flight_check = Some(metrics);
    if refused {
        // The mission stays held, so the script would never start: the run
        // ends here with the failing check.
        end_run(&config, &mut data, &shots, &mut exit, 0);
    }
}

/// Judges the Flight Check frames against the scene (pure; the system above
/// supplies the frames and the nodes it read).
fn flight_check_metrics(
    phase: &FlightCheckPhase,
    rows: &[DrawnRow],
    full: Option<&RawFrame>,
    bare: Option<&RawFrame>,
) -> FlightCheckMetrics {
    let scene = phase.scene();
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
    FlightCheckMetrics {
        frames_delivered: pair.is_some(),
        frame_size: full.map_or((0, 0), |frame| (frame.width, frame.height)),
        drawn_permille: full.map_or(0, |frame| {
            non_black_permille(&frame.data, frame.width, frame.height)
        }),
        texts: scene
            .texts
            .iter()
            .filter(|text| !text.text.is_empty())
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
            .collect(),
        buttons: scene
            .buttons
            .iter()
            .map(|button| {
                let drawn = find(&button.hotspot);
                let region = drawn.map_or([0; 4], |drawn| drawn.region);
                FlightCheckButtonMetrics {
                    hotspot: button.hotspot.clone(),
                    drawn: drawn.is_some_and(|drawn| drawn.visible),
                    region,
                    pixels: pixels(region),
                }
            })
            .collect(),
        launched: false,
        refusal: None,
    }
}

/// Pixels inside `region` (`[x, y, width, height]`, clamped to the frame) that
/// differ between two same-sized RGBA frames by more than a few levels in any
/// colour channel. The alpha channel is ignored.
#[must_use]
pub fn region_difference(a: &[u8], b: &[u8], width: u32, height: u32, region: [u32; 4]) -> usize {
    let [x, y, w, h] = region;
    let x_end = x.saturating_add(w).min(width);
    let y_end = y.saturating_add(h).min(height);
    let mut count = 0;
    for row in y.min(height)..y_end {
        for column in x.min(width)..x_end {
            let at = (row as usize * width as usize + column as usize) * 4;
            let (Some(p), Some(q)) = (a.get(at..at + 3), b.get(at..at + 3)) else {
                continue;
            };
            if p.iter()
                .zip(q)
                .any(|(p, q)| p.abs_diff(*q) > FLIGHT_CHECK_PIXEL_DELTA)
            {
                count += 1;
            }
        }
    }
    count
}

/// The share of an RGBA frame that is off black, in parts per thousand.
fn non_black_permille(data: &[u8], width: u32, height: u32) -> u32 {
    let total = width as usize * height as usize;
    let lit = data
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| {
            pixel[..3]
                .iter()
                .any(|channel| *channel > FLIGHT_CHECK_PIXEL_DELTA)
        })
        .count();
    permille(lit, total)
}

/// Places the player `PLACEMENT_OFFSET_M` abeam a census member, heading at
/// it, at the member's own altitude.
fn place_near_actor(
    data: &SmokeData,
    index: usize,
    player: &Query<&Position, With<MissionPlayerBody>>,
    poses: &Query<(Option<&Position>, Option<&Transform>)>,
) -> Option<Placement> {
    let entry = data.census.get(index)?;
    let target = live_position(entry, player, poses)?;
    let offset = Vec3::new(PLACEMENT_OFFSET_M, 0.0, 0.0);
    let position = target + offset;
    Some(Placement {
        name: entry.name.clone(),
        position_m: position.to_array(),
        yaw: yaw_toward(-offset),
        speed_m_s: data.speed_m_s,
        distance_m: f64::from(PLACEMENT_OFFSET_M),
    })
}

/// Places the player `APPROACH_M` from the declared-hostile cargo zeppelin on
/// the line toward the measured start, heading at the anchor — the #1486
/// placement, so the proximity trigger can only fire after the body has
/// really flown inside its radius.
fn place_for_trigger(data: &mut SmokeData) -> Option<Placement> {
    let anchor = data.anchor_position_m?;
    let subject = anchor_subject_of(&data.census)?;
    let start = data.measured_start_m?;
    let from_start = Vec3::from_array(start) - anchor;
    let horizontal = Vec3::new(from_start.x, 0.0, from_start.z);
    let direction = if horizontal.length_squared() < 1.0 {
        Vec3::NEG_Z
    } else {
        horizontal.normalize()
    };
    data.measurements.approach.anchor = Some(subject.clone());
    let position = anchor + direction * APPROACH_M;
    Some(Placement {
        name: format!("trigger approach to {subject}"),
        position_m: position.to_array(),
        yaw: yaw_toward(-direction),
        speed_m_s: data.speed_m_s,
        distance_m: APPROACH_M_F64,
    })
}

/// The yaw that points Bevy's forward (`-Z`) at `direction`'s horizontal
/// component.
fn yaw_toward(direction: Vec3) -> f32 {
    (-direction.x).atan2(-direction.z)
}

/// The live world position of a census member.
fn live_position(
    entry: &CensusEntry,
    player: &Query<&Position, With<MissionPlayerBody>>,
    poses: &Query<(Option<&Position>, Option<&Transform>)>,
) -> Option<Vec3> {
    if entry.kind == "player" {
        return player.single().ok().map(|position| position.0);
    }
    let (position, transform) = poses.get(entry.pose_entity).ok()?;
    position
        .map(|position| position.0)
        .or_else(|| transform.map(|transform| transform.translation))
}

/// Where a world position lands in the viewport, when the camera can say —
/// `None` when it is behind the camera or outside the rendered viewport.
fn project_onto_screen(
    position: Vec3,
    camera: &Query<(&Camera, &GlobalTransform), With<crate::playtest::PlaytestCameraMarker>>,
) -> Option<Vec2> {
    let (camera, transform) = camera.single().ok()?;
    let viewport = camera.world_to_viewport(transform, position).ok()?;
    let size = camera.physical_viewport_size()?;
    if viewport.x < 0.0
        || viewport.y < 0.0
        || viewport.x > size.x as f32
        || viewport.y > size.y as f32
    {
        return None;
    }
    Some(viewport)
}

/// Whether every live round entity carries a mesh, itself or on a child: the
/// picture a player would see of a shot.
fn all_rounds_drawn(
    rounds: &Query<(Entity, &WeaponRoundMirror)>,
    meshes: &Query<(), With<Mesh3d>>,
    child_of: &Query<(Entity, &ChildOf)>,
) -> bool {
    let mut children: HashMap<Entity, Vec<Entity>> = HashMap::new();
    for (child, parent) in child_of.iter() {
        children.entry(parent.parent()).or_default().push(child);
    }
    rounds.iter().all(|(entity, _)| {
        if meshes.get(entity).is_ok() {
            return true;
        }
        let mut frontier = vec![entity];
        for _ in 0..4 {
            let mut next = Vec::new();
            for parent in frontier.drain(..) {
                for kid in children.get(&parent).into_iter().flatten().copied() {
                    if meshes.get(kid).is_ok() {
                        return true;
                    }
                    next.push(kid);
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        false
    })
}

/// Samples the audio state into the measurements every frame.
fn sample_audio(
    data: &mut SmokeData,
    audio: Option<&AudioSession>,
    mix: Option<&AudioMixReport>,
    bind_log: Option<&AudioBindLog>,
) {
    let bindings = bind_log.map_or(0, |log| {
        log.bound
            .iter()
            .filter(|record| record.role == AudioEmitterRole::Engine)
            .count()
    });
    let loops = audio.map_or(0, |session| session.router.active_loop_count());
    let radio_started = mix.map_or(0, |mix| mix.radio.started.len());
    let radio_active = audio
        .is_some_and(|session| session.radio.active().is_some() || session.radio.pending_len() > 0);
    let measurements = &mut data.measurements.audio;
    measurements.engine_bindings = bindings;
    measurements.active_loops = loops;
    measurements.radio_started = measurements.radio_started.max(radio_started);
    measurements.radio_seen |= radio_active || radio_started > 0;
    measurements.engine_seen |= bindings > 0 && loops > 0;
}

/// Fills the derived measurements once the run has ended: the terrain and sea
/// probes from their frames, the actor pixel counts from their probes, and the
/// world's own water count.
fn finish_measurements(data: &mut SmokeData, shots: &[FrameRecord]) {
    let find = |label: &str| shots.iter().find(|shot| shot.label == label);
    if let Some(world) = find("world") {
        let total = (world.width as usize) * (world.height as usize);
        data.measurements.terrain = Some(TerrainMetrics {
            world_pixels: world.off_clear,
            total_pixels: total,
            covered_permille: permille(world.off_clear, total),
            distinct_colours: world.off_distinct_colours,
            detail_permille: world.off_detail_permille,
            luminance_min: world.off_luminance_min,
            luminance_max: world.off_luminance_max,
            near_luminance_min: world.near_luminance_min,
            near_luminance_max: world.near_luminance_max,
        });
    }
    if let Some(cinematic) = data.measurements.cinematic.as_mut() {
        cinematic.frames = CINEMATIC_FRAME_LABELS
            .iter()
            .filter_map(|label| find(label))
            .map(|shot| CinematicFrame {
                label: shot.label.clone(),
                bars_black_permille: shot.bars_black_permille,
                hud_green_pixels: shot.hud_green_pixels,
            })
            .collect();
    }
    if let Some(hud) = find("hud") {
        data.measurements.hud.region_text_pixels = hud.text_pixels;
    }
    if let (Some(visible), Some(plane_off), Some(bare)) = (
        find("sea_visible"),
        find("sea_absent"),
        find("sea_tiles_absent"),
    ) {
        let total = (visible.width as usize) * (visible.height as usize);
        let sea = visible.off_clear.saturating_sub(bare.off_clear);
        let tiles = plane_off.off_clear.saturating_sub(bare.off_clear);
        data.measurements.sea = Some(SeaMetrics {
            sea_pixels: sea,
            total_pixels: total,
            covered_permille: permille(sea, total),
            tile_pixels: tiles,
            tile_permille: permille(tiles, total),
            water_objects: data.water_objects,
            water_tile_parts: data.water_tile_parts,
        });
    }
    for shot in shots {
        let Some(index) = shot
            .label
            .strip_prefix("actor_")
            .and_then(|index| index.parse::<usize>().ok())
        else {
            continue;
        };
        if let Some(actor) = data.measurements.actors.get_mut(index) {
            actor.pixels = shot.off_clear;
            actor.total_pixels = (shot.width as usize) * (shot.height as usize);
        }
    }
}

/// `part` over `whole` in parts per thousand, `0` for an empty frame.
fn permille(part: usize, whole: usize) -> u32 {
    if whole == 0 {
        return 0;
    }
    u32::try_from(part as u64 * 1000 / whole as u64).unwrap_or(u32::MAX)
}

// --------------------------------------------------------------- screenshots --

fn request_screenshot(
    commands: &mut Commands,
    shots: SmokeShots,
    path: PathBuf,
    label: String,
    clear_colours: Option<[[u8; 3]; 2]>,
    region: Option<[u32; 4]>,
    raw: Option<FlightCheckFrames>,
) {
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>| {
            if let (Some(raw), Some(data)) = (&raw, captured.image.data.as_ref())
                && let Ok(mut frames) = raw.0.lock()
            {
                frames.push(RawFrame {
                    label: label.clone(),
                    width: captured.image.width(),
                    height: captured.image.height(),
                    data: data.clone(),
                });
            }
            let record = measure(
                &captured.image,
                label.clone(),
                path.clone(),
                clear_colours,
                region,
            );
            let delivered = captured.image.data.is_some();
            save_to_disk(path.clone())(captured);
            if delivered && let Ok(mut shots) = shots.0.lock() {
                shots.push(record);
            }
        },
    );
}

/// Measures one delivered frame: luminance, the mission capture's off-clear
/// counts, the world's own colours and (for the HUD frame) the objective
/// region's text pixels.
fn measure(
    image: &bevy::image::Image,
    label: String,
    path: PathBuf,
    clear_colours: Option<[[u8; 3]; 2]>,
    region: Option<[u32; 4]>,
) -> FrameRecord {
    let format = image.texture_descriptor.format;
    let order = channel_order(format);
    let data = image.data.as_deref().unwrap_or(&[]);
    let pixels = data.as_chunks::<4>().0;
    let mut levels = HashSet::new();
    let mut sum = 0_u64;
    let mut off_colours = HashSet::new();
    let mut off_min = u8::MAX;
    let mut off_max = 0_u8;
    let mut detail = 0_usize;
    let mut text_pixels = 0_usize;
    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut luminances = vec![0_u8; pixels.len()];
    let mut detail_mask = vec![false; pixels.len()];
    let colour_of = |index: usize| -> [u8; 3] {
        let pixel = pixels[index];
        [pixel[order[0]], pixel[order[1]], pixel[order[2]]]
    };
    let is_off = |colour: [u8; 3]| {
        clear_colours.is_some_and(|colours| {
            !same_colour(colour, colours[0]) && !same_colour(colour, colours[1])
        })
    };
    for (index, pixel) in pixels.iter().enumerate() {
        let colour = [pixel[order[0]], pixel[order[1]], pixel[order[2]]];
        let luminance =
            ((29 * u32::from(colour[0]) + 150 * u32::from(colour[1]) + 77 * u32::from(colour[2]))
                >> 8) as u8;
        levels.insert(luminance);
        luminances[index] = luminance;
        sum += u64::from(luminance);
        let py = index / width;
        if is_off(colour) {
            off_colours.insert(colour);
            off_min = off_min.min(luminance);
            off_max = off_max.max(luminance);
            // Local detail: a world pixel that differs from a neighbour that
            // is also world geometry. A smooth gradient (fog, sky) has none;
            // a texture has it everywhere.
            let right = if index % width + 1 < width {
                index + 1
            } else {
                0
            };
            let down = (index + width).min(pixels.len().saturating_sub(1));
            let differs = |other: usize| {
                let neighbour = colour_of(other);
                is_off(neighbour)
                    && (0..3).any(|channel| colour[channel].abs_diff(neighbour[channel]) > 8)
            };
            if (index % width + 1 < width && differs(right)) || (py + 1 < height && differs(down)) {
                detail += 1;
                detail_mask[index] = true;
            }
        }
        if let Some([x, y, w, h]) = region {
            let px = (index % width) as u32;
            if px >= x
                && px < x + w
                && (py as u32) >= y
                && (py as u32) < y + h
                && is_objective_text(colour)
            {
                text_pixels += 1;
            }
        }
    }
    let (near_min, near_max) =
        near_terrain_luminance(width, &luminances, &detail_mask).unwrap_or((0, 0));
    let counted = count_world_pixels(data, format, clear_colours);
    let world_pixels = counted.world;
    FrameRecord {
        label,
        path,
        width: image.width(),
        height: image.height(),
        distinct_luminance: levels.len(),
        mean_luminance: if pixels.is_empty() {
            0.0
        } else {
            sum as f32 / pixels.len() as f32
        },
        off_clear: world_pixels,
        near_clear: counted.clear,
        modal: counted.modal,
        off_distinct_colours: off_colours.len(),
        off_detail_permille: permille(detail, world_pixels),
        off_luminance_min: if off_colours.is_empty() { 0 } else { off_min },
        off_luminance_max: off_max,
        near_luminance_min: near_min,
        near_luminance_max: near_max,
        text_pixels: region.map(|_| text_pixels),
        bars_black_permille: bars_black_permille(pixels, order, width, height),
        hud_green_pixels: pixels
            .iter()
            .filter(|pixel| is_hud_green([pixel[order[0]], pixel[order[1]], pixel[order[2]]]))
            .count(),
    }
}

/// Parts per thousand of the letterbox bars' rows (where
/// [`letterbox_bars`](super::letterbox_bars) puts them for this frame's size)
/// that are black: every channel at most 8.
fn bars_black_permille(pixels: &[[u8; 4]], order: [usize; 3], width: usize, height: usize) -> u32 {
    if width == 0 || height == 0 {
        return 0;
    }
    let bars = super::letterbox_bars(width as f32, height as f32);
    let top = (bars.top.round() as usize).min(height);
    let bottom_from = height.saturating_sub(bars.bottom.round() as usize);
    let (mut black, mut total) = (0_usize, 0_usize);
    for (index, pixel) in pixels.iter().enumerate() {
        let row = index / width;
        if row < top || row >= bottom_from {
            total += 1;
            if order.iter().all(|&channel| pixel[channel] <= 8) {
                black += 1;
            }
        }
    }
    permille(black, total)
}

/// Whether a framebuffer pixel is the cockpit HUD's own green
/// (`Color::srgb(0.55, 1.0, 0.55)`, 140/255/140), within a hair.
fn is_hud_green(colour: [u8; 3]) -> bool {
    colour[0].abs_diff(140) <= 4 && colour[1] >= 251 && colour[2].abs_diff(140) <= 4
}

/// Side of the square tiles [`near_terrain_luminance`] classifies, in pixels.
const TERRAIN_TILE: usize = 32;

/// A tile is terrain when at least one pixel in this many (10 %) differs from
/// a neighbouring world pixel. A texture does that everywhere; open water, a
/// flat fill and a tile only crossed by a coastline's edge (a few per cent)
/// do not.
const TERRAIN_TILE_DETAIL_DENOMINATOR: usize = 10;

/// The luminance range of the **nearest terrain** in a probe frame, or
/// `None` when the frame shows no terrain.
///
/// The old probe sampled the frame's bottom third, which at some starts is
/// open water (C3/M01 starts over the sea: the check measured the sea, spread
/// 5). Terrain is found from the draw itself instead: the frame is cut into
/// [`TERRAIN_TILE`] squares, a square is terrain when its world pixels carry
/// local detail (see [`TERRAIN_TILE_DETAIL_DENOMINATOR`]), and the near field
/// is the lowest third (at least one row) of the lowest unbroken run of rows
/// holding such squares — the closest terrain, where fog is least, wherever
/// the camera points, and not a textured cloud bank further up the frame. The
/// range is taken over the squares' detail pixels only, so the flat water a
/// coastline tile also holds does not inflate it.
///
/// `luminance` is per pixel row-major and `detail` marks world pixels (off the
/// clear colour) that differ from a world neighbour.
#[must_use]
pub fn near_terrain_luminance(width: usize, luminance: &[u8], detail: &[bool]) -> Option<(u8, u8)> {
    if width == 0 || luminance.len() != detail.len() {
        return None;
    }
    let height = luminance.len() / width;
    let tiles_x = width.div_ceil(TERRAIN_TILE);
    let tiles_y = height.div_ceil(TERRAIN_TILE);
    let mut counts = vec![0_usize; tiles_x * tiles_y];
    for (index, &is_detail) in detail.iter().enumerate() {
        if is_detail {
            counts[(index / width / TERRAIN_TILE) * tiles_x + (index % width) / TERRAIN_TILE] += 1;
        }
    }
    let is_terrain =
        |tile: usize| counts[tile] * TERRAIN_TILE_DETAIL_DENOMINATOR >= TERRAIN_TILE * TERRAIN_TILE;
    let has_terrain = |row: usize| (0..tiles_x).any(|column| is_terrain(row * tiles_x + column));
    let bottom = (0..tiles_y).rev().find(|&row| has_terrain(row))?;
    let top = (0..=bottom)
        .rev()
        .take_while(|&row| has_terrain(row))
        .last()
        .unwrap_or(bottom);
    let span = bottom - top + 1;
    let first_near = bottom + 1 - (span / 3).max(1);
    let mut min = u8::MAX;
    let mut max = 0_u8;
    for (index, &level) in luminance.iter().enumerate() {
        let row = index / width / TERRAIN_TILE;
        let tile = row * tiles_x + (index % width) / TERRAIN_TILE;
        if detail[index] && row >= first_near && is_terrain(tile) {
            min = min.min(level);
            max = max.max(level);
        }
    }
    (min <= max).then_some((min, max))
}

/// The channel order the surface's format stores colour in.
fn channel_order(format: TextureFormat) -> [usize; 3] {
    match format {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => [2, 1, 0],
        _ => [0, 1, 2],
    }
}

/// Whether a framebuffer pixel is the objective line's own text colour (the
/// widget's yellow `Color::srgb(1.0, 0.85, 0.2)`).
fn is_objective_text(colour: [u8; 3]) -> bool {
    colour[0] >= 150 && colour[1] >= 110 && colour[2] <= 130 && colour[0] >= colour[2] + 60
}

// ------------------------------------------------------------------ report --

fn build_report(data: &SmokeData, shots: Vec<FrameRecord>) -> MissionSmokeReport {
    let checks = evaluate(&data.measurements);
    let failures = checks
        .iter()
        .filter(|check| check.applies && !check.passed)
        .map(|check| check.id.clone())
        .collect();
    MissionSmokeReport {
        mission: data.measurements.mission.clone(),
        frames: data.measurements.frames,
        ticks: data.measurements.ticks,
        checks,
        measurements: data.measurements.clone(),
        shots,
        placements: data.placements.clone(),
        failures,
    }
}

/// The Flight Check measurements as a JSON object (`null` when the run never
/// reached one).
fn flight_check_json(metrics: Option<&FlightCheckMetrics>) -> String {
    let Some(fc) = metrics else {
        return "null".to_owned();
    };
    let texts = fc
        .texts
        .iter()
        .map(|text| {
            format!(
                "{{\"row\":{},\"expected\":{},\"drawn\":{},\"visible\":{},\
                 \"region\":{:?},\"pixels\":{}}}",
                json_string(&text.row),
                json_string(&text.expected),
                text.drawn
                    .as_deref()
                    .map_or_else(|| "null".to_owned(), json_string),
                text.visible,
                text.region,
                text.pixels
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let buttons = fc
        .buttons
        .iter()
        .map(|button| {
            format!(
                "{{\"hotspot\":{},\"drawn\":{},\"region\":{:?},\"pixels\":{}}}",
                json_string(&button.hotspot),
                button.drawn,
                button.region,
                button.pixels
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"frames_delivered\":{},\"frame_width\":{},\"frame_height\":{},\
         \"drawn_permille\":{},\"launched\":{},\"refusal\":{},\"texts\":[{texts}],\
         \"buttons\":[{buttons}]}}",
        fc.frames_delivered,
        fc.frame_size.0,
        fc.frame_size.1,
        fc.drawn_permille,
        fc.launched,
        fc.refusal
            .as_deref()
            .map_or_else(|| "null".to_owned(), json_string),
    )
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

fn json_option_f64(value: Option<f64>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

fn json_option_bool(value: Option<bool>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

fn json_option_usize(value: Option<usize>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

/// Writes `report.json` into `dir`.
fn write_artifacts(
    dir: &Path,
    data: &SmokeData,
    shots: &[FrameRecord],
) -> Result<(), (PathBuf, std::io::Error)> {
    std::fs::create_dir_all(dir).map_err(|error| (dir.to_path_buf(), error))?;
    let report = build_report(data, shots.to_vec());
    let checks = report
        .checks
        .iter()
        .map(|check| {
            format!(
                "{{\"id\":{},\"applies\":{},\"passed\":{},\"detail\":{}}}",
                json_string(&check.id),
                check.applies,
                check.passed,
                json_string(&check.detail)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let shots = report
        .shots
        .iter()
        .map(|shot| {
            format!(
                "{{\"label\":{},\"path\":{},\"width\":{},\"height\":{},\
                 \"distinct_luminance\":{},\"mean_luminance\":{},\"off_clear\":{},\
                 \"near_clear\":{},\"off_distinct_colours\":{},\"off_detail_permille\":{},\
                 \"off_luminance_spread\":{},\"near_luminance_spread\":{},\
                 \"text_pixels\":{},\"bars_black_permille\":{},\"hud_green_pixels\":{}}}",
                json_string(&shot.label),
                json_string(&shot.path.display().to_string()),
                shot.width,
                shot.height,
                shot.distinct_luminance,
                shot.mean_luminance,
                shot.off_clear,
                shot.near_clear,
                shot.off_distinct_colours,
                shot.off_detail_permille,
                shot.off_luminance_max
                    .saturating_sub(shot.off_luminance_min),
                shot.near_luminance_max
                    .saturating_sub(shot.near_luminance_min),
                json_option_usize(shot.text_pixels),
                shot.bars_black_permille,
                shot.hud_green_pixels,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let actors = report
        .measurements
        .actors
        .iter()
        .map(|actor| {
            format!(
                "{{\"kind\":{},\"name\":{},\"asleep\":{},\"placed\":{},\"placement_m\":{},\
                 \"on_screen\":{},\"pixels\":{},\"total_pixels\":{},\"note\":{}}}",
                json_string(&actor.kind),
                json_string(&actor.name),
                actor.asleep,
                actor.placed,
                json_option_f64(actor.placement_m),
                actor.on_screen,
                actor.pixels,
                actor.total_pixels,
                actor
                    .note
                    .as_deref()
                    .map_or_else(|| "null".to_owned(), json_string),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let wingmen = report
        .measurements
        .wingmen
        .iter()
        .map(|sample| {
            let distances = sample
                .distances_m
                .iter()
                .map(|(name, metres)| {
                    format!(
                        "{{\"name\":{},\"distance_m\":{}}}",
                        json_string(name),
                        json_option_f64(metres.is_finite().then_some(*metres))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"second\":{},\"distances\":[{distances}]}}",
                sample.second
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let placements = report
        .placements
        .iter()
        .map(|line| json_string(line))
        .collect::<Vec<_>>()
        .join(",");
    let failures = report
        .failures
        .iter()
        .map(|id| json_string(id))
        .collect::<Vec<_>>()
        .join(",");
    let model_line = report
        .measurements
        .hud
        .model_line
        .as_deref()
        .map_or_else(|| "null".to_owned(), json_string);
    let drawn_line = report
        .measurements
        .hud
        .drawn_line
        .as_deref()
        .map_or_else(|| "null".to_owned(), json_string);
    let cinematic = report.measurements.cinematic.as_ref().map_or_else(
        || "null".to_owned(),
        |c| {
            format!(
                "{{\"total_ticks\":{},\"played_ticks\":{},\"max_drift_m\":{},\
                 \"max_turn_deg\":{},\"bars_on_while_playing\":{},\"bars_off_after\":{},\"handed_over\":{}}}",
                c.total_ticks,
                c.played_ticks,
                json_option_f64(
                    c.max_drift_m
                        .is_finite()
                        .then_some(f64::from(c.max_drift_m))
                ),
                json_option_f64(
                    c.max_turn_deg
                        .is_finite()
                        .then_some(f64::from(c.max_turn_deg))
                ),
                c.bars_on_while_playing,
                c.bars_off_after,
                c.handed_over
            )
        },
    );
    let first_control = report.measurements.first_control.as_ref().map_or_else(
        || "null".to_owned(),
        |f| {
            format!(
                "{{\"tick\":{},\"frame\":{},\"pose\":{{\"position_m\":[{},{},{}]}},\
                 \"heading\":{{\"yaw_deg\":{},\"pitch_deg\":{},\"roll_deg\":{},\
                 \"forward\":[{},{},{}]}},\"moves_before\":{},\
                 \"framebuffer\":\"first_control\"}}",
                f.tick,
                f.frame,
                f.position_m[0],
                f.position_m[1],
                f.position_m[2],
                f.yaw_deg,
                f.pitch_deg,
                f.roll_deg,
                f.forward[0],
                f.forward[1],
                f.forward[2],
                f.moves_before
            )
        },
    );
    let body = format!(
        "{{\"kind\":{kind},\"mission\":{mission},\"human_play\":false,\
         \"mode\":{mode},\"player_teleported\":{teleported},\
         \"wingman_threshold_m\":{threshold},\"wingmen\":[{wingmen}],\
         \"verified_original\":false,\"frames\":{frames},\"ticks\":{ticks},\
         \"hud\":{{\"model_line\":{model_line},\"drawn_line\":{drawn_line},\
         \"drawn_visible\":{visible},\"region_text_pixels\":{region},\"overlay_hidden\":{overlay_hidden}}},\
         \"flight_check\":{flight_check},\
         \"guns\":{{\"effects\":{effects},\"max_live_rounds\":{live},\"rounds_drawn\":{drawn},\
         \"rounds_examined\":{examined},\
         \"flash_frames\":{flash_frames},\"flashes_undrawn\":{flashes_undrawn}}},\"rocket\":{{\"launched\":{launched},\
         \"max_rockets\":{max_rockets},\"refusals\":[{refusals}]}},\
         \"audio\":{{\"radio_seen\":{radio_seen},\"engine_seen\":{engine_seen},\
         \"engine_bindings\":{bindings},\"active_loops\":{loops},\
         \"radio_started\":{radio_started}}},\
         \"cinematic\":{cinematic},\"first_control\":{first_control},\
         \"approach\":{{\"placed\":{approach_placed},\"anchor\":{anchor},\
         \"frames\":{approach_frames},\"objective_woke\":{woke}}},\
         \"actors\":[{actors}],\"placements\":[{placements}],\"shots\":[{shots}],\
         \"checks\":[{checks}],\"failures\":[{failures}],\"passed\":{passed}}}\n",
        kind = json_string(SMOKE_KIND),
        mission = json_string(&report.mission),
        mode = json_string(if report.measurements.no_teleport {
            "no_teleport"
        } else {
            "scripted"
        }),
        teleported = !report.placements.is_empty(),
        threshold = MAX_WINGMAN_DISTANCE_M,
        wingmen = wingmen,
        frames = report.frames,
        ticks = report.ticks,
        model_line = model_line,
        drawn_line = drawn_line,
        visible = report.measurements.hud.drawn_visible,
        overlay_hidden = report.measurements.hud.overlay_hidden,
        region = json_option_usize(report.measurements.hud.region_text_pixels),
        flight_check = flight_check_json(report.measurements.flight_check.as_ref()),
        effects = report.measurements.guns.effects,
        live = report.measurements.guns.max_live_rounds,
        drawn = json_option_bool(report.measurements.guns.rounds_drawn),
        examined = report.measurements.guns.rounds_examined,
        flash_frames = report.measurements.guns.flash_frames,
        flashes_undrawn = report.measurements.guns.flashes_undrawn,
        launched = report.measurements.rocket.launched,
        max_rockets = report.measurements.rocket.max_rockets,
        refusals = report
            .measurements
            .rocket
            .refusals
            .iter()
            .map(|refusal| json_string(refusal))
            .collect::<Vec<_>>()
            .join(","),
        radio_seen = report.measurements.audio.radio_seen,
        engine_seen = report.measurements.audio.engine_seen,
        bindings = report.measurements.audio.engine_bindings,
        loops = report.measurements.audio.active_loops,
        radio_started = report.measurements.audio.radio_started,
        cinematic = cinematic,
        first_control = first_control,
        approach_placed = report.measurements.approach.placed,
        anchor = report
            .measurements
            .approach
            .anchor
            .as_deref()
            .map_or_else(|| "null".to_owned(), json_string),
        approach_frames = report.measurements.approach.frames,
        woke = report.measurements.approach.objective_woke,
        actors = actors,
        placements = placements,
        shots = shots,
        checks = checks,
        failures = failures,
        passed = report.failures.is_empty(),
    );
    let path = dir.join(SMOKE_REPORT_FILE);
    std::fs::write(&path, body).map_err(|error| (path, error))
}

/// Turns the finished run into the process result: the report and the PNGs
/// are written first, then a failing check becomes a refusal that names it.
///
/// # Errors
///
/// [`CaptureRefusal::Incomplete`] when any check failed (naming them) or the
/// run ended before it finished.
pub fn finish(
    handle: &MissionSmokeHandle,
    request: &crate::playtest::SmokeRequest,
) -> Result<MissionSmokeReport, CaptureRefusal> {
    let outcome = handle.take().ok_or_else(|| {
        CaptureRefusal::Incomplete("the mission smoke ended before it finished".to_owned())
    })?;
    let report = outcome.map_err(CaptureRefusal::Incomplete)?;
    println!(
        "mission smoke: {} — {} checks, {} failed, framebuffers {} in {}",
        report.mission,
        report.checks.len(),
        report.failures.len(),
        report.shots.len(),
        request.capture_dir.display(),
    );
    for check in &report.checks {
        println!(
            "  [{}] {} — {}",
            if !check.applies {
                "skip"
            } else if check.passed {
                "pass"
            } else {
                "FAIL"
            },
            check.id,
            check.detail
        );
    }
    if report.failures.is_empty() {
        Ok(report)
    } else {
        Err(CaptureRefusal::Incomplete(format!(
            "the M01 smoke failed {} of {} checks: {}",
            report.failures.len(),
            report.checks.iter().filter(|check| check.applies).count(),
            report.failures.join(", ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playtest::SmokeRequest;

    fn good_frames() -> Vec<CinematicFrame> {
        CINEMATIC_FRAME_LABELS
            .iter()
            .map(|label| {
                let held = *label != "cinematic_handover";
                CinematicFrame {
                    label: (*label).to_owned(),
                    bars_black_permille: if held { 1000 } else { 0 },
                    hud_green_pixels: if held { 0 } else { 900 },
                }
            })
            .collect()
    }

    fn frames_with(label: &str, bars: u32, hud: usize) -> Vec<CinematicFrame> {
        let mut frames = good_frames();
        for frame in frames.iter_mut().filter(|frame| frame.label == label) {
            frame.bars_black_permille = bars;
            frame.hud_green_pixels = hud;
        }
        frames
    }

    /// **`accept_m01_owner_smoke_cinematic_` the pixel measures read bars and HUD.**
    ///
    /// A synthetic 4:3 frame: black over the measured bar rows reads 1000 per
    /// mille, the same frame with the picture showing through the top bar
    /// reads less, and the HUD green is counted exactly.
    #[test]
    fn accept_m01_owner_smoke_cinematic_pixels_measure_bars_and_hud() {
        let (w, h) = (64_usize, 48_usize);
        let bars = crate::mission_session::letterbox_bars(w as f32, h as f32);
        let (top, bottom_from) = (bars.top.round() as usize, h - bars.bottom.round() as usize);
        let mut pixels = vec![[90_u8, 120, 200, 255]; w * h];
        for (index, pixel) in pixels.iter_mut().enumerate() {
            let row = index / w;
            if row < top || row >= bottom_from {
                *pixel = [0, 0, 0, 255];
            }
        }
        let order = [0, 1, 2];
        assert_eq!(bars_black_permille(&pixels, order, w, h), 1000);
        assert_eq!(
            pixels
                .iter()
                .filter(|p| is_hud_green([p[0], p[1], p[2]]))
                .count(),
            0
        );
        for pixel in pixels.iter_mut().take(w * top / 2) {
            *pixel = [90, 120, 200, 255];
        }
        assert!(bars_black_permille(&pixels, order, w, h) < MIN_BARS_BLACK_PERMILLE);
        assert!(is_hud_green([140, 255, 140]));
        assert!(!is_hud_green([200, 255, 140]));
    }

    /// **`accept_m01_owner_smoke_cinematic_` a restore leaves the held UI alone.**
    ///
    /// The probe's restore used to write `Visibility::Inherited` onto every UI
    /// node, which showed the letterbox bars after hand-over and the HUD during
    /// the phase (#1704). A node marked as cinematic-owned keeps its
    /// visibility through a restore; an ordinary node is restored.
    #[test]
    fn accept_m01_owner_smoke_cinematic_restore_leaves_owned_ui_alone() {
        use super::super::launch_cinematic::CinematicOwnedVisibility;
        let mut app = bevy::prelude::App::new();
        app.insert_resource(ProbeState(ProbeMode::Restored));
        let owned = app
            .world_mut()
            .spawn((
                Node::default(),
                Visibility::Hidden,
                CinematicOwnedVisibility,
            ))
            .id();
        let plain = app
            .world_mut()
            .spawn((Node::default(), Visibility::Hidden))
            .id();
        app.add_systems(Update, apply_probe);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(owned),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(plain),
            Some(&Visibility::Inherited)
        );
    }

    /// **`accept_m01_owner_smoke_cinematic_` the judge reads the phase.**
    ///
    /// A run that went through the launch cinematic is judged on it: the
    /// phase played to its end, handed over, held the body and had the bars on
    /// and then off. Any one of those failing fails the check; a run without
    /// the phase has no such check.
    #[test]
    fn accept_m01_owner_smoke_cinematic_check_judges_the_phase() {
        let good = CinematicMetrics {
            total_ticks: 4818,
            played_ticks: 4818,
            max_drift_m: 0.0,
            max_turn_deg: 0.0,
            bars_on_while_playing: true,
            bars_off_after: true,
            handed_over: true,
            frames: good_frames(),
        };
        let verdict = |c: Option<CinematicMetrics>| {
            evaluate(&SmokeMeasurements {
                cinematic: c,
                ..SmokeMeasurements::default()
            })
            .into_iter()
            .find(|check| check.id == "launch_cinematic_plays")
            .map(|check| check.passed)
        };
        assert_eq!(verdict(None), None);
        assert_eq!(verdict(Some(good.clone())), Some(true));
        for broken in [
            CinematicMetrics {
                handed_over: false,
                ..good.clone()
            },
            CinematicMetrics {
                played_ticks: 100,
                ..good.clone()
            },
            CinematicMetrics {
                max_drift_m: 40.0,
                ..good.clone()
            },
            CinematicMetrics {
                max_turn_deg: 30.0,
                ..good.clone()
            },
            CinematicMetrics {
                bars_on_while_playing: false,
                ..good.clone()
            },
            CinematicMetrics {
                bars_off_after: false,
                ..good.clone()
            },
            CinematicMetrics {
                frames: Vec::new(),
                ..good.clone()
            },
            // The bars not seen while it plays, though the flag says on.
            CinematicMetrics {
                frames: frames_with("cinematic_mid", 0, 0),
                ..good.clone()
            },
            // The HUD seen while it plays.
            CinematicMetrics {
                frames: frames_with("cinematic_end", 1000, 900),
                ..good.clone()
            },
            // The bars still seen after hand-over.
            CinematicMetrics {
                frames: frames_with("cinematic_handover", 1000, 900),
                ..good.clone()
            },
            // No HUD after hand-over.
            CinematicMetrics {
                frames: frames_with("cinematic_handover", 0, 0),
                ..good.clone()
            },
        ] {
            assert_eq!(verdict(Some(broken.clone())), Some(false), "{broken:?}");
        }
    }

    /// **`accept_m01_smoke_no_teleport_` the mode never moves the player.**
    ///
    /// The production script systems and placement writer run for the whole
    /// no-teleport script over a stand-in player body; the body's pose after
    /// the run is the pose it started with, the report names no placement,
    /// and a wingman sample exists for every fixed time.
    #[test]
    fn accept_m01_smoke_no_teleport_never_moves_the_player() {
        let dir = std::env::temp_dir().join("accept_m01_smoke_no_teleport_never_moves");
        let request = SmokeRequest {
            seconds: 45,
            capture_dir: dir.clone(),
        };
        let plugin = MissionSmokePlugin::new(request).with_mode(MissionSmokeMode::NoTeleport);
        let handle = plugin.handle();
        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(SmokeConfig {
                request: plugin.request.clone(),
                mode: plugin.mode,
                handle: plugin.handle.clone(),
            })
            .init_resource::<SmokeShots>()
            .init_resource::<FlightCheckFrames>()
            .init_resource::<SmokeData>()
            .init_resource::<ProbeState>()
            .init_resource::<KeyQueue>()
            .init_resource::<PendingPlacement>()
            .init_resource::<PlaytestState>()
            .insert_resource(PlaytestCamera::new())
            .init_resource::<PhysicsTickLedger>()
            .add_systems(Update, apply_placement);
        add_script_systems(&mut app);
        let start = Vec3::new(120.0, 900.0, -4_000.0);
        let body = app
            .world_mut()
            .spawn((
                MissionPlayerBody,
                Position(start),
                Rotation::default(),
                LinearVelocity::default(),
                AngularVelocity::default(),
                GlobalTransform::from_translation(start),
            ))
            .id();
        for _ in 0..(NO_TELEPORT_SHOT_FRAME + SHOT_GRACE_FRAMES + 10) {
            app.update();
            if handle.0.lock().is_ok_and(|slot| slot.is_some()) {
                break;
            }
        }
        let report = handle
            .take()
            .expect("the run finished")
            .expect("the report was written");
        assert!(report.placements.is_empty(), "{:?}", report.placements);
        assert_eq!(
            app.world().get::<Position>(body).map(|position| position.0),
            Some(start),
            "nothing wrote the player body's pose"
        );
        assert!(app.world().resource::<PendingPlacement>().0.is_none());
        let seconds: Vec<u32> = report
            .measurements
            .wingmen
            .iter()
            .map(|s| s.second)
            .collect();
        assert_eq!(seconds, WINGMAN_SAMPLE_SECONDS);
        assert!(report.measurements.no_teleport);
        // No wingman spawned in this stand-in world: the check says so.
        assert!(report.failures.contains(&"wingmen_near_player".to_owned()));
        let json = std::fs::read_to_string(dir.join(SMOKE_REPORT_FILE)).expect("report.json");
        assert!(json.contains("\"mode\":\"no_teleport\""), "{json}");
        assert!(json.contains("\"player_teleported\":false"), "{json}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **`accept_m01_smoke_first_control_` the frame comes at the hand-over.**
    ///
    /// The production script systems run over a stand-in player body under a
    /// launch cinematic phase. While the phase plays there is no first control
    /// pose and no framebuffer requested; on the first frame after the
    /// hand-over the pose is read, `first_control` is the first framebuffer the
    /// script asks for, and a later move of the body does not change it.
    #[test]
    fn accept_m01_smoke_first_control_is_taken_at_the_handover_before_any_move() {
        let dir = std::env::temp_dir().join("accept_m01_smoke_first_control_handover");
        let plugin = MissionSmokePlugin::new(SmokeRequest {
            seconds: 45,
            capture_dir: dir.clone(),
        });
        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(SmokeConfig {
                request: plugin.request.clone(),
                mode: plugin.mode,
                handle: plugin.handle.clone(),
            })
            .init_resource::<SmokeShots>()
            .init_resource::<FlightCheckFrames>()
            .init_resource::<SmokeData>()
            .init_resource::<ProbeState>()
            .init_resource::<KeyQueue>()
            .init_resource::<PendingPlacement>()
            .init_resource::<PlaytestState>()
            .insert_resource(PlaytestCamera::new())
            .init_resource::<PhysicsTickLedger>()
            .insert_resource(LaunchCinematic::new(5).expect("a phase"));
        add_script_systems(&mut app);
        let start = Vec3::new(-1426.0, 250.0, -1813.0);
        let rotation = Quat::from_rotation_y(40.0_f32.to_radians());
        let body = app
            .world_mut()
            .spawn((
                MissionPlayerBody,
                Position(start),
                Rotation(rotation),
                LinearVelocity::default(),
                AngularVelocity::default(),
                GlobalTransform::from_translation(start),
            ))
            .id();
        for _ in 0..5 {
            app.update();
            assert!(
                app.world()
                    .resource::<SmokeData>()
                    .measurements
                    .first_control
                    .is_none()
            );
            assert!(
                !app.world()
                    .resource::<SmokeData>()
                    .requested
                    .contains(&"first_control".to_owned())
            );
            app.world_mut()
                .resource_mut::<LaunchCinematic>()
                .complete_tick();
        }
        app.update();
        // A scripted move after the frame must not reach the recorded pose.
        app.world_mut().get_mut::<Position>(body).expect("body").0 = Vec3::new(9.0, 9.0, 9.0);
        for _ in 0..40 {
            app.update();
        }
        let data = app.world().resource::<SmokeData>();
        let first = data
            .measurements
            .first_control
            .clone()
            .expect("a first control pose");
        assert_eq!(first.position_m, [-1426.0, 250.0, -1813.0]);
        assert!((first.yaw_deg - 40.0).abs() < 1e-3, "{first:?}");
        assert_eq!(first.moves_before, 0);
        // Only the cinematic's own held-phase shots may precede it.
        let at = data
            .requested
            .iter()
            .position(|l| l == "first_control")
            .expect("the frame was requested");
        assert!(
            data.requested[..at]
                .iter()
                .all(|l| l.starts_with("cinematic_") && l != "cinematic_handover"),
            "{:?}",
            data.requested
        );
        assert_eq!(
            data.requested
                .iter()
                .filter(|l| *l == "first_control")
                .count(),
            1
        );
        let verdict = evaluate(&data.measurements)
            .into_iter()
            .find(|check| check.id == "first_control_frame")
            .expect("the check exists");
        assert!(verdict.passed, "{}", verdict.detail);
        let moved = FirstControlMetrics {
            moves_before: 1,
            ..first
        };
        let verdict = evaluate(&SmokeMeasurements {
            first_control: Some(moved),
            ..SmokeMeasurements::default()
        })
        .into_iter()
        .find(|check| check.id == "first_control_frame")
        .expect("the check exists");
        assert!(!verdict.passed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **`accept_m01_smoke_first_control_` a run without the pose fails by name.**
    #[test]
    fn accept_m01_smoke_first_control_missing_frame_fails() {
        let verdict = evaluate(&SmokeMeasurements::default())
            .into_iter()
            .find(|check| check.id == "first_control_frame")
            .expect("the check exists");
        assert!(verdict.applies && !verdict.passed, "{}", verdict.detail);
        let with_pose = SmokeMeasurements {
            first_control: Some(FirstControlMetrics::default()),
            missing_shots: vec!["first_control".to_owned()],
            ..SmokeMeasurements::default()
        };
        let verdict = evaluate(&with_pose)
            .into_iter()
            .find(|check| check.id == "first_control_frame")
            .expect("the check exists");
        assert!(!verdict.passed, "a pose without its framebuffer passes");
    }

    /// A flight check in the original's row format, names and numbers invented
    /// (the same shape the `m01_lc_fc_c` acceptance tests use).
    const FC_LAYOUT: &str = "\
[@FlightCheck@]
V1=GX,400
V5=V5,0xFF112233

FC_BACKGROUND\t=P,Back.jpg,0,0,1,1,0,0,0
FC_B_FLYMISSION\t=B,Fly.png,50,<GX>,0,0,,,,
FC_B_RETURNBRIEF=B,Ret.png,30,<GX>,0,0,,0x1000,1
FC_T_TITLE\t=T,IDS_FC_TITLE,10,10,0,190,24,<V5>,1
FC_T_MISSION    =T,,10,40,0,400,0,0xFF2D3843,0
FC_T_OBJECTIVES =T,,10,60,0,200,360000,0xFF2D3843,0
";

    fn fc_scene() -> super::super::flight_check::FlightCheckScene {
        use crate::mission_start::AIRFRAME_TABLE;
        use crate::ui::front_end::{Entry, FlightCheckView, OriginalLayout};
        use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
        use cs_types::evidence::ClaimId;
        let extents = |art: &str| match art {
            "Back.jpg" => Some((640, 480)),
            "Fly.png" | "Ret.png" => Some((80, 80)),
            _ => None,
        };
        let strings = |id: &str| (id == "IDS_FC_TITLE").then(|| "Flight Check".to_owned());
        let airframe = Resolved::Known(Known::new(
            ContentId::from_source(ContentKind::Airframe, AIRFRAME_TABLE[0].scene_root)
                .expect("valid id"),
            Provenance::designed(ClaimId::new("m01lcfcd.test").expect("valid claim")),
        ));
        let view = FlightCheckView::build(
            Entry::Known("Alpha - One".to_owned()),
            &["MSG_ONE".to_owned()],
            |_| Ok("first objective".to_owned()),
            &airframe,
            &[],
        );
        let layout = OriginalLayout::parse(FC_LAYOUT).expect("the layout parses");
        super::super::flight_check::FlightCheckScene::build(&layout, &view, &extents, &strings)
            .expect("the scene builds")
    }

    /// The headless smoke app on a held Flight Check, with the overlay nodes
    /// laid out as the window would lay them out: every text row and button of
    /// the scene at a 100-pixel-wide cell of a 400x200 frame.
    fn held_app(dir: &Path, bare_differs: bool) -> (App, MissionSmokeHandle) {
        use bevy::math::Affine2;
        let plugin = MissionSmokePlugin::new(SmokeRequest {
            seconds: 45,
            capture_dir: dir.to_path_buf(),
        });
        let handle = plugin.handle();
        let mut app = App::new();
        app.add_plugins(bevy::time::TimePlugin)
            .add_message::<AppExit>()
            .insert_resource(SmokeConfig {
                request: plugin.request.clone(),
                mode: plugin.mode,
                handle: plugin.handle.clone(),
            })
            .init_resource::<SmokeShots>()
            .init_resource::<FlightCheckFrames>()
            .init_resource::<SmokeData>()
            .init_resource::<ProbeState>()
            .init_resource::<KeyQueue>()
            .init_resource::<PendingPlacement>()
            .init_resource::<PlaytestState>()
            .insert_resource(PlaytestCamera::new())
            .init_resource::<PhysicsTickLedger>();
        let loadout = crate::ui::front_end::Loadout {
            player: Some(
                cs_types::content::ContentId::from_source(
                    cs_types::content::ContentKind::Airframe,
                    crate::mission_start::AIRFRAME_TABLE[0].scene_root,
                )
                .expect("valid id"),
            ),
            wingmate: None,
            ammunition: Some(super::super::mission_ammunition_id()),
        };
        let scene = fc_scene();
        let mut full = vec![0x40_u8; 400 * 200 * 4];
        let bare = full.clone();
        let mut cell = 0_u32;
        let mut rows: Vec<(String, Option<String>)> = Vec::new();
        for text in scene.texts.iter().filter(|text| !text.text.is_empty()) {
            rows.push((text.id.clone(), Some(text.text.clone())));
        }
        for button in &scene.buttons {
            rows.push((button.hotspot.clone(), None));
        }
        for (row, text) in rows {
            let region = [cell * 12, 20, 10, 10];
            cell += 1;
            if bare_differs {
                for y in region[1]..region[1] + region[3] {
                    for x in region[0]..region[0] + region[2] {
                        let at = (y as usize * 400 + x as usize) * 4;
                        full[at] = 0xF0;
                    }
                }
            }
            let mut entity = app.world_mut().spawn((
                ScreenOverlay { row: Some(row) },
                InheritedVisibility::VISIBLE,
                UiGlobalTransform::from(Affine2::from_translation(Vec2::new(
                    region[0] as f32 + 5.0,
                    region[1] as f32 + 5.0,
                ))),
                ComputedNode {
                    size: Vec2::new(10.0, 10.0),
                    ..Default::default()
                },
            ));
            if let Some(text) = text {
                entity.insert(Text::new(text));
            }
        }
        {
            let frames = app.world().resource::<FlightCheckFrames>();
            let mut frames = frames.0.lock().expect("lock");
            for (label, data) in [("flight_check", full), ("flight_check_bare", bare)] {
                frames.push(RawFrame {
                    label: label.to_owned(),
                    width: 400,
                    height: 200,
                    data,
                });
            }
        }
        super::super::flight_check::install_flight_check(
            &mut app,
            FlightCheckPhase::open(scene, loadout).expect("the phase opens"),
        );
        add_script_systems(&mut app);
        (app, handle)
    }

    /// **`accept_m01_lc_fc_d_` the smoke passes through the Flight Check.**
    ///
    /// The production smoke systems run over a held phase whose overlay nodes
    /// and framebuffers are laid out by hand: the script's clock does not run
    /// while the screen is up, the screen is judged against the scene, Fly
    /// Mission is delivered through the front-end machine, and the clock then
    /// starts from zero.
    #[test]
    fn accept_m01_lc_fc_d_the_smoke_judges_the_screen_then_delivers_fly_mission() {
        let dir = std::env::temp_dir().join("accept_m01_lc_fc_d_judges");
        let (mut app, _handle) = held_app(&dir, true);
        for _ in 0..FLIGHT_CHECK_BARE_SHOT_OFFSET {
            app.update();
        }
        assert!(
            app.world().resource::<FlightCheckPhase>().is_pending(),
            "the screen is up until the shots are taken"
        );
        assert_eq!(
            app.world().resource::<SmokeData>().frame,
            0,
            "the script's clock has not started"
        );
        app.update();
        let phase = app.world().resource::<FlightCheckPhase>();
        assert!(!phase.is_pending(), "Fly Mission was delivered");
        assert_eq!(
            phase.front_end().screen(),
            crate::ui::front_end::Screen::Flight
        );
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<SmokeData>().frame,
            11,
            "the script's clock starts at Fly Mission: the releasing frame is its first"
        );
        let metrics = app
            .world()
            .resource::<SmokeData>()
            .measurements
            .flight_check
            .clone()
            .expect("the screen was measured");
        assert!(metrics.launched, "{metrics:?}");
        assert!(metrics.frames_delivered);
        assert!(metrics.drawn_permille >= MIN_FLIGHT_CHECK_DRAWN_PERMILLE);
        let rows: Vec<&str> = metrics.texts.iter().map(|text| text.row.as_str()).collect();
        assert!(rows.contains(&"FC_T_MISSION") && rows.contains(&"FC_T_OBJECTIVES"));
        assert!(
            metrics
                .texts
                .iter()
                .all(|text| text.drawn.as_deref() == Some(text.expected.as_str())
                    && text.pixels == 100),
            "{metrics:?}"
        );
        assert!(metrics.buttons.iter().all(|button| button.pixels == 100));
        let checks = evaluate_flight_check(&app.world().resource::<SmokeData>().measurements);
        assert!(
            checks.iter().all(|check| check.passed),
            "every Flight Check check passes: {checks:?}"
        );
        assert_eq!(checks.len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **`accept_m01_lc_fc_d_` a screen that adds nothing to the frame fails.**
    ///
    /// The full frame equals the bare pane, so no text row or button added a
    /// pixel: the smoke still delivers Fly Mission (the mission must run) and
    /// the report names the checks that failed.
    #[test]
    fn accept_m01_lc_fc_d_a_screen_that_draws_nothing_fails_by_name() {
        let dir = std::env::temp_dir().join("accept_m01_lc_fc_d_blank");
        let (mut app, _handle) = held_app(&dir, false);
        for _ in 0..(FLIGHT_CHECK_BARE_SHOT_OFFSET + 3) {
            app.update();
        }
        let measurements = app.world().resource::<SmokeData>().measurements.clone();
        let failed: Vec<String> = evaluate_flight_check(&measurements)
            .into_iter()
            .filter(|check| !check.passed)
            .map(|check| check.id)
            .collect();
        assert_eq!(
            failed,
            ["flight_check_text_matches", "flight_check_buttons_drawn"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
