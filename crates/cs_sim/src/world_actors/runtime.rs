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

use std::collections::BTreeMap;

use cs_script::ir::{ActorId, SymbolId};
use cs_types::Tick;
use cs_types::content::ContentId;

use super::anchor::AnchorSample;
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
}

impl WorldActorKind {
    /// Every declared kind, in a stable order.
    pub const ALL: [Self; 4] = [Self::Rail, Self::Road, Self::Water, Self::Kinematic];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rail => "rail",
            Self::Road => "road",
            Self::Water => "water",
            Self::Kinematic => "kinematic",
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
    /// The live (non-wreck) pose at `tick`.
    fn live_pose(&self, tick: Tick) -> Pose {
        match &self.motion {
            MotionState::Held(pose) => *pose,
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
    /// [`NonUnitOrientation`](WorldActorError::NonUnitOrientation) or
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
            ActorMotion::Trajectory(t) => Ok(MotionState::Trajectory(t.clone())),
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
                    match self.graph.presence(g.gate) {
                        None => {
                            return Err(WorldActorError::UnknownGate {
                                actor: spec.actor,
                                gate: g.gate,
                            });
                        }
                        // An already-destroyed gate never blocks; an intact
                        // one a fresh spawn may touch but never start past.
                        Some(Presence::Intact) => {
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
                        Some(Presence::Destroyed) => {}
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

    /// The canonical pose of `actor` at the set's current tick — the same
    /// value `anchor_sample` turns into the pose the renderer and pickup
    /// eligibility share.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn pose(&self, actor: ActorId) -> Result<Pose, WorldActorError> {
        let a = self
            .actors
            .get(&actor)
            .ok_or(WorldActorError::UnknownActor(actor))?;
        Ok(a.destroyed_pose.unwrap_or_else(|| a.live_pose(self.tick)))
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
    /// # Errors
    ///
    /// [`WorldActorError::Graph`] wrapping [`GraphError::UnknownActor`].
    pub fn destroy(&mut self, actor: ActorId) -> Result<Vec<ActorId>, WorldActorError> {
        let lost = self.graph.destroy(actor)?;
        for &a in &lost {
            if let Some(record) = self.actors.get_mut(&a) {
                let mut wreck = record.live_pose(self.tick);
                wreck.velocity_m_s = [0.0; 3];
                wreck.angular_velocity_rad_s = [0.0; 3];
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
    /// [`WorldActorError::DuplicateActor`] when `spec.actor` is taken or
    /// [`WorldActorError::Graph`] on the registry edit.
    pub fn release(
        &mut self,
        anchor: &AnchorSample,
        spec: PayloadSpec,
        kind: WorldActorKind,
    ) -> Result<ReleasedPayload, WorldActorError> {
        if self.actors.contains_key(&spec.actor) {
            return Err(WorldActorError::DuplicateActor(spec.actor));
        }
        let payload = release_payload(anchor, spec.clone());
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
                MotionState::Held(_) | MotionState::Trajectory(_) => {}
                MotionState::Free {
                    position_m,
                    velocity_m_s,
                    ..
                } => {
                    *position_m = add(*position_m, velocity_m_s.map(|v| v * dt));
                }
                MotionState::Route { plan, state } => {
                    let old_pose = state.pose;
                    let limit = plan
                        .gates()
                        .iter()
                        .filter(|g| self.graph.presence(g.gate) == Some(Presence::Intact))
                        .map(super::route::RouteGate::stop_line_m)
                        .fold(f64::INFINITY, f64::min);
                    let new_progress = (state.progress_m + plan.speed_m_s() * dt)
                        .min(limit)
                        .min(plan.length_m());
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
                        &self.graph,
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
        graph: &SupportGraph,
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
        // Held exactly when progress sits on an intact gate's stop line.
        let held = if limit.is_finite() && new_progress == limit {
            plan.gates()
                .iter()
                .filter(|g| graph.presence(g.gate) == Some(Presence::Intact))
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
