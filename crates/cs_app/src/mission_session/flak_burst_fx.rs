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
//! **Not drawn: `call_flak_trails`.** Its five `flak_trail1..5` objects are
//! thrown by `OBJECT_MOTION` (`TRANSLATION_RANGE_MIN/MAX`, `GRAVITY LOCAL`,
//! `FORWARD_ROTATION`) whose units and frame are unmeasured; guessing them
//! would fabricate a motion. [`FlakBurstFxReport::trails_undrawn`] counts every
//! burst whose trails were not drawn.

use std::fmt;
use std::path::Path;

use bevy::prelude::{
    App, BackgroundColor, Color, Commands, Component, Entity, FixedPostUpdate, GlobalTransform,
    GlobalZIndex, IntoScheduleConfigs, Node, Plugin, PointLight, Query, Res, ResMut, Resource,
    Transform, Val, Vec3, Visibility, With, default,
};
use cs_formats::zbd::{
    EffectAnimation, EffectEvent, ReaderNode, read_animation_definitions, read_reader_nodes,
};
use cs_types::Tick;

use super::burst_puffs::{PendingOrdnanceBursts, PuffBurst, spawn_burst_puffs};
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
    /// Bursts whose `call_flak_trails` was not drawn (unmeasured motion).
    pub trails_undrawn: u64,
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
        report.trails_undrawn += 1;
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
                (present_flak_bursts, advance_flak_fx)
                    .chain()
                    .after(drive_mission_ordnance)
                    .before(spawn_burst_puffs),
            );
    }
}
