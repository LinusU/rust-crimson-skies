//! The turret update law (FLAK-B2, #1881): gates, hostile-team target search
//! and scorer, attack/bored timers, aim and the 15-degree alignment, fire
//! timing.
//!
//! The law is the one measured in
//! `docs/findings/2026-10-21-flak-a-turret-update-law.md` (rows
//! `[[turret_update_fact]]` of `data/owner-static/engine-facts.toml`, claim
//! `OWNER-STATIC-2026-10-21`, provenance `static`, nothing `verified_original`).
//! It runs over the entries [`crate::mission_turrets`] binds, in integer
//! simulation ticks, and its only output is a list of
//! [`TurretFireIntent`]s: "turret X fires from muzzle M in direction D". Turning
//! an intent into a weapon entity is FLAK-B3 (#1882).
//!
//! # Inputs the update does not own
//!
//! The update is a pure step over caller-supplied world facts:
//!
//! * [`TargetCandidate`]s — what the four target wrappers would list (the
//!   caller applies each wrapper's own liveness gate and sets
//!   [`TargetCandidate::accepts`]);
//! * [`MountPose`] — the mount's world position, its yaw and whether the
//!   `latch` conditions (mask 4 on the mount node, the node and `DEACTIVATE`)
//!   hold;
//! * a [`SightAnswer`] callback for the player ray. **No world ray is wired
//!   here**: a caller that has none answers [`SightAnswer::Unavailable`], and
//!   an unavailable answer never counts as clear — the turret does not engage
//!   and [`TurretRuntime::sight_unavailable`] counts the refusal.
//!
//! # Unknowns kept unknown
//!
//! * `AMMO` has no consumer in this path and is not read.
//! * Weapon descriptor flag `0x40` (lead aiming, intercept requirement) is
//!   unknown for `wep_27`/`wep_29`, so only the non-lead path (target minus
//!   muzzle) is implemented; [`LEAD_AIM_UNKNOWN_REASON`] names the gap.
//! * The distribution inside the `INACCURACY` cone (`0x460940`) is unmeasured:
//!   the measured part is that it takes two `rand01` draws; [`perturb`] maps
//!   them to azimuth and polar angle uniformly — a project choice.
//! * The angle convention of `aim` (`atan2` of the direction in the mount
//!   frame) is taken as yaw about +Y from +Z and pitch up positive; the
//!   alignment test is independent of it, the limits are not.
//! * The `EFFECT` duration (`entry+0x158`) has no named record key; a record
//!   with an `EFFECT` cannot be built into a config until it is supplied.
//! * Draw order between the constructor's draws (bored end, first fire time)
//!   is not recovered; this module draws the bored end first.

use cs_types::random::SplitMix64;

use crate::mission_turrets::{TurretEntry, TurretSpec};

/// The domain of the turret draw stream (`"TURRET_D"`), see
/// [`SplitMix64::for_domain`].
pub const TURRET_DRAW_DOMAIN: u64 = 0x5455_5252_4554_5F44;
/// `cos(15 degrees)` as the original states it (`0x608b70`): a turret engages
/// only when the barrel is within 15 degrees of the target direction.
pub const ALIGNED_COS: f64 = 0.965_926;
/// The score weight of a candidate (`0x4ac760`).
pub const WEIGHT_DEFAULT: f64 = 1.0;
/// The score weight of the local player's vehicle (`target.score_player_weight`).
pub const WEIGHT_PLAYER: f64 = 0.8;
/// The score weight of a `TargetVehicle` of class 4 (`target.score_vehicle_class4`).
pub const WEIGHT_VEHICLE_CLASS4: f64 = 1.4;
/// The weight multiplier of the score (`0x603534`).
pub const WEIGHT_SCALE: f64 = 1200.0;
/// Added to a turret candidate's distance (`target.score_turret_penalty`).
pub const TURRET_CANDIDATE_PENALTY_M: f64 = 37.5;
/// Added to the player ray's target height (`los.ray`).
pub const SIGHT_TARGET_LIFT_M: f64 = 0.2;
/// `FIRE_RATE` absent: the constructor's `(1.0, 1.0)` (`default.fire_rate`).
pub const DEFAULT_FIRE_RATE_S: (f64, f64) = (1.0, 1.0);
/// `ATTACK_INTERVAL` absent: `(10.0, 10.0)` (`default.attack_interval`).
pub const DEFAULT_ATTACK_INTERVAL_S: (f64, f64) = (10.0, 10.0);
/// `BORED_INTERVAL` absent: `(0, 0)` (`default.bored_interval`).
pub const DEFAULT_BORED_INTERVAL_S: (f64, f64) = (0.0, 0.0);
/// Why the lead-aiming path is not implemented.
pub const LEAD_AIM_UNKNOWN_REASON: &str = "whether the weapon descriptor's flag 0x40 is set (lead aiming, intercept requirement) is \
     unknown for the flak weapons; only the target-minus-muzzle direction is implemented";

/// Why a spec cannot be turned into a [`TurretConfig`], named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigRefusal(pub String);

fn pair(
    name: &str,
    values: Option<&Vec<f64>>,
    duplicate_single: bool,
    absent: Option<(f64, f64)>,
) -> Result<(f64, f64), ConfigRefusal> {
    match values.map(Vec::as_slice) {
        Some([a, b]) => Ok((*a, *b)),
        Some([a]) if duplicate_single => Ok((*a, *a)),
        Some(other) => Err(ConfigRefusal(format!(
            "{name} states {} value(s); the measured shape is a pair",
            other.len()
        ))),
        None => absent.ok_or_else(|| {
            ConfigRefusal(format!(
                "{name} is not stated and its original default is not measured"
            ))
        }),
    }
}

/// One turret's tuning, from its record.
#[derive(Clone, Debug, PartialEq)]
pub struct TurretConfig {
    /// The team id.
    pub team: u32,
    /// `DETECTION_RANGE`, metres.
    pub detection_range_m: f64,
    /// `SHOOT_UP_ONLY`.
    pub shoot_up_only: bool,
    /// `FIRE_RATE` (min, max), seconds.
    pub fire_rate_s: (f64, f64),
    /// `FIRE_LIMITS` (burst seconds, reload seconds); a first value of 0 is no
    /// limit.
    pub fire_limits_s: (f64, f64),
    /// `INACCURACY`, radians.
    pub inaccuracy_rad: f64,
    /// `ATTACK_INTERVAL` (min, max), seconds.
    pub attack_s: (f64, f64),
    /// `BORED_INTERVAL` (min, max), seconds.
    pub bored_s: (f64, f64),
    /// `PITCH` limits, radians; applied only when the pair differs.
    pub pitch_rad: (f64, f64),
    /// `YAW` limits, radians; applied only when the pair differs.
    pub yaw_rad: (f64, f64),
    /// The `EFFECT` wind-up duration, seconds, when the turret has an
    /// `EFFECT` node.
    pub effect_s: Option<f64>,
    /// Whether the record names a `WEAPON`.
    pub has_weapon: bool,
    /// Firepoint offsets from the mount position, metres, round-robin.
    pub firepoints_m: Vec<[f64; 3]>,
}

impl TurretConfig {
    /// Reads the tuning from `spec`. An absent field takes the constructor's
    /// default (`default.*` rows); a field of an unmeasured shape, or an
    /// `EFFECT` without a duration, is a [`ConfigRefusal`], never a guess.
    /// `effect_s` is the wind-up duration for a record with `EFFECT`
    /// and `firepoints_m` the `PARTS` firepoints' offsets.
    ///
    /// # Errors
    ///
    /// Every field that could not be read, in order.
    pub fn from_spec(
        spec: &TurretSpec,
        effect_s: Option<f64>,
        firepoints_m: Vec<[f64; 3]>,
    ) -> Result<Self, Vec<ConfigRefusal>> {
        let mut refusals = Vec::new();
        let mut take = |result: Result<(f64, f64), ConfigRefusal>| match result {
            Ok(value) => value,
            Err(refusal) => {
                refusals.push(refusal);
                (0.0, 0.0)
            }
        };
        let fire_rate_s = take(pair(
            "FIRE_RATE",
            spec.fire_rate.as_ref(),
            true,
            Some(DEFAULT_FIRE_RATE_S),
        ));
        let fire_limits_s = take(pair(
            "FIRE_LIMITS",
            spec.fire_limits.as_ref(),
            false,
            Some((0.0, 0.0)),
        ));
        let attack_s = take(pair(
            "ATTACK_INTERVAL",
            spec.attack_interval.as_ref(),
            true,
            Some(DEFAULT_ATTACK_INTERVAL_S),
        ));
        let bored_s = take(pair(
            "BORED_INTERVAL",
            spec.bored_interval.as_ref(),
            true,
            Some(DEFAULT_BORED_INTERVAL_S),
        ));
        let pitch = take(pair("PITCH", spec.pitch.as_ref(), false, Some((0.0, 0.0))));
        let yaw = take(pair("YAW", spec.yaw.as_ref(), false, Some((0.0, 0.0))));
        if spec.effect.is_some() && effect_s.is_none() {
            refusals.push(ConfigRefusal(
                "EFFECT is stated but its wind-up duration (entry+0x158) has no named source"
                    .to_owned(),
            ));
        }
        if !refusals.is_empty() {
            return Err(refusals);
        }
        let radians = |degrees: (f64, f64)| (degrees.0.to_radians(), degrees.1.to_radians());
        Ok(Self {
            team: spec.team_id(),
            detection_range_m: spec.detection_range(),
            shoot_up_only: spec.shoot_up_only.unwrap_or(false),
            fire_rate_s,
            fire_limits_s,
            inaccuracy_rad: spec.inaccuracy.unwrap_or(0.0).to_radians(),
            attack_s,
            bored_s,
            pitch_rad: radians(pitch),
            yaw_rad: radians(yaw),
            effect_s: spec.effect.as_ref().and(effect_s),
            has_weapon: spec.weapon.is_some(),
            firepoints_m,
        })
    }
}

/// Which target wrapper lists a candidate. The variants are in the search's
/// priority order: a hit in an earlier list wins outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetKind {
    /// `TargetProjectile`.
    Projectile,
    /// `TargetStruct`; scanned only when structs are included.
    Struct,
    /// `TargetTurret`.
    Turret,
    /// `TargetVehicle`.
    Vehicle,
}

/// One object a turret may target.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetCandidate {
    /// A stable id of the object.
    pub id: u64,
    /// The wrapper that lists it.
    pub kind: TargetKind,
    /// Its integer team id.
    pub team: u32,
    /// World position, metres.
    pub position_m: [f64; 3],
    /// Whether it is the local player's vehicle.
    pub is_player: bool,
    /// For a vehicle: `obj+0x67c == 4`.
    pub vehicle_class4: bool,
    /// The wrapper's own liveness gate (`target.accept_*`).
    pub accepts: bool,
}

/// The answer of a line-of-sight ray from a turret to the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SightAnswer {
    /// Nothing blocks the ray.
    Clear,
    /// The terrain or a node blocks it.
    Blocked,
    /// The caller has no world ray. Never treated as clear.
    Unavailable,
}

/// One turret mount's pose and latch conditions for this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MountPose {
    /// World position, metres.
    pub position_m: [f64; 3],
    /// The mount's yaw about +Y, radians.
    pub yaw_rad: f64,
    /// The latch conditions hold: mask 4 on the mount node, the node and the
    /// `DEACTIVATE` node.
    pub active: bool,
}

/// The world facts of one tick.
#[derive(Clone, Copy, Debug)]
pub struct TurretFrame<'a> {
    /// The committed simulation tick.
    pub tick: u64,
    /// The targets, in a stable order.
    pub candidates: &'a [TargetCandidate],
    /// The local player's team id.
    pub player_team: u32,
    /// `[0x629c20] != 0`: structs are always included.
    pub include_structs_flag: bool,
    /// `[[0x71c298]+0x91d] != 0`: skips the engage step (name unknown).
    pub player_flag: bool,
    /// `[0x64f66e] != 0`: skips the timer step (name unknown).
    pub freeze_timers: bool,
}

/// "Turret X fires from muzzle M in direction D", for FLAK-B3.
#[derive(Clone, Debug, PartialEq)]
pub struct TurretFireIntent {
    /// The tick of the shot.
    pub tick: u64,
    /// Index of the turret in the runtime list.
    pub turret: usize,
    /// The entry's name.
    pub name: String,
    /// Index of the firepoint used.
    pub firepoint: usize,
    /// The muzzle position, metres.
    pub muzzle_m: [f64; 3],
    /// The unit fire direction, `INACCURACY` applied.
    pub direction: [f64; 3],
    /// The target's id.
    pub target: u64,
}

/// The attack/bored state (`entry+0x1d0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackState {
    /// 0: attacking.
    Attacking,
    /// 1: bored; a bored turret never fires.
    Bored,
}

/// The per-turret mutable state of the update.
#[derive(Clone, Debug, PartialEq)]
pub struct TurretRuntime {
    name: String,
    config: TurretConfig,
    live: bool,
    latch: bool,
    state: AttackState,
    state_end: u64,
    next_fire: u64,
    limit_left_s: f64,
    engaged: bool,
    target: Option<u64>,
    sight_next: u64,
    sight: Option<bool>,
    sight_unavailable: u64,
    effect_on: bool,
    firepoint: usize,
    pitch: f64,
    yaw: f64,
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

fn normalised(v: [f64; 3]) -> Option<[f64; 3]> {
    let length = v.iter().map(|c| c * c).sum::<f64>().sqrt();
    (length > 0.0).then(|| [v[0] / length, v[1] / length, v[2] / length])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A direction inside the cone of half-angle `cone_rad` around the unit
/// `direction`, from two `rand01` draws (azimuth, polar fraction).
#[must_use]
pub fn perturb(direction: [f64; 3], cone_rad: f64, u_azimuth: f64, u_polar: f64) -> [f64; 3] {
    let helper = if direction[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let side = normalised(cross(direction, helper)).unwrap_or([1.0, 0.0, 0.0]);
    let up = cross(direction, side);
    let polar = cone_rad * u_polar;
    let azimuth = std::f64::consts::TAU * u_azimuth;
    let (sp, cp) = polar.sin_cos();
    let (sa, ca) = azimuth.sin_cos();
    std::array::from_fn(|i| direction[i] * cp + (side[i] * ca + up[i] * sa) * sp)
}

fn clamp_pitch(pitch: f64, limits: (f64, f64)) -> f64 {
    if limits.0 == limits.1 {
        pitch
    } else {
        pitch.clamp(limits.0.min(limits.1), limits.0.max(limits.1))
    }
}

fn clamp_yaw(yaw: f64, limits: (f64, f64)) -> f64 {
    if limits.0 == limits.1 {
        return yaw;
    }
    let (lo, hi) = (limits.0.min(limits.1), limits.0.max(limits.1));
    let tau = std::f64::consts::TAU;
    let wrapped = yaw - tau * (yaw / tau).round();
    for candidate in [wrapped, wrapped + tau, wrapped - tau] {
        if (lo..=hi).contains(&candidate) {
            return candidate;
        }
    }
    let angular = |limit: f64| {
        let d = (wrapped - limit).rem_euclid(tau);
        d.min(tau - d)
    };
    if angular(lo) <= angular(hi) { lo } else { hi }
}

fn ticks(seconds: f64, hz: u32) -> u64 {
    (seconds.max(0.0) * f64::from(hz)).ceil() as u64
}

/// The whole turret system: every runtime and the one seeded draw stream.
#[derive(Clone, Debug, PartialEq)]
pub struct TurretSystem {
    hz: u32,
    rng: SplitMix64,
    turrets: Vec<TurretRuntime>,
}

impl TurretSystem {
    /// Builds the runtimes for `entries` (each with its config, in the same
    /// order), constructing at tick `now`. The stream is
    /// [`TURRET_DRAW_DOMAIN`] under `mission_seed`.
    ///
    /// # Panics
    ///
    /// When `hz` is zero.
    #[must_use]
    pub fn new(
        mission_seed: u64,
        hz: u32,
        now: u64,
        turrets: Vec<(TurretEntry, TurretConfig)>,
    ) -> Self {
        assert!(hz > 0, "the tick rate is non-zero");
        let mut rng = SplitMix64::for_domain(mission_seed, TURRET_DRAW_DOMAIN);
        let turrets = turrets
            .into_iter()
            .map(|(entry, config)| {
                let rand = rng.unit_f64();
                let attack_draw =
                    config.attack_s.0 + rng.unit_f64() * (config.attack_s.1 - config.attack_s.0);
                let bored_end = now + ticks(rand * attack_draw, hz);
                let (min, max) = config.fire_rate_s;
                let first_fire = now + ticks((min + max) * rng.unit_f64() * 0.5 + 1.0, hz);
                TurretRuntime {
                    name: entry.name,
                    live: entry.live,
                    latch: false,
                    state: AttackState::Bored,
                    state_end: bored_end,
                    next_fire: first_fire,
                    limit_left_s: config.fire_limits_s.0,
                    engaged: false,
                    target: None,
                    sight_next: 0,
                    sight: None,
                    sight_unavailable: 0,
                    effect_on: false,
                    firepoint: 0,
                    pitch: 0.0,
                    yaw: 0.0,
                    config,
                }
            })
            .collect();
        Self { hz, rng, turrets }
    }

    /// The runtimes.
    #[must_use]
    pub fn turrets(&self) -> &[TurretRuntime] {
        &self.turrets
    }

    /// Restores a turret's attack/bored state and the tick it ends at (the
    /// save-state copy of `entry+0x1d0/0x1c8/0x1cc`).
    pub fn restore_attack_state(&mut self, turret: usize, state: AttackState, end_tick: u64) {
        if let Some(runtime) = self.turrets.get_mut(turret) {
            runtime.state = state;
            runtime.state_end = end_tick;
        }
    }

    /// Sets a turret's live byte (the wake ledger's write).
    pub fn set_live(&mut self, turret: usize, live: bool) {
        if let Some(runtime) = self.turrets.get_mut(turret) {
            runtime.live = live;
        }
    }

    fn draw_between(&mut self, (min, max): (f64, f64)) -> f64 {
        min + self.rng.unit_f64() * (max - min)
    }

    /// Runs one tick for every turret; `mounts` is parallel to the runtimes.
    /// Returns the shots of this tick.
    pub fn update(
        &mut self,
        frame: &TurretFrame<'_>,
        mounts: &[MountPose],
        sight: &mut dyn FnMut([f64; 3], [f64; 3]) -> SightAnswer,
    ) -> Vec<TurretFireIntent> {
        let mut intents = Vec::new();
        for index in 0..self.turrets.len() {
            let Some(mount) = mounts.get(index) else {
                continue;
            };
            if let Some(intent) = self.update_one(index, frame, mount, sight) {
                intents.push(intent);
            }
        }
        intents
    }

    fn update_one(
        &mut self,
        index: usize,
        frame: &TurretFrame<'_>,
        mount: &MountPose,
        sight: &mut dyn FnMut([f64; 3], [f64; 3]) -> SightAnswer,
    ) -> Option<TurretFireIntent> {
        let hz = self.hz;
        let dt = 1.0 / f64::from(hz);
        let now = frame.tick;
        // gate.latch: a dropped latch switches the effect off and releases the
        // target, then returns.
        {
            let turret = &mut self.turrets[index];
            if !mount.active {
                if turret.latch {
                    turret.latch = false;
                    turret.effect_on = false;
                    turret.target = None;
                    turret.engaged = false;
                }
                return None;
            }
            turret.latch = true;
            if !turret.live || !turret.config.has_weapon {
                return None;
            }
        }
        if frame.player_flag {
            return None;
        }
        if !frame.freeze_timers {
            self.step_timers(index, now);
        }
        // Target step.
        let include_structs =
            frame.include_structs_flag || self.turrets[index].config.team != frame.player_team;
        let chosen = self.search(index, frame, mount, include_structs);
        let turret = &mut self.turrets[index];
        let Some(chosen) = chosen else {
            turret.target = None;
            turret.engaged = false;
            Self::recharge(turret, dt);
            return None;
        };
        turret.target = Some(chosen.id);
        // Line of sight: the player only, at 1.0 + rand01 s intervals.
        let mut seen = true;
        if chosen.is_player {
            if now >= turret.sight_next || turret.sight.is_none() {
                let mut to = chosen.position_m;
                to[1] += SIGHT_TARGET_LIFT_M;
                match sight(mount.position_m, to) {
                    SightAnswer::Unavailable => {
                        turret.sight = None;
                        turret.sight_unavailable += 1;
                    }
                    answer => {
                        turret.sight = Some(answer == SightAnswer::Clear);
                        let interval = 1.0 + self.rng.unit_f64();
                        self.turrets[index].sight_next = now + ticks(interval, hz);
                    }
                }
            }
            seen = self.turrets[index].sight == Some(true);
        }
        let turret = &mut self.turrets[index];
        if !seen {
            turret.engaged = false;
            Self::recharge(turret, dt);
            return None;
        }
        // Aim.
        let local = [
            chosen.position_m[0] - mount.position_m[0],
            chosen.position_m[1] - mount.position_m[1],
            chosen.position_m[2] - mount.position_m[2],
        ];
        let Some(toward) = normalised(local) else {
            turret.engaged = false;
            return None;
        };
        let want_yaw = toward[0].atan2(toward[2]) - mount.yaw_rad;
        let want_pitch = toward[1].atan2(toward[0].hypot(toward[2]));
        turret.pitch = clamp_pitch(want_pitch, turret.config.pitch_rad);
        turret.yaw = clamp_yaw(want_yaw, turret.config.yaw_rad);
        let world_yaw = turret.yaw + mount.yaw_rad;
        let barrel = [
            world_yaw.sin() * turret.pitch.cos(),
            turret.pitch.sin(),
            world_yaw.cos() * turret.pitch.cos(),
        ];
        let dot: f64 = (0..3).map(|i| barrel[i] * toward[i]).sum();
        turret.engaged = dot > ALIGNED_COS && turret.state == AttackState::Attacking;
        if !turret.engaged {
            Self::recharge(turret, dt);
            return None;
        }
        self.fire(index, frame, mount, chosen.id, chosen.position_m)
    }

    fn recharge(turret: &mut TurretRuntime, dt: f64) {
        let limit = turret.config.fire_limits_s.0;
        if limit > 0.0 && !turret.engaged {
            turret.limit_left_s = (turret.limit_left_s + dt).min(limit);
        }
    }

    fn step_timers(&mut self, index: usize, now: u64) {
        let hz = self.hz;
        let (state, end) = (self.turrets[index].state, self.turrets[index].state_end);
        if now <= end {
            return;
        }
        let (next, range) = match state {
            AttackState::Bored => (AttackState::Attacking, self.turrets[index].config.attack_s),
            AttackState::Attacking => (AttackState::Bored, self.turrets[index].config.bored_s),
        };
        let duration = self.draw_between(range);
        let turret = &mut self.turrets[index];
        turret.state = next;
        turret.state_end = now + ticks(duration, hz);
    }

    fn search(
        &self,
        index: usize,
        frame: &TurretFrame<'_>,
        mount: &MountPose,
        include_structs: bool,
    ) -> Option<TargetCandidate> {
        let config = &self.turrets[index].config;
        let mut best: [Option<(f64, &TargetCandidate)>; 4] = [None; 4];
        for candidate in frame.candidates {
            if !candidate.accepts
                || candidate.team == 0
                || config.team == 0
                || candidate.team == config.team
                || (candidate.kind == TargetKind::Struct && !include_structs)
                || (config.shoot_up_only && candidate.position_m[1] < mount.position_m[1])
            {
                continue;
            }
            let mut distance = dist(mount.position_m, candidate.position_m);
            if distance > config.detection_range_m {
                continue;
            }
            let weight = if candidate.is_player {
                WEIGHT_PLAYER
            } else if candidate.kind == TargetKind::Vehicle && candidate.vehicle_class4 {
                WEIGHT_VEHICLE_CLASS4
            } else {
                WEIGHT_DEFAULT
            };
            if candidate.kind == TargetKind::Turret {
                distance += TURRET_CANDIDATE_PENALTY_M;
            }
            let score = weight * WEIGHT_SCALE + distance;
            let slot = &mut best[candidate.kind as usize];
            if slot.is_none_or(|(held, _)| score < held) {
                *slot = Some((score, candidate));
            }
        }
        best.into_iter().flatten().next().map(|(_, c)| c.clone())
    }

    fn fire(
        &mut self,
        index: usize,
        frame: &TurretFrame<'_>,
        mount: &MountPose,
        target: u64,
        target_position: [f64; 3],
    ) -> Option<TurretFireIntent> {
        let hz = self.hz;
        let now = frame.tick;
        let dt = 1.0 / f64::from(hz);
        let turret = &mut self.turrets[index];
        if now < turret.next_fire {
            return None;
        }
        let count = turret.config.firepoints_m.len();
        if count == 0 {
            return None;
        }
        // fire.effect_windup / fire.effect_off.
        if let Some(duration) = turret.config.effect_s {
            if !turret.effect_on {
                turret.effect_on = true;
                turret.next_fire = now + ticks(duration, hz);
                turret.firepoint = (turret.firepoint + 1) % count;
                return None;
            }
            turret.effect_on = false;
        }
        let slot = turret.firepoint % count;
        let offset = turret.config.firepoints_m[slot];
        let muzzle: [f64; 3] = std::array::from_fn(|i| mount.position_m[i] + offset[i]);
        let aim = normalised(std::array::from_fn(|i| target_position[i] - muzzle[i]))
            .unwrap_or([0.0, 0.0, 1.0]);
        let inaccuracy = turret.config.inaccuracy_rad;
        let limits = turret.config.fire_limits_s;
        turret.firepoint = (slot + 1) % count;
        let name = turret.name.clone();
        let direction = if inaccuracy != 0.0 {
            let (u_azimuth, u_polar) = (self.rng.unit_f64(), self.rng.unit_f64());
            perturb(aim, inaccuracy, u_azimuth, u_polar)
        } else {
            aim
        };
        if limits.0 > 0.0 {
            let turret = &mut self.turrets[index];
            turret.limit_left_s -= dt;
            if turret.limit_left_s <= 0.0 {
                turret.limit_left_s = limits.0;
                turret.engaged = false;
                turret.next_fire = now + ticks(limits.1, hz);
            }
        } else {
            let draw = self.draw_between(self.turrets[index].config.fire_rate_s);
            self.turrets[index].next_fire = now + ticks(draw, hz);
        }
        Some(TurretFireIntent {
            tick: now,
            turret: index,
            name,
            firepoint: slot,
            muzzle_m: muzzle,
            direction,
            target,
        })
    }
}

impl TurretRuntime {
    /// The entry's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The live byte.
    #[must_use]
    pub fn live(&self) -> bool {
        self.live
    }

    /// The attack/bored state.
    #[must_use]
    pub fn state(&self) -> AttackState {
        self.state
    }

    /// The held target's id.
    #[must_use]
    pub fn target(&self) -> Option<u64> {
        self.target
    }

    /// Whether the barrel is within 15 degrees of the target (`entry+0x16c`).
    #[must_use]
    pub fn engaged(&self) -> bool {
        self.engaged
    }

    /// How often the player ray had no answer, so the turret did not engage.
    #[must_use]
    pub fn sight_unavailable(&self) -> u64 {
        self.sight_unavailable
    }

    /// The clamped (yaw, pitch) written into the turret nodes, radians.
    #[must_use]
    pub fn angles(&self) -> (f64, f64) {
        (self.yaw, self.pitch)
    }
}

/// The scope's `TURRET` specs, read from the installation in the original's
/// mission/group/shared mount order ([`crate::mission_turrets::read_turret_specs`]
/// over the first archive that holds `ai.zrd`). Empty when none does.
#[must_use]
pub fn scope_turret_specs(
    install_root: &std::path::Path,
    mission_dir: &str,
    group_dir: &str,
) -> Vec<TurretSpec> {
    crate::mission_session::parts::read_scope_turrets(install_root, mission_dir, group_dir).specs
}
