//! The capital-ship session runtime (F35-B): one per-session set whose
//! integer ticks move registered ships, gate hits through the weakpoint
//! windows and answer turret aim/fire commands.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-B`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`CapitalShipSet`] is the production path the [`CapitalShip`] aggregate
//! was built for, and it follows the `world_actors::WorldActorSet`
//! conventions: monotonic ticks, a canonical pose per ship, velocities that
//! are always the measured per-tick displacement and never a stale authored
//! value, and wreck poses captured at destruction with zeroed velocities.
//!
//! # Designed movement model
//!
//! A ship follows its authored [`Trajectory`] through a *drive* position in
//! trajectory ticks. Each step advances the drive by
//! [`CapitalShip::propulsion_fraction`] — the intact engines' share of the
//! declared total — so a ship at full power keeps its authored schedule,
//! losing an engine slows it proportionally, and losing all of them stops
//! it. Reported velocity is the actual displacement over the stepped tick,
//! so a half-powered ship reports half the authored speed. A ship with no
//! engines has no propulsion to lose: its course is unpowered (like a
//! scheduled train) and it keeps schedule; a ship with no trajectory is
//! moored and holds its registered pose. A destroyed ship freezes at its
//! current pose as a zero-velocity wreck — the staged crash/sinking phase
//! is F35-C.
//!
//! All of this is designed behavior; no original capital-ship coefficient
//! or rule is measured. See
//! `docs/findings/2026-10-04-f35-b-movement-weakpoints-and-turrets.md`.

use std::collections::BTreeMap;

use cs_script::ir::ActorId;
use cs_types::Tick;

use super::motion::PropulsionError;
use super::ship::{CapitalShip, HitError, HitOutcome, TurretAim, TurretRefusal};
use super::subsystem::{DisableOutcome, SubsystemGraphError, SubsystemKey};
use crate::world_actors::Quat;
use crate::world_actors::trajectory::{Pose, Trajectory};

/// One hit delivered to a capital ship: the struck actor and subsystem, the
/// damage applied, and the tick it lands on.
///
/// The set refuses a hit stamped for any tick but the current one
/// ([`CapitalRuntimeError::ForeignTick`]): hit policy always runs against
/// the committed tick, so a stale or future hit can never resolve against
/// the wrong weakpoint phase.
#[derive(Clone, Debug, PartialEq)]
pub struct CapitalHit {
    /// The ship struck.
    pub target: ActorId,
    /// The subsystem struck.
    pub subsystem: SubsystemKey,
    /// The damage delivered, in section-integrity units.
    pub damage: f64,
    /// The tick the hit lands on; must equal the set's current tick.
    pub at: Tick,
}

impl CapitalHit {
    /// Validates the hit's damage.
    ///
    /// # Errors
    ///
    /// [`HitError::NonFiniteDamage`] or [`HitError::NegativeDamage`].
    pub fn try_new(
        target: ActorId,
        subsystem: SubsystemKey,
        damage: f64,
        at: Tick,
    ) -> Result<Self, HitError> {
        if !damage.is_finite() {
            return Err(HitError::NonFiniteDamage { subsystem });
        }
        if damage < 0.0 {
            return Err(HitError::NegativeDamage {
                subsystem,
                value: damage,
            });
        }
        Ok(Self {
            target,
            subsystem,
            damage,
            at,
        })
    }
}

/// What a stepped tick produced, in ship order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CapitalShipEvent {
    /// A ship's drive reached the end of its authored course; it holds the
    /// final pose from that tick on.
    CourseCompleted {
        /// The arrived ship.
        actor: ActorId,
        /// The tick it arrived.
        at: Tick,
    },
}

/// Why a [`CapitalShipSet`] operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum CapitalRuntimeError {
    /// A zero ticks-per-second rate was requested.
    ZeroTickRate,
    /// The actor id is already registered; ids are never reused within a
    /// session, not even over a destroyed record.
    DuplicateActor(ActorId),
    /// The named actor is not registered.
    UnknownShip(ActorId),
    /// A ship with no authored course needs a start pose; none was given.
    NoCourse {
        /// The course-less ship.
        actor: ActorId,
    },
    /// A ship with an authored course must not also be given a start pose:
    /// the pose would be dead state beside the course-driven one.
    CourseAndPose {
        /// The ship carrying both.
        actor: ActorId,
    },
    /// A trajectory whose tick rate differs from the set's: its sampled
    /// velocity is derived in the trajectory's own timebase, so a mismatch
    /// would report a speed the set's ticks do not produce.
    TickRateMismatch {
        /// The ship carrying the mismatched trajectory.
        actor: ActorId,
        /// The set's tick rate.
        set_ticks_per_second: u32,
        /// The trajectory's tick rate.
        trajectory_ticks_per_second: u32,
    },
    /// A named field was NaN or infinite.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// An orientation was not a unit quaternion.
    NonUnitOrientation,
    /// `advance_to` was asked for a tick the set already passed; ticks are
    /// never replayed backwards.
    NonMonotonicTick {
        /// The set's current tick.
        current: Tick,
        /// The rejected target.
        requested: Tick,
    },
    /// The tick counter would wrap.
    TickOverflow,
    /// A hit was stamped for a tick other than the set's current one: hit
    /// policy always resolves against the committed tick.
    ForeignTick {
        /// The set's current tick.
        expected: Tick,
        /// The tick the hit carried.
        found: Tick,
    },
    /// The ship refused the hit.
    Hit(HitError),
    /// The turret refused the command.
    Turret(TurretRefusal),
    /// The subsystem graph refused the transition.
    Subsystem(SubsystemGraphError),
    /// The propulsion total could not be computed — an unresolved thrust
    /// leaves the ship's motion unknown rather than approximated.
    Propulsion(PropulsionError),
}

impl std::fmt::Display for CapitalRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroTickRate => write!(f, "the tick rate must not be zero"),
            Self::DuplicateActor(actor) => write!(f, "actor {actor:?} is already registered"),
            Self::UnknownShip(actor) => write!(f, "actor {actor:?} is not a registered ship"),
            Self::NoCourse { actor } => {
                write!(f, "ship {actor:?} has no course and no start pose")
            }
            Self::CourseAndPose { actor } => write!(
                f,
                "ship {actor:?} has an authored course; a start pose cannot apply"
            ),
            Self::TickRateMismatch {
                actor,
                set_ticks_per_second,
                trajectory_ticks_per_second,
            } => write!(
                f,
                "ship {actor:?} runs at {trajectory_ticks_per_second} ticks/s, the set at {set_ticks_per_second}"
            ),
            Self::NonFinite { field } => write!(f, "{field} is not finite"),
            Self::NonUnitOrientation => write!(f, "the orientation is not a unit quaternion"),
            Self::NonMonotonicTick { current, requested } => write!(
                f,
                "cannot advance to tick {requested:?}: the set is already at {current:?}"
            ),
            Self::TickOverflow => write!(f, "the tick counter would wrap"),
            Self::ForeignTick { expected, found } => write!(
                f,
                "the hit is stamped for tick {found:?}, but the set is at {expected:?}"
            ),
            Self::Hit(source) => write!(f, "the ship refused the hit: {source}"),
            Self::Turret(source) => write!(f, "the turret refused: {source}"),
            Self::Subsystem(source) => write!(f, "the subsystem graph refused: {source}"),
            Self::Propulsion(source) => write!(f, "the propulsion total refused: {source}"),
        }
    }
}

impl std::error::Error for CapitalRuntimeError {}

impl From<HitError> for CapitalRuntimeError {
    fn from(source: HitError) -> Self {
        Self::Hit(source)
    }
}

impl From<TurretRefusal> for CapitalRuntimeError {
    fn from(source: TurretRefusal) -> Self {
        Self::Turret(source)
    }
}

impl From<SubsystemGraphError> for CapitalRuntimeError {
    fn from(source: SubsystemGraphError) -> Self {
        Self::Subsystem(source)
    }
}

impl From<PropulsionError> for CapitalRuntimeError {
    fn from(source: PropulsionError) -> Self {
        Self::Propulsion(source)
    }
}

/// One registered ship's session state.
#[derive(Clone, Debug, PartialEq)]
struct ShipState {
    /// The contract aggregate: subsystems, pools, mounts.
    ship: CapitalShip,
    /// Progress along the authored course, in trajectory ticks. One step
    /// advances it by the ship's propulsion fraction, so a crippled ship
    /// falls behind its schedule instead of teleporting along it.
    drive_ticks: f64,
    /// The last committed pose. Its velocity fields are the measured
    /// displacement of the previous step.
    pose: Pose,
    /// The wreck pose captured when a lethal subsystem destroyed the ship:
    /// the live pose with velocities zeroed, held from then on.
    wreck: Option<Pose>,
    /// Whether the `CourseCompleted` event already fired.
    course_done: bool,
}

/// One session's capital ships: registry, course stepping and hit/turret
/// operations.
#[derive(Clone, Debug, PartialEq)]
pub struct CapitalShipSet {
    ticks_per_second: u32,
    tick: Tick,
    ships: BTreeMap<ActorId, ShipState>,
}

impl CapitalShipSet {
    /// An empty set at [`Tick`] 0 stepping `ticks_per_second` fixed ticks
    /// per second.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::ZeroTickRate`].
    pub fn new(ticks_per_second: u32) -> Result<Self, CapitalRuntimeError> {
        if ticks_per_second == 0 {
            return Err(CapitalRuntimeError::ZeroTickRate);
        }
        Ok(Self {
            ticks_per_second,
            tick: Tick(0),
            ships: BTreeMap::new(),
        })
    }

    /// The fixed dt of one step, in seconds.
    #[must_use]
    pub fn dt_seconds(&self) -> f64 {
        1.0 / f64::from(self.ticks_per_second)
    }

    /// The last committed tick.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Every registered ship, in id order.
    pub fn ships(&self) -> impl Iterator<Item = ActorId> + '_ {
        self.ships.keys().copied()
    }

    /// Registers one ship.
    ///
    /// A ship with an authored [`Trajectory`] starts at its schedule's tick
    /// 0 pose — with the sampled authored velocity, like a spawned cruiser
    /// — and refuses a start pose it could never use. A ship with no
    /// trajectory is moored: `start` is required, and its velocities are
    /// zeroed because a held pose never moves.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::DuplicateActor`], [`NoCourse`](CapitalRuntimeError::NoCourse),
    /// [`CourseAndPose`](CapitalRuntimeError::CourseAndPose),
    /// [`TickRateMismatch`](CapitalRuntimeError::TickRateMismatch),
    /// [`NonFinite`](CapitalRuntimeError::NonFinite) or
    /// [`NonUnitOrientation`](CapitalRuntimeError::NonUnitOrientation).
    pub fn register(
        &mut self,
        ship: CapitalShip,
        start: Option<Pose>,
    ) -> Result<(), CapitalRuntimeError> {
        let actor = ship.actor();
        if self.ships.contains_key(&actor) {
            return Err(CapitalRuntimeError::DuplicateActor(actor));
        }
        let (drive_ticks, pose) = match ship.trajectory() {
            Some(trajectory) => {
                if start.is_some() {
                    return Err(CapitalRuntimeError::CourseAndPose { actor });
                }
                if trajectory.ticks_per_second() != self.ticks_per_second {
                    return Err(CapitalRuntimeError::TickRateMismatch {
                        actor,
                        set_ticks_per_second: self.ticks_per_second,
                        trajectory_ticks_per_second: trajectory.ticks_per_second(),
                    });
                }
                (0.0, trajectory.sample(Tick(0)))
            }
            None => {
                let start = start.ok_or(CapitalRuntimeError::NoCourse { actor })?;
                check_finite("position_m", &start.position_m)?;
                check_finite("velocity_m_s", &start.velocity_m_s)?;
                check_finite("angular_velocity_rad_s", &start.angular_velocity_rad_s)?;
                check_unit(start.orientation)?;
                (
                    0.0,
                    Pose {
                        position_m: start.position_m,
                        orientation: start.orientation,
                        velocity_m_s: [0.0; 3],
                        angular_velocity_rad_s: [0.0; 3],
                    },
                )
            }
        };
        let wreck = ship.is_destroyed().then(|| zeroed(pose));
        self.ships.insert(
            actor,
            ShipState {
                ship,
                drive_ticks,
                pose,
                wreck,
                course_done: false,
            },
        );
        Ok(())
    }

    /// The canonical pose of `actor` at the set's current tick — the wreck
    /// pose once the ship is destroyed.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`].
    pub fn pose(&self, actor: ActorId) -> Result<Pose, CapitalRuntimeError> {
        let state = self
            .ships
            .get(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?;
        Ok(state.wreck.unwrap_or(state.pose))
    }

    /// The registered ship's contract aggregate.
    #[must_use]
    pub fn ship(&self, actor: ActorId) -> Option<&CapitalShip> {
        self.ships.get(&actor).map(|state| &state.ship)
    }

    /// Whether a lethal subsystem has destroyed the ship.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`].
    pub fn is_destroyed(&self, actor: ActorId) -> Result<bool, CapitalRuntimeError> {
        Ok(self
            .ships
            .get(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?
            .ship
            .is_destroyed())
    }

    /// The ship's course progress in trajectory ticks, for diagnostics.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`].
    pub fn drive_ticks(&self, actor: ActorId) -> Result<f64, CapitalRuntimeError> {
        Ok(self
            .ships
            .get(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?
            .drive_ticks)
    }

    /// Advances the set exactly one tick and returns the transitions that
    /// tick produced, in ship order.
    ///
    /// Every live ship's drive advances by its propulsion fraction; a
    /// destroyed ship's wreck pose is held untouched. An unresolved engine
    /// thrust refuses the whole step before any ship moves — the set never
    /// commits a partial motion it could not compute honestly.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::TickOverflow`] or
    /// [`CapitalRuntimeError::Propulsion`].
    pub fn step(&mut self) -> Result<Vec<CapitalShipEvent>, CapitalRuntimeError> {
        let next = self
            .tick
            .0
            .checked_add(1)
            .ok_or(CapitalRuntimeError::TickOverflow)?;
        // Every moving ship's rate is computed before any state changes:
        // one unresolved thrust refuses the step as a whole. A moored ship
        // has no course to drive, so its propulsion is irrelevant to it.
        let mut rates = Vec::with_capacity(self.ships.len());
        for (actor, state) in &self.ships {
            if state.wreck.is_some() || state.ship.trajectory().is_none() {
                continue;
            }
            rates.push((*actor, state.ship.propulsion_fraction()?));
        }
        self.tick = Tick(next);
        let dt = self.dt_seconds();
        let mut events = Vec::new();
        for (actor, rate) in rates {
            let state = self
                .ships
                .get_mut(&actor)
                .expect("the registry cannot change mid-step");
            let Some(trajectory) = state.ship.trajectory() else {
                continue;
            };
            let end = trajectory.end_tick().0 as f64;
            state.drive_ticks = (state.drive_ticks + rate).min(end);
            let target = sample_drive(trajectory, state.drive_ticks);
            let old = state.pose;
            state.pose = Pose {
                position_m: target.position_m,
                orientation: target.orientation,
                velocity_m_s: sub(target.position_m, old.position_m).map(|d| d / dt),
                angular_velocity_rad_s: old
                    .orientation
                    .slerp_angular_velocity(target.orientation)
                    .map(|v| v / dt),
            };
            if !state.course_done && state.drive_ticks >= end {
                state.course_done = true;
                events.push(CapitalShipEvent::CourseCompleted {
                    actor,
                    at: self.tick,
                });
            }
        }
        Ok(events)
    }

    /// Advances the set to `to`, one committed tick at a time, and returns
    /// every event the steps produced.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::NonMonotonicTick`], [`TickOverflow`](CapitalRuntimeError::TickOverflow)
    /// or [`CapitalRuntimeError::Propulsion`].
    pub fn advance_to(&mut self, to: Tick) -> Result<Vec<CapitalShipEvent>, CapitalRuntimeError> {
        if to < self.tick {
            return Err(CapitalRuntimeError::NonMonotonicTick {
                current: self.tick,
                requested: to,
            });
        }
        let mut events = Vec::new();
        while self.tick < to {
            events.extend(self.step()?);
        }
        Ok(events)
    }

    /// Applies `hit` to its target ship at the set's current tick.
    ///
    /// The hit's `at` must equal the committed tick, so the weakpoint
    /// decision always sees the same phase the world just stepped to.
    /// A hit that destroys the ship freezes its wreck pose here, not on a
    /// later step.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`], [`ForeignTick`](CapitalRuntimeError::ForeignTick)
    /// or [`CapitalRuntimeError::Hit`].
    pub fn apply_hit(&mut self, hit: &CapitalHit) -> Result<HitOutcome, CapitalRuntimeError> {
        if hit.at != self.tick {
            return Err(CapitalRuntimeError::ForeignTick {
                expected: self.tick,
                found: hit.at,
            });
        }
        let state = self
            .ships
            .get_mut(&hit.target)
            .ok_or(CapitalRuntimeError::UnknownShip(hit.target))?;
        let outcome = state.ship.apply_hit(&hit.subsystem, hit.damage, hit.at)?;
        Self::freeze_if_destroyed(state);
        Ok(outcome)
    }

    /// Disables one subsystem through the graph — the scripted destruction
    /// path, equivalent to a landed hit's effect but not gated by exposure
    /// or damage. Returns what the disable applied.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`CapitalRuntimeError::Subsystem`].
    pub fn disable(
        &mut self,
        actor: ActorId,
        key: &SubsystemKey,
    ) -> Result<DisableOutcome, CapitalRuntimeError> {
        let state = self
            .ships
            .get_mut(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?;
        let outcome = state.ship.disable(key)?;
        Self::freeze_if_destroyed(state);
        Ok(outcome)
    }

    /// Commands `actor`'s turret `key` to bear on the body-frame
    /// `direction`; see [`CapitalShip::aim_turret`].
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`CapitalRuntimeError::Turret`].
    pub fn aim_turret(
        &mut self,
        actor: ActorId,
        key: &SubsystemKey,
        direction: [f64; 3],
    ) -> Result<TurretAim, CapitalRuntimeError> {
        Ok(self
            .ships
            .get_mut(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?
            .ship
            .aim_turret(key, direction)?)
    }

    /// The body-frame direction `actor`'s turret `key` would fire along;
    /// see [`CapitalShip::may_fire`].
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`CapitalRuntimeError::Turret`].
    pub fn may_fire(
        &self,
        actor: ActorId,
        key: &SubsystemKey,
    ) -> Result<[f64; 3], CapitalRuntimeError> {
        Ok(self
            .ships
            .get(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?
            .ship
            .may_fire(key)?)
    }

    /// Captures the wreck pose the first time the ship is seen destroyed.
    fn freeze_if_destroyed(state: &mut ShipState) {
        if state.ship.is_destroyed() && state.wreck.is_none() {
            state.wreck = Some(zeroed(state.pose));
        }
    }
}

/// A held pose: position and orientation kept, velocities zeroed.
fn zeroed(pose: Pose) -> Pose {
    Pose {
        position_m: pose.position_m,
        orientation: pose.orientation,
        velocity_m_s: [0.0; 3],
        angular_velocity_rad_s: [0.0; 3],
    }
}

/// Samples a trajectory at a fractional tick: linear position, slerped
/// orientation between the bracketing tick samples.
fn sample_drive(trajectory: &Trajectory, drive_ticks: f64) -> Pose {
    let lo = drive_ticks.floor();
    let hi = drive_ticks.ceil();
    let a = trajectory.sample(Tick(lo.min(u64::MAX as f64) as u64));
    if hi == lo {
        return a;
    }
    let b = trajectory.sample(Tick(hi.min(u64::MAX as f64) as u64));
    let t = drive_ticks - lo;
    Pose {
        position_m: lerp(a.position_m, b.position_m, t),
        orientation: a.orientation.slerp(b.orientation, t),
        velocity_m_s: lerp(a.velocity_m_s, b.velocity_m_s, t),
        angular_velocity_rad_s: lerp(a.angular_velocity_rad_s, b.angular_velocity_rad_s, t),
    }
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn check_finite(field: &'static str, value: &[f64; 3]) -> Result<(), CapitalRuntimeError> {
    if !value.iter().all(|v| v.is_finite()) {
        return Err(CapitalRuntimeError::NonFinite { field });
    }
    Ok(())
}

fn check_unit(orientation: Quat) -> Result<(), CapitalRuntimeError> {
    if !orientation.is_unit() {
        return Err(CapitalRuntimeError::NonUnitOrientation);
    }
    Ok(())
}
