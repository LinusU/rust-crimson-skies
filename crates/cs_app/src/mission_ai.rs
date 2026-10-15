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
//!   modelled (no measured mount); the `WAKEUP_ENEMIES` wake that would
//!   start `bsfury_1` has no consumer yet, so a `starts_asleep` record is
//!   spawned **held** (kinematic, at its measured pose) exactly as #1435
//!   held it, and never flies; and M01's zeppelins declare
//!   `DeclaredMotion::Held` with no route polyline, so "the cargo zeppelin
//!   flies its route" has no measured route to fly.
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

use std::collections::BTreeMap;
use std::path::Path;

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, Component, Entity, FixedPreUpdate, Plugin, Quat, Query, ResMut, Resource, Vec3, With,
    World,
};

use cs_content::original_airframe::{import_retail_airframe, import_retail_globals};
use cs_sim::damage::ActorId;
use cs_sim::flight::original::{FORCE_TO_ACCEL, PROVENANCE_LABEL};
use cs_sim::flight::{FlightInput, OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_sim::targeting::{TargetClass, TargetRecord};
use cs_sim::weapons::{FireIntent, FireIntentId};
use cs_types::content::{ContentId, ContentKind};
use cs_types::space::{Quaternion, WorldPosition};

use crate::mission_actors::ScriptedAircraftActor;
use crate::mission_session::{
    MISSION_FACTION_HOSTILE, MISSION_FACTION_PLAYER, MissionPlayerBody, MissionWeapons,
    attach_gun_rig, designed_loadout,
};
use crate::mission_start::{ScriptedAircraft, SpawnDispatch};
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

/// **Designed**: the wingman's station in its leader's body frame — right,
/// up and **behind** (the nose is `-Z`, so `+Z` is astern): 45 m outboard,
/// 18 m up, 75 m astern.
pub const STATION_OFFSET_M: [f32; 3] = [45.0, 18.0, 75.0];

/// **Designed**: throttle added per metre the station point lies ahead of a
/// wingman's nose. The station error is an along-track error, so falling
/// 100 m behind adds 0.30 throttle and closing 100 m past it removes it.
pub const WINGMAN_THROTTLE_GAIN: f64 = 0.003;

/// **Designed**: the lowest throttle a wingman will fly, so a station error
/// behind it can never leave the aircraft at idle and falling out of the sky.
pub const WINGMAN_MIN_THROTTLE: f64 = 0.2;

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
    pending: Vec<cs_sim::damage::ActorId>,
    /// The intent sequence within [`MISSION_AI_FIRE_PRODUCER`].
    sequence: u32,
}

/// Why a scripted aircraft flies a law at all, or does not.
///
/// The variant is decided once, at spawn, from the record's measured fields
/// ([`behaviour_for`]) and never re-derived: a law that changed its own role
/// mid-flight would hide the measured record it read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionAiBehaviour {
    /// The record's measured `starts_asleep` field is set (#681): no law is
    /// attached and the body stays kinematic at its measured pose until a
    /// `WAKEUP_ENEMIES` consumer exists.
    Asleep,
    /// A record opposed to the player's flight: close on [`TargetRule`] and
    /// hold it.
    Pursue {
        /// What to engage: the record's own stored primary target when it
        /// names one, otherwise the player body.
        target: TargetRule,
    },
    /// A record of the player's flight whose stored `primary_target` names
    /// another aircraft of that flight: hold the designed station on it.
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
        Self {
            behaviour: behaviour_for(record, player_team),
            team: record.team,
            target: None,
            leader: None,
            range_m: f32::NAN,
            decisions: 0,
            cooldown: 0,
            shots: 0,
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
#[must_use]
pub fn behaviour_for(record: &ScriptedAircraft, player_team: Option<u32>) -> MissionAiBehaviour {
    if record.starts_asleep.unwrap_or(false) {
        return MissionAiBehaviour::Asleep;
    }
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
/// designed [`STATION_OFFSET_M`] in the leader's own body frame (so `+Z` is
/// astern and the station sits behind the leader).
#[must_use]
pub fn station_point(leader_position: Vec3, leader_orientation: Quat, offset: [f32; 3]) -> Vec3 {
    leader_position + leader_orientation * Vec3::from(offset)
}

/// **The designed wingman throttle**: the leader's own speed as a fraction of
/// this airframe's full-throttle speed (`fd_speed`, imported from
/// `vehicle.zrd`), never below the session's cruise, plus the along-track
/// station error.
///
/// `range_error_m` is the station point projected on this aircraft's nose —
/// positive when the station lies ahead. A wingman whose leader and it share
/// an airframe (M01's do: both are the `devastator` chain) therefore settles
/// at exactly its leader's throttle when the station error is zero.
#[must_use]
pub fn wingman_throttle(range_error_m: f32, leader_speed_m_s: f64, fd_speed_m_s: f64) -> f64 {
    let matched = if fd_speed_m_s > 0.0 {
        (leader_speed_m_s / fd_speed_m_s).clamp(0.0, 1.0)
    } else {
        START_THROTTLE
    };
    let base = START_THROTTLE.max(matched);
    (base + f64::from(range_error_m) * WINGMAN_THROTTLE_GAIN).clamp(WINGMAN_MIN_THROTTLE, 1.0)
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
    /// prefix in `records`, and nothing else: the `player` record flies the
    /// composition's own seat and an unmatched name has no record to import.
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
    pub fn prepare(install_root: &Path, records: &[ScriptedAircraft]) -> Self {
        let mut prepared = Self::default();
        for record in records {
            let SpawnDispatch::VehicleRecord(prefix) = &record.dispatch else {
                continue;
            };
            if prepared.models.contains_key(prefix) {
                continue;
            }
            match Self::import(install_root, prefix) {
                Ok(model) => {
                    prepared.models.insert(prefix.clone(), model);
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

    for (_entity, mut ai, mut flight, position, rotation, script) in &mut aircraft {
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
                let resolved = cast
                    .iter()
                    .find(|(_, record, ..)| record == leader)
                    .cloned();
                match resolved {
                    Some((
                        leader_entity,
                        _,
                        leader_position,
                        leader_orientation,
                        leader_velocity,
                    )) => {
                        let station =
                            station_point(leader_position, leader_orientation, STATION_OFFSET_M);
                        let error = station - here;
                        let throttle = wingman_throttle(
                            error.dot(nose),
                            f64::from(leader_velocity.length()),
                            flight.model.airframe.fd_speed,
                        );
                        ai.leader = Some(leader_entity);
                        (error, throttle, Some(station))
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
fn arm_mission_ai(world: &mut World) {
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

    /// The wingman's throttle: equal to the leader's speed fraction when the
    /// station error is zero, more when the station lies ahead, never idle.
    #[test]
    fn accept_m01_play_ai_the_wingman_throttle_tracks_the_station_error() {
        let fd_speed = 113.0;
        let cruise = f64::from(crate::playtest::command::CRUISE_THROTTLE);
        let matched = wingman_throttle(0.0, cruise * fd_speed, fd_speed);
        assert!(
            (matched - cruise).abs() < 1.0e-9,
            "a zero station error settles at the leader's own speed fraction: {matched}"
        );
        let behind = wingman_throttle(200.0, cruise * fd_speed, fd_speed);
        assert!(
            behind > matched,
            "a station ahead of the wingman adds throttle: {behind}"
        );
        let ahead = wingman_throttle(-500.0, cruise * fd_speed, fd_speed);
        assert!(
            ahead >= WINGMAN_MIN_THROTTLE,
            "a large overshoot still keeps the aircraft flying: {ahead}"
        );
        let at_rest = wingman_throttle(0.0, 0.0, fd_speed);
        assert!(
            (at_rest - cruise).abs() < 1.0e-9,
            "a leader still at rest never drags the wingman below the session cruise: {at_rest}"
        );
    }
}
