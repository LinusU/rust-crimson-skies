//! The F34-B world-actor runtime: one per-session set of catalog-driven
//! rail, road, water and kinematic actors that integer ticks move, closed
//! gates hold and the support graph destroys.
//!
//! Spec: `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`,
//! stage `### F34-B`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`WorldActorSet`] is the production path the contract types were built
//! for. It owns the session's [`SupportGraph`], so a destroyable
//! support/cargo edge is data on the set, never a name check, and
//! [`WorldActorSet::destroy`] flips geometry and collision together through
//! [`Presence`]. Motion is a function of committed ticks and destruction
//! events only: [`WorldActorSet::step`] takes no visibility, residency or
//! presentation input, so a culled actor keeps moving exactly like an
//! on-screen one (non-negotiable behavior 5).
//!
//! The non-negotiables this stage makes structural:
//!
//! * **Position and velocity share one update.** A route follower's
//!   reported velocity is the actual per-tick displacement divided by dt,
//!   so a partial move into a gate's stop line reports the reduced speed,
//!   not the cruise speed it intended (non-negotiable 1). Angular velocity
//!   is the actual orientation delta over the same tick.
//! * **A ground route stops at a closed gate** (non-negotiable 3). The
//!   follower clamps progress at the gate's declared stop line while the
//!   gate actor is [`Presence::Intact`]; destroying the gate opens the
//!   passage permanently, whether the convoy already waits there (destroy
//!   after arrival) or has not reached it yet (destroy before arrival) —
//!   the AC02 order pair.
//! * **Detached payloads inherit source motion and keep identity**
//!   (non-negotiable 4): [`WorldActorSet::release`] registers the
//!   [`release_payload`] output as a new free-drifting actor with the
//!   carrier's faction and the payload's objective id.
//!
//! Trains, trucks, boats, gates, generators and elevators are
//! [`WorldActorKind`] catalog values on a typed registration record, not
//! scenery the runtime pokes by name. Which kind names and motion
//! parameters the original assigns is unmeasured: every value here is
//! designed engine contract
//! (`docs/findings/2026-10-01-f34-a-world-actor-motion-and-dependency.md`).

use std::collections::{BTreeMap, BTreeSet};

use cs_script::ir::{ActorId, SymbolId};
use cs_types::Tick;
use cs_types::content::ContentId;

use super::anchor::{AnchorSample, AnchorSocket, anchor_sample};
use super::graph::{GraphError, Presence, SupportGraph};
use super::math::{Quat, add, sub};
use super::release::{PayloadSpec, ReleasedPayload, release_payload};
use super::route::RoutePlan;
use super::trajectory::{Pose, Trajectory};
use crate::ai::navigation::heading_from_direction;

/// The catalog kind a world actor registers as.
///
/// Designed vocabulary: the original's actor-type enumeration is
/// unmeasured, so the set distinguishes only the motion domains the spec
/// names — rail, road, water and mission machinery — and keeps the kind as
/// identity, not as a motion capability. Any kind may carry any
/// [`ActorMotion`]; a locked water gate and a road convoy block through the
/// same [`RoutePlan`] gate rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorldActorKind {
    /// Rail-bound stock: trains and stock cars.
    Rail,
    /// Road vehicles: trucks, convoy cars.
    Road,
    /// Watercraft: boats and barges.
    Water,
    /// Mission machinery: gates, generators, elevators and similar.
    Kinematic,
    /// An airship: the measured `zeppelin` record family, placed as a
    /// scope's world actors.
    Airship,
}

impl WorldActorKind {
    /// Every declared kind, in a stable order.
    pub const ALL: [Self; 5] = [
        Self::Rail,
        Self::Road,
        Self::Water,
        Self::Kinematic,
        Self::Airship,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rail => "rail",
            Self::Road => "road",
            Self::Water => "water",
            Self::Kinematic => "kinematic",
            Self::Airship => "airship",
        }
    }
}

/// How one actor moves, declared at registration.
#[derive(Clone, Debug, PartialEq)]
pub enum ActorMotion {
    /// A fixed pose: gates, generators and parked machinery.
    Held {
        /// World position.
        position_m: [f64; 3],
        /// World orientation.
        orientation: Quat,
    },
    /// An authored tick-indexed path (`F34-A` [`Trajectory`]): timetabled
    /// trains, elevators, turntables — anything whose schedule cannot wait.
    Trajectory(Trajectory),
    /// A [`RoutePlan`] follower: advances one authored cruise speed along
    /// the polyline per tick and holds at closed gates. `start_progress_m`
    /// is its arc length at registration, `0` for a fresh spawn.
    Route {
        /// The validated route.
        plan: RoutePlan,
        /// Arc length it starts at.
        start_progress_m: f64,
    },
    /// A detached payload drifting on its release velocity: the boat a
    /// carrier lets go keeps the carrier's motion until content gives it a
    /// route of its own (F34-C).
    Free {
        /// World position.
        position_m: [f64; 3],
        /// World velocity.
        velocity_m_s: [f64; 3],
        /// World orientation.
        orientation: Quat,
    },
    /// Cargo riding a carrier's anchor socket (F34-C): the actor's world
    /// pose and velocity are the socket's, recomputed from the carrier's
    /// canonical pose every read, so a carried boat moves exactly with the
    /// deck that carries it. `socket` must name the carrier
    /// (`socket.actor == carrier`); a declared socket that cannot is
    /// refused at registration.
    Carried {
        /// The actor whose pose drives this one.
        carrier: ActorId,
        /// The carrier's attachment point.
        socket: AnchorSocket,
    },
}

/// The typed registration record for one world actor.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldActorSpec {
    /// A fresh mission-scoped id; never reused within a session.
    pub actor: ActorId,
    /// Its catalog kind.
    pub kind: WorldActorKind,
    /// Allegiance, shared with released payloads.
    pub faction: ContentId,
    /// The objective this actor counts for, if any.
    pub objective: Option<SymbolId>,
    /// How it moves.
    pub motion: ActorMotion,
}

/// What a stepped tick produced, in the order the set produced it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorldActorEvent {
    /// A route follower reached a closed gate's stop line and is now held.
    HeldAtGate {
        /// The held follower.
        actor: ActorId,
        /// The intact gate holding it.
        gate: ActorId,
        /// The tick it was first held.
        at: Tick,
    },
    /// A held follower moved again because the gate that held it was
    /// destroyed.
    ResumedFromGate {
        /// The resumed follower.
        actor: ActorId,
        /// The gate that had held it.
        gate: ActorId,
        /// The tick it moved again.
        at: Tick,
    },
    /// A route follower reached the end of its route and stopped.
    RouteCompleted {
        /// The arrived follower.
        actor: ActorId,
        /// The tick it arrived.
        at: Tick,
    },
}

/// Why a [`WorldActorSet`] operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldActorError {
    /// The actor id is already registered; ids are never reused within a
    /// session, not even over a destroyed record.
    DuplicateActor(ActorId),
    /// The named actor is not registered.
    UnknownActor(ActorId),
    /// A route gate named the follower itself.
    SelfGate {
        /// The follower.
        actor: ActorId,
    },
    /// A route gate names an actor that is not registered. Gates must be
    /// registered before the followers whose routes declare them.
    UnknownGate {
        /// The follower whose route declared it.
        actor: ActorId,
        /// The unregistered gate.
        gate: ActorId,
    },
    /// A follower registered with progress already beyond a closed gate's
    /// stop line — a state the follower could never reach while the gate
    /// stands.
    BeyondClosedGate {
        /// The follower.
        actor: ActorId,
        /// The intact gate it spawned past.
        gate: ActorId,
        /// Its claimed start progress.
        progress_m: f64,
        /// The gate's stop line.
        stop_line_m: f64,
    },
    /// A route follower registered with progress outside `[0, length_m]`.
    StartBeyondRoute {
        /// The claimed start progress.
        progress_m: f64,
        /// The route's length.
        length_m: f64,
    },
    /// A named field was NaN or infinite.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// An orientation was not a unit quaternion.
    NonUnitOrientation,
    /// A zero ticks-per-second rate was requested.
    ZeroTickRate,
    /// A [`Trajectory`] motion whose tick rate differs from the set's: its
    /// sampled velocity is derived in the trajectory's own timebase, so a
    /// mismatch would report a speed the set's ticks do not produce.
    TickRateMismatch {
        /// The actor carrying the mismatched trajectory.
        actor: ActorId,
        /// The set's tick rate.
        set_ticks_per_second: u32,
        /// The trajectory's tick rate.
        trajectory_ticks_per_second: u32,
    },
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
    /// The support graph refused an edit.
    Graph(GraphError),
    /// A `Carried` motion or [`WorldActorSet::attach`] named a carrier the
    /// set does not have.
    UnknownCarrier {
        /// The actor that would be carried.
        actor: ActorId,
        /// The unregistered carrier.
        carrier: ActorId,
    },
    /// A `Carried` socket's `actor` field names someone other than the
    /// carrier — the socket must be the carrier's own attachment point.
    AnchorOwnerMismatch {
        /// The actor that would be carried.
        actor: ActorId,
        /// The carrier the socket should belong to.
        carrier: ActorId,
        /// The actor the socket actually names.
        socket_owner: ActorId,
    },
    /// Attaching `actor` under `carrier` would make the carrier carried by
    /// its own cargo — the carriage chain must stay acyclic.
    CarriageCycle {
        /// The actor that would be carried.
        actor: ActorId,
        /// The carrier it would ride.
        carrier: ActorId,
    },
    /// An operation required an intact actor and named a destroyed one.
    ActorDestroyed {
        /// The destroyed actor.
        actor: ActorId,
    },
    /// [`WorldActorSet::detach`] named an actor that is not carried.
    NotCarried {
        /// The actor that is not cargo.
        actor: ActorId,
    },
    /// An operation named an actor already collected by an external taker.
    AlreadyCollected {
        /// The collected actor.
        actor: ActorId,
    },
}

impl From<GraphError> for WorldActorError {
    fn from(err: GraphError) -> Self {
        Self::Graph(err)
    }
}

/// The yaw-only orientation a route direction implies: heading `0` faces
/// `-Z`, positive turns nose-left about `+Y` (the `FLIGHT-PHYSICS`
/// convention), shared with `crate::ai::navigation`. A (near-)vertical
/// segment has no defined heading: the follower keeps its last yaw.
fn route_orientation(direction: [f64; 3], fallback: Quat) -> Quat {
    let horizontal = direction[0].hypot(direction[2]);
    if horizontal < 1e-9 {
        return fallback;
    }
    let heading = heading_from_direction(direction[0], direction[2]);
    let half = heading * 0.5;
    Quat([0.0, half.sin(), 0.0, half.cos()])
}

/// Per-follower route state.
#[derive(Clone, Debug, PartialEq)]
struct RouteState {
    progress_m: f64,
    pose: Pose,
    /// The intact gate currently holding it, if any.
    held_gate: Option<ActorId>,
    /// Whether the route end was reached and reported.
    completed: bool,
}

/// Per-actor runtime motion.
#[derive(Clone, Debug, PartialEq)]
enum MotionState {
    Held(Pose),
    Trajectory(Trajectory),
    Route {
        plan: RoutePlan,
        state: RouteState,
    },
    Free {
        position_m: [f64; 3],
        velocity_m_s: [f64; 3],
        orientation: Quat,
    },
    /// Riding a carrier's anchor socket; its pose is resolved from the
    /// carrier's canonical pose on every read, never stepped on its own.
    Carried {
        carrier: ActorId,
        socket: AnchorSocket,
    },
    /// Taken aboard by an external taker: the actor keeps its registry id
    /// and last resolved pose but no longer moves under this set's control.
    /// The taker owns its presentation from the latch on; the frozen pose
    /// is where it left the world.
    Collected(Pose),
}

/// One registered actor.
#[derive(Clone, Debug, PartialEq)]
struct WorldActor {
    kind: WorldActorKind,
    faction: ContentId,
    objective: Option<SymbolId>,
    motion: MotionState,
    /// The wreck pose captured at destruction: held from then on, with
    /// zeroed velocities, whatever the motion would have said.
    destroyed_pose: Option<Pose>,
}

impl WorldActor {
    /// The live (non-wreck) pose at `tick` for every motion that owns its
    /// own state. A `Carried` actor has no pose of its own — it is resolved
    /// from the carrier by [`WorldActorSet::resolved_live_pose`].
    fn live_pose(&self, tick: Tick) -> Pose {
        match &self.motion {
            MotionState::Held(pose) | MotionState::Collected(pose) => *pose,
            MotionState::Trajectory(t) => t.sample(tick),
            MotionState::Route { state, .. } => state.pose,
            MotionState::Free {
                position_m,
                velocity_m_s,
                orientation,
            } => Pose {
                position_m: *position_m,
                orientation: *orientation,
                velocity_m_s: *velocity_m_s,
                angular_velocity_rad_s: [0.0; 3],
            },
            MotionState::Carried { .. } => {
                unreachable!("a carried pose resolves through the carrier")
            }
        }
    }
}

/// One session's world actors: registry, support graph and tick stepping.
///
/// Construction order matters in one place only: a [`RoutePlan`] gate names
/// its controlling actor by id, and [`WorldActorSet::register`] refuses a
/// follower whose route references an actor the set does not have yet —
/// register gates and machinery first, then the movers, the same order a
/// mission load would produce them.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldActorSet {
    ticks_per_second: u32,
    tick: Tick,
    graph: SupportGraph,
    actors: BTreeMap<ActorId, WorldActor>,
    /// Actors whose declared gate passages are scripted open (F34-C): a
    /// scripted transition, not destruction, lifts the hold — and a later
    /// scripted close re-imposes it. Presence stays monotonic: a destroyed
    /// gate's flag is dead state, its passage open permanently.
    open_passages: BTreeSet<ActorId>,
}

impl WorldActorSet {
    /// An empty set at [`Tick`] 0 stepping `ticks_per_second` fixed ticks
    /// per second.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::ZeroTickRate`].
    pub fn new(ticks_per_second: u32) -> Result<Self, WorldActorError> {
        if ticks_per_second == 0 {
            return Err(WorldActorError::ZeroTickRate);
        }
        Ok(Self {
            ticks_per_second,
            tick: Tick(0),
            graph: SupportGraph::default(),
            actors: BTreeMap::new(),
            open_passages: BTreeSet::new(),
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

    /// Every registered actor id, in id order.
    pub fn actors(&self) -> impl Iterator<Item = ActorId> + '_ {
        self.actors.keys().copied()
    }

    /// Registers one actor. Gates a route declares must already be
    /// registered; a follower may not start past a closed gate's stop line.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::DuplicateActor`], [`SelfGate`](WorldActorError::SelfGate),
    /// [`UnknownGate`](WorldActorError::UnknownGate),
    /// [`BeyondClosedGate`](WorldActorError::BeyondClosedGate),
    /// [`StartBeyondRoute`](WorldActorError::StartBeyondRoute),
    /// [`NonFinite`](WorldActorError::NonFinite),
    /// [`NonUnitOrientation`](WorldActorError::NonUnitOrientation),
    /// [`TickRateMismatch`](WorldActorError::TickRateMismatch) or
    /// [`Graph`](WorldActorError::Graph).
    pub fn register(&mut self, spec: WorldActorSpec) -> Result<(), WorldActorError> {
        if self.actors.contains_key(&spec.actor) {
            return Err(WorldActorError::DuplicateActor(spec.actor));
        }
        let motion = self.validate_motion(&spec)?;
        self.graph.add_actor(spec.actor)?;
        self.actors.insert(
            spec.actor,
            WorldActor {
                kind: spec.kind,
                faction: spec.faction,
                objective: spec.objective,
                motion,
                destroyed_pose: None,
            },
        );
        Ok(())
    }

    /// Validates a spec's motion against the registry and builds its
    /// initial [`MotionState`].
    fn validate_motion(&self, spec: &WorldActorSpec) -> Result<MotionState, WorldActorError> {
        match &spec.motion {
            ActorMotion::Held {
                position_m,
                orientation,
            } => {
                check_finite("position_m", position_m)?;
                check_unit(*orientation)?;
                Ok(MotionState::Held(Pose {
                    position_m: *position_m,
                    orientation: *orientation,
                    velocity_m_s: [0.0; 3],
                    angular_velocity_rad_s: [0.0; 3],
                }))
            }
            ActorMotion::Trajectory(t) => {
                if t.ticks_per_second() != self.ticks_per_second {
                    return Err(WorldActorError::TickRateMismatch {
                        actor: spec.actor,
                        set_ticks_per_second: self.ticks_per_second,
                        trajectory_ticks_per_second: t.ticks_per_second(),
                    });
                }
                Ok(MotionState::Trajectory(t.clone()))
            }
            ActorMotion::Free {
                position_m,
                velocity_m_s,
                orientation,
            } => {
                check_finite("position_m", position_m)?;
                check_finite("velocity_m_s", velocity_m_s)?;
                check_unit(*orientation)?;
                Ok(MotionState::Free {
                    position_m: *position_m,
                    velocity_m_s: *velocity_m_s,
                    orientation: *orientation,
                })
            }
            ActorMotion::Carried { carrier, socket } => {
                if socket.actor != *carrier {
                    return Err(WorldActorError::AnchorOwnerMismatch {
                        actor: spec.actor,
                        carrier: *carrier,
                        socket_owner: socket.actor,
                    });
                }
                if !self.actors.contains_key(carrier) {
                    return Err(WorldActorError::UnknownCarrier {
                        actor: spec.actor,
                        carrier: *carrier,
                    });
                }
                check_finite("offset_m", &socket.offset_m)?;
                // A cycle is impossible here: the actor being registered is
                // new, so nothing is carried by it yet. Registering cargo on
                // a wreck is allowed — it rides the wreck's frozen pose.
                Ok(MotionState::Carried {
                    carrier: *carrier,
                    socket: *socket,
                })
            }
            ActorMotion::Route {
                plan,
                start_progress_m,
            } => {
                let start = *start_progress_m;
                if !start.is_finite() {
                    return Err(WorldActorError::NonFinite {
                        field: "start_progress_m",
                    });
                }
                if start < 0.0 || start > plan.length_m() {
                    return Err(WorldActorError::StartBeyondRoute {
                        progress_m: start,
                        length_m: plan.length_m(),
                    });
                }
                let mut held_gate = None;
                for g in plan.gates() {
                    if g.gate == spec.actor {
                        return Err(WorldActorError::SelfGate { actor: spec.actor });
                    }
                    if self.graph.presence(g.gate).is_none() {
                        return Err(WorldActorError::UnknownGate {
                            actor: spec.actor,
                            gate: g.gate,
                        });
                    }
                    // Only a *closed* gate constrains the spawn: a destroyed,
                    // scripted-open or collected gate never blocks, so a
                    // fresh spawn may legitimately sit past its stop line.
                    if self.passage_closed(g.gate) {
                        if start > g.stop_line_m() {
                            return Err(WorldActorError::BeyondClosedGate {
                                actor: spec.actor,
                                gate: g.gate,
                                progress_m: start,
                                stop_line_m: g.stop_line_m(),
                            });
                        }
                        // Gates ascend in `at_m`, so the first match is
                        // the same nearest gate `classify_route_end`
                        // reports; taking a later one would emit a
                        // spurious resume on the first step.
                        if held_gate.is_none()
                            && start == g.stop_line_m()
                            && start < plan.length_m()
                        {
                            held_gate = Some(g.gate);
                        }
                    }
                }
                let completed = start >= plan.length_m();
                let position_m = plan.position_at(start);
                let orientation = route_orientation(plan.direction_at(start), Quat::IDENTITY);
                // Cruising unless spawned held or at the end; the first
                // step keeps this exact velocity as its displacement.
                let velocity_m_s = if completed || held_gate.is_some() {
                    [0.0; 3]
                } else {
                    plan.direction_at(start).map(|d| d * plan.speed_m_s())
                };
                Ok(MotionState::Route {
                    plan: plan.clone(),
                    state: RouteState {
                        progress_m: start,
                        pose: Pose {
                            position_m,
                            orientation,
                            velocity_m_s,
                            angular_velocity_rad_s: [0.0; 3],
                        },
                        held_gate,
                        completed,
                    },
                })
            }
        }
    }

    /// Declares that `dependent` is lost when `supporter` is destroyed.
    /// Both actors must be registered.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::Graph`] wrapping
    /// [`GraphError::UnknownActor`]/[`GraphError::Cycle`].
    pub fn declare_support(
        &mut self,
        supporter: ActorId,
        dependent: ActorId,
    ) -> Result<(), WorldActorError> {
        Ok(self.graph.add_support(supporter, dependent)?)
    }

    /// The presence (combined geometry+collision state) of `actor`, or
    /// `None` when it is not registered.
    #[must_use]
    pub fn presence(&self, actor: ActorId) -> Option<Presence> {
        self.graph.presence(actor)
    }

    /// Whether `gate` currently blocks its declared passages: intact, not
    /// scripted open and not collected out of the world. A destroyed
    /// gate's passage is open permanently; a scripted-open one until it
    /// re-closes; a collected one's owner took it out of the world, so it
    /// can never hold a follower again.
    fn passage_closed(&self, gate: ActorId) -> bool {
        self.graph.presence(gate) == Some(Presence::Intact)
            && !self.open_passages.contains(&gate)
            && !matches!(
                self.actors.get(&gate).map(|a| &a.motion),
                Some(MotionState::Collected(_))
            )
    }

    /// The live pose of `actor` with carriage resolved: walks the
    /// carried-by chain up to the first actor that owns its pose, then
    /// folds each socket's [`anchor_sample`] back down the chain. Chains
    /// are acyclic by construction — registration and [`Self::attach`]
    /// refuse them — and actors are never removed, so every hop names a
    /// registered actor. Each link reads the carrier's *current* pose, so
    /// cargo on a wreck rides the wreck.
    fn resolved_pose(&self, actor: ActorId) -> Pose {
        let mut owner = actor;
        let mut sockets = Vec::new();
        for _ in 0..self.actors.len() {
            let record = self
                .actors
                .get(&owner)
                .expect("carriage chains only name registered actors");
            let MotionState::Carried { carrier, socket } = &record.motion else {
                break;
            };
            sockets.push(*socket);
            owner = *carrier;
        }
        let base = self
            .actors
            .get(&owner)
            .expect("carriage chains only name registered actors");
        let mut pose = base
            .destroyed_pose
            .unwrap_or_else(|| base.live_pose(self.tick));
        for socket in sockets.iter().rev() {
            let anchor = anchor_sample(self.tick, &pose, socket);
            pose = Pose {
                position_m: anchor.position_m,
                orientation: anchor.orientation,
                velocity_m_s: anchor.velocity_m_s,
                // The socket turns with the carrier, so the carried actor's
                // angular velocity is the carrier's, re-derived each read.
                angular_velocity_rad_s: pose.angular_velocity_rad_s,
            };
        }
        pose
    }

    /// The canonical pose of `actor` at the set's current tick — the same
    /// value `anchor_sample` turns into the pose the renderer and pickup
    /// eligibility share. For a carried actor this is the socket anchor on
    /// the carrier's canonical pose, resolved the same way the renderer
    /// resolves it.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn pose(&self, actor: ActorId) -> Result<Pose, WorldActorError> {
        let a = self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?;
        Ok(a.destroyed_pose
            .unwrap_or_else(|| self.resolved_pose(actor)))
    }

    /// The catalog kind of `actor`.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn kind(&self, actor: ActorId) -> Result<WorldActorKind, WorldActorError> {
        Ok(self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?
            .kind)
    }

    /// The faction of `actor`.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn faction(&self, actor: ActorId) -> Result<&ContentId, WorldActorError> {
        Ok(&self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?
            .faction)
    }

    /// The objective identity of `actor`, if it carries one.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn objective(&self, actor: ActorId) -> Result<Option<SymbolId>, WorldActorError> {
        Ok(self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?
            .objective)
    }

    /// A route follower's arc-length progress, `None` for other motions.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn route_progress_m(&self, actor: ActorId) -> Result<Option<f64>, WorldActorError> {
        match &self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?
            .motion
        {
            MotionState::Route { state, .. } => Ok(Some(state.progress_m)),
            _ => Ok(None),
        }
    }

    /// The intact gate currently holding `actor`, `None` when it is moving
    /// or not route-bound.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn held_gate(&self, actor: ActorId) -> Result<Option<ActorId>, WorldActorError> {
        match &self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?
            .motion
        {
            MotionState::Route { state, .. } => Ok(state.held_gate),
            _ => Ok(None),
        }
    }

    /// Destroys `actor` and everything that transitively depends on it.
    /// Each destroyed actor freezes as a zero-velocity wreck at its current
    /// pose; each intact gate it removes unblocks every follower it held on
    /// the next step. Returns the cascade in the graph's breadth order.
    ///
    /// Every wreck pose is resolved against the still-live set first, so a
    /// carried actor freezes at the socket of its carrier's pre-destruction
    /// pose even when the carrier falls in the same cascade.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::Graph`] wrapping [`GraphError::UnknownActor`].
    pub fn destroy(&mut self, actor: ActorId) -> Result<Vec<ActorId>, WorldActorError> {
        let lost = self.graph.destroy(actor)?;
        let wrecks: Vec<(ActorId, Pose)> = lost
            .iter()
            .map(|&a| {
                let mut wreck = self.resolved_pose(a);
                wreck.velocity_m_s = [0.0; 3];
                wreck.angular_velocity_rad_s = [0.0; 3];
                (a, wreck)
            })
            .collect();
        for (a, wreck) in wrecks {
            if let Some(record) = self.actors.get_mut(&a) {
                record.destroyed_pose = Some(wreck);
                if let MotionState::Route { state, .. } = &mut record.motion {
                    state.held_gate = None;
                }
            }
        }
        Ok(lost)
    }

    /// Releases `spec` at `anchor`: the payload registers as a new actor of
    /// `kind`, drifting on the anchor's velocity plus the authored
    /// ejection, keeping the spec's faction and its own objective identity.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::DuplicateActor`] when `spec.actor` is taken,
    /// [`NonFinite`](WorldActorError::NonFinite) on a non-finite ejection,
    /// anchor or resulting drift, or [`WorldActorError::Graph`] on the
    /// registry edit.
    pub fn release(
        &mut self,
        anchor: &AnchorSample,
        spec: PayloadSpec,
        kind: WorldActorKind,
    ) -> Result<ReleasedPayload, WorldActorError> {
        if self.actors.contains_key(&spec.actor) {
            return Err(WorldActorError::DuplicateActor(spec.actor));
        }
        check_finite("eject_m_s", &spec.eject_m_s)?;
        let payload = release_payload(anchor, spec.clone());
        // The anchor is caller-supplied: a non-finite sample must not put a
        // NaN actor into the set either.
        check_finite("position_m", &payload.position_m)?;
        check_finite("velocity_m_s", &payload.velocity_m_s)?;
        self.graph.add_actor(spec.actor)?;
        self.actors.insert(
            spec.actor,
            WorldActor {
                kind,
                faction: spec.faction,
                objective: spec.objective,
                motion: MotionState::Free {
                    position_m: payload.position_m,
                    velocity_m_s: payload.velocity_m_s,
                    orientation: anchor.orientation,
                },
                destroyed_pose: None,
            },
        );
        Ok(payload)
    }

    /// Latches `actor` onto `carrier`'s anchor socket: from this call its
    /// canonical pose and velocity are the socket's, recomputed against the
    /// carrier each read — the pickup half of the F34-C cargo wiring.
    /// Re-latching to a different carrier or socket moves the binding.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`] / [`UnknownCarrier`](WorldActorError::UnknownCarrier)
    /// for unregistered ids, [`ActorDestroyed`](WorldActorError::ActorDestroyed)
    /// when either side is a wreck, [`AlreadyCollected`](WorldActorError::AlreadyCollected)
    /// when `actor` already left the world,
    /// [`AnchorOwnerMismatch`](WorldActorError::AnchorOwnerMismatch) when the
    /// socket does not belong to `carrier`,
    /// [`CarriageCycle`](WorldActorError::CarriageCycle) when `carrier` is
    /// transitively carried by `actor`, or
    /// [`NonFinite`](WorldActorError::NonFinite) on a non-finite offset.
    /// Returns the socket's anchor sample at the latch tick.
    pub fn attach(
        &mut self,
        actor: ActorId,
        carrier: ActorId,
        socket: AnchorSocket,
    ) -> Result<AnchorSample, WorldActorError> {
        let record = self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?;
        if record.destroyed_pose.is_some() {
            return Err(WorldActorError::ActorDestroyed { actor });
        }
        if matches!(record.motion, MotionState::Collected(_)) {
            return Err(WorldActorError::AlreadyCollected { actor });
        }
        if socket.actor != carrier {
            return Err(WorldActorError::AnchorOwnerMismatch {
                actor,
                carrier,
                socket_owner: socket.actor,
            });
        }
        let carrier_record = self
            .actors
            .get(&carrier)
            .ok_or(WorldActorError::UnknownCarrier { actor, carrier })?;
        if carrier_record.destroyed_pose.is_some() {
            return Err(WorldActorError::ActorDestroyed { actor: carrier });
        }
        if matches!(carrier_record.motion, MotionState::Collected(_)) {
            return Err(WorldActorError::AlreadyCollected { actor: carrier });
        }
        check_finite("offset_m", &socket.offset_m)?;
        // The carriage chain must stay acyclic: refuse when the carrier is
        // transitively carried by the actor — `attach` would make cargo
        // carry its own carrier.
        let mut link = carrier;
        for _ in 0..self.actors.len() {
            if link == actor {
                return Err(WorldActorError::CarriageCycle { actor, carrier });
            }
            let MotionState::Carried { carrier: next, .. } = &self
                .actors
                .get(&link)
                .expect("carriage chains only name registered actors")
                .motion
            else {
                break;
            };
            link = *next;
        }
        self.actors.get_mut(&actor).expect("checked above").motion =
            MotionState::Carried { carrier, socket };
        let carrier_pose = self.resolved_pose(carrier);
        Ok(anchor_sample(self.tick, &carrier_pose, &socket))
    }

    /// Lets a carried actor go: `actor` leaves its carrier's socket and
    /// drifts on the socket's velocity plus the authored ejection, keeping
    /// the faction and objective it was registered with — the boat a
    /// carrier releases, AC03. Returns the release kinematics.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`], [`ActorDestroyed`](WorldActorError::ActorDestroyed),
    /// [`NotCarried`](WorldActorError::NotCarried) when `actor` does not ride
    /// a socket, or [`NonFinite`](WorldActorError::NonFinite) on a
    /// non-finite ejection.
    pub fn detach(
        &mut self,
        actor: ActorId,
        eject_m_s: [f64; 3],
    ) -> Result<ReleasedPayload, WorldActorError> {
        let record = self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?;
        if record.destroyed_pose.is_some() {
            return Err(WorldActorError::ActorDestroyed { actor });
        }
        let MotionState::Carried { carrier, socket } = &record.motion else {
            return Err(WorldActorError::NotCarried { actor });
        };
        let (carrier, socket) = (*carrier, *socket);
        check_finite("eject_m_s", &eject_m_s)?;
        let carrier_pose = self.resolved_pose(carrier);
        let anchor = anchor_sample(self.tick, &carrier_pose, &socket);
        let record = self.actors.get_mut(&actor).expect("checked above");
        let payload = release_payload(
            &anchor,
            PayloadSpec {
                actor,
                faction: record.faction.clone(),
                objective: record.objective,
                eject_m_s,
            },
        );
        check_finite("position_m", &payload.position_m)?;
        check_finite("velocity_m_s", &payload.velocity_m_s)?;
        record.motion = MotionState::Free {
            position_m: payload.position_m,
            velocity_m_s: payload.velocity_m_s,
            orientation: anchor.orientation,
        };
        Ok(payload)
    }

    /// Marks `actor` collected by an external taker — the pickup completion
    /// for a cargo or passenger item the player's own craft takes aboard.
    /// The actor keeps its registry id (ids are never reused) and freezes
    /// at the pose it left the world at; the taker owns it from there.
    /// A collected gate can never hold a follower again.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`], [`ActorDestroyed`](WorldActorError::ActorDestroyed)
    /// or [`AlreadyCollected`](WorldActorError::AlreadyCollected). Returns
    /// the pose it left the world at.
    pub fn collect(&mut self, actor: ActorId) -> Result<Pose, WorldActorError> {
        let record = self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?;
        if record.destroyed_pose.is_some() {
            return Err(WorldActorError::ActorDestroyed { actor });
        }
        if matches!(record.motion, MotionState::Collected(_)) {
            return Err(WorldActorError::AlreadyCollected { actor });
        }
        let pose = self.resolved_pose(actor);
        self.actors.get_mut(&actor).expect("checked above").motion = MotionState::Collected(pose);
        Ok(pose)
    }

    /// The scripted gate transition (F34-C): `open` lifts the hold `gate`'s
    /// declared passages impose without destroying it, and a later close
    /// re-imposes it on followers that have not yet crossed — never on one
    /// that legitimately passed while open. The flag is dead state on a
    /// destroyed gate: presence is monotonic, so that passage stays open.
    /// Returns whether the open state actually changed.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn set_gate_open(&mut self, gate: ActorId, open: bool) -> Result<bool, WorldActorError> {
        if !self.actors.contains_key(&gate) {
            return Err(WorldActorError::UnknownActor(gate));
        }
        Ok(if open {
            self.open_passages.insert(gate)
        } else {
            self.open_passages.remove(&gate)
        })
    }

    /// The carrier and socket `actor` rides, when it is carried.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn carried_by(
        &self,
        actor: ActorId,
    ) -> Result<Option<(ActorId, AnchorSocket)>, WorldActorError> {
        match &self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?
            .motion
        {
            MotionState::Carried { carrier, socket } => Ok(Some((*carrier, *socket))),
            _ => Ok(None),
        }
    }

    /// Whether `actor` was collected by an external taker.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn is_collected(&self, actor: ActorId) -> Result<bool, WorldActorError> {
        Ok(matches!(
            self.actors
                .get(&actor)
                .ok_or(WorldActorError::UnknownActor(actor))?
                .motion,
            MotionState::Collected(_)
        ))
    }

    /// Whether `gate`'s declared passages are scripted open.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn gate_open(&self, gate: ActorId) -> Result<bool, WorldActorError> {
        if !self.actors.contains_key(&gate) {
            return Err(WorldActorError::UnknownActor(gate));
        }
        Ok(self.open_passages.contains(&gate))
    }

    /// Advances the set exactly one tick and returns the transitions that
    /// tick produced, in actor-id order.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::TickOverflow`].
    pub fn step(&mut self) -> Result<Vec<WorldActorEvent>, WorldActorError> {
        let next = self
            .tick
            .0
            .checked_add(1)
            .ok_or(WorldActorError::TickOverflow)?;
        self.tick = Tick(next);
        let dt = self.dt_seconds();
        // Which gates block this tick: presence, scripted opens and
        // collections are all caller-invoked, so the snapshot one step
        // takes cannot change while it runs.
        let closed_gates: BTreeSet<ActorId> = self
            .actors
            .values()
            .filter_map(|a| match &a.motion {
                MotionState::Route { plan, .. } => Some(plan.gates().iter().map(|g| g.gate)),
                _ => None,
            })
            .flatten()
            .filter(|&g| self.passage_closed(g))
            .collect();
        let mut events = Vec::new();
        let ids: Vec<ActorId> = self.actors.keys().copied().collect();
        for id in ids {
            if self.graph.presence(id) != Some(Presence::Intact) {
                continue;
            }
            let actor = self
                .actors
                .get_mut(&id)
                .expect("registry and graph cannot disagree");
            match &mut actor.motion {
                MotionState::Held(_)
                | MotionState::Trajectory(_)
                | MotionState::Carried { .. }
                | MotionState::Collected(_) => {}
                MotionState::Free {
                    position_m,
                    velocity_m_s,
                    ..
                } => {
                    *position_m = add(*position_m, velocity_m_s.map(|v| v * dt));
                }
                MotionState::Route { plan, state } => {
                    let old_pose = state.pose;
                    // Only a closed stop line at or ahead of the follower
                    // constrains it. A scripted close may land behind a
                    // follower that legitimately crossed while the passage
                    // was open: that gate's line no longer applies — the
                    // follower is neither pulled back nor pinned where it
                    // crossed.
                    let limit = plan
                        .gates()
                        .iter()
                        .filter(|g| closed_gates.contains(&g.gate))
                        .map(super::route::RouteGate::stop_line_m)
                        .filter(|stop_line_m| *stop_line_m >= state.progress_m)
                        .fold(f64::INFINITY, f64::min);
                    let new_progress = (state.progress_m + plan.speed_m_s() * dt)
                        .min(limit)
                        .min(plan.length_m())
                        // Progress never regresses, whatever the clamps say.
                        .max(state.progress_m);
                    state.progress_m = new_progress;
                    let position_m = plan.position_at(new_progress);
                    let orientation =
                        route_orientation(plan.direction_at(new_progress), old_pose.orientation);
                    state.pose = Pose {
                        position_m,
                        orientation,
                        // The actual displacement over this tick: a partial
                        // move into a stop line reports reduced speed, and a
                        // held follower reports zero — never a stale cruise
                        // value (non-negotiable 1).
                        velocity_m_s: sub(position_m, old_pose.position_m).map(|d| d / dt),
                        angular_velocity_rad_s: old_pose
                            .orientation
                            .slerp_angular_velocity(orientation)
                            .map(|v| v / dt),
                    };
                    Self::classify_route_end(
                        &mut events,
                        &closed_gates,
                        id,
                        self.tick,
                        plan,
                        state,
                        limit,
                    );
                }
            }
        }
        Ok(events)
    }

    /// The held/completed/resumed edge classification after one follower's
    /// progress was updated. Edges only: a state reported once, never
    /// re-emitted while it continues.
    fn classify_route_end(
        events: &mut Vec<WorldActorEvent>,
        closed_gates: &BTreeSet<ActorId>,
        actor: ActorId,
        at: Tick,
        plan: &RoutePlan,
        state: &mut RouteState,
        limit: f64,
    ) {
        let new_progress = state.progress_m;
        if new_progress >= plan.length_m() {
            if let Some(gate) = state.held_gate.take() {
                events.push(WorldActorEvent::ResumedFromGate { actor, gate, at });
            }
            if !state.completed {
                state.completed = true;
                events.push(WorldActorEvent::RouteCompleted { actor, at });
            }
            return;
        }
        // Held exactly when progress sits on a closed gate's stop line.
        let held = if limit.is_finite() && new_progress == limit {
            plan.gates()
                .iter()
                .filter(|g| closed_gates.contains(&g.gate))
                .filter(|g| g.stop_line_m() == limit)
                .min_by(|a, b| {
                    a.at_m
                        .partial_cmp(&b.at_m)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|g| g.gate)
        } else {
            None
        };
        if state.held_gate != held {
            if let Some(gate) = state.held_gate {
                events.push(WorldActorEvent::ResumedFromGate { actor, gate, at });
            }
            if let Some(gate) = held {
                events.push(WorldActorEvent::HeldAtGate { actor, gate, at });
            }
            state.held_gate = held;
        }
    }

    /// Advances the set to `to`, stepping one tick at a time — gates are
    /// re-read every tick, so a destroy between steps takes effect on the
    /// very next one. Returns every produced event in tick order.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::NonMonotonicTick`] when `to` is behind the set's
    /// tick and [`WorldActorError::TickOverflow`].
    pub fn advance_to(&mut self, to: Tick) -> Result<Vec<WorldActorEvent>, WorldActorError> {
        if to < self.tick {
            return Err(WorldActorError::NonMonotonicTick {
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
}

fn check_finite(field: &'static str, value: &[f64; 3]) -> Result<(), WorldActorError> {
    if value.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(WorldActorError::NonFinite { field })
    }
}

fn check_unit(orientation: Quat) -> Result<(), WorldActorError> {
    if orientation.is_unit() {
        Ok(())
    } else {
        Err(WorldActorError::NonUnitOrientation)
    }
}

#[cfg(test)]
mod tests {
    //! The F34-C carriage and scripted-gate surface at the layer that owns
    //! it: a `Carried` actor's canonical pose is the socket on its carrier's
    //! pose, every carriage operation refuses the states it must, and a
    //! collected or destroyed gate never holds a follower again.

    use cs_types::content::ContentKind;

    use super::super::route::RouteGate;
    use super::*;

    const CARRIER: ActorId = ActorId(1);
    const BOAT: ActorId = ActorId(2);
    const CRATE: ActorId = ActorId(3);
    const GATE: ActorId = ActorId(4);
    const CONVOY: ActorId = ActorId(5);

    fn faction() -> ContentId {
        ContentId::from_source(ContentKind::Faction, "synthetic.f34c.faction")
            .expect("a valid faction id")
    }

    fn socket(actor: ActorId, socket: u16, offset_m: [f64; 3]) -> AnchorSocket {
        AnchorSocket {
            actor,
            socket,
            offset_m,
        }
    }

    /// The carrier's deck: three meters up, where the boat rides.
    fn deck() -> AnchorSocket {
        socket(CARRIER, 1, [0.0, 3.0, 0.0])
    }

    fn carrier() -> WorldActorSpec {
        WorldActorSpec {
            actor: CARRIER,
            kind: WorldActorKind::Water,
            faction: faction(),
            objective: Some(SymbolId(7)),
            motion: ActorMotion::Free {
                position_m: [200.0, 0.0, 0.0],
                velocity_m_s: [0.0, 8.0, 0.0],
                orientation: Quat::IDENTITY,
            },
        }
    }

    fn boat() -> WorldActorSpec {
        WorldActorSpec {
            actor: BOAT,
            kind: WorldActorKind::Water,
            faction: faction(),
            objective: Some(SymbolId(21)),
            motion: ActorMotion::Carried {
                carrier: CARRIER,
                socket: deck(),
            },
        }
    }

    fn held(actor: ActorId, position_m: [f64; 3]) -> WorldActorSpec {
        WorldActorSpec {
            actor,
            kind: WorldActorKind::Kinematic,
            faction: faction(),
            objective: None,
            motion: ActorMotion::Held {
                position_m,
                orientation: Quat::IDENTITY,
            },
        }
    }

    /// A 100 m route at 10 m/s (1 m/tick at 10 ticks/s) held by the gate's
    /// 45 m stop line until the gate opens.
    fn convoy() -> WorldActorSpec {
        WorldActorSpec {
            actor: CONVOY,
            kind: WorldActorKind::Road,
            faction: faction(),
            objective: None,
            motion: ActorMotion::Route {
                plan: RoutePlan::try_new(
                    vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
                    10.0,
                    vec![RouteGate {
                        gate: GATE,
                        at_m: 50.0,
                        stop_before_m: 5.0,
                    }],
                )
                .expect("a legal route"),
                start_progress_m: 0.0,
            },
        }
    }

    fn set(specs: Vec<WorldActorSpec>) -> WorldActorSet {
        let mut set = WorldActorSet::new(10).expect("a positive tick rate");
        for spec in specs {
            set.register(spec).expect("a legal registration");
        }
        set
    }

    /// Ten ticks of drift are not exactly eight meters; the pose is.
    fn assert_approx(got: [f64; 3], want: [f64; 3]) {
        for (g, w) in got.into_iter().zip(want) {
            assert!((g - w).abs() < 1e-9, "{got:?} != {want:?}");
        }
    }

    /// The carrier, the boat on its deck and a held crate.
    fn harbor() -> WorldActorSet {
        set(vec![carrier(), boat(), held(CRATE, [200.0, 40.0, 0.0])])
    }

    #[test]
    fn accept_f34_c_a_destroyed_carrier_freezes_its_cargo_at_the_pre_destruction_pose() {
        let mut set = harbor();
        set.advance_to(Tick(10)).expect("ten legal ticks");

        // The boat's canonical pose is the deck socket on the carrier's pose,
        // so it carries the carrier's velocity with it.
        let deck_pose = set.pose(BOAT).expect("a registered boat");
        assert_approx(deck_pose.position_m, [200.0, 11.0, 0.0]);
        assert_eq!(deck_pose.velocity_m_s, [0.0, 8.0, 0.0]);

        // Destroying the carrier resolves the wreck poses against the
        // still-live set first: the cargo freezes where the deck stood.
        assert_eq!(
            set.destroy(CARRIER).expect("a registered actor"),
            vec![CARRIER]
        );
        let wreck = set.pose(CARRIER).expect("a registered carrier");
        let frozen_cargo = set.pose(BOAT).expect("a registered boat");
        assert_approx(wreck.position_m, [200.0, 8.0, 0.0]);
        assert_eq!(wreck.velocity_m_s, [0.0; 3]);
        assert_approx(frozen_cargo.position_m, [200.0, 11.0, 0.0]);
        assert_eq!(frozen_cargo.velocity_m_s, [0.0; 3]);

        // A carried actor is never stepped on its own, so the cargo stays on
        // the wreck instead of drifting away from it.
        set.advance_to(Tick(30)).expect("legal ticks");
        assert_approx(
            set.pose(BOAT).expect("a registered boat").position_m,
            [200.0, 11.0, 0.0],
        );
        assert_approx(
            set.pose(CARRIER).expect("registered").position_m,
            [200.0, 8.0, 0.0],
        );
    }

    #[test]
    fn accept_f34_c_cargo_collects_at_the_socket_it_rode_and_answers_nothing_after() {
        let mut set = harbor();

        // The collected pose is the socket anchor the cargo left the world
        // at — the same value the renderer and pickup judge read.
        assert_eq!(
            set.collect(BOAT).expect("a live actor"),
            set.pose(BOAT).expect("pose")
        );
        assert_eq!(
            set.pose(BOAT).expect("a registered boat").position_m,
            [200.0, 3.0, 0.0]
        );
        assert!(set.is_collected(BOAT).expect("a registered boat"));
        // Collection ends the carriage and the id is still the actor's own.
        assert_eq!(set.carried_by(BOAT).expect("a registered boat"), None);
        assert_eq!(set.carried_by(CARRIER).expect("registered"), None);
        assert_eq!(set.faction(BOAT).expect("registered"), &faction());

        // Twice is a named refusal, not a second collection.
        assert_eq!(
            set.collect(BOAT).unwrap_err(),
            WorldActorError::AlreadyCollected { actor: BOAT }
        );
        // And it cannot re-enter the world through any carriage operation.
        assert_eq!(
            set.attach(BOAT, CARRIER, deck()).unwrap_err(),
            WorldActorError::AlreadyCollected { actor: BOAT }
        );
        assert_eq!(
            set.detach(BOAT, [0.0; 3]).unwrap_err(),
            WorldActorError::NotCarried { actor: BOAT }
        );
    }

    #[test]
    fn accept_f34_c_a_collected_or_destroyed_actor_refuses_every_carriage_operation() {
        // A collected carrier cannot winch anything aboard.
        let mut set = harbor();
        set.collect(CARRIER).expect("a live actor");
        assert_eq!(
            set.attach(CRATE, CARRIER, deck()).unwrap_err(),
            WorldActorError::AlreadyCollected { actor: CARRIER }
        );

        // A destroyed carrier cannot either, and a destroyed cargo cannot
        // leave: wrecks are named, never guessed.
        let mut set = harbor();
        set.destroy(CARRIER).expect("a registered actor");
        assert_eq!(
            set.attach(CRATE, CARRIER, deck()).unwrap_err(),
            WorldActorError::ActorDestroyed { actor: CARRIER }
        );
        let mut set = harbor();
        set.destroy(CRATE).expect("a registered actor");
        assert_eq!(
            set.attach(CRATE, CARRIER, deck()).unwrap_err(),
            WorldActorError::ActorDestroyed { actor: CRATE }
        );
        assert_eq!(
            set.collect(CRATE).unwrap_err(),
            WorldActorError::ActorDestroyed { actor: CRATE }
        );

        // A detached cargo released on its socket's velocity plus the
        // authored ejection, keeping its faction and objective (AC03).
        let mut set = harbor();
        let payload = set.detach(BOAT, [0.0, 0.0, 2.0]).expect("a carried actor");
        assert_eq!(payload.position_m, [200.0, 3.0, 0.0]);
        assert_eq!(payload.velocity_m_s, [0.0, 8.0, 2.0]);
        assert_eq!(set.carried_by(BOAT).expect("registered"), None);
        assert_eq!(set.faction(BOAT).expect("registered"), &faction());
        assert_eq!(set.objective(BOAT).expect("registered"), Some(SymbolId(21)));
        assert_eq!(
            set.pose(BOAT).expect("registered").velocity_m_s,
            [0.0, 8.0, 2.0]
        );

        // A second release is `NotCarried`, and a non-finite ejection never
        // turns cargo into drift.
        assert_eq!(
            set.detach(BOAT, [0.0; 3]).unwrap_err(),
            WorldActorError::NotCarried { actor: BOAT }
        );
        let mut set = harbor();
        assert_eq!(
            set.detach(BOAT, [f64::NAN, 0.0, 0.0]).unwrap_err(),
            WorldActorError::NonFinite { field: "eject_m_s" }
        );
        assert_eq!(
            set.carried_by(BOAT).expect("registered"),
            Some((CARRIER, deck())),
            "a refused release leaves the carriage untouched"
        );
    }

    #[test]
    fn accept_f34_c_attach_refuses_a_foreign_socket_a_cycle_and_a_non_finite_offset() {
        let mut set = harbor();

        // The socket must be the named carrier's own attachment point.
        assert_eq!(
            set.attach(CRATE, GATE, deck()).unwrap_err(),
            WorldActorError::AnchorOwnerMismatch {
                actor: CRATE,
                carrier: GATE,
                socket_owner: CARRIER,
            }
        );
        // A non-finite offset never becomes cargo motion.
        assert_eq!(
            set.attach(
                CRATE,
                CARRIER,
                socket(CARRIER, 2, [f64::INFINITY, 0.0, 0.0])
            )
            .unwrap_err(),
            WorldActorError::NonFinite { field: "offset_m" }
        );
        // Cargo that already rides the carrier cannot carry it back.
        assert!(set.attach(CRATE, CARRIER, deck()).is_ok());
        assert_eq!(
            set.attach(CARRIER, CRATE, socket(CRATE, 1, [0.0, 0.0, 0.0]))
                .unwrap_err(),
            WorldActorError::CarriageCycle {
                actor: CARRIER,
                carrier: CRATE
            }
        );
        assert_eq!(set.carried_by(CARRIER).expect("registered"), None);

        // An unregistered carrier is named, never invented.
        let mut set = harbor();
        assert_eq!(
            set.attach(CRATE, ActorId(77), socket(ActorId(77), 1, [0.0, 3.0, 0.0]))
                .unwrap_err(),
            WorldActorError::UnknownCarrier {
                actor: CRATE,
                carrier: ActorId(77)
            }
        );
        assert_eq!(
            set.attach(ActorId(77), CARRIER, deck()).unwrap_err(),
            WorldActorError::UnknownActor(ActorId(77))
        );
    }

    #[test]
    fn accept_f34_c_a_collected_gate_can_never_hold_a_follower_again() {
        // Control: an intact closed gate holds the convoy on its stop line.
        let mut intact = set(vec![held(GATE, [50.0, 0.0, 0.0]), convoy()]);
        let events = intact.advance_to(Tick(50)).expect("legal ticks");
        assert!(events.contains(&WorldActorEvent::HeldAtGate {
            actor: CONVOY,
            gate: GATE,
            at: Tick(45)
        }));
        assert_eq!(intact.held_gate(CONVOY).expect("registered"), Some(GATE));

        // A taker that collected the gate took it out of the world: the
        // passage is open from then on, and the scripted flag is dead state
        // on a gate that is no longer there.
        let mut taken = set(vec![held(GATE, [50.0, 0.0, 0.0]), convoy()]);
        taken.collect(GATE).expect("a live actor");
        assert!(!taken.set_gate_open(GATE, false).expect("registered"));
        let events = taken.advance_to(Tick(50)).expect("legal ticks");
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, WorldActorEvent::HeldAtGate { actor: CONVOY, .. })),
            "a collected gate never holds: {events:?}"
        );
        let progress = taken
            .route_progress_m(CONVOY)
            .expect("registered")
            .expect("a route");
        assert!((progress - 50.0).abs() < 1e-9, "progress {progress}");
        // The open flag itself is still settable and reported, so a script
        // that drives it is never silently dropped.
        assert!(taken.set_gate_open(GATE, true).expect("registered"));
        assert!(taken.gate_open(GATE).expect("registered"));
        assert!(
            taken.set_gate_open(GATE, false).expect("registered"),
            "closing a scripted-open gate is a real state change"
        );
        assert!(!taken.gate_open(GATE).expect("registered"));
    }
}
