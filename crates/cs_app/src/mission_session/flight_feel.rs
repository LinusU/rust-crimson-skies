//! The mission's force feedback: the flight's state, gathered for
//! [`FlightFeel`](crate::force_feedback::FlightFeel) and played on the host
//! joystick (SIDEWINDER-FFB2, #1968).
//!
//! * The flight law's own step reports the player's airspeed and stall margin
//!   ([`PlayerFlightFeel`], written by the playtest's flight driver).
//! * The weapon step's report says when the player's guns fired and when
//!   another actor hit the player — the same filters as the debrief's tally and
//!   the hit sound.
//! * Every [`TerrainContactJudged`] of the player's aircraft is an impact.
//!
//! [`drive_flight_feel`] decides each frame whether flight is live — focused,
//! not paused, in the flight context, out of the launch cinematic, not crashed,
//! with a player aircraft — and hands the gate and the cues to
//! [`HostJoysticks::update_feel`], which stops every effect the moment any of
//! them stops holding.

use bevy::prelude::{
    App, FixedPostUpdate, IntoScheduleConfigs, MessageReader, Plugin, PostUpdate, Query, Res,
    ResMut, Resource, With, World,
};
use bevy::time::{Real, Time};
use cs_types::input::InputContext;

use crate::force_feedback::{FeelGate, FlightCues, PlayerFlightFeel};
use crate::input::hid::HostJoysticks;
use crate::input::platform::PlatformInput;
use crate::playtest::fatal_contact::{CrashSequence, TerrainContactJudged};
use crate::playtest::scene::PlaytestAircraft;
use crate::weapons::WeaponScheduleReport;

use super::player_victim::PlayerVictim;

/// How long the guns feel keeps rattling after the last round, seconds, so the
/// gaps between rounds do not start and stop it.
const GUNS_HOLD_S: f32 = 0.15;

/// The cues gathered since the last frame.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct FlightFeelCues {
    /// Rounds the player's guns fired.
    pub shots: u64,
    /// Hits the player took from another actor.
    pub hits: u64,
    /// The hardest impact severity, `0..=1`.
    pub impact: f32,
    /// Seconds since the guns last fired.
    since_shot_s: f32,
}

impl Default for FlightFeelCues {
    fn default() -> Self {
        Self {
            shots: 0,
            hits: 0,
            impact: 0.0,
            // The guns have not fired yet.
            since_shot_s: f32::INFINITY,
        }
    }
}

/// Wires the mission's flight state to the host joystick's force feedback.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlightFeelPlugin;

impl Plugin for FlightFeelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerFlightFeel>()
            .init_resource::<FlightFeelCues>()
            .add_systems(
                FixedPostUpdate,
                collect_weapon_cues
                    .after(crate::weapons::drive_weapon_session)
                    .before(crate::damage::drive_damage_schedule),
            )
            .add_systems(PostUpdate, (collect_impacts, drive_flight_feel).chain());
    }
}

/// Counts the player's rounds and the hits the player took this fixed tick.
fn collect_weapon_cues(
    victim: Option<Res<PlayerVictim>>,
    report: Option<Res<WeaponScheduleReport>>,
    mut cues: ResMut<FlightFeelCues>,
) {
    let (Some(victim), Some(report)) = (victim, report) else {
        return;
    };
    let Some(stepped) = &report.stepped else {
        return;
    };
    let player = victim.actor;
    cues.shots += stepped
        .accepted
        .iter()
        .filter(|fired| fired.shooter == player)
        .count() as u64;
    cues.hits += stepped
        .routed
        .iter()
        .flat_map(|routed| routed.outcome.sweep.damage())
        .filter(|hit| hit.target == player && hit.attacker != Some(player))
        .count() as u64;
}

/// Records the hardest terrain or obstacle impact of the player's aircraft.
fn collect_impacts(
    mut judged: MessageReader<TerrainContactJudged>,
    players: Query<(), With<PlaytestAircraft>>,
    mut cues: ResMut<FlightFeelCues>,
) {
    for contact in judged.read() {
        if players.get(contact.aircraft).is_ok() && contact.severity.is_finite() {
            let severity = if contact.fatal {
                1.0
            } else {
                contact.severity.clamp(0.0, 1.0) as f32
            };
            cues.impact = cues.impact.max(severity);
        }
    }
}

/// Whether flight is live for force feedback this frame.
#[must_use]
pub fn feel_gate(world: &World) -> FeelGate {
    let Some(platform) = world.get_resource::<PlatformInput>() else {
        return FeelGate::NoControl;
    };
    let session = platform.session();
    if !session.is_focused() {
        return FeelGate::Unfocused;
    }
    if session.is_paused() {
        return FeelGate::Paused;
    }
    if session.context() != InputContext::Flight
        || super::launch_cinematic::launch_cinematic_playing(world)
        || world.contains_resource::<CrashSequence>()
        || world.contains_resource::<super::debrief_screen::DebriefScreen>()
    {
        return FeelGate::NoControl;
    }
    FeelGate::Flying
}

/// Plays the frame's force feedback.
fn drive_flight_feel(world: &mut World) {
    if !world.contains_resource::<HostJoysticks>() {
        return;
    }
    let has_player = world
        .query_filtered::<(), With<PlaytestAircraft>>()
        .iter(world)
        .next()
        .is_some();
    let gate = match feel_gate(world) {
        FeelGate::Flying if !has_player => FeelGate::NoControl,
        gate => gate,
    };
    let dt_s = world
        .get_resource::<Time<Real>>()
        .map_or(0.0, |time| time.delta_secs());
    let flight = world
        .get_resource::<PlayerFlightFeel>()
        .copied()
        .unwrap_or_default();
    let cues = {
        let mut gathered = world.resource_mut::<FlightFeelCues>();
        if gathered.shots > 0 {
            gathered.since_shot_s = 0.0;
        } else {
            gathered.since_shot_s += dt_s;
        }
        let cues = FlightCues {
            airspeed_mps: flight.airspeed_mps,
            stall_margin: flight.stall_margin,
            guns_firing: gathered.shots > 0 || gathered.since_shot_s < GUNS_HOLD_S,
            hit: if gathered.hits > 0 { 1.0 } else { 0.0 },
            impact: gathered.impact,
        };
        gathered.shots = 0;
        gathered.hits = 0;
        gathered.impact = 0.0;
        cues
    };
    world
        .resource_mut::<HostJoysticks>()
        .update_feel(gate, dt_s, &cues);
}
