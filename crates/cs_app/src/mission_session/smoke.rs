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
//! | `world_terrain_drawn` | the frame around the spawn holds world pixels off the window's clear colour |
//! | `world_terrain_textured` | those pixels hold more than one flat grey (distinct colours) |
//! | `world_terrain_lit` | those pixels carry a luminance spread (shading, not a flat fill) |
//! | `world_sea_visible` | the sea plane covers pixels in a differential probe, when the world's records declare water |
//! | `hud_objective_current` | the recovered lifecycle model marks a current objective during the run |
//! | `hud_objective_matches` | the drawn line equals the read model's current line, and its pixels are on screen |
//! | `actors_project_and_cover` | every spawned actor projects onto the screen and covers more than `MIN_ACTOR_PIXELS` |
//! | `weapon_guns_fire` | holding `Space` for a second sends more than one round through the mission's weapon session |
//! | `weapon_rounds_drawn` | every live round entity carries a mesh, i.e. a tracer the player can see |
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
use bevy::input::keyboard::KeyCode;
use bevy::input::{ButtonInput, InputSystems};
use bevy::prelude::{
    App, AppExit, ChildOf, ClearColor, Commands, Entity, GlobalTransform, IntoScheduleConfigs,
    Last, Local, Mesh3d, MessageWriter, On, Plugin, PreUpdate, Quat, Query, Res, ResMut, Resource,
    Text, Transform, Update, Vec2, Vec3, Visibility, With,
};
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::ui::{ComputedNode, Node};
use cs_content::world::SurfaceRole;

use super::compose::MissionPlayerBody;
use super::evidence::{CaptureRefusal, clear_colour_encodings, count_world_pixels, same_colour};
use super::host::MissionHost;
use super::lifecycles::ObjectiveLifecycles;
use super::weapons::MissionWeaponReport;
use crate::audio::loops::AudioSession;
use crate::audio::mixer::AudioMixReport;
use crate::audio::spawn::{AudioBindLog, AudioEmitterRole};
use crate::input::platform::PlatformInput;
use crate::input::session::PauseReason;
use crate::mission_actors::{MissionActorSpawnReport, MissionZeppelinActor};
use crate::ordnance::OrdnanceItemMirror;
use crate::physics::PhysicsTickLedger;
use crate::playtest::scene::PlaytestAircraft;
use crate::playtest::{PlaytestCamera, PlaytestState};
use crate::render::horizon::{HorizonDome, HorizonSea};
use crate::render::precipitation::PrecipitationLayer;
use crate::render::weather::WeatherLayer;
use crate::ui::hud::{MissionMessages, OBJECTIVE_HUD_CAPTION, ObjectiveHud};
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
/// Frames into the sea phase at which everything is restored.
const SEA_RESTORE_OFFSET: u32 = 10;
/// Frames into the sea phase at which the session resumes.
const SEA_RESUME_OFFSET: u32 = 12;

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
/// The least luminance spread **in the near field** (the closest third of the
/// probe frame, where the fog is negligible) for the terrain to be called
/// *lit*.
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

/// How far the player is stood from a spawned actor for its probe, metres —
/// abeam, so the probe never puts the player in the actor's path.
const PLACEMENT_OFFSET_M: f32 = 400.0;

/// How near a residency visual must sit to a world actor's bind-time pose to
/// be taken for that actor's drawn body, metres (designed, fail-closed): the
/// airship measures ~100 m across and its own visual is spawned at exactly
/// the pose the world-actor session binds, while the next world object is
/// hundreds of metres away.
const VISUAL_MATCH_M: f32 = 200.0;

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
    /// The entity that draws it, when anything does: the residency's visual
    /// for a zeppelin's subject, the player's own body for the player, and
    /// `None` for a scripted aircraft (whose record spawns a physics body
    /// with no visual today).
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
    /// The smallest luminance in the near field (the closest third of the
    /// frame), where fog is negligible: what the *lit* check reads.
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
    /// How many of the world's object records declare a known water surface.
    pub water_objects: usize,
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
    let mut checks = Vec::new();

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
                    "the near field spans luminance {}..{} (spread {}, floor {}); the whole \
                     frame spans {}..{}, whose spread is the fog band rather than shading",
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
        Some(sea) if sea.water_objects == 0 => checks.push(Check {
            id: "world_sea_visible".to_owned(),
            applies: false,
            passed: false,
            detail: "the world's object records declare no known water surface, so no sea is \
                     required here (the drawn sea plane is the horizon plugin's designed one)"
                .to_owned(),
        }),
        Some(sea) => checks.push(Check {
            id: "world_sea_visible".to_owned(),
            applies: true,
            passed: sea.covered_permille >= MIN_SEA_COVERAGE_PERMILLE,
            detail: format!(
                "the sea plane covers {} of {} pixels ({} per mille, floor {}); {} of the \
                 world's objects declare water",
                sea.sea_pixels,
                sea.total_pixels,
                sea.covered_permille,
                MIN_SEA_COVERAGE_PERMILLE,
                sea.water_objects
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
    let hud_match = m.hud.model_line == m.hud.drawn_line
        && (m.hud.model_line.is_none()
            || (m.hud.drawn_visible
                && m.hud.region_text_pixels.unwrap_or(0) >= MIN_OBJECTIVE_TEXT_PIXELS));
    checks.push(Check {
        id: "hud_objective_matches".to_owned(),
        applies: true,
        passed: hud_match,
        detail: format!(
            "model {:?}, drawn {:?} (visible {}, region text pixels {:?}): the drawn line must \
             equal the read model's and hold at least {MIN_OBJECTIVE_TEXT_PIXELS} pixels of its \
             own text when one is current",
            m.hud.model_line, m.hud.drawn_line, m.hud.drawn_visible, m.hud.region_text_pixels
        ),
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
    /// The smallest luminance among those pixels in the near field (the
    /// closest third of the frame), where fog is negligible.
    pub near_luminance_min: u8,
    /// The largest luminance among those pixels in the near field.
    pub near_luminance_max: u8,
    /// Yellow-ish objective-text pixels counted inside the objective UI region
    /// (`None` for frames the HUD probe did not measure).
    pub text_pixels: Option<usize>,
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
    handle: MissionSmokeHandle,
}

impl MissionSmokePlugin {
    /// A smoke run that writes into `request.capture_dir`.
    #[must_use]
    pub fn new(request: crate::playtest::SmokeRequest) -> Self {
        Self {
            request,
            handle: MissionSmokeHandle::default(),
        }
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
    /// [`Self::SeaVisible`] plus the sea plane.
    SeaAbsent,
    /// Hide everything drawn except this entity (`None`: the player only).
    Actor(Option<Entity>),
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
    handle: MissionSmokeHandle,
}

/// The run's own state, filled by [`smoke_step`].
#[derive(Resource, Default)]
struct SmokeData {
    frame: u32,
    phase: Phase,
    phase_start: u32,
    census: Vec<CensusEntry>,
    measured_start_m: Option<[f32; 3]>,
    anchor_position_m: Option<Vec3>,
    water_objects: usize,
    speed_m_s: f32,
    measurements: SmokeMeasurements,
    placements: Vec<String>,
    requested: Vec<String>,
    shots_requested: usize,
    effects_at_hold_start: Option<u64>,
    hud_region: Option<[u32; 4]>,
    rounds_probe_pending: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Settle,
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
            handle: self.handle.clone(),
        })
        .init_resource::<SmokeShots>()
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
        .add_systems(
            Last,
            (smoke_census, smoke_sample, smoke_script, smoke_rounds).chain(),
        );
    }
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
    mut last: Local<ProbeMode>,
    mut visibility: Query<&mut Visibility>,
    domes: Query<Entity, With<HorizonDome>>,
    seas: Query<Entity, With<HorizonSea>>,
    weather: Query<Entity, With<WeatherLayer>>,
    precip: Query<Entity, With<PrecipitationLayer>>,
    players: Query<Entity, With<PlaytestAircraft>>,
    visuals: Query<Entity, With<WorldVisual>>,
    ui: Query<Entity, With<Node>>,
) {
    let mode = mode.0;
    if *last != mode {
        *last = mode;
        println!("mission smoke probe: {mode:?}");
    }
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
    write_all(&mut visibility, domes.iter(), hidden);
    write_all(&mut visibility, weather.iter(), hidden);
    write_all(&mut visibility, precip.iter(), hidden);
    write_all(&mut visibility, seas.iter(), sea_value);
    write_all(
        &mut visibility,
        players.iter(),
        if anything_hidden { hidden } else { shown },
    );
    write_all(&mut visibility, visuals.iter(), world_value);
    write_all(
        &mut visibility,
        ui.iter(),
        if anything_hidden { hidden } else { shown },
    );
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

/// The objective widget's own text on screen and its visibility, when one
/// carries the caption.
fn drawn_objective(
    nodes: &Query<(&Text, &Visibility, &GlobalTransform, &ComputedNode)>,
) -> Option<(String, bool)> {
    let (text, visibility, _, _) = nodes
        .iter()
        .find(|(text, ..)| text.0.starts_with(OBJECTIVE_HUD_CAPTION))?;
    Some((text.0.clone(), matches!(visibility, Visibility::Visible)))
}

/// The objective widget's region in framebuffer pixels, from the node's
/// physical-pixel layout.
fn objective_region(
    nodes: &Query<(&Text, &Visibility, &GlobalTransform, &ComputedNode)>,
) -> Option<[u32; 4]> {
    let (_, _, transform, node) = nodes
        .iter()
        .find(|(text, ..)| text.0.starts_with(OBJECTIVE_HUD_CAPTION))?;
    let center = {
        let translation = transform.translation();
        Vec2::new(translation.x, translation.y)
    };
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
    visuals: &Query<(Entity, &Transform), With<WorldVisual>>,
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
            // Nothing in the composition draws a scripted aircraft's record
            // today: it is a physics body with no visual, so its probe counts
            // the pixels nothing draws.
            drawn_entity: None,
            asleep: spawned.asleep,
            subject: None,
            hostile: false,
        });
    }
    for bound in &report.zeppelins {
        let (hostile, subject, visual) = zeppelins
            .iter()
            .find(|(_, binding, _)| binding.subject == bound.subject)
            .map_or(
                (false, bound.subject.key().to_owned(), None),
                |(actor, binding, _)| {
                    // The residency keys its objects by node slot
                    // (`node-<n>`), never by the scene-node name the actor's
                    // subject carries, so the drawn visual of a world actor is
                    // matched by **pose**: the residency's own visual at the
                    // actor's bind-time position, within
                    // [`VISUAL_MATCH_M`]. The rule is designed and
                    // fail-closed — no match is no drawn entity, and the
                    // probe then counts the pixels nothing drew.
                    let anchor = Vec3::new(
                        actor.position_m[0] as f32,
                        actor.position_m[1] as f32,
                        actor.position_m[2] as f32,
                    );
                    let visual = visuals
                        .iter()
                        .filter_map(|(entity, transform)| {
                            let distance = transform.translation.distance(anchor);
                            (distance <= VISUAL_MATCH_M).then_some((entity, distance))
                        })
                        .min_by(|left, right| left.1.total_cmp(&right.1))
                        .map(|(entity, _)| entity);
                    (
                        actor
                            .roster
                            .as_ref()
                            .is_some_and(|roster| roster.faction.key() == "enemy"),
                        binding.subject.key().to_owned(),
                        visual,
                    )
                },
            );
        census.push(CensusEntry {
            kind: "zeppelin",
            name: subject.clone(),
            pose_entity: bound.entity,
            drawn_entity: visual,
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
    visuals: Query<(Entity, &Transform), With<WorldVisual>>,
    player: Query<Entity, With<MissionPlayerBody>>,
    residency: Option<Res<WorldResidency>>,
) {
    data.frame += 1;
    let frame = data.frame;
    if frame != 1 {
        return;
    }
    let (census, water_objects) = build_census(
        spawn_report.as_deref(),
        player.single().ok().unwrap_or(Entity::PLACEHOLDER),
        &zeppelins,
        &visuals,
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
            // Fail-closed by name: a member the census could not find a drawn
            // entity for says so, rather than reporting an unexplained zero.
            note: entry.drawn_entity.is_none().then(|| match entry.kind {
                "aircraft" => "the record spawns a physics body with no visual, so nothing \
                               draws this actor"
                    .to_owned(),
                "zeppelin" => format!(
                    "no residency visual sat within {VISUAL_MATCH_M} m of the actor's bind-time \
                     pose, so its drawn body could not be matched to it"
                ),
                _ => "no drawn entity was found for this member".to_owned(),
            }),
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

/// Samples the live world into the measurements every frame: the audio queue,
/// the weapon session's rounds and the ordnance session's rockets.
#[allow(clippy::too_many_arguments)]
fn smoke_sample(
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
            let effects = u64::try_from(weapons.effects().len()).unwrap_or(u64::MAX);
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
    if let Some(line) = current_objective(host.as_deref(), messages.as_deref()) {
        data.measurements.approach.objective_woke = true;
        if data.measurements.hud.model_line.is_none() {
            data.measurements.hud.model_line = Some(line);
        }
    }
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
    objective_nodes: Query<(&Text, &Visibility, &GlobalTransform, &ComputedNode)>,
    player: Query<&Position, With<MissionPlayerBody>>,
    poses: Query<(Option<&Position>, Option<&Transform>)>,
    camera: Query<(&Camera, &GlobalTransform), With<crate::playtest::PlaytestCameraMarker>>,
) {
    let frame = data.frame;
    if frame == 1 {
        // A capture directory that cannot be created surfaces later, when the
        // artifacts are written: the refusal names the path either way.
        let _ = std::fs::create_dir_all(&config.request.capture_dir);
    }

    // --- the script --------------------------------------------------------
    let mut due: Vec<String> = Vec::new();
    let at = frame.saturating_sub(data.phase_start);
    match data.phase {
        Phase::Settle if frame >= SETTLE_FRAMES => {
            data.phase = Phase::WorldProbe;
            data.phase_start = frame + 1;
        }
        Phase::Settle => {}
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
                queue.0.push(KeyOp::Pause);
                probe.0 = ProbeMode::SeaVisible;
            } else if at == SEA_VISIBLE_SHOT_OFFSET {
                due.push("sea_visible".to_owned());
            } else if at == SEA_HIDE_OFFSET {
                probe.0 = ProbeMode::SeaAbsent;
            } else if at == SEA_ABSENT_SHOT_OFFSET {
                due.push("sea_absent".to_owned());
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
        );
    }

    // --- the end -----------------------------------------------------------
    let delivered = shots.0.lock().map_or(0, |shots| shots.len());
    let grace_over = data.phase == Phase::Final && frame >= data.phase_start + SHOT_GRACE_FRAMES;
    let script_done = data.phase == Phase::Final && delivered >= data.shots_requested;
    if (script_done || grace_over) && frame > data.phase_start {
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
        finish_measurements(&mut data, &delivered);
        let outcome = write_artifacts(&config.request.capture_dir, &data, &delivered)
            .map(|()| build_report(&data, delivered))
            .map_err(|(path, error)| format!("cannot write {}: {error}", path.display()));
        if let Ok(mut slot) = config.handle.0.lock() {
            *slot = Some(outcome);
        }
        data.phase = Phase::Ended;
        exit.write(AppExit::Success);
    }
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
    if let (Some(visible), Some(absent)) = (find("sea_visible"), find("sea_absent")) {
        let total = (visible.width as usize) * (visible.height as usize);
        let sea = visible.off_clear.saturating_sub(absent.off_clear);
        data.measurements.sea = Some(SeaMetrics {
            sea_pixels: sea,
            total_pixels: total,
            covered_permille: permille(sea, total),
            water_objects: data.water_objects,
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
) {
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>| {
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
    let mut near_min = u8::MAX;
    let mut near_max = 0_u8;
    let mut text_pixels = 0_usize;
    let width = image.width() as usize;
    let height = image.height() as usize;
    let near_from = height.saturating_mul(2) / 3;
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
        sum += u64::from(luminance);
        let py = index / width;
        if is_off(colour) {
            off_colours.insert(colour);
            off_min = off_min.min(luminance);
            off_max = off_max.max(luminance);
            if py >= near_from {
                near_min = near_min.min(luminance);
                near_max = near_max.max(luminance);
            }
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
        near_luminance_min: if near_min == u8::MAX { 0 } else { near_min },
        near_luminance_max: near_max,
        text_pixels: region.map(|_| text_pixels),
    }
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
                 \"text_pixels\":{}}}",
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
                 \"on_screen\":{},\"pixels\":{},\"total_pixels\":{}}}",
                json_string(&actor.kind),
                json_string(&actor.name),
                actor.asleep,
                actor.placed,
                json_option_f64(actor.placement_m),
                actor.on_screen,
                actor.pixels,
                actor.total_pixels,
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
    let body = format!(
        "{{\"kind\":{kind},\"mission\":{mission},\"human_play\":false,\
         \"verified_original\":false,\"frames\":{frames},\"ticks\":{ticks},\
         \"hud\":{{\"model_line\":{model_line},\"drawn_line\":{drawn_line},\
         \"drawn_visible\":{visible},\"region_text_pixels\":{region}}},\
         \"guns\":{{\"effects\":{effects},\"max_live_rounds\":{live},\"rounds_drawn\":{drawn},\
         \"rounds_examined\":{examined}}},\"rocket\":{{\"launched\":{launched},\
         \"max_rockets\":{max_rockets},\"refusals\":[{refusals}]}},\
         \"audio\":{{\"radio_seen\":{radio_seen},\"engine_seen\":{engine_seen},\
         \"engine_bindings\":{bindings},\"active_loops\":{loops},\
         \"radio_started\":{radio_started}}},\
         \"approach\":{{\"placed\":{approach_placed},\"anchor\":{anchor},\
         \"frames\":{approach_frames},\"objective_woke\":{woke}}},\
         \"actors\":[{actors}],\"placements\":[{placements}],\"shots\":[{shots}],\
         \"checks\":[{checks}],\"failures\":[{failures}],\"passed\":{passed}}}\n",
        kind = json_string(SMOKE_KIND),
        mission = json_string(&report.mission),
        frames = report.frames,
        ticks = report.ticks,
        model_line = model_line,
        drawn_line = drawn_line,
        visible = report.measurements.hud.drawn_visible,
        region = json_option_usize(report.measurements.hud.region_text_pixels),
        effects = report.measurements.guns.effects,
        live = report.measurements.guns.max_live_rounds,
        drawn = json_option_bool(report.measurements.guns.rounds_drawn),
        examined = report.measurements.guns.rounds_examined,
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
