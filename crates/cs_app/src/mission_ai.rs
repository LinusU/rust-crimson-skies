//! Mission aircraft AI (M01-PLAY-AI, #1436): the **designed** pursuit-and-fire
//! law that flies the scripted aircraft [`crate::mission_actors`] spawns.
//!
//! Feature sheets: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`
//! and `specs/F32-ai-combat-formations-aces-and-difficulty.md` — both keep
//! their original-data stages (F31-D, F32-D) open, and neither has an
//! original AI control law measured. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md` (one force producer, one pose owner,
//! no second integrator).
//!
//! # What is measured here and what is not
//!
//! * **The law it flies is measured.** Every aircraft this module spawns
//!   carries [`PlaytestOriginalFlight`] with `is_player: false`, so the
//!   recovered fixed-wing law of task #796 (`OWNER-STATIC-2026-10-08`, not
//!   `verified_original`) runs its own AI branch: the lift target always
//!   along the nose, the 4.47 m/s forward-speed floor and no fuel burn.
//!   The per-airframe parameters are the `vehicle.zrd` records the measured
//!   name-prefix dispatch resolves (`bsfury`, `devastator`, `rusdevastator`,
//!   `wingman`), imported through [`cs_content::original_airframe`] exactly
//!   vehicle.zrd` exactly
//!   as the player's are — including the AI records' explicit
//!   [`Resolved::Unknown`](cs_types::content::Resolved::Unknown) fuel, which
//!   is `0.0` here because the law burns
//!   fuel only for a player seat (`cs_sim::flight::original::OriginalState::fuel`
//!   says so itself).
//! * **The control law on top of it is designed.** The original's AI control
//!   law sits at `0x48c220` and was never recovered (`docs/findings/
//!   2026-10-08-flight-original-fixed-wing-law.md`, open unknown). What the
//!   task calls a pursuit-and-fire law is therefore project design: aim the
//!   nose at the target, bank into the turn to build turn rate, add rudder
//!   for the first degrees of alignment ([`pursuit_command`]), and open fire
//!   inside a designed window and cone ([`should_fire`]). Every gain, angle
//!   bound, range and round count below is labelled **designed** with its
//!   own comment and is not a claim about the original. The guns themselves
//!   are the mission's designed loadout (#1437): this module asks, the gun
//!   bank's own cadence decides whether a round leaves the barrel.
//! * **The role split reads measured fields with a designed meaning.**
//!   [`behaviour_for`] uses only `aiv.zrd`'s own fields — `team`,
//!   `primary_target` and `starts_asleep` (#1453 measured all three) — but
//!   what a team number means engine-wide, and what the original did with a
//!   stored primary target, are unmeasured
//!   (`docs/findings/2026-10-15-m01-aiv-layout.md`, unknowns 4 and 5). The
//!   reading used here — the record's team against the `player` record's own
//!   measured team, and the primary target as the aircraft to engage — is
//!   **designed** and says nothing about the original's choice.
//! * **What stays unknown:** the ±5 % AI parameter roll the original applies
//!   (`0x4771c6`, task #805) is *not* applied, so every AI airframe flies
//!   the exact retail numbers; the original's rear-facing gunner is not
//!   modelled (no measured mount); and M01's zeppelins declare
//!   `DeclaredMotion::Held` with no route polyline, so "the cargo zeppelin
//!   flies its route" has no measured route to fly.
//!
//! # The wake (M01-PLAY-AI-WAKE, #1487)
//!
//! M01's `OBJECTIVE13` spells the measured `WAKEUP_ENEMIES` directive
//! `[text bsfury_1, text blackswanzep]` through the production readers
//! (`crates/cs_app/tests/accept_m01_lc_directive_c.rs` pins the shipped site;
//! `cs_content::mission_control::MeasuredDirective` pins the measured
//! semantics: wake each named vehicle whose asleep byte is set, or each named
//! zeppelin whose dormant byte is set, and do nothing for a name already
//! awake). The composed host step consumes every such emission the control
//! program has published and not already applied ([`WakeEnemiesLedger`]), and
//! a name that resolves to a **sleeping** scripted aircraft is **released**
//! ([`release_woken_aircraft`]): the held kinematic body becomes the dynamic
//! body an awake record spawns as — the record's own measured airframe on the
//! law's AI seat, the behaviour the record's measured fields select once the
//! sleep is consumed, and the mass the law's force request integrates
//! against. Its weapons follow on the next fixed tick through the same
//! [`arm_mission_ai`] every other flown record goes through. A name that
//! resolves to nothing this runtime drives is refused **by name**, never
//! silently dropped: `blackswanzep` is a zeppelin, and the world-actor wake
//! (dormant byte, route) is not modelled here (`WorldActorSession` declares
//! no wake surface).
//!
//! # Schedule
//!
//! [`MissionAiPlugin`] registers two chained systems in `FixedPreUpdate`,
//! before the fixed-tick flight driver of [`crate::playtest`] steps any
//! record:
//!
//! * [`arm_mission_ai`] gives every flown record the mission's designed gun
//!   rig once the weapon session exists — the guns registered for the
//!   record's own session actor, the same hardpoint mounts the player's rig
//!   hangs, and the faction the shot's relation reads
//!   ([`crate::mission_session::designed_target_rules`]). It skips entities
//!   already bound to this generation, so a restart's re-opened session
//!   re-arms the actors the restart respawned;
//! * [`decide_mission_aircraft_ai`] writes the held [`FlightInput`] on the
//!   aircraft's own [`PlaytestOriginalFlight`] and, for a pursuer inside
//!   the designed window and cone ([`should_fire`]), queues a fire intent.
//!
//! The existing flight driver then seeds the law from the authoritative
//! Avian pose, steps it and hands the acceleration back as one force
//! request — the same consumer mapping the player's seat uses, so there is
//! one integrator and one command boundary. The queued fire intents leave
//! through [`append_ai_fire_orders`], which the mission's own weapon
//! publisher calls while it builds the tick's record: a tick's weapon
//! inputs are consumed exactly once, so an attacker's shots ride the
//! mission's publication instead of pushing a second one.
//!
//! The wake consumer rides the mission host's step instead of this plugin:
//! the control program's directive log is the host's own record, and the
//! ledger that deduplicates it is host state ([`WakeEnemiesLedger`] on
//! [`crate::mission_session::MissionHost`]). A release made there is visible
//! to this plugin on the **next** fixed tick.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use avian3d::prelude::{LinearVelocity, Mass, NoAutoMass, Position, Rotation};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, Component, Entity, FixedPreUpdate, Plugin, Quat, Query, ResMut, Resource, Vec3, With,
    World,
};

use cs_content::original_airframe::{import_retail_airframe, import_retail_globals};
use cs_script::ir::{DirectiveOperation, Value};
use cs_script::runtime::{ExecutionKey, MissionState};
use cs_sim::damage::ActorId;
use cs_sim::flight::original::{FORCE_TO_ACCEL, PROVENANCE_LABEL};
use cs_sim::flight::{FlightInput, OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_sim::targeting::{TargetClass, TargetRecord};
use cs_sim::weapons::{FireIntent, FireIntentId};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::space::{Quaternion, WorldPosition};

use crate::mission_actors::ScriptedAircraftActor;
use crate::mission_session::{
    MISSION_FACTION_HOSTILE, MISSION_FACTION_PLAYER, MissionPlayerBody, MissionStage,
    MissionWeapons, attach_gun_rig, designed_loadout, ensure_mission_weapons,
};
use crate::mission_start::{ScriptedAircraft, SpawnDispatch};
use crate::physics::{BodyMode, set_body_mode};
use crate::playtest::scene::PlaytestOriginalFlight;
use crate::targeting::TargetingSession;
use crate::weapons::{WeaponActorBinding, WeaponOrder, WeaponSession, WeaponTickInput};

/// The throttle the session starts every aircraft at, the player's own cruise
/// ([`crate::playtest::command::CRUISE_THROTTLE`]).
const START_THROTTLE: f64 = crate::playtest::command::CRUISE_THROTTLE as f64;

/// **Designed**: the largest bank the pursuit law asks for, 60°. It bounds
/// how hard an attacker can turn in one commitment and keeps a pursuer from
/// rolling inverted while closing.
pub const PURSUIT_MAX_BANK_RAD: f32 = std::f32::consts::FRAC_PI_3;

/// **Designed**: roll input per radian of bank error. A 60° bank error
/// saturates the axis, so the law commits to the turn immediately and eases
/// out as the bank approaches the commanded one.
pub const PURSUIT_BANK_GAIN: f32 = 1.6;

/// **Designed**: the largest pitch attitude the pursuit law asks for, ~35°.
/// It bounds a dive or a climb so a target below the nose cannot pull the
/// aircraft through a loop to reach it.
pub const PURSUIT_MAX_PITCH_RAD: f32 = 0.610_865_2;

/// **Designed**: pitch input per radian of elevation error.
pub const PURSUIT_PITCH_GAIN: f32 = 1.8;

/// **Designed**: rudder input per radian of azimuth error. It lines the nose
/// up in the first degrees; the bank does the fast turning.
pub const PURSUIT_YAW_GAIN: f32 = 0.5;

/// The fixed name a record's stored primary target uses for the player body.
pub const PLAYER_TARGET_NAME: &str = "player";

/// **Designed**: how far ahead a hold-course aircraft looks along its heading
/// for the direction it keeps flying toward, in metres.
pub const HOLD_COURSE_LOOKAHEAD_M: f32 = 1_200.0;

/// **Measured (static)**: the follower's station in the leader's body frame,
/// for any non-player leader (`0x61fb98`, engine-facts rows
/// `station_offset_other_leader_{x,y,z}` = `(8, -2, -8)`; the routine
/// `0x41e760`, `docs/findings/2026-10-17-m01-wingman-formation-control-law.md`).
///
/// The original's third component is "8 m behind the leader along the
/// forward row". This crate's nose is `-Z`, so astern is `+Z` and the stored
/// `-8` is written `+8`. **Not measured**: which side basis row 0 is on and
/// that row 1 is up (`finding` §5); the signs `+x` right and `+y` up are the
/// reading this crate keeps.
pub const STATION_OFFSET_M: [f32; 3] = [8.0, -2.0, 8.0];

/// **Measured (static)**: the follower's station when its resolved leader is
/// the player — `(6, 0, 18)` at `0x61fb88` (engine-facts rows
/// `station_offset_player_leader_{x,y,z}`), selected at `0x41e792` only when
/// `leader == [0x71c298]`, the global player vehicle (`0x41e78a`;
/// `docs/findings/2026-10-18-m01-wingman-player-leader.md` §2).
///
/// The original's third component lies along the basis row the control
/// writer treats as "ahead" (`bz > 0`, finding §4): this crate's nose is
/// `-Z`, so ahead is written `-18`. **Not measured**: which side basis row 0
/// is on, that row 1 is up, or that row 2 is the leader's nose rather than
/// its tail (finding §2/§5); the signs `+x` right, `+y` up and `-z` ahead
/// are the readings this crate keeps, consistent with [`STATION_OFFSET_M`].
pub const STATION_OFFSET_PLAYER_M: [f32; 3] = [6.0, 0.0, -18.0];

/// **Measured (static)**: the player-led wingman's engage window — the
/// follower's unforced sub-state runs the transition table (finding §2), and
/// state 0 flips to the station law when the player is awake, the squared
/// gap to the player is under this (700 m; `0x6035e0`, row
/// `follow_engage_dist2`) and the follower's own speed exceeds
/// [`FOLLOW_ENGAGE_SPEED_M_S`].
pub const FOLLOW_ENGAGE_DIST2: f32 = 490_000.0;

/// **Measured (static)**: the follower speed the player-leader engage
/// condition asks for, 20.576 m/s (`0x603594`, row `follow_engage_speed`).
pub const FOLLOW_ENGAGE_SPEED_M_S: f32 = 20.576;

/// **Measured (static)**: lower clamp of the follower's throttle target
/// (`0x61fb28`, row `follow_throttle_min`).
pub const FOLLOW_THROTTLE_MIN: f64 = 0.4;

/// **Measured (static)**: upper clamp of the follower's throttle target and
/// the multiplier on the airframe speed that caps the demanded speed
/// (`0x61fb2c`, row `follow_throttle_max`).
pub const FOLLOW_THROTTLE_MAX: f64 = 1.5;

/// **Measured (static)**: demanded speed per metre the slot lies ahead along
/// the leader's velocity (`0x61fb38`, row `follow_speed_gain_along`).
pub const FOLLOW_GAIN_ALONG: f32 = 0.15;

/// **Measured (static)**: demanded speed per metre of cross-track distance to
/// the slot (`0x61fb3c`, row `follow_speed_gain_cross`).
pub const FOLLOW_GAIN_CROSS: f32 = 0.025;

/// **Measured (static)**: the demanded speed stays within the leader's speed
/// plus or minus this, 60 mph in m/s (`0x603510`, row `follow_speed_band`).
pub const FOLLOW_SPEED_BAND_M_S: f32 = 26.8224;

/// **Measured (static)**: the demanded speed never falls below this, 50 mph
/// in m/s (`0x60351c`, row `follow_speed_floor`).
pub const FOLLOW_SPEED_FLOOR_M_S: f32 = 22.352;

/// **Measured (static)**: the demanded speed when the leader's velocity is
/// zero, 180 mph in m/s (`0x41b6af`).
pub const FOLLOW_REST_SPEED_M_S: f32 = 80.4672;

/// **Measured (static)**: throttle-target change per unit of the original's
/// frame scalar (`0x603508`, row `follow_throttle_slew`). **Designed
/// reading:** the frame scalar (`0x9ad744`) is unmeasured; this crate reads
/// it as seconds, so the target moves 0.35 per second.
pub const FOLLOW_THROTTLE_SLEW_PER_S: f64 = 0.35;

/// **Measured (static)**: beyond this squared distance from the player the
/// throttle target is set directly instead of slewed — 2000 m
/// (`0x60350c`, row `follow_rubber_band_dist2`).
pub const FOLLOW_RUBBER_BAND_DIST2: f32 = 4_000_000.0;

/// **Designed**: the throttle a pursuer flies, full power. The original's AI
/// throttle policy is part of the unrecovered control law (`0x48c220`).
pub const PURSUIT_THROTTLE: f64 = 1.0;

/// **Designed**: the range an attacker must be inside to open fire, in
/// metres. The mission's designed round flies at 640 m/s for 90 ticks
/// (~480 m), so a shot taken inside this window can still reach its target
/// when the round expires; firing is judged, hitting is not — the aim is the
/// law's own geometry.
pub const FIRE_RANGE_M: f32 = 600.0;

/// **Designed**: the half-angle the nose must be within of the target's
/// direction to open fire (~12°).
pub const FIRE_CONE_RAD: f32 = 0.209_439_51;

/// **Designed**: fixed ticks between two fire intents of one aircraft. The
/// gun bank's own cadence (`MISSION_TICKS_BETWEEN_SHOTS`, #1437) still
/// decides whether a round actually leaves the barrel; this is only how
/// often the AI asks.
pub const FIRE_INTERVAL_TICKS: u32 = 8;

/// **Designed**: rounds each AI aircraft's bank starts with. The mission's
/// designed loadout gives the player 250 (#1437); an attacker that never
/// reloads starts with more, and neither number is a measurement.
pub const AI_STARTING_ROUNDS: u64 = 500;

/// The producer serial the AI's fire intents carry: 201–204 are the
/// mission's router, ordnance, damage and cockpit producers (#1437).
pub const MISSION_AI_FIRE_PRODUCER: u32 = 205;

/// Whether an attacker may open fire on a target `range_m` away when its
/// nose points `alignment` (a cosine) off the target's direction: inside the
/// designed window **and** inside the designed cone.
#[must_use]
pub fn should_fire(range_m: f32, alignment: f32) -> bool {
    range_m.is_finite()
        && alignment.is_finite()
        && range_m <= FIRE_RANGE_M
        && alignment >= FIRE_CONE_RAD.cos()
}

/// Resource: the fire decisions the fixed-tick law has written for the
/// committed tick, waiting for the mission's own weapon publication to carry
/// them.
///
/// A tick's [`WeaponTickInput`](crate::weapons::WeaponTickInput) is consumed
/// exactly once, so an attacker never pushes a second record for the same
/// tick: [`append_ai_fire_orders`] joins these shooters to the mission's own
/// publication instead.
#[derive(Resource, Clone, Debug, Default)]
pub struct MissionAiFireOrders {
    /// The session-qualified shooters that decided to fire this tick.
    ///
    /// Public like [`MissionAiReport`]'s own fields: the composed spawn and
    /// teardown write this resource (a generation's pending intents never
    /// ride into the next one), and the acceptance suite seeds and reads it
    /// to pin that.
    pub pending: Vec<cs_sim::damage::ActorId>,
    /// The intent sequence within [`MISSION_AI_FIRE_PRODUCER`].
    pub sequence: u32,
}

/// Why a scripted aircraft flies a law at all, or does not.
///
/// The variant is decided once, from the record's measured fields
/// ([`behaviour_for`]) and never re-derived: a law that changed its own role
/// mid-flight would hide the measured record it read. The one exception is the
/// measured sleep the mission's own `WAKEUP_ENEMIES` consumes: a woken record
/// takes the behaviour its remaining measured fields select
/// ([`woken_behaviour_for`]), because the sleep the spawn read is no longer
/// true. The transition is one-way and is made by the wake, never by the law.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionAiBehaviour {
    /// The record's measured `starts_asleep` field is set (#681): no law is
    /// attached and the body stays kinematic at its measured pose until
    /// `WAKEUP_ENEMIES` names it ([`release_woken_aircraft`]).
    Asleep,
    /// A record opposed to the player's flight: close on [`TargetRule`] and
    /// hold it.
    Pursue {
        /// What to engage: the record's own stored primary target when it
        /// names one, otherwise the player body.
        target: TargetRule,
    },
    /// A record of the player's flight whose stored `primary_target` names
    /// another aircraft of that flight — or the fixed name `player`, which
    /// the original's leader resolution binds to the player vehicle itself
    /// (`docs/findings/2026-10-18-m01-wingman-player-leader.md` §1): hold the
    /// measured station on it.
    Wingman {
        /// The record name the measured primary target spells.
        leader: String,
    },
    /// A record with no measured target: keep the spawn heading and
    /// altitude at the session's cruise.
    HoldCourse,
}

/// What a [`MissionAiBehaviour::Pursue`] record engages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetRule {
    /// The measured `primary_target` field's record name, when the record
    /// stores one.
    Record(String),
    /// The composition's `MissionPlayerBody`. The fallback for a hostile
    /// record that stores no primary target — **designed**: the original's
    /// meaning of a missing primary target is unmeasured, and a hostile in
    /// this mission engaging the player is the behaviour the task asks for.
    Player,
}

/// One AI aircraft's own record: the behaviour decided from its measured
/// fields, plus what the last decision resolved.
#[derive(Component, Clone, Debug)]
pub struct MissionAircraftAi {
    /// The behaviour this record's measured fields selected.
    pub behaviour: MissionAiBehaviour,
    /// The measured `team` field, verbatim.
    pub team: Option<u32>,
    /// The entity the last pursuit decision engaged, when it resolved.
    pub target: Option<Entity>,
    /// The entity the last station decision resolved as the leader.
    pub leader: Option<Entity>,
    /// The distance to the resolved target point at the last decision, in
    /// metres — `f32::NAN` while no target has resolved.
    pub range_m: f32,
    /// How many fixed ticks this aircraft has been given a command.
    pub decisions: u64,
    /// Fixed ticks left before this aircraft may ask to fire again.
    pub cooldown: u32,
    /// Fire intents this aircraft has asked for since the spawn.
    pub shots: u64,
    /// The follower's throttle target (the original's `vehicle+0x124`),
    /// slewed by [`follow_throttle_target`]; a wingman flies
    /// `min(1, target)`.
    pub throttle_target: f64,
    /// Whether a **player-led** wingman's engage transition has fired. The
    /// original runs the `+0xd8` sub-state table for a player leader (state
    /// 0 → 1 inside [`FOLLOW_ENGAGE_DIST2`] and faster than
    /// [`FOLLOW_ENGAGE_SPEED_M_S`]; finding §2), while a non-player leader's
    /// sub-state is forced every call — so the latch exists only on the
    /// player branch. Its initial state and the route-object goal states are
    /// unmeasured (finding §5); until the gate fires the designed stand-in
    /// holds the spawn heading.
    pub follow_engaged: bool,
}

impl MissionAircraftAi {
    /// The behaviour for a spawned record, from its measured fields and the
    /// measured team of the stage's `player` record.
    ///
    /// The reading is **designed**: `team` and `primary_target` are measured
    /// values (#1453) whose engine-wide meaning is not
    /// (`docs/findings/2026-10-15-m01-aiv-layout.md` unknowns 4 and 5). A
    /// record whose team was never read states no side, and a law may not
    /// invent one: it holds its course instead.
    #[must_use]
    pub fn for_record(record: &ScriptedAircraft, player_team: Option<u32>) -> Self {
        Self::with_behaviour(record, behaviour_for(record, player_team))
    }

    /// The record a **woken** aircraft gets: the same measured fields, with
    /// the measured `starts_asleep` value already consumed by the mission's
    /// own `WAKEUP_ENEMIES`, so the behaviour is the one those remaining
    /// fields select ([`woken_behaviour_for`]).
    ///
    /// The reading is designed exactly as [`Self::for_record`]'s is; the only
    /// difference is which measured field the transition read — the wake
    /// consumed the sleep, so the law does not re-sleep a record the mission
    /// just woke.
    #[must_use]
    pub fn woken_for_record(record: &ScriptedAircraft, player_team: Option<u32>) -> Self {
        Self::with_behaviour(record, woken_behaviour_for(record, player_team))
    }

    /// The common construction both entries share: the chosen behaviour and
    /// the measured `team` beside it.
    #[must_use]
    fn with_behaviour(record: &ScriptedAircraft, behaviour: MissionAiBehaviour) -> Self {
        Self {
            behaviour,
            team: record.team,
            target: None,
            leader: None,
            range_m: f32::NAN,
            decisions: 0,
            cooldown: 0,
            shots: 0,
            throttle_target: START_THROTTLE,
            follow_engaged: false,
        }
    }

    /// Whether this record is opposed to the player's flight — the faction
    /// the targeting session roster it under. It is the same designed
    /// reading of the measured `team` field the behaviour was decided from,
    /// never a second opinion: a record with no measured side holds its
    /// course and rosters with the player's flight.
    #[must_use]
    pub const fn opposed(&self) -> bool {
        matches!(self.behaviour, MissionAiBehaviour::Pursue { .. })
    }
}

/// The behaviour a record's measured fields select — see
/// [`MissionAircraftAi::for_record`] for what is measured and what is
/// designed.
///
/// The measured `starts_asleep` field wins when it is set (#681): no law is
/// attached until the mission's `WAKEUP_ENEMIES` consumes that sleep.
#[must_use]
pub fn behaviour_for(record: &ScriptedAircraft, player_team: Option<u32>) -> MissionAiBehaviour {
    if record.starts_asleep.unwrap_or(false) {
        return MissionAiBehaviour::Asleep;
    }
    woken_behaviour_for(record, player_team)
}

/// The behaviour a record's measured fields select once its measured
/// `starts_asleep` value has been consumed by the mission's own
/// `WAKEUP_ENEMIES` wake.
///
/// It is [`behaviour_for`]'s reading with the sleep term removed: the same
/// measured `team` against the stage's `player` record, the same reading of
/// the stored `primary_target`, the same refusal to invent a side for a record
/// whose team was never read. What a team number means engine-wide stays
/// unmeasured (`docs/findings/2026-10-15-m01-aiv-layout.md`, unknowns 4 and
/// 5) and so is this reading of it.
#[must_use]
pub fn woken_behaviour_for(
    record: &ScriptedAircraft,
    player_team: Option<u32>,
) -> MissionAiBehaviour {
    let Some(team) = record.team else {
        // No measured side: hold the course rather than invent an enemy.
        return MissionAiBehaviour::HoldCourse;
    };
    if Some(team) == player_team {
        return match &record.primary_target {
            Some(leader) => MissionAiBehaviour::Wingman {
                leader: leader.clone(),
            },
            None => MissionAiBehaviour::HoldCourse,
        };
    }
    let target = match &record.primary_target {
        // The player body is addressed by its fixed name, not a cast record.
        Some(name) if name == PLAYER_TARGET_NAME => TargetRule::Player,
        Some(name) => TargetRule::Record(name.clone()),
        None => TargetRule::Player,
    };
    MissionAiBehaviour::Pursue { target }
}

/// The bank attitude of an orientation, positive **right-wing-down**.
///
/// The body's `+X` axis moves only under a roll (yaw rotates it in the
/// horizontal plane and pitch leaves it fixed), so the sign and magnitude of
/// its vertical component are exactly the roll angle.
#[must_use]
pub fn bank_angle(orientation: Quat) -> f32 {
    let right = orientation * Vec3::X;
    (-right.y).clamp(-1.0, 1.0).asin()
}

/// The nose's elevation above the horizon, positive nose-up.
#[must_use]
pub fn pitch_angle(orientation: Quat) -> f32 {
    let nose = orientation * Vec3::NEG_Z;
    nose.y.clamp(-1.0, 1.0).asin()
}

/// **The designed pursuit law's steering half**: one command that puts the
/// nose on `to_target`.
///
/// The geometry is expressed in the aircraft's body frame (nose `-Z`, right
/// `+X`, up `+Y`): `azimuth` is positive when the target lies to the right,
/// `elevation` when it lies above. The command then *aims* with pitch and
/// rudder and *turns* by banking, because a fixed-wing's fast turn comes
/// from a banked lift vector, not from the rudder:
///
/// * roll drives the bank toward the azimuth, bounded by
///   [`PURSUIT_MAX_BANK_RAD`],
/// * pitch drives the nose toward the elevation, bounded by
///   [`PURSUIT_MAX_PITCH_RAD`],
/// * yaw takes the first part of the azimuth with the opposite sign
///   ([`FlightInput::yaw`] is positive nose-left),
/// * `throttle` passes through unchanged — the caller's policy (full power
///   for a pursuer, a station controller for a wingman).
///
/// The output goes through [`FlightInput::clamped`], so a deflection can
/// never leave the axis ranges; every term above is finite by construction
/// and the clamp is the axis boundary, not a guess.
#[must_use]
pub fn pursuit_command(orientation: Quat, to_target: Vec3, throttle: f64) -> FlightInput {
    let local = orientation.inverse() * to_target;
    let azimuth = local.x.atan2(-local.z);
    let elevation = local.y.atan2(local.x.hypot(local.z));
    let desired_bank = azimuth.clamp(-PURSUIT_MAX_BANK_RAD, PURSUIT_MAX_BANK_RAD);
    let desired_pitch = elevation.clamp(-PURSUIT_MAX_PITCH_RAD, PURSUIT_MAX_PITCH_RAD);
    let roll = (desired_bank - bank_angle(orientation)) * PURSUIT_BANK_GAIN;
    let pitch = (desired_pitch - pitch_angle(orientation)) * PURSUIT_PITCH_GAIN;
    let yaw = -azimuth * PURSUIT_YAW_GAIN;
    FlightInput::clamped(
        f64::from(pitch),
        f64::from(roll),
        f64::from(yaw),
        throttle,
        false,
    )
    .expect("every term is finite and the axes are clamped")
}

/// The world-space point a wingman holds: the leader's position plus the
/// measured [`STATION_OFFSET_M`] in the leader's own body frame (so `+Z` is
/// astern and the station sits behind the leader).
#[must_use]
pub fn station_point(leader_position: Vec3, leader_orientation: Quat, offset: [f32; 3]) -> Vec3 {
    leader_position + leader_orientation * Vec3::from(offset)
}

/// **The measured demanded speed** of the follower (`0x41b560`, steps 2-4 of
/// the finding), in m/s.
///
/// `to_slot` is the slot point minus the follower's position; the demand is
/// the leader's speed plus [`FOLLOW_GAIN_ALONG`] per metre of the gap along
/// the leader's velocity plus [`FOLLOW_GAIN_CROSS`] per metre across it,
/// banded to the leader's speed plus or minus [`FOLLOW_SPEED_BAND_M_S`], then
/// capped at [`FOLLOW_THROTTLE_MAX`] times `max_speed_m_s` and floored at
/// [`FOLLOW_SPEED_FLOOR_M_S`].
///
/// **Designed reading:** the original caps by the vehicle word `+0x668`
/// (unmeasured, finding §5); `max_speed_m_s` is this airframe's full-throttle
/// speed. The original's altitude clamp and the vehicle definition's two
/// speed words (`def+0x1e4/0x1e8`) are not applied.
#[must_use]
pub fn follow_demanded_speed(to_slot: Vec3, leader_velocity: Vec3, max_speed_m_s: f32) -> f32 {
    let leader_speed = leader_velocity.length();
    let wanted = if leader_speed > 0.0 {
        let along = to_slot.dot(leader_velocity / leader_speed);
        let cross = (to_slot.length_squared() - along * along).max(0.0).sqrt();
        leader_speed + FOLLOW_GAIN_ALONG * along + FOLLOW_GAIN_CROSS * cross
    } else {
        FOLLOW_REST_SPEED_M_S
    };
    let banded = wanted.clamp(
        leader_speed - FOLLOW_SPEED_BAND_M_S,
        leader_speed + FOLLOW_SPEED_BAND_M_S,
    );
    let capped = if max_speed_m_s > 0.0 {
        banded.min(max_speed_m_s * FOLLOW_THROTTLE_MAX as f32)
    } else {
        banded
    };
    capped.max(FOLLOW_SPEED_FLOOR_M_S)
}

/// **The measured throttle target** (`0x41bb5c`..`0x41bbfd`): the next value
/// of the follower's target throttle.
///
/// Farther than 2000 m from the player the target is the demanded speed over
/// `max_speed_m_s` directly; otherwise it moves [`FOLLOW_THROTTLE_SLEW_PER_S`]
/// times `dt_s` up when the aircraft is slower than demanded and down when it
/// is not. Either way it stays in `[FOLLOW_THROTTLE_MIN, FOLLOW_THROTTLE_MAX]`.
/// With no airframe speed to divide by the target is left where it is.
#[must_use]
pub fn follow_throttle_target(
    target: f64,
    demanded_speed_m_s: f32,
    own_speed_m_s: f32,
    max_speed_m_s: f32,
    player_distance_squared: Option<f32>,
    dt_s: f64,
) -> f64 {
    let next = if player_distance_squared.is_some_and(|d2| d2 > FOLLOW_RUBBER_BAND_DIST2) {
        if max_speed_m_s <= 0.0 {
            return target;
        }
        f64::from(demanded_speed_m_s / max_speed_m_s)
    } else if own_speed_m_s < demanded_speed_m_s {
        target + FOLLOW_THROTTLE_SLEW_PER_S * dt_s
    } else {
        target - FOLLOW_THROTTLE_SLEW_PER_S * dt_s
    };
    next.clamp(FOLLOW_THROTTLE_MIN, FOLLOW_THROTTLE_MAX)
}

// -------------------------------------------------- airframe preparation ---

/// One scripted aircraft's imported airframe: the recovered law with the
/// `vehicle.zrd` record the measured name-prefix dispatch resolved.
#[derive(Clone, Debug)]
pub struct MissionAiAirframe {
    /// The law with this record's imported parameters.
    pub model: OriginalFlightModel,
    /// The `vehicle.zrd` record the parameters came from (`rusdevastator`).
    pub record: String,
    /// The record's `kind_of` chain, ancestor first.
    pub inheritance_chain: Vec<String>,
    /// `OWNER-STATIC-2026-10-08`: static analysis, never `verified_original`.
    pub provenance: &'static str,
}

/// Every airframe the mission's scripted aircraft fly, keyed by the measured
/// name-prefix dispatch's prefix, plus the prefixes that would not import.
///
/// Built once, in `stage_for`, where the installation path exists; the
/// composition carries it on the stage so a spawn or a restart never reads
/// the installation again.
#[derive(Clone, Debug, Default)]
pub struct MissionAiAirframes {
    /// The imported records, by `vehicle.zrd` prefix.
    pub models: BTreeMap<String, MissionAiAirframe>,
    /// One entry per prefix that refused to import, carrying the reader's
    /// own message: the record it names then spawns **held**, never half
    /// flown, and the refusal stays visible on the stage.
    pub refusals: Vec<String>,
}

impl MissionAiAirframes {
    /// Imports an airframe for every distinct `SpawnDispatch::VehicleRecord`
    /// prefix in `records`, plus the `vehicle.zrd` record
    /// `SpawnDispatch::WingmanOne` resolves to — the player airframe row's
    /// `w` pointer (measured `0x426d60`, finding §1), handed in as
    /// `wingman_one` by the stage that holds it (`MissionFlight::wingman_record`).
    /// The `player` record flies the composition's own seat, an unmatched
    /// name has no record to import, and a `WingmanFour` keeps the refusal
    /// the spawn reports — its measured alternative (the player's own
    /// airframe on `c3/m05`/`c4/m04`, `0x47502c`..`0x4750a4`) is not built
    /// here.
    ///
    /// An AI chain states no `fuel` load — an explicit unknown, never a
    /// substituted number — and that is **not** a refusal here: the
    /// law burns fuel only for a player seat, so the spawned record carries
    /// `0.0`, exactly what `OriginalState::fuel` documents for AI aircraft.
    ///
    /// # Errors
    ///
    /// Never: a prefix that will not import is recorded in
    /// [`Self::refusals`] and its aircraft spawns held instead.
    #[must_use]
    pub fn prepare(
        install_root: &Path,
        records: &[ScriptedAircraft],
        wingman_one: Option<&str>,
    ) -> Self {
        let mut prepared = Self::default();
        for record in records {
            let resolved = match &record.dispatch {
                SpawnDispatch::VehicleRecord(prefix) => Some(prefix.clone()),
                SpawnDispatch::WingmanOne => match wingman_one {
                    Some(template) => Some(template.to_owned()),
                    // The resolved record travels on the stage's
                    // `MissionFlight`; a caller that supplies none gets the
                    // refusal named instead of a guessed record.
                    None => {
                        prepared.refusals.push(format!(
                            "{}: the measured `wingman_1` dispatch resolves through the player \
                             airframe row's `w` pointer (`0x426d60`), and this stage supplies no \
                             player airframe record to resolve it against",
                            record.name
                        ));
                        None
                    }
                },
                _ => None,
            };
            let Some(prefix) = resolved else { continue };
            if prepared.models.contains_key(&prefix) {
                continue;
            }
            match Self::import(install_root, &prefix) {
                Ok(model) => {
                    prepared.models.insert(prefix, model);
                }
                Err(reason) => prepared.refusals.push(format!(
                    "{prefix}: the airframe could not be imported: {reason}"
                )),
            }
        }
        prepared
    }

    fn import(install_root: &Path, prefix: &str) -> Result<MissionAiAirframe, String> {
        let parameters = import_retail_airframe(install_root, prefix)
            .map_err(|error| format!("the parameters could not be imported: {error}"))?;
        let law = import_retail_globals(install_root)
            .map_err(|error| format!("the flight globals could not be imported: {error}"))?;
        let airframe = OriginalAirframe::from_values(&parameters.field_values())
            .map_err(|error| format!("the parameters were refused: {error}"))?;
        let globals = OriginalGlobals::from_values(&law.law_values())
            .map_err(|error| format!("the flight globals were refused: {error}"))?;
        Ok(MissionAiAirframe {
            model: OriginalFlightModel::full(airframe, globals),
            record: prefix.to_owned(),
            inheritance_chain: parameters.inheritance_chain.clone(),
            provenance: PROVENANCE_LABEL,
        })
    }
}

/// The declared mass of a body flying `airframe`: the imported weight over
/// the law's own force scale (`W / 9.82`), the same mapping
/// `docs/contracts/FLIGHT-PHYSICS.md` states for the player's body, so the
/// one force request the driver submits accelerates it by exactly the
/// acceleration the law computed.
#[must_use]
pub fn spawn_mass_kg(airframe: &MissionAiAirframe) -> f32 {
    (airframe.model.airframe.veh_weight / FORCE_TO_ACCEL) as f32
}

/// The flight record a spawned scripted aircraft carries: the imported
/// airframe, the law's own state at the measured spawn pose (at rest — the
/// original's AI spawn speed is not recovered, and the law's AI branch holds
/// a forward floor of 4.47 m/s anyway), and the AI seat.
///
/// `fuel` is `0.0`: an AI chain states no fuel load as an explicit unknown,
/// and the law reads it only for a player seat.
#[must_use]
pub fn ai_flight_record(
    airframe: &MissionAiAirframe,
    position_m: Vec3,
    orientation: Quat,
) -> PlaytestOriginalFlight {
    let [x, y, z, w] = orientation.normalize().to_array().map(f64::from);
    let orientation =
        Quaternion::try_new([x, y, z, w]).expect("an Avian rotation is a unit quaternion");
    PlaytestOriginalFlight {
        model: airframe.model.clone(),
        state: cs_sim::flight::OriginalState {
            position_m: position_m.to_array().map(f64::from),
            velocity_mps: [0.0, 0.0, 0.0],
            orientation,
            angular_momentum_world: [0.0; 3],
            throttle: START_THROTTLE,
            fuel: 0.0,
            level_off: false,
        },
        command: FlightInput {
            throttle: START_THROTTLE,
            ..FlightInput::NEUTRAL
        },
        hold_attitude: false,
        is_player: false,
        provenance: airframe.provenance,
    }
}

// ----------------------------------------------------------------- the wake ---

/// Why a `WAKEUP_ENEMIES` site's arguments named no release: they are not the
/// top-level text list the measured M01 site spells.
pub const WAKE_ENEMIES_ARGUMENTS_REASON: &str = "the WAKEUP_ENEMIES site's arguments are not the top-level text list the measured \
     installation spells, so no name was resolved and nothing was woken";

/// Why a `WAKEUP_ENEMIES` name woke nothing: no scripted aircraft of this
/// mission carries that measured record name. The measured effect resolves
/// such a name as a vehicle, else as a zeppelin; a zeppelin wake is a
/// world-actor concern this runtime does not model, so a name that is not a
/// scripted aircraft is named here instead of being silently treated as one.
pub const WAKE_ENEMIES_UNKNOWN_NAME_REASON: &str = "no scripted aircraft of this mission carries this WAKEUP_ENEMIES name; the \
     measured effect resolves a vehicle first and a zeppelin second, and the zeppelin wake is \
     not modelled here (WorldActorSession declares no wake surface)";

/// Why a `WAKEUP_ENEMIES` name woke nothing: the sleeping record's measured
/// name-prefix dispatch has no imported `vehicle.zrd` airframe on the stage,
/// so there is no law to release it under. It stays held rather than flying a
/// body with no law.
pub const WAKE_ENEMIES_NO_AIRFRAME_REASON: &str = "the record is asleep, but the stage carries no imported airframe for its \
     measured name prefix, so there is no law to release it under; it stays held";

/// Why a `WAKEUP_ENEMIES` name woke nothing: the entity carries no pose (or
/// no rigid body) to release from, so the release refuses rather than
/// teleporting or half-releasing it.
pub const WAKE_ENEMIES_NOT_A_BODY_REASON: &str = "the named scripted aircraft carries no live body pose to release, so the \
     release refused rather than half-building a flying record";

/// One scripted aircraft a `WAKEUP_ENEMIES` wake released into the designed
/// law.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WokenAircraft {
    name: String,
    at: Tick,
}

impl WokenAircraft {
    /// The record name the directive spelled, verbatim.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The committed tick the host consumed the directive on.
    #[must_use]
    pub const fn at(&self) -> Tick {
        self.at
    }
}

/// Why a `WAKEUP_ENEMIES` name released nothing, named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WakeRefusal {
    name: String,
    reason: &'static str,
    at: Tick,
}

impl WakeRefusal {
    /// The name the site spelled, verbatim; empty when the site's own
    /// arguments were not the measured shape.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Why nothing was woken, verbatim.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }

    /// The committed tick the directive was consumed on.
    #[must_use]
    pub const fn at(&self) -> Tick {
        self.at
    }
}

/// The mission host's `WAKEUP_ENEMIES` consumer.
///
/// It applies every `WakeEnemies` emission the control program's directive log
/// carries **exactly once**, deduplicating by execution key the way
/// [`crate::mission_animations::WakeAnimationLedger`] deduplicates the
/// animation wakes (the log holds the whole session, so only the key says
/// which entry is new), and releases each name that resolves to a sleeping
/// scripted aircraft ([`release_woken_aircraft`]) at the tick being stepped.
///
/// The measured effect wakes a vehicle whose asleep byte is set and a
/// zeppelin whose dormant byte is set, and does nothing for a name already
/// awake. This ledger performs the vehicle half and the "already awake"
/// half — the measured do-nothing — and names a name that is neither, rather
/// than guessing which kind it meant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WakeEnemiesLedger {
    applied: BTreeSet<ExecutionKey>,
    released: Vec<WokenAircraft>,
    refused: Vec<WakeRefusal>,
}

impl WakeEnemiesLedger {
    /// Consumes every `WakeEnemies` emission of `state` this ledger has not
    /// already applied, and answers the releases this call produced in
    /// directive-key order.
    ///
    /// Nothing is released twice and nothing is silently skipped: a site whose
    /// arguments are not the measured shape, a name no scripted aircraft
    /// carries, a sleeping record whose airframe did not import and a record
    /// with no live body are all kept as [`WakeRefusal`]s. The one measured
    /// no-op — a name for something already awake — is neither a release nor a
    /// refusal.
    pub fn consume(
        &mut self,
        world: &mut World,
        state: &MissionState,
        tick: Tick,
    ) -> Vec<WokenAircraft> {
        let mut released = Vec::new();
        for emission in state.directives() {
            if emission.operation != DirectiveOperation::WakeEnemies {
                continue;
            }
            let key = emission.key.execution_key();
            if !self.applied.insert(key) {
                continue;
            }
            let Some(names) = wake_enemy_names(&emission.args) else {
                self.refused.push(WakeRefusal {
                    name: String::new(),
                    reason: WAKE_ENEMIES_ARGUMENTS_REASON,
                    at: tick,
                });
                continue;
            };
            for name in names {
                match release_woken_aircraft(world, &name) {
                    WakeRelease::Released => {
                        released.push(WokenAircraft { name, at: tick });
                    }
                    WakeRelease::AlreadyAwake => {}
                    WakeRelease::Refused { reason } => self.refused.push(WakeRefusal {
                        name,
                        reason,
                        at: tick,
                    }),
                }
            }
        }
        self.released.extend(released.iter().cloned());
        released
    }

    /// How many `WakeEnemies` emissions this ledger has applied.
    #[must_use]
    pub fn applied(&self) -> usize {
        self.applied.len()
    }

    /// Every release, in the order the host consumed the directives.
    #[must_use]
    pub fn released(&self) -> &[WokenAircraft] {
        &self.released
    }

    /// Every name that woke nothing and why, in the order it was consumed.
    #[must_use]
    pub fn refused(&self) -> &[WakeRefusal] {
        &self.refused
    }
}

/// The names one `WAKEUP_ENEMIES` site spells: every top-level text argument,
/// in the site's own order. `None` when the arguments are not that measured
/// shape — the operation's own measured parse is a list of names, and no other
/// shape is interpreted rather than read as a name.
pub(crate) fn wake_enemy_names(args: &[Value]) -> Option<Vec<String>> {
    let mut names = Vec::with_capacity(args.len());
    for value in args {
        match value {
            Value::Str(name) if !name.is_empty() => names.push(name.clone()),
            _ => return None,
        }
    }
    Some(names)
}

/// What releasing one `WAKEUP_ENEMIES` name did.
enum WakeRelease {
    /// A sleeping scripted aircraft was released into the designed law.
    Released,
    /// The name resolves to a scripted aircraft that is not asleep: the
    /// measured wake does nothing for it.
    AlreadyAwake,
    /// Nothing was released, for the named reason.
    Refused { reason: &'static str },
}

/// Releases the sleeping scripted aircraft `name` into the designed law: the
/// held kinematic body becomes the dynamic AI body an awake record spawns as.
///
/// The release is exactly the awake half of the spawn
/// ([`crate::mission_actors::spawn_mission_actors`]), run here on the live
/// body instead of a fresh one:
///
/// * the body's declared mass becomes the record's imported weight over the
///   law's own force scale ([`spawn_mass_kg`]), bound with [`NoAutoMass`] so
///   the collider can never replace it — the kinematic spawn declared none
///   precisely so the release would declare the measured one;
/// * [`set_body_mode`] swaps kinematic → dynamic **without writing pose or
///   velocity**, so the held body is not teleported and starts from rest;
/// * the record's own airframe lands on the law's AI seat as
///   [`ai_flight_record`] and the behaviour its measured fields select now the
///   sleep is consumed as [`MissionAircraftAi::woken_for_record`];
/// * the record's own [`ScriptedAircraftActor::asleep`] mark is cleared and
///   the [`MissionAiReport`] moves the name from `held` to `flown` (and lists
///   it in `woken`), so the report describes the record that is flying.
///
/// The weapons are not hung here: [`arm_mission_ai`] arms every record
/// carrying a [`MissionAircraftAi`] on the next fixed tick, exactly as it arms
/// a spawned one.
fn release_woken_aircraft(world: &mut World, name: &str) -> WakeRelease {
    let Some(entity) = ({
        let mut query = world.query::<(Entity, &ScriptedAircraftActor)>();
        query
            .iter(world)
            .find(|(_, actor)| actor.record.name == name)
            .map(|(entity, _)| entity)
    }) else {
        return WakeRelease::Refused {
            reason: WAKE_ENEMIES_UNKNOWN_NAME_REASON,
        };
    };
    let Some((record, vehicle_record, asleep)) =
        world.get::<ScriptedAircraftActor>(entity).map(|actor| {
            (
                actor.record.clone(),
                actor.vehicle_record.clone(),
                actor.asleep,
            )
        })
    else {
        return WakeRelease::Refused {
            reason: WAKE_ENEMIES_UNKNOWN_NAME_REASON,
        };
    };
    if !asleep {
        // The measured effect does nothing for a name already awake.
        return WakeRelease::AlreadyAwake;
    }
    // The airframe and the player's own measured team, read off the stage
    // before the entity is touched (the stage borrow cannot outlive the
    // mutation below).
    let (airframe, player_team) = {
        let Some(stage) = world.get_resource::<MissionStage>() else {
            return WakeRelease::Refused {
                reason: WAKE_ENEMIES_NO_AIRFRAME_REASON,
            };
        };
        let Some(airframe) = stage.ai_airframes.models.get(&vehicle_record).cloned() else {
            return WakeRelease::Refused {
                reason: WAKE_ENEMIES_NO_AIRFRAME_REASON,
            };
        };
        let player_team = stage
            .aircraft
            .iter()
            .find(|record| record.dispatch == SpawnDispatch::Player)
            .and_then(|record| record.team);
        (airframe, player_team)
    };
    let Some((position, orientation)) = world
        .get::<Position>(entity)
        .zip(world.get::<Rotation>(entity))
        .map(|(position, rotation)| (position.0, rotation.0))
    else {
        return WakeRelease::Refused {
            reason: WAKE_ENEMIES_NOT_A_BODY_REASON,
        };
    };

    // The measured mass the release declares, and no collider-derived one:
    // `set_body_mode` refuses a dynamic release without a positive `Mass`, and
    // `NoAutoMass` keeps the collider from overwriting the number the law's
    // force request is scaled by.
    world
        .entity_mut(entity)
        .insert((NoAutoMass, Mass(spawn_mass_kg(&airframe))));
    if set_body_mode(world, entity, BodyMode::Dynamic).is_err() {
        return WakeRelease::Refused {
            reason: WAKE_ENEMIES_NOT_A_BODY_REASON,
        };
    }
    world.entity_mut(entity).insert((
        ai_flight_record(&airframe, position, orientation),
        MissionAircraftAi::woken_for_record(&record, player_team),
    ));
    if let Some(mut actor) = world.get_mut::<ScriptedAircraftActor>(entity) {
        actor.asleep = false;
    }
    if let Some(mut report) = world.get_resource_mut::<MissionAiReport>() {
        report
            .held
            .retain(|entry| !entry.starts_with(&format!("{name}:")));
        if !report.flown.iter().any(|flown| flown == name) {
            report.flown.push(name.to_owned());
        }
        if !report.woken.iter().any(|woken| woken == name) {
            report.woken.push(name.to_owned());
        }
    }
    WakeRelease::Released
}

// ------------------------------------------------------------- the systems ---

/// What the AI has done this run: which records fly, which are held and why,
/// and how many decisions the fixed-tick law has written. Replaced on each
/// spawn (a restart reports its own generation), incremented by the decision
/// system.
#[derive(Resource, Clone, Debug, Default)]
pub struct MissionAiReport {
    /// The record names flying the designed law, in roster order.
    pub flown: Vec<String>,
    /// The record names **not** flying it, each with the reason it is held.
    pub held: Vec<String>,
    /// The record names a `WAKEUP_ENEMIES` wake has released since this
    /// spawn: a record that spawned held and now flies. It is a subset of
    /// [`Self::flown`] and empty for a spawn that woke nothing.
    pub woken: Vec<String>,
    /// Every refusal on this path, verbatim.
    pub refusals: Vec<String>,
    /// The record names the mission armed with the designed gun rig.
    pub armed: Vec<String>,
    /// Commands written since this spawn.
    pub decisions: u64,
    /// Pursuit decisions that found no live target and fell back to holding
    /// the course.
    pub no_target: u64,
    /// Fire intents the law has asked the weapon session for since this
    /// spawn — how many, not whether a round left the barrel; the gun bank's
    /// own cadence decides that, and the acceptance suite reads it there.
    pub fire_orders: u64,
}

/// The mission's AI: one decision system on the fixed tick, before the
/// flight driver steps the records it commands.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionAiPlugin;

impl Plugin for MissionAiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MissionAiReport>()
            .init_resource::<MissionAiFireOrders>();
        app.add_systems(
            FixedPreUpdate,
            (arm_mission_ai, decide_mission_aircraft_ai).chain(),
        );
    }
}

/// One fixed tick of the designed law: resolve each aircraft's target,
/// station or course point and write the command the flight driver will step
/// in the same tick.
///
/// Target resolution is by the measured record name (the mission's own
/// identity, so an ECS reorder cannot rename a target) and, for the player
/// fallback, by [`MissionPlayerBody`].
fn decide_mission_aircraft_ai(
    mut report: ResMut<MissionAiReport>,
    mut orders: ResMut<MissionAiFireOrders>,
    mut aircraft: Query<(
        Entity,
        &mut MissionAircraftAi,
        &mut PlaytestOriginalFlight,
        &Position,
        &Rotation,
        &LinearVelocity,
        &ScriptedAircraftActor,
    )>,
    actors: Query<(
        Entity,
        &ScriptedAircraftActor,
        &Position,
        &Rotation,
        &LinearVelocity,
    )>,
    players: Query<(Entity, &Position, &Rotation, &LinearVelocity), With<MissionPlayerBody>>,
) {
    // The cast: every scripted aircraft addressed by its measured record
    // name with the pose this tick's physics has committed, plus the player
    // body the fallback rule engages.
    let cast: Vec<CastMember> = actors
        .iter()
        .map(|(entity, actor, position, rotation, velocity)| {
            (
                entity,
                actor.record.name.clone(),
                position.0,
                rotation.0,
                velocity.0,
            )
        })
        .collect();
    let player: Option<CastMember> =
        players
            .iter()
            .next()
            .map(|(entity, position, rotation, velocity)| {
                (
                    entity,
                    PLAYER_TARGET_NAME.to_owned(),
                    position.0,
                    rotation.0,
                    velocity.0,
                )
            });

    for (_entity, mut ai, mut flight, position, rotation, own_velocity, script) in &mut aircraft {
        ai.cooldown = ai.cooldown.saturating_sub(1);
        let behaviour = ai.behaviour.clone();
        let here = position.0;
        let orientation = rotation.0;
        let nose = orientation * Vec3::NEG_Z;
        let (aim, throttle, aim_point) = match behaviour {
            MissionAiBehaviour::Asleep => continue,
            MissionAiBehaviour::Pursue { ref target } => {
                match resolve(target, &cast, player.as_ref()) {
                    Some((target_entity, _, target_position, _, _)) => {
                        ai.target = Some(target_entity);
                        let to_target = target_position - here;
                        maybe_fire(&mut ai, &mut orders, script.damage_actor, nose, to_target);
                        (to_target, PURSUIT_THROTTLE, Some(target_position))
                    }
                    None => {
                        report.no_target += 1;
                        ai.target = None;
                        (hold_course_direction(nose), START_THROTTLE, None)
                    }
                }
            }
            MissionAiBehaviour::Wingman { ref leader } => {
                // The measured leader selection (finding §1): the stored
                // primary target resolves against the vehicle registry by
                // name, and the fixed name `player` binds the player
                // vehicle — the follower then tests *identity*
                // (`leader == [0x71c298]` at `0x41e78a`), not the name.
                let resolved = if leader == PLAYER_TARGET_NAME {
                    player.clone()
                } else {
                    cast.iter()
                        .find(|(_, record, ..)| record == leader)
                        .cloned()
                };
                match resolved {
                    Some((
                        leader_entity,
                        _,
                        leader_position,
                        leader_orientation,
                        leader_velocity,
                    )) => {
                        ai.leader = Some(leader_entity);
                        // A player leader runs the unforced sub-state table:
                        // state 0 engages when the player is awake, the gap
                        // is under 700 m and the follower's own speed
                        // exceeds 20.576 m/s (finding §2). The unengaged
                        // states' goals are route objects this runtime never
                        // gives it, so the designed stand-in holds the spawn
                        // heading until the gate fires.
                        let player_leader = player
                            .as_ref()
                            .is_some_and(|(entity, ..)| *entity == leader_entity);
                        if player_leader
                            && !ai.follow_engaged
                            && own_velocity.0.length() > FOLLOW_ENGAGE_SPEED_M_S
                            && leader_position.distance_squared(here) < FOLLOW_ENGAGE_DIST2
                        {
                            ai.follow_engaged = true;
                        }
                        if player_leader && !ai.follow_engaged {
                            (hold_course_direction(nose), START_THROTTLE, None)
                        } else {
                            let offset = if player_leader {
                                STATION_OFFSET_PLAYER_M
                            } else {
                                STATION_OFFSET_M
                            };
                            let station =
                                station_point(leader_position, leader_orientation, offset);
                            let error = station - here;
                            let max_speed = flight.model.airframe.fd_speed as f32;
                            let demanded = follow_demanded_speed(error, leader_velocity, max_speed);
                            ai.throttle_target = follow_throttle_target(
                                ai.throttle_target,
                                demanded,
                                own_velocity.0.length(),
                                max_speed,
                                player
                                    .as_ref()
                                    .map(|(_, _, p, ..)| p.distance_squared(here)),
                                1.0 / f64::from(crate::physics::BASELINE_FIXED_HZ),
                            );
                            let throttle = ai.throttle_target.min(1.0);
                            (error, throttle, Some(station))
                        }
                    }
                    None => {
                        report.no_target += 1;
                        ai.leader = None;
                        (hold_course_direction(nose), START_THROTTLE, None)
                    }
                }
            }
            MissionAiBehaviour::HoldCourse => (hold_course_direction(nose), START_THROTTLE, None),
        };
        flight.command = pursuit_command(orientation, aim, throttle);
        ai.range_m = aim_point.map_or(f32::NAN, |point| (point - here).length());
        ai.decisions += 1;
        report.decisions += 1;
    }
}

/// One addressable aircraft: its entity, measured record name (or the
/// player's fixed name), and the pose and velocity of this tick.
type CastMember = (Entity, String, Vec3, Quat, Vec3);

/// Queues one fire intent when the designed window and the designed cone
/// both hold and this aircraft's own interval has run out.
///
/// The gun bank's cadence (`MISSION_TICKS_BETWEEN_SHOTS`, #1437) still
/// decides whether a round actually leaves the barrel: this is the AI's ask,
/// not the shot.
fn maybe_fire(
    ai: &mut MissionAircraftAi,
    orders: &mut MissionAiFireOrders,
    shooter: ActorId,
    nose: Vec3,
    to_target: Vec3,
) {
    if ai.cooldown > 0 {
        return;
    }
    if !should_fire(to_target.length(), nose.dot(to_target.normalize_or_zero())) {
        return;
    }
    orders.pending.push(shooter);
    ai.cooldown = FIRE_INTERVAL_TICKS;
    ai.shots += 1;
}

/// Joins this tick's fire decisions to the mission's own weapon publication.
///
/// A tick's [`WeaponTickInput`] is consumed exactly once, so an attacker
/// never pushes a second record for the same tick: the publisher calls this
/// while it builds the mission's record and the AI's
/// [`WeaponOrder::Fire`] intents ride along, under their own producer
/// serial.
pub fn append_ai_fire_orders(world: &mut World, input: &mut WeaponTickInput) {
    let drained = {
        let Some(mut orders) = world.get_resource_mut::<MissionAiFireOrders>() else {
            return;
        };
        if orders.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut orders.pending);
        let first = orders.sequence;
        orders.sequence = orders.sequence.wrapping_add(pending.len() as u32);
        (pending, first)
    };
    let (pending, first) = drained;
    for (offset, shooter) in pending.iter().enumerate() {
        input.orders.push(WeaponOrder::Fire(FireIntent {
            id: FireIntentId {
                session: input.session,
                tick: input.at,
                producer: MISSION_AI_FIRE_PRODUCER,
                sequence: first + offset as u32,
            },
            shooter: *shooter,
        }));
    }
    if let Some(mut report) = world.get_resource_mut::<MissionAiReport>() {
        report.fire_orders += pending.len() as u64;
    }
}

/// Arms every aircraft the law flies with the mission's designed gun rig,
/// once the weapon session exists: the guns registered for the record's own
/// session actor, the rig's designed mounts under its body, and the faction
/// the shot's relation reads ([`crate::mission_session::designed_target_rules`]).
///
/// This runs on the fixed tick before the decision — a composed mission
/// opens its session at `PostStartup`, and a restart re-opens it mid-run, so
/// an entity already bound to **this** generation's session is skipped and
/// anything else is armed now. Nothing here invents a gun: the loadout is
/// the mission's own designed one (#1437).
///
/// The restart's re-open lands in `PostUpdate` (`mission_host_restart`),
/// one schedule stage after this pass — so the pass runs
/// [`ensure_mission_weapons`] first and never registers a fresh generation's
/// actor into the dead generation's session for the one tick the ordering
/// would otherwise allow.
fn arm_mission_ai(world: &mut World) {
    ensure_mission_weapons(world);
    if world.get_resource::<WeaponSession>().is_none() {
        // The session opens after Startup; the next tick arms the records.
        return;
    }
    let Some(mission) = world.get_resource::<MissionWeapons>().cloned() else {
        return;
    };
    let mut candidates: Vec<(Entity, ActorId, bool, Vec3, String)> = Vec::new();
    let mut stale: Vec<String> = Vec::new();
    {
        let mut query = world.query::<(
            Entity,
            &MissionAircraftAi,
            &ScriptedAircraftActor,
            &Position,
            Option<&WeaponActorBinding>,
        )>();
        for (entity, ai, script, position, binding) in query.iter(world) {
            match binding {
                Some(binding)
                    if binding.generation == mission.generation()
                        && binding.actor == script.damage_actor =>
                {
                    continue;
                }
                Some(_) => {
                    stale.push(script.record.name.clone());
                    continue;
                }
                None => {}
            }
            candidates.push((
                entity,
                script.damage_actor,
                ai.opposed(),
                position.0,
                script.record.name.clone(),
            ));
        }
    }
    for name in &stale {
        if let Some(mut report) = world.get_resource_mut::<MissionAiReport>() {
            let entry = format!(
                "{name}: a rig bound to another session generation is left alone; the composed \
                 run respawns its actors together with the session"
            );
            if !report.refusals.contains(&entry) {
                report.refusals.push(entry);
            }
        }
    }

    let loadout = designed_loadout();
    let mut armed: Vec<String> = Vec::new();
    let mut refusals: Vec<String> = Vec::new();
    for (entity, actor, opposed, position, name) in candidates {
        let registered = world
            .get_resource_mut::<WeaponSession>()
            .map(|mut session| {
                session.register(
                    actor,
                    &loadout.guns,
                    loadout.bank.clone(),
                    AI_STARTING_ROUNDS,
                )
            });
        match registered {
            None => break,
            Some(Err(error)) => {
                refusals.push(format!(
                    "{name}: the weapon session refuses the guns: {error}"
                ));
                continue;
            }
            Some(Ok(_)) => {}
        }
        if let Err(error) = attach_gun_rig(world, entity, actor) {
            refusals.push(format!("{name}: the gun rig refuses: {error}"));
            continue;
        }
        // The faction the shot's relation reads: opposed records roster as
        // the designed hostile, everything else with the player's own
        // flight, so friendly fire stays inside #1437's declared rules.
        let Some(mut targeting) = world.get_resource_mut::<TargetingSession>() else {
            refusals.push(format!(
                "{name}: no targeting session to roster the faction into"
            ));
            continue;
        };
        let key = if opposed {
            MISSION_FACTION_HOSTILE
        } else {
            MISSION_FACTION_PLAYER
        };
        let Ok(faction) = ContentId::from_source(ContentKind::Faction, key) else {
            refusals.push(format!("{name}: the faction id {key:?} refuses"));
            continue;
        };
        let Ok(position) = WorldPosition::try_new([
            f64::from(position.x),
            f64::from(position.y),
            f64::from(position.z),
        ]) else {
            refusals.push(format!(
                "{name}: the spawn position is not a world position, so the record cannot be \
                 rostered"
            ));
            continue;
        };
        let record = TargetRecord {
            actor,
            faction,
            class: TargetClass::Aircraft,
            objective: false,
            revealed: true,
            phase_eligible: true,
            position,
        };
        if let Err(error) = targeting.store_mut().register(record) {
            refusals.push(format!(
                "{name}: the targeting store refuses the record: {error}"
            ));
            continue;
        }
        armed.push(name);
    }

    if armed.is_empty() && refusals.is_empty() {
        return;
    }
    if let Some(mut report) = world.get_resource_mut::<MissionAiReport>() {
        for entry in armed {
            if !report.armed.contains(&entry) {
                report.armed.push(entry);
            }
        }
        // A refusal that repeats every tick is recorded once: the reason
        // stays visible without the report growing without bound.
        for entry in refusals {
            if !report.refusals.contains(&entry) {
                report.refusals.push(entry);
            }
        }
    }
}

/// Resolves a pursuit rule to the cast member it names, the player body
/// being addressed by [`MissionPlayerBody`] rather than by a record name.
fn resolve(
    rule: &TargetRule,
    cast: &[CastMember],
    player: Option<&CastMember>,
) -> Option<CastMember> {
    match rule {
        TargetRule::Record(name) => cast.iter().find(|(_, record, ..)| record == name).cloned(),
        TargetRule::Player => player.cloned(),
    }
}

/// The offset a hold-course aircraft steers along: straight ahead on its own
/// heading at its own altitude, so the same steering half keeps both. It is
/// a vector from the aircraft, like the other behaviours' `to_target`, not a
/// world point.
fn hold_course_direction(nose: Vec3) -> Vec3 {
    let flat = Vec3::new(nose.x, 0.0, nose.z);
    let forward = if flat.length_squared() > 1.0e-6 {
        flat.normalize()
    } else {
        // Nose straight up or down: hold the horizontal heading the body
        // still has rather than inventing one.
        Vec3::NEG_Z
    };
    forward * HOLD_COURSE_LOOKAHEAD_M
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    #[test]
    fn accept_m01_play_ai_hold_course_steers_along_the_heading() {
        // Level flight on a yawed heading, far from the origin: the steering
        // half must read a direction, so the command stays near neutral.
        let orientation = Quat::from_rotation_y(0.7);
        let nose = orientation * Vec3::NEG_Z;
        let direction = hold_course_direction(nose);
        assert!((direction.length() - HOLD_COURSE_LOOKAHEAD_M).abs() < 1.0);
        assert!(direction.y.abs() < 1.0e-3);
        let command = pursuit_command(orientation, direction, 0.5);
        assert!(command.roll.abs() < 0.05 && command.yaw.abs() < 0.05);
    }

    fn record(
        name: &str,
        team: Option<u32>,
        primary: Option<&str>,
        asleep: bool,
    ) -> ScriptedAircraft {
        ScriptedAircraft {
            name: name.to_owned(),
            index: 0,
            field_count: 81,
            netids: None,
            stored_pose: None,
            team,
            group: None,
            enabled: Some(true),
            primary_target: primary.map(str::to_owned),
            title: None,
            starts_asleep: Some(asleep),
            dispatch: crate::mission_start::spawn_dispatch(name),
            provenance: cs_types::content::Provenance::designed(
                cs_types::evidence::ClaimId::new("test.m01-play-ai.record")
                    .expect("the claim id is valid"),
            ),
        }
    }

    /// The role split reads the measured fields: asleep first, then the
    /// player's own team, then the opposition.
    #[test]
    fn accept_m01_play_ai_the_behaviour_reads_the_measured_fields() {
        assert_eq!(
            behaviour_for(&record("bsfury_1", Some(2), Some("player"), true), Some(1)),
            MissionAiBehaviour::Asleep,
            "the measured starts_asleep field wins: no law attaches"
        );
        assert_eq!(
            behaviour_for(
                &record("wingman_2", Some(1), Some("devastator_2"), false),
                Some(1)
            ),
            MissionAiBehaviour::Wingman {
                leader: "devastator_2".to_owned()
            },
            "a record of the player's flight whose primary target names another \
             record of that flight holds station on it"
        );
        assert_eq!(
            behaviour_for(&record("devastator_2", Some(1), None, false), Some(1)),
            MissionAiBehaviour::HoldCourse,
            "a record of the player's flight with no stored target holds its course"
        );
        assert_eq!(
            behaviour_for(&record("rusdevastator_1", Some(2), None, false), Some(1)),
            MissionAiBehaviour::Pursue {
                target: TargetRule::Player
            },
            "a record of another team with no stored target engages the player"
        );
        assert_eq!(
            behaviour_for(&record("bsfury_1", Some(2), Some("player"), false), Some(1)),
            MissionAiBehaviour::Pursue {
                target: TargetRule::Player
            },
            "a stored primary target naming the player engages the player body"
        );
        assert_eq!(
            behaviour_for(
                &record("bsfury_1", Some(2), Some("wingman_2"), false),
                Some(1)
            ),
            MissionAiBehaviour::Pursue {
                target: TargetRule::Record("wingman_2".to_owned())
            },
            "any other stored primary target is engaged by record name"
        );
        assert_eq!(
            behaviour_for(&record("unknown_team", None, None, false), Some(1)),
            MissionAiBehaviour::HoldCourse,
            "a record whose team was never read states no side: hold, never invent one"
        );
    }

    /// The steering half aims the nose: a target to the right banks and
    /// yaws right, a target above pitches up, and every axis stays in range.
    #[test]
    fn accept_m01_play_ai_the_pursuit_command_turns_toward_the_target() {
        let level = Quat::IDENTITY;
        let right = pursuit_command(level, Vec3::new(400.0, 0.0, -400.0), 1.0);
        assert!(
            right.roll > 0.0,
            "a target to the right banks right-wing-down: {}",
            right.roll
        );
        assert!(
            right.yaw < 0.0,
            "positive yaw is nose-left, so a target to the right takes negative yaw: {}",
            right.yaw
        );
        assert!(
            right.pitch.abs() < 0.3,
            "a target on the horizon needs almost no pitch: {}",
            right.pitch
        );
        assert_eq!(right.throttle, 1.0, "the caller's throttle passes through");

        let above = pursuit_command(level, Vec3::new(0.0, 400.0, -400.0), 0.5);
        assert!(above.pitch > 0.0, "a target above pitches nose-up");
        assert!(above.roll.abs() < 0.1, "a target ahead needs no bank");

        let behind = pursuit_command(level, Vec3::new(0.0, 0.0, 600.0), 1.0);
        assert!(
            behind.yaw.abs() >= right.yaw.abs(),
            "a target astern takes at least the rudder a target abeam takes"
        );
        assert!(
            behind.roll.abs() <= 1.0 && behind.pitch.abs() <= 1.0 && behind.yaw.abs() <= 1.0,
            "every axis is clamped into [-1, 1]"
        );
    }

    /// The station point sits behind the leader in the leader's own frame.
    #[test]
    fn accept_m01_play_ai_the_station_point_sits_astern_of_the_leader() {
        let leader_position = Vec3::new(10.0, 100.0, -50.0);
        let station = station_point(leader_position, Quat::IDENTITY, STATION_OFFSET_M);
        assert_eq!(
            station,
            Vec3::new(
                10.0 + STATION_OFFSET_M[0],
                100.0 + STATION_OFFSET_M[1],
                -50.0 + STATION_OFFSET_M[2]
            ),
            "with the nose along -Z, the station's +Z offset is astern"
        );
        // The same leader turned 180°: the station turns with it.
        let turned = station_point(
            leader_position,
            Quat::from_rotation_y(std::f32::consts::PI),
            STATION_OFFSET_M,
        );
        assert!(
            (turned.z - (-50.0 - STATION_OFFSET_M[2])).abs() < 1.0e-4,
            "the station follows the leader's heading: {turned:?}"
        );
    }

    /// The fire window is a range **and** a cone: both must hold, and
    /// neither side of either bound fires.
    #[test]
    fn accept_m01_play_ai_the_fire_window_is_range_and_cone() {
        assert!(
            should_fire(FIRE_RANGE_M * 0.5, 1.0),
            "inside the window and pointed at the target: fire"
        );
        assert!(
            !should_fire(FIRE_RANGE_M * 1.5, 1.0),
            "outside the designed range: hold"
        );
        assert!(
            !should_fire(FIRE_RANGE_M * 0.5, FIRE_CONE_RAD.cos() - 0.01),
            "inside the range but outside the designed cone: hold"
        );
        assert!(
            !should_fire(f32::NAN, 1.0),
            "a non-finite range is never a firing solution"
        );
        assert!(
            should_fire(FIRE_RANGE_M, FIRE_CONE_RAD.cos()),
            "the bounds themselves are inclusive: the window is closed, not open"
        );
    }

    /// The measured demanded speed: leader speed plus the along and cross
    /// terms, banded to +-60 mph of the leader, floored at 50 mph.
    #[test]
    fn accept_m01_wingman_formation_b_demanded_speed_follows_the_measured_law() {
        let velocity = Vec3::new(0.0, 0.0, -100.0);
        // 100 m ahead along the velocity: 100 + 0.15 * 100 = 115 m/s.
        let ahead = follow_demanded_speed(Vec3::new(0.0, 0.0, -100.0), velocity, 200.0);
        assert!((ahead - 115.0).abs() < 1.0e-3, "{ahead}");
        // 100 m across: 100 + 0.025 * 100 = 102.5 m/s.
        let across = follow_demanded_speed(Vec3::new(100.0, 0.0, 0.0), velocity, 200.0);
        assert!((across - 102.5).abs() < 1.0e-3, "{across}");
        // 700 m ahead asks for more than the band allows: leader + 26.8224.
        let far = follow_demanded_speed(Vec3::new(0.0, 0.0, -700.0), velocity, 200.0);
        assert!(
            (far - (100.0 + FOLLOW_SPEED_BAND_M_S)).abs() < 1.0e-3,
            "{far}"
        );
        // 700 m behind the slot falls to the band's lower edge.
        let behind = follow_demanded_speed(Vec3::new(0.0, 0.0, 700.0), velocity, 200.0);
        assert!(
            (behind - (100.0 - FOLLOW_SPEED_BAND_M_S)).abs() < 1.0e-3,
            "{behind}"
        );
        // The airframe cap is 1.5 x its speed.
        let capped = follow_demanded_speed(Vec3::new(0.0, 0.0, -700.0), velocity, 60.0);
        assert!((capped - 90.0).abs() < 1.0e-3, "{capped}");
        // The floor is 50 mph.
        let slow = follow_demanded_speed(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), 200.0);
        assert!((slow - FOLLOW_SPEED_FLOOR_M_S).abs() < 1.0e-3, "{slow}");
    }

    /// The measured throttle target: slews 0.35 per second toward the demand
    /// inside 2000 m of the player, jumps to it beyond, and stays in
    /// `[0.4, 1.5]`.
    #[test]
    fn accept_m01_wingman_formation_b_throttle_target_slews_and_rubber_bands() {
        let up = follow_throttle_target(0.5, 120.0, 100.0, 200.0, Some(0.0), 1.0);
        assert!((up - 0.85).abs() < 1.0e-9, "{up}");
        let down = follow_throttle_target(0.5, 100.0, 120.0, 200.0, Some(0.0), 1.0);
        assert!(
            (down - 0.4).abs() < 1.0e-9,
            "clamped at the minimum: {down}"
        );
        let top = follow_throttle_target(1.4, 120.0, 100.0, 200.0, Some(0.0), 1.0);
        assert!((top - 1.5).abs() < 1.0e-9, "clamped at the maximum: {top}");
        let jump = follow_throttle_target(0.5, 120.0, 100.0, 200.0, Some(4_000_001.0), 0.01);
        assert!(
            (jump - 0.6).abs() < 1.0e-6,
            "beyond 2000 m: demand / speed: {jump}"
        );
        let held = follow_throttle_target(0.5, 120.0, 100.0, 0.0, Some(5.0e6), 0.01);
        assert_eq!(held, 0.5, "no airframe speed leaves the target alone");
    }

    /// The constants this module hard-codes are the committed
    /// `[[ai_follow]]` rows (`va` and value) of the owner-static table.
    #[test]
    fn accept_m01_wingman_formation_b_constants_match_the_committed_rows() {
        let text = include_str!("../../../data/owner-static/engine-facts.toml");
        let value_of = |name: &str| -> f64 {
            let at = text
                .find(&format!("name = \"{name}\""))
                .unwrap_or_else(|| panic!("row {name} is committed"));
            let row = &text[at..];
            let row = &row[..row.find("\n\n").unwrap_or(row.len())];
            let line = row
                .lines()
                .find(|l| l.starts_with("value = "))
                .expect("a value");
            line["value = ".len()..].parse().expect("a float")
        };
        let f = |name| value_of(name) as f32;
        assert_eq!(f("station_offset_other_leader_x"), STATION_OFFSET_M[0]);
        assert_eq!(f("station_offset_other_leader_y"), STATION_OFFSET_M[1]);
        assert_eq!(f("station_offset_other_leader_z"), -STATION_OFFSET_M[2]);
        assert_eq!(value_of("follow_throttle_min"), FOLLOW_THROTTLE_MIN);
        assert_eq!(value_of("follow_throttle_max"), FOLLOW_THROTTLE_MAX);
        assert_eq!(f("follow_speed_gain_along"), FOLLOW_GAIN_ALONG);
        assert_eq!(f("follow_speed_gain_cross"), FOLLOW_GAIN_CROSS);
        assert_eq!(f("follow_speed_band"), FOLLOW_SPEED_BAND_M_S);
        assert_eq!(f("follow_speed_floor"), FOLLOW_SPEED_FLOOR_M_S);
        assert_eq!(value_of("follow_throttle_slew"), FOLLOW_THROTTLE_SLEW_PER_S);
        assert_eq!(f("follow_rubber_band_dist2"), FOLLOW_RUBBER_BAND_DIST2);
    }

    /// The wake consumes the measured sleep: the behaviour a woken record
    /// flies is [`behaviour_for`]'s reading of the same fields with the sleep
    /// term removed, and a record that never slept is unchanged.
    #[test]
    fn accept_m01_play_ai_wake_the_woken_behaviour_consumes_the_sleep() {
        let asleep = record("bsfury_1", Some(2), Some("player"), true);
        assert_eq!(
            behaviour_for(&asleep, Some(1)),
            MissionAiBehaviour::Asleep,
            "the spawn reads the measured sleep"
        );
        assert_eq!(
            woken_behaviour_for(&asleep, Some(1)),
            MissionAiBehaviour::Pursue {
                target: TargetRule::Player
            },
            "with the sleep consumed the same measured fields select the pursuit"
        );
        assert_eq!(
            MissionAircraftAi::woken_for_record(&asleep, Some(1)).behaviour,
            MissionAiBehaviour::Pursue {
                target: TargetRule::Player
            },
            "the woken record component carries that behaviour"
        );
        let awake = record("rusdevastator_1", Some(2), None, false);
        assert_eq!(
            woken_behaviour_for(&awake, Some(1)),
            behaviour_for(&awake, Some(1)),
            "a record that never slept takes the same behaviour either way"
        );
        let unknown = record("unknown_team", None, None, true);
        assert_eq!(
            woken_behaviour_for(&unknown, Some(1)),
            MissionAiBehaviour::HoldCourse,
            "a woken record whose team was never read still states no side"
        );
    }

    /// The wake site's names are the top-level texts the measured site spells;
    /// any other shape resolves no name rather than being read as one.
    #[test]
    fn accept_m01_play_ai_wake_the_site_names_are_top_level_texts() {
        assert_eq!(
            wake_enemy_names(&[
                Value::Str("bsfury_1".to_owned()),
                Value::Str("blackswanzep".to_owned()),
            ]),
            Some(vec!["bsfury_1".to_owned(), "blackswanzep".to_owned()]),
            "the measured two-text site resolves both names in its own order"
        );
        assert_eq!(
            wake_enemy_names(&[]),
            Some(Vec::new()),
            "a site that spells no name wakes nothing and is not refused"
        );
        assert_eq!(
            wake_enemy_names(&[Value::Str(String::new())]),
            None,
            "an empty text is not a name"
        );
        assert_eq!(
            wake_enemy_names(&[Value::Int(3)]),
            None,
            "a scalar argument is not the measured shape"
        );
        assert_eq!(
            wake_enemy_names(&[Value::List(vec![Value::Str("bsfury_1".to_owned())])]),
            None,
            "a nested list is not the measured shape"
        );
    }

    /// The constants the player-leader branch adds are the committed
    /// `[[ai_follow]]` rows of the owner-static table — the slot the
    /// `0x41e792` select picks for a player leader, and the state-0 engage
    /// window it sits behind.
    #[test]
    fn accept_m01_wingman_formation_player_leader_constants_match_the_committed_rows() {
        let text = include_str!("../../../data/owner-static/engine-facts.toml");
        let value_of = |name: &str| -> f64 {
            let at = text
                .find(&format!("name = \"{name}\""))
                .unwrap_or_else(|| panic!("row {name} is committed"));
            let row = &text[at..];
            let row = &row[..row.find("\n\n").unwrap_or(row.len())];
            let line = row
                .lines()
                .find(|l| l.starts_with("value = "))
                .expect("a value");
            line["value = ".len()..].parse().expect("a float")
        };
        let f = |name| value_of(name) as f32;
        assert_eq!(
            f("station_offset_player_leader_x"),
            STATION_OFFSET_PLAYER_M[0]
        );
        assert_eq!(
            f("station_offset_player_leader_y"),
            STATION_OFFSET_PLAYER_M[1]
        );
        // The committed z lies along the original's "ahead" basis row; this
        // crate's nose is -Z, so ahead is written -z, exactly as
        // `station_offset_other_leader_z` maps onto `-STATION_OFFSET_M[2]`.
        assert_eq!(
            f("station_offset_player_leader_z"),
            -STATION_OFFSET_PLAYER_M[2]
        );
        assert_eq!(f("follow_engage_dist2"), FOLLOW_ENGAGE_DIST2);
        assert_eq!(f("follow_engage_speed"), FOLLOW_ENGAGE_SPEED_M_S);
    }

    /// A `vehicle.zrd` airframe for the decide tests: the synthetic field
    /// tables the campaign fixture declares, imported through the same
    /// `from_values` boundaries the production import runs.
    fn synthetic_airframe(record_name: &str) -> MissionAiAirframe {
        let airframe = OriginalAirframe::from_values(&[
            ("roll_torque", 7.5),
            ("pitch_torque", 3.3),
            ("rudder_torque", 2.0),
            ("level_off_rate", 1.0),
            ("return_rate", 3.0),
            ("ang_momentum_damp", 5.0),
            ("rec_moments_inertia_x", 1.18),
            ("rec_moments_inertia_y", 1.0),
            ("rec_moments_inertia_z", 1.1),
            ("fd_speed", 135.0),
            ("engine_factor", 0.62),
            ("drag_factor", 0.37),
            ("veh_weight", 1900.0),
            ("ref_area", 330.0),
            ("gravity", 20.0),
        ])
        .expect("the declared airframe fields are complete");
        let globals = OriginalGlobals::from_values(&[
            ("nom_gravity", 20.0),
            ("lift_aoa_0_deg", 5.0),
            ("lift_aoa_1_deg", 9.0),
            ("max_aoa_deg", 46.0),
            ("high_g_0", 9.0),
            ("high_g_1", 15.0),
            ("low_g_0", -6.0),
            ("low_g_1", -9.0),
            ("lift_accel_rate", 0.75),
            ("stall_mag", 1.25),
            ("turn_fade_in_mph", 10.0),
            ("turn_fade_out_mph", 50.0),
            ("yaw_low_speed", 0.0625),
            ("yaw_high_speed", 0.17),
            ("yaw_fade_in_mph", 10.0),
            ("yaw_max_mph", 50.0),
            ("yaw_fade_out_mph", 400.0),
        ])
        .expect("the declared globals are complete");
        MissionAiAirframe {
            model: OriginalFlightModel::full(airframe, globals),
            record: record_name.to_owned(),
            inheritance_chain: Vec::new(),
            provenance: PROVENANCE_LABEL,
        }
    }

    /// A decide world: the two resources the system mutates, then whichever
    /// bodies the test asks for.
    fn decide_world() -> World {
        let mut world = World::new();
        world.init_resource::<MissionAiReport>();
        world.init_resource::<MissionAiFireOrders>();
        world
    }

    /// A scripted aircraft body the decide system's queries see: its own
    /// record, law and pose. `player_team` is the measured team the spawn
    /// passes `for_record` — the same value the stage's `player` record
    /// stores.
    fn scripted_aircraft(
        world: &mut World,
        record: &ScriptedAircraft,
        player_team: Option<u32>,
        position: Vec3,
        velocity: Vec3,
    ) -> Entity {
        world
            .spawn((
                MissionAircraftAi::for_record(record, player_team),
                ai_flight_record(&synthetic_airframe("wingman"), position, Quat::IDENTITY),
                Position(position),
                Rotation(Quat::IDENTITY),
                LinearVelocity(velocity),
                ScriptedAircraftActor {
                    record: record.clone(),
                    vehicle_record: "wingman".to_owned(),
                    damage_actor: cs_sim::damage::ActorId {
                        session: cs_types::net::SessionId::new(9).expect("session 9"),
                        serial: 7,
                    },
                    generation: crate::scene::SceneGeneration(0),
                    asleep: false,
                },
            ))
            .id()
    }

    /// One player body the leader resolution binds by component, exactly as
    /// the composed mission presents it.
    fn player_body(world: &mut World, position: Vec3, velocity: Vec3) -> Entity {
        world
            .spawn((
                MissionPlayerBody,
                Position(position),
                Rotation(Quat::IDENTITY),
                LinearVelocity(velocity),
            ))
            .id()
    }

    /// A scripted leader the roster can carry: another scripted aircraft
    /// entity, so the cast — not the player query — supplies it.
    fn scripted_leader(world: &mut World, name: &str, position: Vec3, velocity: Vec3) -> Entity {
        scripted_aircraft(
            world,
            &record(name, Some(1), None, false),
            Some(1),
            position,
            velocity,
        )
    }

    /// A player-led wingman inside the engage window and moving resolves the
    /// player **body** as its leader — `player` is not a record name the cast
    /// can carry — latches `follow_engaged`, and aims at the player-leader
    /// slot ahead of the player, not the other-leader slot.
    #[test]
    fn accept_m01_wingman_formation_a_player_leader_engages_and_aims_at_the_player_slot() {
        let mut world = decide_world();
        let player = player_body(
            &mut world,
            Vec3::new(0.0, 150.0, 0.0),
            Vec3::new(0.0, 0.0, -80.0),
        );
        // Betty's measured spawn offset is roughly (+48, 0, +107) — well
        // inside the 700 m window — so only the speed gate stands between
        // her and the station law once she is moving.
        let here = Vec3::new(48.0, 150.0, 107.0);
        let wingman = scripted_aircraft(
            &mut world,
            &record("wingman_1", Some(1), Some("player"), false),
            Some(1),
            here,
            Vec3::new(0.0, 0.0, -80.0),
        );

        world
            .run_system_once(decide_mission_aircraft_ai)
            .expect("the decide system runs");

        let ai = world
            .get::<MissionAircraftAi>(wingman)
            .expect("the wingman's law");
        assert_eq!(
            ai.leader,
            Some(player),
            "the stored `player` target binds the player body, not a record"
        );
        assert!(
            ai.follow_engaged,
            "inside 700 m and faster than 20.576 m/s the engage latch fires"
        );
        let station = station_point(
            Vec3::new(0.0, 150.0, 0.0),
            Quat::IDENTITY,
            STATION_OFFSET_PLAYER_M,
        );
        assert!(
            (ai.range_m - (station - here).length()).abs() < 1.0e-3,
            "the aim point is the player-leader slot (6, 0, -18), not the \
             other-leader one: {} vs {}",
            ai.range_m,
            (station - here).length()
        );
        let flight = world
            .get::<PlaytestOriginalFlight>(wingman)
            .expect("the wingman's flight record");
        assert!(
            flight.command.throttle >= FOLLOW_THROTTLE_MIN,
            "the engaged wingman flies the follow law's throttle, not the \
             parked course's: {}",
            flight.command.throttle
        );
    }

    /// Both arms of the measured gate must hold: a player-led wingman that is
    /// close but too slow, and one that is fast but beyond 700 m, keep the
    /// spawn-heading stand-in and never latch `follow_engaged` — while the
    /// leader itself still resolves.
    #[test]
    fn accept_m01_wingman_formation_the_engage_gate_needs_range_and_speed() {
        let mut world = decide_world();
        let player = player_body(
            &mut world,
            Vec3::new(0.0, 150.0, 0.0),
            Vec3::new(0.0, 0.0, -80.0),
        );
        let slow = scripted_aircraft(
            &mut world,
            &record("wingman_1", Some(1), Some("player"), false),
            Some(1),
            Vec3::new(48.0, 150.0, 107.0),
            Vec3::new(0.0, 0.0, -10.0),
        );
        let far = scripted_aircraft(
            &mut world,
            &record("wingman_2", Some(1), Some("player"), false),
            Some(1),
            Vec3::new(0.0, 150.0, 800.0),
            Vec3::new(0.0, 0.0, -80.0),
        );

        world
            .run_system_once(decide_mission_aircraft_ai)
            .expect("the decide system runs");

        for (entity, why) in [
            (slow, "inside 700 m but slower than 20.576 m/s"),
            (far, "faster than 20.576 m/s but 800 m out"),
        ] {
            let ai = world.get::<MissionAircraftAi>(entity).expect("the law");
            assert_eq!(
                ai.leader,
                Some(player),
                "the leader still resolves while the gate holds: {why}"
            );
            assert!(
                !ai.follow_engaged,
                "the latch stays down while one arm fails: {why}"
            );
            assert!(
                ai.range_m.is_nan(),
                "the unengaged stand-in holds the spawn heading and aims at \
                 nothing: {why}"
            );
            let flight = world
                .get::<PlaytestOriginalFlight>(entity)
                .expect("the flight record");
            assert!(
                (flight.command.throttle - START_THROTTLE).abs() < 1.0e-9,
                "the stand-in flies the course-hold throttle: {why}"
            );
        }
    }

    /// A wingman whose leader is another scripted record keeps the measured
    /// non-player path: no engage gate — the original forces its sub-state
    /// every call — so `follow_engaged` stays down and the other-leader slot
    /// applies.
    #[test]
    fn accept_m01_wingman_formation_a_scripted_leader_skips_the_engage_gate() {
        let mut world = decide_world();
        let _player = player_body(
            &mut world,
            Vec3::new(0.0, 150.0, 0.0),
            Vec3::new(0.0, 0.0, -80.0),
        );
        let leader = scripted_leader(
            &mut world,
            "devastator_2",
            Vec3::new(0.0, 150.0, -300.0),
            Vec3::new(0.0, 0.0, -80.0),
        );
        let here = Vec3::new(48.0, 150.0, 107.0);
        let wingman = scripted_aircraft(
            &mut world,
            &record("wingman_2", Some(1), Some("devastator_2"), false),
            Some(1),
            here,
            Vec3::new(0.0, 0.0, -10.0),
        );

        world
            .run_system_once(decide_mission_aircraft_ai)
            .expect("the decide system runs");

        let ai = world
            .get::<MissionAircraftAi>(wingman)
            .expect("the wingman's law");
        assert_eq!(ai.leader, Some(leader), "the scripted leader resolves");
        assert!(
            !ai.follow_engaged,
            "a scripted leader's sub-state is forced, so the latch never sets"
        );
        let station = station_point(
            Vec3::new(0.0, 150.0, -300.0),
            Quat::IDENTITY,
            STATION_OFFSET_M,
        );
        assert!(
            (ai.range_m - (station - here).length()).abs() < 1.0e-3,
            "the aim point is the other-leader slot (8, -2, 8): {} vs {}",
            ai.range_m,
            (station - here).length()
        );
    }
}
