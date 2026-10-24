//! The `flak_effect` a flak round's burst starts (FLAK-C3, #1885).
//!
//! FLAK-C1 settled that a proximity or range burst of `wep_27` starts the
//! default `IMPACT` row's animation `flak_effect` (`flak_control.zrd`) at the
//! burst point. Its stored parts are read here and presented from an
//! [`OrdnanceBurst`] published on [`PendingFlakBursts`]:
//!
//! * the orange light flash: `LIGHT_STATE flak_light1` (`RANGE`, `COLOR`) and
//!   its two `LIGHT_ANIMATION` steps, drawn as a point light ([`FlakFlash`]);
//! * the `large_black_smokeball`: handed to the burst puffs of #1806 as a
//!   [`PuffBurst`] at the burst point, unchanged;
//! * the close-range framebuffer flicker: the `IF PLAYER_RANGE` gate and the
//!   `FBFX_COLOR_FROM_TO` steps of `frame_buffer_effects1`, drawn as a
//!   full-window colour overlay ([`FlakFlicker`]) only when the burst is within
//!   the stored range of the player body. FLAK-C1 measured the compare: the
//!   engine stores the range **squared** and passes at `distance² <= range²`.
//!
//! # What is designed, what is not drawn
//!
//! Filed under [`FLAK_FX_DESIGN_CLAIM`] (the data states no unit or curve):
//! the second `RANGE` number is the point light's range and each
//! `LIGHT_ANIMATION` adds its first two numbers linearly over its `RUN_TIME`
//! (the third and fourth words are unmeasured and ignored); the light's
//! intensity; the overlay blends the stored RGBA of each `FBFX_COLOR_FROM_TO`
//! linearly over its `RUN_TIME`, steps in stored order; the flicker distance is
//! measured from the burst point (what the animation node is for an animation
//! started at a position is unresolved, FLAK-C1) and it starts with the burst.
//!
//! # The flak trails (FLAK-C3-TRAILS, #1925)
//!
//! `call_flak_trails` (`flak_trails.zrd`) throws five `flak_trailN` objects
//! with `OBJECT_MOTION`, each followed by a `squeezpuffer` every
//! `DISTANCE_INTERVAL`. The motion's law is measured from the executor
//! (`docs/findings/2026-10-24-flak-c3-trails-object-motion.md`, rows
//! `[[object_motion_fact]]`): the two `TRANSLATION_RANGE_*` records give, per
//! word, a uniform draw between MIN and MAX; word 0 is the heading in degrees
//! (0 points to -Z), word 1 the elevation in degrees (`e90 = elevation / 90`),
//! word 2 the launch speed and word 3 a constant acceleration along the same
//! direction; the direction is `(-sin h * T, e90, -cos h * T)` with
//! `T = 1 - |e90|`; `GRAVITY LOCAL g` adds `g` to the acceleration's second
//! component; every step moves the object by `step * velocity` and then adds
//! `step * acceleration` to the velocity, for exactly `RUN_TIME` seconds
//! ([`trail_launch`], [`TrailBody::step`]).
//!
//! **Designed, filed under [`FLAK_TRAILS_DESIGN_CLAIM`]:** the engine's
//! random table is `rand()` at run time, so the draws are a deterministic
//! generator seeded from the burst; the motion frame is the world's (the
//! animation node's own frame is unresolved, FLAK-C1); a puff is emitted each
//! time the object has travelled `DISTANCE_INTERVAL` metres, none at the
//! start; a puff starts at `WORLD_VELOCITY` plus a draw between the random
//! velocities and is otherwise a burst puff. **Not applied:** `FORWARD_ROTATION`
//! (it turns the object about its own axes and the object's puffer sits at its
//! origin, so no puff moves), the ground clamp of the executor (it keeps a
//! falling object above the terrain; its reach is unmeasured), the trail
//! objects' own geometry (unmeasured), the `CALL_SEQUENCE` `AT_NODE` offsets,
//! `GROWTH_FACTOR`, `FADE_RANGE`, `DEVIATION_DISTANCE` and the time keys of
//! `TEXTURE_SEQUENCE`.

use std::fmt;
use std::path::Path;

use bevy::asset::Assets;
use bevy::prelude::{
    App, BackgroundColor, Color, Commands, Component, Entity, FixedPostUpdate, GlobalTransform,
    GlobalZIndex, IntoScheduleConfigs, Node, Plugin, PointLight, Query, Res, ResMut, Resource,
    StandardMaterial, Transform, Val, Vec3, Visibility, With, default,
};
use cs_formats::zbd::{
    EffectAnimation, EffectEvent, ReaderNode, read_animation_definitions, read_reader_nodes,
};
use cs_types::Tick;

use super::burst_puffs::{
    BurstPuff, BurstPuffArt, PendingOrdnanceBursts, PuffBurst, Splitmix, build_art,
    spawn_burst_puffs, spawn_puff,
};
use super::compose::MissionPlayerBody;
use super::flak_round::{BURST_IMPACT_ROW, OrdnanceBurst};
use super::launch_sound::reader_member;
use super::weapons::drive_mission_ordnance;
use crate::physics::PhysicsTickLedger;

/// The claim the light and overlay curves are filed under.
pub const FLAK_FX_DESIGN_CLAIM: &str = "flak-c3.designed-flash-and-flicker-curves";

/// The reader archive and member holding the effect.
pub const FLAK_EFFECT_MEMBER: &str = "flak_control.zrd";
/// The animation a flak burst starts.
pub const FLAK_EFFECT_ANIMATION: &str = "flak_effect";
/// The sequence the `IF PLAYER_RANGE` gate calls.
pub const FLAK_FLICKER_SEQUENCE: &str = "frame_buffer_effects1";

/// Designed: candela of the flash light (the data states no intensity).
const FLASH_INTENSITY: f32 = 4.0e6;

/// One `LIGHT_ANIMATION` step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashStep {
    /// The first two `RANGE` numbers: added to the light's range pair.
    pub delta_range: [f32; 2],
    /// `RUN_TIME`, seconds.
    pub run_time_s: f32,
}

/// One `FBFX_COLOR_FROM_TO` step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlickerStep {
    /// `FROM`: r, g, b, a.
    pub from: [f32; 4],
    /// `TO`: r, g, b, a.
    pub to: [f32; 4],
    /// `RUN_TIME`, seconds.
    pub run_time_s: f32,
}

/// What the stored `flak_effect` states.
#[derive(Clone, Debug, PartialEq)]
pub struct FlakEffectDefinition {
    /// `LIGHT_STATE` `RANGE`.
    pub light_range: [f32; 2],
    /// `LIGHT_STATE` `COLOR`.
    pub light_color: [f32; 3],
    /// The `LIGHT_ANIMATION` steps, in stored order.
    pub flash_steps: Vec<FlashStep>,
    /// `IF PLAYER_RANGE`, as stored (metres; the engine squares it).
    pub player_range_m: f32,
    /// The flicker steps, in stored order.
    pub flicker_steps: Vec<FlickerStep>,
}

/// Why the effect could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlakEffectError {
    /// The archive or token stream could not be read.
    Read(String),
    /// The record lacks a part the presentation needs.
    Unmeasured(String),
}

impl fmt::Display for FlakEffectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(r) => write!(f, "flak effect unreadable: {r}"),
            Self::Unmeasured(r) => write!(f, "flak effect unmeasured: {r}"),
        }
    }
}

impl std::error::Error for FlakEffectError {}

/// The values after `key` in an `"A" (..) "B" (..)` run of sibling pairs.
fn field<'n, 'a>(pairs: &'n [ReaderNode<'a>], key: &str) -> Option<&'n [ReaderNode<'a>]> {
    pairs
        .chunks(2)
        .find(|pair| pair[0].as_str() == Some(key))
        .and_then(|pair| pair.get(1))
        .and_then(ReaderNode::as_list)
}

fn floats<const N: usize>(values: &[ReaderNode<'_>]) -> Option<[f32; N]> {
    let numbers: Vec<f32> = values.iter().filter_map(ReaderNode::as_number).collect();
    numbers.get(..N)?.try_into().ok()
}

fn other_events<'n, 'a>(
    animation: &'n EffectAnimation<'a>,
    keyword: &'n str,
) -> impl Iterator<Item = &'n [ReaderNode<'a>]> + 'n {
    animation
        .sequences
        .iter()
        .flat_map(|sequence| sequence.events.iter())
        .filter_map(move |event| match event {
            EffectEvent::Other { keyword: k, value } if *k == keyword => value.as_list(),
            _ => None,
        })
}

/// Reads `flak_effect` from the retail reader archive.
///
/// # Errors
///
/// [`FlakEffectError`] when the archive cannot be read or a part is absent.
pub fn read_flak_effect(install_root: &Path) -> Result<FlakEffectDefinition, FlakEffectError> {
    let bytes = reader_member(
        install_root,
        super::burst_puffs::BURST_PUFF_ARCHIVE,
        FLAK_EFFECT_MEMBER,
    )
    .map_err(FlakEffectError::Read)?;
    let nodes = read_reader_nodes(&bytes).map_err(|e| FlakEffectError::Read(e.to_string()))?;
    let animations =
        read_animation_definitions(&nodes).map_err(|e| FlakEffectError::Read(e.to_string()))?;
    definition_from(&animations)
}

/// Builds the definition from decoded animations.
///
/// # Errors
///
/// [`FlakEffectError::Unmeasured`] when `flak_effect` or one of its parts is
/// absent.
pub fn definition_from(
    animations: &[EffectAnimation<'_>],
) -> Result<FlakEffectDefinition, FlakEffectError> {
    let need = |what: &str| FlakEffectError::Unmeasured(format!("{FLAK_EFFECT_ANIMATION}: {what}"));
    let effect = animations
        .iter()
        .find(|a| a.id() == FLAK_EFFECT_ANIMATION)
        .ok_or_else(|| need("no such animation"))?;
    let light = effect
        .lights()
        .find(|l| l.range.is_some() && l.color.is_some())
        .ok_or_else(|| need("no LIGHT_STATE with RANGE and COLOR"))?;
    let flash_steps = other_events(effect, "LIGHT_ANIMATION")
        .filter_map(|pairs| {
            let range = field(pairs, "RANGE").and_then(floats::<2>)?;
            let run = field(pairs, "RUN_TIME").and_then(floats::<1>)?;
            Some(FlashStep {
                delta_range: range,
                run_time_s: run[0],
            })
        })
        .collect::<Vec<_>>();
    if flash_steps.is_empty() {
        return Err(need("no LIGHT_ANIMATION step"));
    }
    let player_range_m = other_events(effect, "IF")
        .find_map(|pairs| field(pairs, "PLAYER_RANGE").and_then(floats::<1>))
        .map(|r| r[0])
        .ok_or_else(|| need("no IF PLAYER_RANGE"))?;
    let flicker = animations
        .iter()
        .find(|a| a.id() == FLAK_EFFECT_ANIMATION)
        .into_iter()
        .flat_map(|a| a.sequences.iter())
        .find(|s| s.name == Some(FLAK_FLICKER_SEQUENCE))
        .ok_or_else(|| need("no frame_buffer_effects1 sequence"))?;
    let flicker_steps = flicker
        .events
        .iter()
        .filter_map(|event| match event {
            EffectEvent::Other { keyword, value } if *keyword == "FBFX_COLOR_FROM_TO" => {
                value.as_list()
            }
            _ => None,
        })
        .filter_map(|pairs| {
            Some(FlickerStep {
                from: field(pairs, "FROM").and_then(floats::<4>)?,
                to: field(pairs, "TO").and_then(floats::<4>)?,
                run_time_s: field(pairs, "RUN_TIME").and_then(floats::<1>)?[0],
            })
        })
        .collect::<Vec<_>>();
    if flicker_steps.is_empty() {
        return Err(need("no FBFX_COLOR_FROM_TO step"));
    }
    Ok(FlakEffectDefinition {
        light_range: light.range.unwrap_or_default(),
        light_color: light.color.unwrap_or_default(),
        flash_steps,
        player_range_m,
        flicker_steps,
    })
}

/// The flash light's range pair `age_s` seconds after the burst, `None` once
/// the last step has run.
#[must_use]
pub fn flash_range_at(definition: &FlakEffectDefinition, age_s: f32) -> Option<[f32; 2]> {
    let mut range = definition.light_range;
    let mut start = 0.0_f32;
    for step in &definition.flash_steps {
        let end = start + step.run_time_s;
        if age_s < end {
            let t = if step.run_time_s > 0.0 {
                (age_s - start) / step.run_time_s
            } else {
                1.0
            };
            return Some([
                (range[0] + step.delta_range[0] * t).max(0.0),
                (range[1] + step.delta_range[1] * t).max(0.0),
            ]);
        }
        range = [
            range[0] + step.delta_range[0],
            range[1] + step.delta_range[1],
        ];
        start = end;
    }
    None
}

/// The overlay's RGBA `age_s` seconds after the burst, `None` once the last
/// step has run.
#[must_use]
pub fn flicker_color_at(definition: &FlakEffectDefinition, age_s: f32) -> Option<[f32; 4]> {
    let mut start = 0.0_f32;
    for step in &definition.flicker_steps {
        let end = start + step.run_time_s;
        if age_s < end {
            let t = if step.run_time_s > 0.0 {
                (age_s - start) / step.run_time_s
            } else {
                1.0
            };
            return Some(std::array::from_fn(|i| {
                step.from[i] + (step.to[i] - step.from[i]) * t
            }));
        }
        start = end;
    }
    None
}

/// Whether a burst at `burst_m` flickers the frame for a player at `player_m`:
/// the engine's compare, `distance² <= PLAYER_RANGE²` (FLAK-C1).
#[must_use]
pub fn flickers(definition: &FlakEffectDefinition, burst_m: [f64; 3], player_m: [f64; 3]) -> bool {
    let distance_sq: f64 = burst_m
        .iter()
        .zip(player_m)
        .map(|(a, b)| (a - b) * (a - b))
        .sum();
    let range = f64::from(definition.player_range_m);
    distance_sq <= range * range
}

/// The claim the trail draws, frame and puff details are filed under.
pub const FLAK_TRAILS_DESIGN_CLAIM: &str = "flak-c3-trails.designed-draws-frame-and-puffs";

/// The reader member holding `call_flak_trails`.
pub const FLAK_TRAILS_MEMBER: &str = "flak_trails.zrd";
/// The animation `flak_effect` calls for its trails.
pub const FLAK_TRAILS_ANIMATION: &str = "call_flak_trails";

/// One stored `OBJECT_MOTION` of a trail object.
#[derive(Clone, Debug, PartialEq)]
pub struct TrailMotion {
    /// `NAME`: the object (`flak_trail1`..).
    pub node: String,
    /// `GRAVITY LOCAL g`: added to the acceleration's second component.
    pub gravity: f32,
    /// `TRANSLATION_RANGE_MIN`: heading degrees, elevation degrees, speed,
    /// acceleration.
    pub range_min: [f32; 4],
    /// `TRANSLATION_RANGE_MAX`, same words.
    pub range_max: [f32; 4],
    /// `RUN_TIME`, seconds.
    pub run_time_s: f32,
}

/// The stored `PUFFER_STATE` that follows one trail object.
#[derive(Clone, Debug, PartialEq)]
pub struct TrailPuffer {
    /// `DISTANCE_INTERVAL`: one puff each time the object has moved this far.
    pub distance_interval_m: f32,
    /// `WORLD_VELOCITY`.
    pub world_velocity: [f32; 3],
    /// `MIN_RANDOM_VELOCITY`.
    pub min_velocity: [f32; 3],
    /// `MAX_RANDOM_VELOCITY`.
    pub max_velocity: [f32; 3],
    /// `FRICTION`.
    pub friction: f32,
    /// `SIZE_RANGE`.
    pub size: [f32; 2],
    /// `LIFETIME_RANGE`, seconds.
    pub lifetime_s: [f32; 2],
}

/// One trail object and its puffer.
#[derive(Clone, Debug, PartialEq)]
pub struct FlakTrail {
    /// The object's motion.
    pub motion: TrailMotion,
    /// Its puffer.
    pub puffer: TrailPuffer,
}

/// What the stored `call_flak_trails` states.
#[derive(Clone, Debug, PartialEq)]
pub struct FlakTrailsDefinition {
    /// The trail objects in stored order.
    pub trails: Vec<FlakTrail>,
}

/// Reads `call_flak_trails` from the retail reader archive.
///
/// # Errors
///
/// [`FlakEffectError`] when the archive cannot be read or a part is absent.
pub fn read_flak_trails(install_root: &Path) -> Result<FlakTrailsDefinition, FlakEffectError> {
    let bytes = reader_member(
        install_root,
        super::burst_puffs::BURST_PUFF_ARCHIVE,
        FLAK_TRAILS_MEMBER,
    )
    .map_err(FlakEffectError::Read)?;
    let nodes = read_reader_nodes(&bytes).map_err(|e| FlakEffectError::Read(e.to_string()))?;
    let animations =
        read_animation_definitions(&nodes).map_err(|e| FlakEffectError::Read(e.to_string()))?;
    trails_from(&animations)
}

/// Builds the trails from decoded animations.
///
/// # Errors
///
/// [`FlakEffectError::Unmeasured`] when the animation, a motion or its puffer
/// is absent or states a gravity other than `LOCAL <number>` (`DEFAULT` is a
/// run-time global, unmeasured).
pub fn trails_from(
    animations: &[EffectAnimation<'_>],
) -> Result<FlakTrailsDefinition, FlakEffectError> {
    let need =
        |what: String| FlakEffectError::Unmeasured(format!("{FLAK_TRAILS_ANIMATION}: {what}"));
    let animation = animations
        .iter()
        .find(|a| a.id() == FLAK_TRAILS_ANIMATION)
        .ok_or_else(|| need("no such animation".into()))?;
    let mut trails = Vec::new();
    for pairs in other_events(animation, "OBJECT_MOTION") {
        let node = field(pairs, "NAME")
            .and_then(|v| v.first())
            .and_then(ReaderNode::as_str)
            .ok_or_else(|| need("an OBJECT_MOTION without NAME".into()))?;
        let missing = |key: &str| need(format!("{node}: no {key}"));
        let gravity = match field(pairs, "GRAVITY") {
            Some([mode, value]) if mode.as_str() == Some("LOCAL") => value.as_number(),
            _ => None,
        }
        .ok_or_else(|| missing("GRAVITY LOCAL <number>"))?;
        let range_min = field(pairs, "TRANSLATION_RANGE_MIN")
            .and_then(floats::<4>)
            .ok_or_else(|| missing("TRANSLATION_RANGE_MIN"))?;
        let range_max = field(pairs, "TRANSLATION_RANGE_MAX")
            .and_then(floats::<4>)
            .ok_or_else(|| missing("TRANSLATION_RANGE_MAX"))?;
        let run_time_s = field(pairs, "RUN_TIME")
            .and_then(floats::<1>)
            .ok_or_else(|| missing("RUN_TIME"))?[0];
        let puffer = animation
            .puffers()
            .find(|p| p.active == Some(true) && p.at_node.as_ref().is_some_and(|a| a.node == node))
            .ok_or_else(|| missing("an active PUFFER_STATE AT_NODE it"))?;
        let puffer_missing = |key: &str| need(format!("{}: no {key}", puffer.name));
        trails.push(FlakTrail {
            motion: TrailMotion {
                node: node.to_owned(),
                gravity,
                range_min,
                range_max,
                run_time_s,
            },
            puffer: TrailPuffer {
                distance_interval_m: puffer
                    .distance_interval
                    .filter(|d| d.is_finite() && *d > 0.0)
                    .ok_or_else(|| puffer_missing("a positive DISTANCE_INTERVAL"))?,
                world_velocity: puffer
                    .world_velocity
                    .ok_or_else(|| puffer_missing("WORLD_VELOCITY"))?,
                min_velocity: puffer
                    .min_random_velocity
                    .ok_or_else(|| puffer_missing("MIN_RANDOM_VELOCITY"))?,
                max_velocity: puffer
                    .max_random_velocity
                    .ok_or_else(|| puffer_missing("MAX_RANDOM_VELOCITY"))?,
                friction: puffer.friction.ok_or_else(|| puffer_missing("FRICTION"))?,
                size: puffer
                    .size_range
                    .ok_or_else(|| puffer_missing("SIZE_RANGE"))?,
                lifetime_s: puffer
                    .lifetime_range
                    .ok_or_else(|| puffer_missing("LIFETIME_RANGE"))?,
            },
        });
    }
    if trails.is_empty() {
        return Err(need("no OBJECT_MOTION".into()));
    }
    Ok(FlakTrailsDefinition { trails })
}

/// The launch the executor computes from one `OBJECT_MOTION`: the initial
/// velocity and the constant acceleration, in the motion's frame (metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailLaunch {
    /// Initial velocity, metres per second.
    pub velocity: Vec3,
    /// Constant acceleration, metres per second squared.
    pub acceleration: Vec3,
}

/// The executor's launch from four unit draws (heading, elevation, speed,
/// acceleration), each a position between the stored MIN and MAX of its word
/// (`static`, `[[object_motion_fact]]` rows `exec.draw_heading`..
/// `exec.gravity_local`).
#[must_use]
pub fn trail_launch(motion: &TrailMotion, draws: [f32; 4]) -> TrailLaunch {
    let word =
        |i: usize| motion.range_min[i] + (motion.range_max[i] - motion.range_min[i]) * draws[i];
    let heading = word(0).to_radians();
    let e90 = word(1) / 90.0;
    let horizontal = if e90 < 0.0 { 1.0 + e90 } else { 1.0 - e90 };
    let direction = Vec3::new(
        -heading.sin() * horizontal,
        e90,
        -heading.cos() * horizontal,
    );
    TrailLaunch {
        velocity: direction * word(2),
        acceleration: direction * word(3) + Vec3::new(0.0, motion.gravity, 0.0),
    }
}

/// One trail object in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailBody {
    /// Position, metres.
    pub position: Vec3,
    /// Velocity, metres per second.
    pub velocity: Vec3,
    /// Acceleration, metres per second squared.
    pub acceleration: Vec3,
    /// Seconds flown.
    pub elapsed_s: f32,
    /// `RUN_TIME`.
    pub run_time_s: f32,
}

impl TrailBody {
    /// A body launched from `position`.
    #[must_use]
    pub fn launched(position: Vec3, launch: TrailLaunch, run_time_s: f32) -> Self {
        Self {
            position,
            velocity: launch.velocity,
            acceleration: launch.acceleration,
            elapsed_s: 0.0,
            run_time_s,
        }
    }

    /// One frame of `frame_s` seconds in the executor's order: the last frame
    /// is trimmed so the flight lasts exactly `RUN_TIME`; the object moves by
    /// `step * velocity` and then `step * acceleration` is added to the
    /// velocity (`exec.step_dt`, `exec.step_displacement`,
    /// `exec.velocity_integrate`). Returns the position before the move, and
    /// whether the flight is over (`exec.run_time_end`).
    pub fn step(&mut self, frame_s: f32) -> (Vec3, bool) {
        let before = self.position;
        let elapsed = self.elapsed_s + frame_s;
        let step = if elapsed > self.run_time_s {
            frame_s - (elapsed - self.run_time_s)
        } else {
            frame_s
        }
        .max(0.0);
        self.position += self.velocity * step;
        self.velocity += self.acceleration * step;
        self.elapsed_s = elapsed;
        (before, elapsed >= self.run_time_s)
    }
}

/// The measured trails, as the stage carries them.
#[derive(Resource, Clone, Debug)]
pub struct FlakTrailsResource(pub FlakTrailsDefinition);

/// Component: one flying trail object (an empty node: only its puffs are
/// drawn).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FlakTrailBody {
    /// Index into [`FlakTrailsDefinition::trails`].
    pub trail: usize,
    /// The flight.
    pub body: TrailBody,
    /// Metres moved since the last puff.
    pub since_puff_m: f32,
    /// State of the deterministic generator.
    pub rng: u64,
}

/// Flak bursts published by the round's owner and not yet presented.
#[derive(Resource, Clone, Debug, Default)]
pub struct PendingFlakBursts(pub Vec<OrdnanceBurst>);

/// The measured effect, as the stage carries it.
#[derive(Resource, Clone, Debug)]
pub struct FlakEffectResource(pub FlakEffectDefinition);

/// What the presentation did, for the smoke check and the tests.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlakBurstFxReport {
    /// Bursts presented.
    pub bursts_presented: u64,
    /// Bursts dropped: no effect installed, or not the default `IMPACT` row.
    pub bursts_undrawn: u64,
    /// Flash lights spawned.
    pub flashes_spawned: u64,
    /// Smoke balls handed to the burst puffs.
    pub smoke_balls_requested: u64,
    /// Flickers started (player within the stored range).
    pub flickers_started: u64,
    /// Bursts whose flicker was skipped (player out of range or absent).
    pub flickers_skipped: u64,
    /// Bursts whose `call_flak_trails` was not drawn: no trails installed.
    pub trails_undrawn: u64,
    /// Trail objects thrown.
    pub trails_thrown: u64,
    /// Trail puffs spawned.
    pub trail_puffs_spawned: u64,
    /// Trail puffs dropped: no puff art installed.
    pub trail_puffs_undrawn: u64,
}

/// Component: the flash light of one burst.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FlakFlash {
    /// Tick it was born on.
    pub born: Tick,
}

/// Component: the framebuffer overlay of one close burst.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FlakFlicker {
    /// Tick it was born on.
    pub born: Tick,
}

fn seconds_since(ledger: &PhysicsTickLedger, born: Tick) -> f32 {
    ledger.ticks.saturating_sub(born.0) as f32 * ledger.timestep_s
}

fn present_flak_bursts(
    mut commands: Commands,
    mut pending: ResMut<PendingFlakBursts>,
    mut puffs: ResMut<PendingOrdnanceBursts>,
    mut report: ResMut<FlakBurstFxReport>,
    effect: Option<Res<FlakEffectResource>>,
    trails: Option<Res<FlakTrailsResource>>,
    player: Query<&GlobalTransform, With<MissionPlayerBody>>,
) {
    for burst in std::mem::take(&mut pending.0) {
        let Some(effect) = effect
            .as_ref()
            .filter(|_| burst.impact_row == BURST_IMPACT_ROW)
        else {
            report.bursts_undrawn += 1;
            continue;
        };
        let definition = &effect.0;
        report.bursts_presented += 1;
        let [x, y, z] = burst.position_m.map(|c| c as f32);
        let position = Vec3::new(x, y, z);
        commands.spawn((
            FlakFlash { born: burst.at },
            PointLight {
                color: Color::srgb(
                    definition.light_color[0],
                    definition.light_color[1],
                    definition.light_color[2],
                ),
                intensity: FLASH_INTENSITY,
                range: definition.light_range[1],
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(position),
            Visibility::Visible,
        ));
        report.flashes_spawned += 1;
        puffs.0.push(PuffBurst {
            at: burst.at,
            position,
        });
        report.smoke_balls_requested += 1;
        match trails.as_ref() {
            Some(trails) => {
                let seed = burst.at.0
                    ^ burst.position_m[0].to_bits()
                    ^ (burst.position_m[1].to_bits() << 21)
                    ^ (burst.position_m[2].to_bits() << 42);
                for (index, trail) in trails.0.trails.iter().enumerate() {
                    let mut rng = Splitmix(seed ^ ((index as u64 + 1) << 56));
                    let draws = std::array::from_fn(|_| rng.next_unit());
                    let launch = trail_launch(&trail.motion, draws);
                    commands.spawn(FlakTrailBody {
                        trail: index,
                        body: TrailBody::launched(position, launch, trail.motion.run_time_s),
                        since_puff_m: 0.0,
                        rng: rng.0,
                    });
                    report.trails_thrown += 1;
                }
            }
            None => report.trails_undrawn += 1,
        }
        let near = player.iter().next().is_some_and(|body| {
            flickers(
                definition,
                burst.position_m,
                body.translation().as_dvec3().to_array(),
            )
        });
        if near {
            commands.spawn((
                FlakFlicker { born: burst.at },
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    position_type: bevy::prelude::PositionType::Absolute,
                    ..default()
                },
                BackgroundColor(Color::NONE),
                GlobalZIndex(i32::MAX - 1),
            ));
            report.flickers_started += 1;
        } else {
            report.flickers_skipped += 1;
        }
    }
}

fn advance_flak_fx(
    mut commands: Commands,
    ledger: Option<Res<PhysicsTickLedger>>,
    effect: Option<Res<FlakEffectResource>>,
    mut flashes: Query<(Entity, &FlakFlash, &mut PointLight)>,
    mut flickers: Query<(Entity, &FlakFlicker, &mut BackgroundColor)>,
) {
    let (Some(ledger), Some(effect)) = (ledger, effect) else {
        return;
    };
    for (entity, flash, mut light) in &mut flashes {
        match flash_range_at(&effect.0, seconds_since(&ledger, flash.born)) {
            Some(range) => light.range = range[1],
            None => commands.entity(entity).despawn(),
        }
    }
    for (entity, flicker, mut background) in &mut flickers {
        match flicker_color_at(&effect.0, seconds_since(&ledger, flicker.born)) {
            Some([r, g, b, a]) => background.0 = Color::srgba(r, g, b, a),
            None => commands.entity(entity).despawn(),
        }
    }
}

fn advance_flak_trails(
    mut commands: Commands,
    ledger: Option<Res<PhysicsTickLedger>>,
    trails: Option<Res<FlakTrailsResource>>,
    art: Option<Res<BurstPuffArt>>,
    mut report: ResMut<FlakBurstFxReport>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut bodies: Query<(Entity, &mut FlakTrailBody)>,
) {
    let (Some(ledger), Some(trails)) = (ledger, trails) else {
        return;
    };
    if ledger.timestep_s <= 0.0 {
        return;
    }
    let tick_hz = 1.0 / ledger.timestep_s;
    for (entity, mut flight) in &mut bodies {
        let trail = &trails.0.trails[flight.trail];
        let puffer = &trail.puffer;
        let (before, done) = flight.body.step(ledger.timestep_s);
        let after = flight.body.position;
        let moved = (after - before).length();
        let mut rng = Splitmix(flight.rng);
        let mut since = flight.since_puff_m + moved;
        while since >= puffer.distance_interval_m {
            since -= puffer.distance_interval_m;
            // The puff sits where the object was when it had moved that far.
            let back = if moved > 0.0 { since / moved } else { 0.0 };
            let origin = after.lerp(before, back);
            let velocity = Vec3::from(puffer.world_velocity)
                + Vec3::new(
                    rng.between([puffer.min_velocity[0], puffer.max_velocity[0]]),
                    rng.between([puffer.min_velocity[1], puffer.max_velocity[1]]),
                    rng.between([puffer.min_velocity[2], puffer.max_velocity[2]]),
                );
            let life_s = rng.between(puffer.lifetime_s).max(0.0);
            let puff = BurstPuff {
                born: ledger.ticks,
                life_ticks: (life_s * tick_hz).ceil().max(1.0) as u64,
                origin,
                velocity,
                size: rng.between(puffer.size),
                friction: puffer.friction,
            };
            let texture = (rng.next_unit() * 1000.0) as usize;
            match art.as_ref().zip(materials.as_mut()) {
                Some((art, materials)) => {
                    spawn_puff(&mut commands, art, materials, puff, texture);
                    report.trail_puffs_spawned += 1;
                }
                None => report.trail_puffs_undrawn += 1,
            }
        }
        flight.since_puff_m = since;
        flight.rng = rng.0;
        if done {
            commands.entity(entity).despawn();
        }
    }
}

/// Installs the flak burst presentation on the fixed loop, between the
/// ordnance step and the burst puffs it feeds.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlakBurstFxPlugin;

impl Plugin for FlakBurstFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingFlakBursts>()
            .init_resource::<PendingOrdnanceBursts>()
            .init_resource::<FlakBurstFxReport>()
            .add_systems(
                FixedPostUpdate,
                (present_flak_bursts, advance_flak_fx, advance_flak_trails)
                    .chain()
                    .after(build_art)
                    .after(drive_mission_ordnance)
                    .before(spawn_burst_puffs),
            );
    }
}
