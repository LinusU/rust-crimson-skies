//! The capital-ship session runtime (F35-B), plus the F35-C wiring that
//! drives launches, captures, cargo and staged destruction from the tick
//! pass.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stages `### F35-B` and `### F35-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
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
//! current pose as a zero-velocity wreck and stays that pose through its
//! staged destruction phase (F35-C) until it despawns.
//!
//! # The F35-C pass
//!
//! One tick pass is the single writer of the wiring in
//! [`wiring::ShipWiring`], and it runs in this order: move the ships, release
//! or cancel their launches, advance the destruction countdowns. Because the
//! release pass is the only path that turns a [`LaunchId`] into an aircraft,
//! a launch resolves exactly once — destroyed bay, destroyed carrier or not
//! — which is what AC03 measures end to end. The discrete commands
//! ([`Self::schedule_launch`], [`Self::release_launch`], [`Self::begin_capture`],
//! [`Self::advance_capture`], [`Self::load_cargo`], [`Self::unload_cargo`],
//! [`Self::despawn_ship`]) return their own result and never leave a
//! half-applied state, so a caller that ordered them knows the outcome
//! without reading the event stream.
//!
//! All of this is designed behavior; no original capital-ship coefficient or
//! rule is measured. See
//! `docs/findings/2026-10-04-f35-b-movement-weakpoints-and-turrets.md` and
//! `docs/findings/2026-10-04-f35-c-launch-capture-cargo-destruction.md`.

use std::collections::BTreeMap;

use cs_script::ir::ActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};

use super::bay::BayState;
use super::capture::{
    CaptureRefusal, CaptureStage, CaptureTicket, CaptureTransaction, ShipControl,
};
use super::launch::release_aircraft;
use super::launch::{LaunchId, LaunchRefusal, LaunchSocket, PendingLaunch, ReleasedAircraft};
use super::motion::PropulsionError;
use super::parts::LaunchBayRig;
use super::ship::{CapitalShip, HitError, HitOutcome, TurretAim, TurretRefusal};
use super::subsystem::{DisableOutcome, SubsystemGraphError, SubsystemKey};
use super::wiring::{
    CaptureProgress, CargoRefusal, DespawnPolicy, DestructionState, LaunchCancelReason,
    LaunchCancellation, LaunchStatus, ShipWiring,
};
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
#[derive(Clone, Debug, PartialEq)]
pub enum CapitalShipEvent {
    /// A ship's drive reached the end of its authored course; it holds the
    /// final pose from that tick on.
    CourseCompleted {
        /// The arrived ship.
        actor: ActorId,
        /// The tick it arrived.
        at: Tick,
    },
    /// A scheduled launch was released exactly once. The
    /// [`ReleasedAircraft`] is the authoritative spawn state: the socket
    /// transform sampled from the carrier's committed pose, the carrier's
    /// motion plus the ejection, and dynamic authority granted here and
    /// nowhere else. This is the order a consumer spawns from — the runtime
    /// does not create the actor itself.
    LaunchReleased {
        /// The carrier the aircraft left.
        carrier: ActorId,
        /// The launch id that produced it.
        id: LaunchId,
        /// The spawn state.
        aircraft: ReleasedAircraft,
        /// The tick the release happened on.
        at: Tick,
    },
    /// A pending launch will never spawn. It was removed from the ledger, so
    /// no later pass can release it and no duplicate aircraft can appear.
    LaunchCancelled {
        /// The carrier whose launch was cancelled.
        carrier: ActorId,
        /// The launch id that will never run.
        id: LaunchId,
        /// Why it was cancelled.
        reason: LaunchCancelReason,
        /// The tick it was cancelled on.
        at: Tick,
    },
    /// The staged destruction phase ended and the ship left the world. Its
    /// record is closed from this tick on.
    Despawned {
        /// The despawned ship.
        actor: ActorId,
        /// The tick it despawned on.
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
    /// A launch was refused.
    Launch(LaunchRefusal),
    /// A cargo operation was refused.
    Cargo(CargoRefusal),
    /// A capture operation was refused.
    Capture(CaptureRefusal),
    /// The ship despawned and its record is closed: no new order may name it.
    ShipDespawned {
        /// The despawned ship.
        actor: ActorId,
        /// The tick it despawned on.
        at: Tick,
    },
    /// A live ship was asked to despawn. Only a destroyed ship leaves the
    /// world through [`Self::despawn_ship`]: removing an intact ship is a
    /// mission removal, a different transition this runtime does not own.
    NotDestroyed {
        /// The ship that is still intact.
        actor: ActorId,
    },
    /// The ticket's attempt ordinal is not the ship's current capture attempt.
    StaleCaptureTicket {
        /// The ship the ticket names.
        actor: ActorId,
        /// The attempt the presented ticket carries.
        ticket: u64,
        /// The attempt the ship actually holds, or zero when it holds none.
        current: u64,
    },
    /// The ship holds no capture attempt at all, so there is nothing to
    /// advance or abort.
    NoCapture {
        /// The ship named by the ticket.
        actor: ActorId,
    },
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
            Self::Launch(source) => write!(f, "the launch refused: {source}"),
            Self::Cargo(source) => write!(f, "the cargo hold refused: {source}"),
            Self::Capture(source) => write!(f, "the capture refused: {source}"),
            Self::ShipDespawned { actor, at } => {
                write!(
                    f,
                    "ship {actor:?} despawned at tick {at:?}; its record is closed"
                )
            }
            Self::NotDestroyed { actor } => write!(
                f,
                "ship {actor:?} is intact: only a destroyed ship despawns"
            ),
            Self::StaleCaptureTicket {
                actor,
                ticket,
                current,
            } => write!(
                f,
                "capture ticket {ticket} for ship {actor:?} is stale; attempt {current} holds it"
            ),
            Self::NoCapture { actor } => write!(f, "ship {actor:?} holds no capture attempt"),
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

impl From<LaunchRefusal> for CapitalRuntimeError {
    fn from(source: LaunchRefusal) -> Self {
        Self::Launch(source)
    }
}

impl From<CargoRefusal> for CapitalRuntimeError {
    fn from(source: CargoRefusal) -> Self {
        Self::Cargo(source)
    }
}

impl From<CaptureRefusal> for CapitalRuntimeError {
    fn from(source: CaptureRefusal) -> Self {
        Self::Capture(source)
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
    /// The F35-C launch, cargo, capture and destruction wiring.
    wiring: ShipWiring,
}

/// One session's capital ships: registry, course stepping and hit/turret
/// operations.
#[derive(Clone, Debug, PartialEq)]
pub struct CapitalShipSet {
    ticks_per_second: u32,
    tick: Tick,
    ships: BTreeMap<ActorId, ShipState>,
    /// How long a destroyed ship stays in the world before it despawns.
    despawn: DespawnPolicy,
    /// The next actor id handed to a released aircraft, as a `u64` so a ship
    /// registered at the top of the `u32` id space pushes the allocator past
    /// it instead of colliding with it. It only ever increases: an id is never
    /// reused within a session, not even after a teardown.
    next_aircraft: u64,
}

impl CapitalShipSet {
    /// An empty set at [`Tick`] 0 stepping `ticks_per_second` fixed ticks
    /// per second, holding every destroyed ship until the mission despawns it
    /// (see [`Self::with_despawn_policy`]).
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::ZeroTickRate`].
    pub fn new(ticks_per_second: u32) -> Result<Self, CapitalRuntimeError> {
        Self::with_despawn_policy(ticks_per_second, DespawnPolicy::HOLD)
    }

    /// An empty set whose destroyed ships despawn `despawn` after destruction.
    ///
    /// This is the F35-C staged-destruction switch: destruction freezes the
    /// wreck and starts the phase, despawn closes the record. Holding the
    /// wreck (`DespawnPolicy::HOLD`) is the default because no original
    /// destruction timing is measured.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::ZeroTickRate`].
    pub fn with_despawn_policy(
        ticks_per_second: u32,
        despawn: DespawnPolicy,
    ) -> Result<Self, CapitalRuntimeError> {
        if ticks_per_second == 0 {
            return Err(CapitalRuntimeError::ZeroTickRate);
        }
        Ok(Self {
            ticks_per_second,
            tick: Tick(0),
            ships: BTreeMap::new(),
            despawn,
            next_aircraft: 0,
        })
    }

    /// The despawn policy this set applies to its wrecks.
    #[must_use]
    pub const fn despawn_policy(&self) -> DespawnPolicy {
        self.despawn
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
        // A registered ship's id must never collide with one the set later
        // hands to a released aircraft, so the allocator starts above it.
        let next = u64::from(ship.actor().0) + 1;
        self.next_aircraft = self.next_aircraft.max(next);
        let despawn = self.despawn;
        let mut wiring = ShipWiring::new(
            &ship.ownership().owner,
            ship.docking_open(),
            ship.cargo().clone(),
        );
        if ship.is_destroyed() {
            // A ship registered as a wreck is already in its staged phase at
            // tick 0; it is not a live ship that happens to be damaged.
            wiring.destruction.destroy(Tick(0), despawn);
            wiring.control.docking_open = false;
        }
        self.ships.insert(
            actor,
            ShipState {
                ship,
                drive_ticks,
                pose,
                wreck,
                course_done: false,
                wiring,
            },
        );
        Ok(())
    }

    /// The canonical pose of `actor` at the set's current tick — the wreck
    /// pose once the ship is destroyed.
    ///
    /// A despawned ship keeps answering this one: it is a historical record of
    /// where the ship left the world, which is what a diagnostic needs. Every
    /// other query and command on a closed record refuses with
    /// [`CapitalRuntimeError::ShipDespawned`].
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
    /// After the motion, the F35-C pass releases every launch whose bay is
    /// open and whose ready tick arrived — allocating a fresh actor id per
    /// release — cancels the launches of a destroyed bay or a dying carrier,
    /// and despawns the wrecks whose staged phase ended.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::TickOverflow`],
    /// [`CapitalRuntimeError::Propulsion`] or, when the session has no actor
    /// ids left for the aircraft this tick would release,
    /// [`CapitalRuntimeError::Launch`] with
    /// [`LaunchRefusal::ActorIdExhausted`]. That last refusal commits the
    /// tick's motion — the pass runs last — but spends no id and resolves no
    /// launch, so a later tick still finds the launches waiting.
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
        // The launch pass runs after the motion so a release samples the pose
        // this tick committed, never the previous tick's.
        events.extend(self.run_launch_pass()?);
        events.extend(self.run_destruction_pass());
        Ok(events)
    }

    /// Resolves every waiting launch exactly once through its ledger, without
    /// creating an aircraft yet.
    ///
    /// A destroyed carrier releases nothing, so its waiting launches are
    /// cancelled; a despawned carrier's record is closed. A live carrier
    /// defers to the F35-A ledger, which releases each id at most once and
    /// cancels the pending launches of a destroyed bay.
    fn resolve_launch_pass(&mut self) -> Vec<LaunchPlan> {
        let at = self.tick;
        let mut plans = Vec::new();
        for (carrier, state) in &mut self.ships {
            if state.wiring.ledger.pending().next().is_none() {
                continue;
            }
            let carrier = *carrier;
            let dead = match state.wiring.destruction {
                DestructionState::Live => None,
                DestructionState::Sinking { .. } => Some(LaunchCancelReason::ShipDestroyed),
                DestructionState::Despawned { .. } => Some(LaunchCancelReason::ShipDespawned),
            };
            let ShipState {
                ship,
                pose,
                wreck,
                wiring,
                ..
            } = state;
            // A dying carrier's bays are unusable even though their
            // subsystems are intact, so their waiting launches are cancelled
            // rather than released from a wreck.
            let bay_state = |bay: &SubsystemKey| match dead {
                Some(_) => BayState::Destroyed,
                None => ship.bay_state(bay, at).unwrap_or(BayState::Concealed),
            };
            let passed = wiring.ledger.release_ready(at, bay_state);
            for launch in passed.cancelled {
                let reason = dead.unwrap_or(LaunchCancelReason::BayDestroyed);
                plans.push(LaunchPlan::Cancel {
                    carrier,
                    id: launch.id.clone(),
                    launch,
                    reason,
                });
            }
            for launch in passed.released {
                // The socket was validated when the launch was scheduled and
                // the rig never changes afterwards, so a release always has
                // one: the launch cannot be scheduled without it.
                let socket = launch_socket(ship, &launch.id.bay).expect(
                    "schedule_launch refuses a bay with no launch rig or an unknown socket",
                );
                let eject_m_s = wiring
                    .ejections
                    .get(&launch.id)
                    .copied()
                    .expect("schedule_launch records every ejection it accepts");
                plans.push(LaunchPlan::Release {
                    carrier,
                    id: launch.id.clone(),
                    socket,
                    pose: wreck.unwrap_or(*pose),
                    eject_m_s,
                });
            }
        }
        plans
    }

    /// Commits one tick's resolved launches: every planned release becomes
    /// exactly one aircraft carrying a freshly allocated session id.
    fn run_launch_pass(&mut self) -> Result<Vec<CapitalShipEvent>, CapitalRuntimeError> {
        // Refuse before anything is resolved when the session cannot supply an
        // id for every launch that could run: resolving a launch and then
        // failing for an id would leave the id resolved with no aircraft behind
        // it. The count is an upper bound, so the reservation below cannot fail
        // once this has passed.
        let waiting: usize = self
            .ships
            .values()
            .map(|state| state.wiring.ledger.pending().count())
            .sum();
        self.ensure_aircraft_capacity(waiting)?;
        let plans = self.resolve_launch_pass();
        let releases = plans
            .iter()
            .filter(|plan| matches!(plan, LaunchPlan::Release { .. }))
            .count();
        // The block is reserved in one go, so an exhausted space cannot release
        // half the launches. A tick with nothing to release needs no id.
        let base = if releases > 0 {
            self.reserve_aircraft(releases)?
        } else {
            0
        };
        let at = self.tick;
        let mut next = base;
        let mut events = Vec::new();
        for plan in plans {
            match plan {
                LaunchPlan::Release {
                    carrier,
                    id,
                    socket,
                    pose,
                    eject_m_s,
                } => {
                    let aircraft = release_aircraft(at, &pose, &socket, ActorId(next), eject_m_s);
                    next += 1;
                    let wiring = &mut self
                        .ships
                        .get_mut(&carrier)
                        .expect("the registry cannot change mid-step")
                        .wiring;
                    wiring.released.insert(id.clone(), aircraft);
                    wiring.ejections.remove(&id);
                    events.push(CapitalShipEvent::LaunchReleased {
                        carrier,
                        id,
                        aircraft,
                        at,
                    });
                }
                LaunchPlan::Cancel {
                    carrier,
                    id,
                    launch,
                    reason,
                } => {
                    let wiring = &mut self
                        .ships
                        .get_mut(&carrier)
                        .expect("the registry cannot change mid-step")
                        .wiring;
                    wiring
                        .cancelled
                        .insert(id.clone(), LaunchCancellation { launch, reason, at });
                    wiring.ejections.remove(&id);
                    events.push(CapitalShipEvent::LaunchCancelled {
                        carrier,
                        id,
                        reason,
                        at,
                    });
                }
            }
        }
        Ok(events)
    }

    /// Whether the session could supply `count` ids right now.
    ///
    /// A session whose ships have consumed the `u32` actor id space cannot
    /// release anything: it refuses instead of wrapping onto an id that already
    /// names something. Because the check is an upper bound on the tick's
    /// releases, such a session refuses every tick while a launch waits — a
    /// loud refusal naming the cause, never a launch resolved with no aircraft
    /// behind it.
    fn ensure_aircraft_capacity(&self, count: usize) -> Result<(), CapitalRuntimeError> {
        let count = u64::try_from(count).map_err(|_| LaunchRefusal::ActorIdExhausted)?;
        let end = self
            .next_aircraft
            .checked_add(count)
            .ok_or(LaunchRefusal::ActorIdExhausted)?;
        u32::try_from(end).map_err(|_| LaunchRefusal::ActorIdExhausted)?;
        Ok(())
    }

    /// Reserves `count` session-fresh actor ids and returns the first. The
    /// block must fit in the `u32` actor id space; nothing is reserved on
    /// failure.
    fn reserve_aircraft(&mut self, count: usize) -> Result<u32, CapitalRuntimeError> {
        let count = u64::try_from(count).map_err(|_| LaunchRefusal::ActorIdExhausted)?;
        let end = self
            .next_aircraft
            .checked_add(count)
            .ok_or(LaunchRefusal::ActorIdExhausted)?;
        let base =
            u32::try_from(self.next_aircraft).map_err(|_| LaunchRefusal::ActorIdExhausted)?;
        u32::try_from(end).map_err(|_| LaunchRefusal::ActorIdExhausted)?;
        self.next_aircraft = end;
        Ok(base)
    }

    /// Advances every wreck's staged destruction phase to the committed tick.
    fn run_destruction_pass(&mut self) -> Vec<CapitalShipEvent> {
        let at = self.tick;
        let mut events = Vec::new();
        for (actor, state) in &mut self.ships {
            if state.wiring.destruction.advance(at) {
                teardown(&mut state.wiring);
                events.push(CapitalShipEvent::Despawned { actor: *actor, at });
            }
        }
        events
    }

    /// Advances the set to `to`, one committed tick at a time, and returns
    /// every event the steps produced.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::NonMonotonicTick`], [`TickOverflow`](CapitalRuntimeError::TickOverflow),
    /// [`CapitalRuntimeError::Propulsion`] or
    /// [`CapitalRuntimeError::Launch`] with
    /// [`LaunchRefusal::ActorIdExhausted`].
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
    /// later step, and starts the staged destruction phase.
    ///
    /// A despawned ship is closed: it takes no further hits, so a projectile
    /// that outlived its target cannot damage a world that no longer holds it.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`], [`ForeignTick`](CapitalRuntimeError::ForeignTick),
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned) or
    /// [`CapitalRuntimeError::Hit`].
    pub fn apply_hit(&mut self, hit: &CapitalHit) -> Result<HitOutcome, CapitalRuntimeError> {
        if hit.at != self.tick {
            return Err(CapitalRuntimeError::ForeignTick {
                expected: self.tick,
                found: hit.at,
            });
        }
        let despawn = self.despawn;
        let state = self
            .ships
            .get_mut(&hit.target)
            .ok_or(CapitalRuntimeError::UnknownShip(hit.target))?;
        refuse_despawned(&state.wiring.destruction, hit.target)?;
        let outcome = state.ship.apply_hit(&hit.subsystem, hit.damage, hit.at)?;
        stage_destruction(state, hit.at, despawn);
        refresh_docking(state);
        Ok(outcome)
    }

    /// Disables one subsystem through the graph — the scripted destruction
    /// path, equivalent to a landed hit's effect but not gated by exposure
    /// or damage. Returns what the disable applied.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`],
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned) or
    /// [`CapitalRuntimeError::Subsystem`].
    pub fn disable(
        &mut self,
        actor: ActorId,
        key: &SubsystemKey,
    ) -> Result<DisableOutcome, CapitalRuntimeError> {
        let despawn = self.despawn;
        let at = self.tick;
        let state = self
            .ships
            .get_mut(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?;
        refuse_despawned(&state.wiring.destruction, actor)?;
        let outcome = state.ship.disable(key)?;
        stage_destruction(state, at, despawn);
        refresh_docking(state);
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

    /// Where the ship is in the destruction/despawn lifecycle.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`].
    pub fn destruction(&self, actor: ActorId) -> Result<DestructionState, CapitalRuntimeError> {
        Ok(self
            .ships
            .get(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?
            .wiring
            .destruction)
    }

    /// Whether the ship has left the world: its record is closed and every new
    /// order names nothing.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`].
    pub fn is_despawned(&self, actor: ActorId) -> Result<bool, CapitalRuntimeError> {
        Ok(self
            .ships
            .get(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?
            .wiring
            .destruction
            .is_despawned())
    }

    /// Closes a destroyed ship's record at the committed tick: the F35-C
    /// despawn, whether [`DespawnPolicy`] asked for it or the mission orders
    /// it here. The staged phase and despawn are separate transitions, so only
    /// an already-destroyed ship may be despawned — a live ship never leaves
    /// the world through this door, and the tick pass reports the automatic
    /// despawn as a [`CapitalShipEvent::Despawned`].
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`],
    /// [`NotDestroyed`](CapitalRuntimeError::NotDestroyed) or
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned) when the ship is
    /// already gone.
    pub fn despawn_ship(&mut self, actor: ActorId) -> Result<(), CapitalRuntimeError> {
        let at = self.tick;
        let state = self
            .ships
            .get_mut(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?;
        if let DestructionState::Despawned { at: gone } = state.wiring.destruction {
            return Err(CapitalRuntimeError::ShipDespawned { actor, at: gone });
        }
        if state.wiring.destruction.is_live() {
            return Err(CapitalRuntimeError::NotDestroyed { actor });
        }
        state.wiring.destruction.despawn_now(at);
        teardown(&mut state.wiring);
        Ok(())
    }

    /// Who the ship's guns, AI, targeting and docking act for.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned): a closed record
    /// answers lifecycle questions only.
    pub fn control(&self, actor: ActorId) -> Result<ShipControl, CapitalRuntimeError> {
        let state = self
            .ships
            .get(&actor)
            .ok_or(CapitalRuntimeError::UnknownShip(actor))?;
        refuse_despawned(&state.wiring.destruction, actor)?;
        Ok(state.wiring.control.clone())
    }

    /// The launches still waiting aboard `carrier`.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned).
    pub fn pending_launches(
        &self,
        carrier: ActorId,
    ) -> Result<Vec<PendingLaunch>, CapitalRuntimeError> {
        let state = self
            .ships
            .get(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        refuse_despawned(&state.wiring.destruction, carrier)?;
        Ok(state.wiring.ledger.pending().cloned().collect())
    }

    /// How one launch id resolved: still waiting, released into exactly one
    /// aircraft, or cancelled with its reason. `Ok(None)` when the ledger never
    /// issued the id.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned).
    pub fn launch_status(
        &self,
        carrier: ActorId,
        id: &LaunchId,
    ) -> Result<Option<LaunchStatus>, CapitalRuntimeError> {
        let state = self
            .ships
            .get(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        refuse_despawned(&state.wiring.destruction, carrier)?;
        Ok(state.wiring.status(id))
    }

    /// Schedules `aircraft` to leave `carrier`'s `bay` at or after
    /// `ready_tick`, pushed out of the socket along `eject_m_s`.
    ///
    /// The bay's declared wiring is validated first: a weapon bay, a launch bay
    /// with no rig or an unresolved release socket refuses by claim, and a bay
    /// whose declared capacity is unresolved refuses rather than becoming an
    /// unbounded hangar. A known capacity bounds the launches *waiting* aboard
    /// it — a released aircraft has left and freed its slot.
    ///
    /// Ids are assigned per bay and never reused, so a cancelled launch can
    /// never be confused with a later one.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`], [`ShipDespawned`](CapitalRuntimeError::ShipDespawned)
    /// or [`CapitalRuntimeError::Launch`] naming the refusal.
    pub fn schedule_launch(
        &mut self,
        carrier: ActorId,
        bay: &SubsystemKey,
        aircraft: ContentId,
        ready_tick: Tick,
        eject_m_s: [f64; 3],
    ) -> Result<LaunchId, CapitalRuntimeError> {
        if !eject_m_s.iter().all(|value| value.is_finite()) {
            return Err(LaunchRefusal::NonFiniteEjection.into());
        }
        let state = self
            .ships
            .get_mut(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        refuse_despawned(&state.wiring.destruction, carrier)?;
        if state.wiring.destruction.is_destroyed() {
            return Err(LaunchRefusal::ShipDestroyed { carrier }.into());
        }
        let ShipState { ship, wiring, .. } = state;
        let rig = ship.launch_rig(bay).ok_or(LaunchRefusal::UnknownBay)?;
        match &rig.offset_m {
            Resolved::Known(_) => {}
            Resolved::Unknown { claim_id, reason } => {
                return Err(LaunchRefusal::SocketUnknown {
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                }
                .into());
            }
        }
        if ship.bay_state(bay, self.tick) == Some(BayState::Destroyed) {
            return Err(LaunchRefusal::BayDestroyed.into());
        }
        let capacity = match &rig.capacity {
            Resolved::Known(known) => known.value,
            Resolved::Unknown { claim_id, reason } => {
                return Err(LaunchRefusal::CapacityUnknown {
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                }
                .into());
            }
        };
        let pending = wiring.pending_in(bay);
        if pending >= capacity {
            return Err(LaunchRefusal::BayFull { capacity, pending }.into());
        }
        let id = wiring.ledger.schedule(bay.clone(), aircraft, ready_tick);
        wiring.ejections.insert(id.clone(), eject_m_s);
        Ok(id)
    }

    /// Releases `id` now instead of waiting for the tick pass.
    ///
    /// This is the retry door: a launch whose bay was shut refuses with the
    /// reason (`NotOpen`, `NotReady`) and stays pending, so calling it again on
    /// a later tick is the retry. An id that already produced its aircraft
    /// refuses [`LaunchRefusal::AlreadyReleased`], and a cancelled one
    /// [`LaunchRefusal::Cancelled`] — neither can ever spawn a second aircraft.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`], [`ShipDespawned`](CapitalRuntimeError::ShipDespawned)
    /// or [`CapitalRuntimeError::Launch`] naming the refusal, including
    /// [`LaunchRefusal::ActorIdExhausted`] when the session has no id left.
    pub fn release_launch(
        &mut self,
        carrier: ActorId,
        id: &LaunchId,
    ) -> Result<ReleasedAircraft, CapitalRuntimeError> {
        let at = self.tick;
        // Decide and resolve inside the ship's own borrow, then spend an id
        // once the borrow is gone: the launch resolves exactly once whether
        // the tick pass or this call did it.
        let (socket, pose, eject_m_s) = {
            let state = self
                .ships
                .get_mut(&carrier)
                .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
            refuse_despawned(&state.wiring.destruction, carrier)?;
            if state.wiring.destruction.is_destroyed() {
                return Err(LaunchRefusal::ShipDestroyed { carrier }.into());
            }
            let ShipState {
                ship,
                pose,
                wreck,
                wiring,
                ..
            } = state;
            match wiring.status(id) {
                Some(LaunchStatus::Released { .. }) => {
                    return Err(LaunchRefusal::AlreadyReleased.into());
                }
                Some(LaunchStatus::Cancelled { .. }) => return Err(LaunchRefusal::Cancelled.into()),
                Some(LaunchStatus::Pending { .. }) | None => {}
            }
            wiring.ledger.release_one(id, at, |bay| {
                ship.bay_state(bay, at).unwrap_or(BayState::Concealed)
            })?;
            let socket = launch_socket(ship, &id.bay)
                .expect("a pending launch was scheduled against a bay with a known socket");
            let eject_m_s = wiring
                .ejections
                .get(id)
                .copied()
                .expect("schedule_launch records every ejection it accepts");
            (socket, wreck.unwrap_or(*pose), eject_m_s)
        };
        let actor = self.allocate_one_aircraft()?;
        let aircraft = release_aircraft(at, &pose, &socket, actor, eject_m_s);
        let wiring = &mut self
            .ships
            .get_mut(&carrier)
            .expect("the registry cannot change mid-command")
            .wiring;
        wiring.released.insert(id.clone(), aircraft);
        wiring.ejections.remove(id);
        Ok(aircraft)
    }

    /// Hands out one session-fresh actor id for a commanded release.
    fn allocate_one_aircraft(&mut self) -> Result<ActorId, CapitalRuntimeError> {
        let actor = self.reserve_aircraft(1)?;
        Ok(ActorId(actor))
    }

    /// What the ship holds.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned).
    pub fn cargo_load(&self, carrier: ActorId) -> Result<f64, CapitalRuntimeError> {
        let state = self
            .ships
            .get(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        refuse_despawned(&state.wiring.destruction, carrier)?;
        Ok(state.wiring.cargo.loaded())
    }

    /// The ship's declared cargo capacity, verbatim.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned).
    pub fn cargo_capacity(&self, carrier: ActorId) -> Result<&Resolved<f64>, CapitalRuntimeError> {
        let state = self
            .ships
            .get(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        refuse_despawned(&state.wiring.destruction, carrier)?;
        Ok(state.wiring.cargo.capacity())
    }

    /// Loads `units` of cargo and returns the new load.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`],
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned) or
    /// [`CapitalRuntimeError::Cargo`] naming the refusal.
    pub fn load_cargo(&mut self, carrier: ActorId, units: f64) -> Result<f64, CapitalRuntimeError> {
        let state = self
            .ships
            .get_mut(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        refuse_despawned(&state.wiring.destruction, carrier)?;
        if state.wiring.destruction.is_destroyed() {
            return Err(CargoRefusal::ShipDestroyed { actor: carrier }.into());
        }
        state.wiring.cargo.load(units).map_err(Into::into)
    }

    /// Unloads `units` of cargo and returns the new load. A dying ship may
    /// still be emptied — unloading reads state that exists, where a load
    /// would add state to a wreck.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`],
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned) or
    /// [`CapitalRuntimeError::Cargo`] naming the refusal.
    pub fn unload_cargo(
        &mut self,
        carrier: ActorId,
        units: f64,
    ) -> Result<f64, CapitalRuntimeError> {
        let state = self
            .ships
            .get_mut(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        refuse_despawned(&state.wiring.destruction, carrier)?;
        state.wiring.cargo.unload(units).map_err(Into::into)
    }

    /// Begins a capture attempt and returns the ticket that qualifies it.
    ///
    /// The attempt records the ship's *current* owner, so a later commit
    /// refuses if ownership moved underneath it. Only one attempt may hold a
    /// ship: a second claimant is refused rather than racing the first to a
    /// commit. Authorization to capture the ship at all is the mission's, not
    /// this runtime's — the caller supplies the owner it is authorized to take.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or [`CapitalRuntimeError::Capture`]
    /// naming the refusal.
    pub fn begin_capture(
        &mut self,
        session: u64,
        carrier: ActorId,
        claimant: ActorId,
        owner_after: ContentId,
    ) -> Result<CaptureTicket, CapitalRuntimeError> {
        let state = self
            .ships
            .get_mut(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        if let Some(progress) = &state.wiring.capture {
            return Err(CaptureRefusal::InProgress {
                ship: carrier,
                claimant: progress.ticket.claimant,
                stage: progress.transaction.stage(),
            }
            .into());
        }
        refuse_despawned(&state.wiring.destruction, carrier)?;
        if state.wiring.destruction.is_destroyed() {
            return Err(CaptureRefusal::ShipDestroyed { ship: carrier }.into());
        }
        let owner_before = state.ship.ownership().owner.clone();
        state.wiring.attempts = state.wiring.attempts.saturating_add(1);
        let attempt = state.wiring.attempts;
        let ticket = CaptureTicket {
            ship: carrier,
            claimant,
            session,
            attempt,
        };
        let transaction =
            CaptureTransaction::begin(session, carrier, claimant, owner_before, owner_after);
        state.wiring.capture = Some(CaptureProgress {
            ticket,
            transaction,
        });
        Ok(ticket)
    }

    /// The capture attempt `carrier` holds, if any.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`ShipDespawned`](CapitalRuntimeError::ShipDespawned).
    pub fn capture(
        &self,
        carrier: ActorId,
    ) -> Result<Option<CaptureProgress>, CapitalRuntimeError> {
        let state = self
            .ships
            .get(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        refuse_despawned(&state.wiring.destruction, carrier)?;
        Ok(state.wiring.capture.clone())
    }

    /// The stage `carrier`'s current attempt reached.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`] or
    /// [`CapitalRuntimeError::NoCapture`].
    pub fn capture_stage(&self, carrier: ActorId) -> Result<CaptureStage, CapitalRuntimeError> {
        self.ships
            .get(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?
            .wiring
            .capture
            .as_ref()
            .map(|progress| progress.transaction.stage())
            .ok_or(CapitalRuntimeError::NoCapture { actor: carrier })
    }

    /// Advances `ticket`'s capture one stage.
    ///
    /// The latch puts the dedicated control owner in while the guns stay with
    /// the previous owner; the completing stage switches ownership, guns,
    /// targeting and the AI relation in the same step, so no consumer ever
    /// reads a new owner beside the old owner's guns. A ticket that is not the
    /// ship's current attempt, a session that is not the attempt's own, and a
    /// ship whose owner moved underneath the attempt all commit nothing.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`],
    /// [`StaleCaptureTicket`](CapitalRuntimeError::StaleCaptureTicket),
    /// [`NoCapture`](CapitalRuntimeError::NoCapture) or
    /// [`CapitalRuntimeError::Capture`] naming the refusal.
    pub fn advance_capture(
        &mut self,
        ticket: &CaptureTicket,
    ) -> Result<CaptureStage, CapitalRuntimeError> {
        let carrier = ticket.ship;
        let state = self
            .ships
            .get_mut(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        // Validate and advance inside the attempt's own borrow, taking only
        // owned data out: the commit below mutates the ship beside it.
        let (stage, ownership, owner_after) = {
            // The owner is read before the attempt is borrowed, so the commit
            // below can mutate the ship beside it.
            let actual = state.ship.ownership().owner.clone();
            let progress = current_capture(state, ticket)?;
            let expected = progress.transaction.owner_before().clone();
            if actual != expected {
                return Err(CaptureRefusal::OwnershipChanged {
                    ship: carrier,
                    expected,
                    actual,
                }
                .into());
            }
            let stage = progress
                .transaction
                .advance()
                .map_err(CapitalRuntimeError::from)?;
            let ownership = progress.transaction.ownership();
            let owner_after = progress.transaction.owner_after().clone();
            (stage, ownership, owner_after)
        };
        let Some(ownership) = ownership else {
            // Not the completing stage: the latch hands the controls to the
            // dedicated owner while the ship is still armed for the old one.
            if stage == CaptureStage::Latching {
                state.wiring.control.control_owner = owner_after;
            }
            return Ok(stage);
        };
        // Commit: ownership first, then every consumer of it, in one step.
        state.ship.adopt_ownership(ownership);
        let owner = state.ship.ownership().owner.clone();
        state.wiring.control.control_owner.clone_from(&owner);
        state.wiring.control.guns_owner.clone_from(&owner);
        // A finished attempt holds the ship no longer: a retry begins a new
        // one with a fresh ticket, and the old ticket can never commit again.
        state.wiring.capture = None;
        Ok(stage)
    }

    /// Aborts `ticket`'s capture, freeing the ship for a retry.
    ///
    /// Ownership never moves on an abort, and the freed slot is what makes the
    /// retry possible: a second attempt gets a new attempt ordinal, so the
    /// aborted ticket stays stale forever.
    ///
    /// # Errors
    ///
    /// [`CapitalRuntimeError::UnknownShip`],
    /// [`StaleCaptureTicket`](CapitalRuntimeError::StaleCaptureTicket),
    /// [`NoCapture`](CapitalRuntimeError::NoCapture) or
    /// [`CapitalRuntimeError::Capture`] naming the refusal.
    pub fn abort_capture(
        &mut self,
        ticket: &CaptureTicket,
    ) -> Result<CaptureStage, CapitalRuntimeError> {
        let carrier = ticket.ship;
        let state = self
            .ships
            .get_mut(&carrier)
            .ok_or(CapitalRuntimeError::UnknownShip(carrier))?;
        let progress = current_capture(state, ticket)?;
        let stage = progress
            .transaction
            .abort()
            .map_err(CapitalRuntimeError::from)?;
        state.wiring.capture = None;
        Ok(stage)
    }
}

/// What one tick's launch pass resolved for a single waiting launch.
#[derive(Clone, Debug, PartialEq)]
enum LaunchPlan {
    /// The launch will spawn: everything its spawn state needs, decided before
    /// any id is spent.
    Release {
        /// The carrier.
        carrier: ActorId,
        /// The launch id.
        id: LaunchId,
        /// The bay's release socket.
        socket: LaunchSocket,
        /// The carrier pose this tick committed.
        pose: Pose,
        /// The ejection the mission scheduled it with.
        eject_m_s: [f64; 3],
    },
    /// The launch will never spawn.
    Cancel {
        /// The carrier.
        carrier: ActorId,
        /// The launch id.
        id: LaunchId,
        /// The launch as it was resolved out of the ledger.
        launch: PendingLaunch,
        /// Why it can never spawn.
        reason: LaunchCancelReason,
    },
}

/// The release socket of `bay`, or `None` when the bay carries no launch rig.
fn launch_socket(ship: &CapitalShip, bay: &SubsystemKey) -> Option<LaunchSocket> {
    let rig: &LaunchBayRig = ship.launch_rig(bay)?;
    let offset_m = match &rig.offset_m {
        Resolved::Known(known) => known.value,
        // Scheduling refuses an unresolved socket, so no release reaches here
        // with one; the caller reports the unknown rather than spawning at an
        // invented transform.
        Resolved::Unknown { .. } => return None,
    };
    Some(LaunchSocket {
        actor: ship.actor(),
        // One declared bay is one declared socket (F35-C designed shape).
        socket: 0,
        offset_m,
    })
}

/// Refuses any order that names a ship that has left the world.
fn refuse_despawned(
    destruction: &DestructionState,
    actor: ActorId,
) -> Result<(), CapitalRuntimeError> {
    if let DestructionState::Despawned { at } = *destruction {
        return Err(CapitalRuntimeError::ShipDespawned { actor, at });
    }
    Ok(())
}

/// The F35-C destruction transition: the first time a lethal subsystem destroys
/// the ship, freeze its pose, start the staged phase and tear down the work
/// that cannot survive it.
fn stage_destruction(state: &mut ShipState, at: Tick, policy: DespawnPolicy) {
    if !state.ship.is_destroyed() || state.wiring.destruction.is_destroyed() {
        return;
    }
    if state.wreck.is_none() {
        state.wreck = Some(zeroed(state.pose));
    }
    state.wiring.destruction.destroy(at, policy);
    // An in-flight capture cannot complete on a wreck: abort it rather than
    // leave a transaction that would commit ownership for a dead ship.
    if let Some(mut progress) = state.wiring.capture.take() {
        let _ = progress.transaction.abort();
    }
}

/// Re-reads the docking gate from the ship's own anchors and its lifecycle, so
/// a destroyed anchor closes boarding eligibility the moment it is destroyed
/// instead of leaving a stale open gate behind. Boarding needs a live ship
/// with an intact anchor: a wreck cannot be boarded even if its anchors hold.
fn refresh_docking(state: &mut ShipState) {
    state.wiring.control.docking_open =
        state.ship.docking_open() && state.wiring.destruction.is_live();
}

/// Closes a ship's wiring when it despawns: nothing is left to hold or try.
fn teardown(wiring: &mut ShipWiring) {
    wiring.capture = None;
    wiring.cargo.clear();
    wiring.control.docking_open = false;
}

/// The attempt `ticket` names, refusing any ticket that is not the ship's
/// current one.
fn current_capture<'a>(
    state: &'a mut ShipState,
    ticket: &CaptureTicket,
) -> Result<&'a mut CaptureProgress, CapitalRuntimeError> {
    let progress = state
        .wiring
        .capture
        .as_mut()
        .ok_or(CapitalRuntimeError::NoCapture { actor: ticket.ship })?;
    if progress.ticket.attempt != ticket.attempt {
        return Err(CapitalRuntimeError::StaleCaptureTicket {
            actor: ticket.ship,
            ticket: ticket.attempt,
            current: progress.ticket.attempt,
        });
    }
    if progress.ticket.session != ticket.session {
        return Err(CaptureRefusal::ForeignSession {
            ship: ticket.ship,
            ticket: ticket.session,
            current: progress.ticket.session,
        }
        .into());
    }
    Ok(progress)
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
