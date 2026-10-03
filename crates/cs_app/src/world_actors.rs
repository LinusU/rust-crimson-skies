//! The world-actor wiring boundary (F34-C).
//!
//! Spec: `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`,
//! stage `### F34-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module sits between the declared world-actor schema
//! ([`cs_content::world_actors`]) and the session runtime
//! ([`cs_sim::world_actors::runtime::WorldActorSet`]), which cannot see each
//! other — `cs_sim` must not depend on `cs_content`
//! (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_world_actors`] — the conversion boundary: a validated
//!   [`cs_content::world_actors::DeclaredWorldActorProgram`] becomes a
//!   [`LoweredWorldActors`] with every actor's kind, faction, objective,
//!   motion, sockets, the support edges, the pickups and the scripted gate
//!   schedule mapped field-wise. Every `Resolved::Unknown` is refused by
//!   name and claim, never given a default; every cross-reference the
//!   schema deliberately left open — a carried actor's carrier and socket,
//!   a pickup's target, an `Attach` socket — resolves here or the lowering
//!   refuses it by name.
//! * [`WorldActorSession`] — the wired producer→runtime→consumer path of
//!   one session generation. [`WorldActorSession::step`] is the only way
//!   in: it applies the producer's [`WorldActorCommand`]s, fires the
//!   declared gate schedule tick by tick, advances the runtime and then
//!   judges every declared pickup against the same canonical pose the
//!   renderer reads through [`WorldActorSession::anchor_pose`]. The
//!   [`WorldActorSessionTick`] it answers carries the ordered
//!   [`WorldActorSessionEvent`] stream and every refusal as a typed
//!   [`WorldActorRefusal`] — error propagation is a queryable fact, never a
//!   swallowed error or a guessed outcome.
//! * [`WorldActorSession::retry`] — teardown/retry: reports what the old
//!   generation still owns in a [`WorldActorTeardown`] and rebuilds a fresh
//!   runtime for the new [`SessionGeneration`] from the same lowered
//!   program, so no destroyed actor, latched pickup, open gate or fired
//!   transition survives into the retry (STATE-TRANSACTIONS "Session
//!   reset": retry restores the authored initial state).
//! * [`WorldActorBinding`] — the ECS record tying an entity to its actor
//!   and catalog subject, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] and
//!   [`crate::capital::CapitalActorBinding`] so a reload can never leave a
//!   stale binding looking live.
//!
//! What the session does **not** decide: the original pickup tolerances,
//! actor-kind names, gate schedules and carriage rules are unmeasured —
//! every value arrives through the declared schema with its provenance, and
//! the session applies, reports or refuses, never invents. Whether an
//! external taker's own body binds to a latched anchor is its consumer's
//! transaction (F36-B); the latch's [`AnchorSample`] is the hand-off.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bevy::ecs::component::Component;
use cs_content::objectives::{ProgramActor, ProgramSymbol};
use cs_content::world_actors::{
    DeclaredMotion, DeclaredPickupCompletion, DeclaredTaker, DeclaredWorldActorProgram,
};
use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::world_actors::Quat;
use cs_sim::world_actors::anchor::{
    AnchorSample, AnchorSocket, PickupEnvelope, anchor_sample, pickup_eligible_pose,
};
use cs_sim::world_actors::graph::Presence;
use cs_sim::world_actors::release::{PayloadSpec, ReleasedPayload};
use cs_sim::world_actors::route::{RouteError, RouteGate, RoutePlan};
use cs_sim::world_actors::runtime::{
    ActorMotion, WorldActorError, WorldActorEvent, WorldActorKind, WorldActorSet, WorldActorSpec,
};
use cs_sim::world_actors::trajectory::{Keyframe, Pose, Trajectory, TrajectoryError};
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use crate::scene::SceneGeneration;

// ---------------------------------------------------------------------------
// Lowering: declared schema -> runtime records
// ---------------------------------------------------------------------------

/// Why a declared world-actor program could not be lowered.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldActorLowerError {
    /// A load-bearing field is `Resolved::Unknown`: the runtime cannot
    /// place, move or judge an actor on a guessed value, so the boundary
    /// refuses rather than inventing one.
    UnknownValue {
        /// The declared field name.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
    /// The runtime refused the declared route polyline or its gates.
    Route {
        /// The actor carrying the route.
        actor: ProgramActor,
        /// The runtime's refusal.
        source: RouteError,
    },
    /// The runtime refused the declared path.
    Trajectory {
        /// The actor carrying the path.
        actor: ProgramActor,
        /// The runtime's refusal.
        source: TrajectoryError,
    },
    /// A carried motion names a carrier the program does not declare.
    UnknownCarrier {
        /// The carried actor.
        actor: ProgramActor,
        /// The undeclared carrier.
        carrier: ProgramActor,
    },
    /// A socket id names no socket on the actor that should own it: a
    /// carried actor's carriage point or an `Attach` completion's latch
    /// point.
    UnknownSocket {
        /// The actor that references it, when one does (a pickup's
        /// `Attach` names the taker's socket instead).
        actor: Option<ProgramActor>,
        /// The actor that should declare the socket.
        owner: ProgramActor,
        /// The missing socket id.
        socket: u16,
    },
    /// Declared carriage would form a cycle: walking the `Carried` chain
    /// from `carrier` reaches `actor` again.
    CarriageCycle {
        /// The carried actor.
        actor: ProgramActor,
        /// The carrier it would ride.
        carrier: ProgramActor,
    },
    /// A pickup names a target actor the program does not declare.
    UnknownPickupTarget {
        /// The pickup.
        symbol: ProgramSymbol,
        /// The undeclared target.
        target: ProgramActor,
    },
    /// A pickup names a world-actor taker the program does not declare.
    UnknownPickupTaker {
        /// The pickup.
        symbol: ProgramSymbol,
        /// The undeclared taker.
        taker: ProgramActor,
    },
}

impl std::fmt::Display for WorldActorLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownValue {
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "{field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::Route { actor, source } => {
                write!(f, "{actor}'s route was refused: {source:?}")
            }
            Self::Trajectory { actor, source } => {
                write!(f, "{actor}'s path was refused: {source:?}")
            }
            Self::UnknownCarrier { actor, carrier } => {
                write!(f, "{actor} rides undeclared carrier {carrier}")
            }
            Self::UnknownSocket {
                actor,
                owner,
                socket,
            } => match actor {
                Some(actor) => {
                    write!(f, "{actor} names socket {socket} {owner} does not declare")
                }
                None => write!(f, "socket {socket} is not declared on {owner}"),
            },
            Self::CarriageCycle { actor, carrier } => {
                write!(f, "{actor} riding {carrier} would close a carriage cycle")
            }
            Self::UnknownPickupTarget { symbol, target } => {
                write!(f, "pickup {symbol} targets undeclared actor {target}")
            }
            Self::UnknownPickupTaker { symbol, taker } => {
                write!(f, "pickup {symbol} is taken by undeclared actor {taker}")
            }
        }
    }
}

impl std::error::Error for WorldActorLowerError {}

/// Who drives a lowered pickup to its latch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoweredTaker {
    /// A registered world actor; its canonical pose is the taker's.
    WorldActor(ActorId),
    /// An external taker the session is probed with per tick.
    External,
}

/// What a satisfied lowered pickup does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoweredPickupCompletion {
    /// The taker binds to the target's anchor; the latch sample is the
    /// hand-off to its own binding transaction.
    Latch,
    /// The target attaches to a socket on the taker and rides it.
    Attach {
        /// The taker's latch socket.
        socket: AnchorSocket,
    },
    /// The target is collected out of the world.
    Collect,
}

/// One lowered pickup, ready for the session to judge each tick.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredPickup {
    /// Its stable identity.
    pub symbol: SymbolId,
    /// The actor whose socket is the pickup point.
    pub target: ActorId,
    /// The resolved pickup socket on `target`.
    pub socket: AnchorSocket,
    /// The bounds a taker must satisfy.
    pub envelope: PickupEnvelope,
    /// Who may drive it.
    pub taker: LoweredTaker,
    /// What a satisfied envelope does.
    pub completion: LoweredPickupCompletion,
}

/// One scripted gate transition, lowered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoweredGateTransition {
    /// The tick it takes effect on.
    pub at: Tick,
    /// The gate actor.
    pub gate: ActorId,
    /// Whether the passage opens (`true`) or closes (`false`).
    pub open: bool,
}

/// The lowered form of one declared world-actor program: everything a
/// session launches and relaunches from.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredWorldActors {
    /// The fixed tick rate the set steps.
    pub ticks_per_second: u32,
    /// The registration records, in authored order.
    pub actors: Vec<WorldActorSpec>,
    /// The catalog subject each actor is built from, for bindings.
    pub subjects: BTreeMap<ActorId, ContentId>,
    /// Each actor's declared attachment sockets, in authored order.
    pub sockets: BTreeMap<ActorId, Vec<AnchorSocket>>,
    /// The declared support edges `(supporter, dependent)`.
    pub support: Vec<(ActorId, ActorId)>,
    /// The lowered pickups.
    pub pickups: Vec<LoweredPickup>,
    /// The scripted gate schedule, sorted by tick with authored order kept
    /// inside a tick.
    pub transitions: Vec<LoweredGateTransition>,
}

impl LoweredWorldActors {
    /// One declared socket on `actor` by id.
    #[must_use]
    pub fn socket(&self, actor: ActorId, socket: u16) -> Option<AnchorSocket> {
        self.sockets
            .get(&actor)
            .and_then(|sockets| sockets.iter().find(|s| s.socket == socket))
            .copied()
    }
}

const fn lower_actor(actor: ProgramActor) -> ActorId {
    ActorId(actor.0)
}

const fn lower_symbol(symbol: ProgramSymbol) -> SymbolId {
    SymbolId(symbol.0)
}

fn required<T: Clone>(value: &Resolved<T>, field: &'static str) -> Result<T, WorldActorLowerError> {
    match value {
        Resolved::Known(known) => Ok(known.value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(WorldActorLowerError::UnknownValue {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// Lowers a validated declared world-actor program into the runtime records
/// a session launches from.
///
/// Every `Resolved::Unknown` refuses by name rather than becoming a
/// default; every cross-reference the schema left open — a carried actor's
/// carrier and socket, a pickup's target or `Attach` socket — resolves
/// against the declared program here or is refused by name. Declared
/// carriage is checked for cycles at the boundary so the runtime's
/// acyclicity invariant is established, not assumed.
///
/// # Errors
///
/// [`WorldActorLowerError`] naming the first declaration that could not
/// lower.
pub fn lower_world_actors(
    declared: &DeclaredWorldActorProgram,
) -> Result<LoweredWorldActors, WorldActorLowerError> {
    let ticks_per_second = required(declared.ticks_per_second(), "ticks_per_second")?;

    let mut actors = Vec::with_capacity(declared.actors().len());
    let mut subjects = BTreeMap::new();
    let mut sockets: BTreeMap<ActorId, Vec<AnchorSocket>> = BTreeMap::new();
    for actor in declared.actors() {
        let id = lower_actor(actor.actor);
        subjects.insert(id, actor.subject.clone());
        sockets.insert(
            id,
            actor
                .sockets
                .iter()
                .map(|socket| {
                    Ok(AnchorSocket {
                        actor: id,
                        socket: socket.socket,
                        offset_m: required(&socket.offset_m, "offset_m")?,
                    })
                })
                .collect::<Result<Vec<_>, WorldActorLowerError>>()?,
        );
        let motion = match &actor.motion {
            DeclaredMotion::Held {
                position_m,
                orientation,
            } => ActorMotion::Held {
                position_m: required(position_m, "position_m")?,
                orientation: Quat(required(orientation, "orientation")?),
            },
            DeclaredMotion::Path(path) => {
                let keyframes = path
                    .keyframes
                    .iter()
                    .map(|key| {
                        Ok(Keyframe {
                            tick: Tick(key.tick),
                            position_m: required(&key.position_m, "position_m")?,
                            orientation: Quat(required(&key.orientation, "orientation")?),
                        })
                    })
                    .collect::<Result<Vec<_>, WorldActorLowerError>>()?;
                let rate = required(&path.ticks_per_second, "ticks_per_second")?;
                ActorMotion::Trajectory(Trajectory::new(keyframes, rate).map_err(|source| {
                    WorldActorLowerError::Trajectory {
                        actor: actor.actor,
                        source,
                    }
                })?)
            }
            DeclaredMotion::Route(route) => {
                let gates = route
                    .gates
                    .iter()
                    .map(|gate| {
                        Ok(RouteGate {
                            gate: lower_actor(gate.gate),
                            at_m: required(&gate.at_m, "at_m")?,
                            stop_before_m: required(&gate.stop_before_m, "stop_before_m")?,
                        })
                    })
                    .collect::<Result<Vec<_>, WorldActorLowerError>>()?;
                let plan = RoutePlan::try_new(
                    required(&route.points, "points")?,
                    required(&route.speed_m_s, "speed_m_s")?,
                    gates,
                )
                .map_err(|source| WorldActorLowerError::Route {
                    actor: actor.actor,
                    source,
                })?;
                ActorMotion::Route {
                    plan,
                    start_progress_m: required(&route.start_progress_m, "start_progress_m")?,
                }
            }
            DeclaredMotion::Free {
                position_m,
                velocity_m_s,
                orientation,
            } => ActorMotion::Free {
                position_m: required(position_m, "position_m")?,
                velocity_m_s: required(velocity_m_s, "velocity_m_s")?,
                orientation: Quat(required(orientation, "orientation")?),
            },
            DeclaredMotion::Carried { carrier, socket } => {
                let Some(owner) = declared.actor(*carrier) else {
                    return Err(WorldActorLowerError::UnknownCarrier {
                        actor: actor.actor,
                        carrier: *carrier,
                    });
                };
                // Declared carriage must be acyclic before the runtime ever
                // sees it: walk the chain of Carried motions from the
                // carrier; reaching this actor closes a cycle. A link that
                // names no declared actor ends the walk — that link's own
                // UnknownCarrier is raised when its dependent lowers.
                let mut link = *carrier;
                while let Some(owner) = declared.actor(link) {
                    let DeclaredMotion::Carried { carrier: next, .. } = &owner.motion else {
                        break;
                    };
                    if *next == actor.actor {
                        return Err(WorldActorLowerError::CarriageCycle {
                            actor: actor.actor,
                            carrier: *carrier,
                        });
                    }
                    link = *next;
                }
                let Some(offset) = owner
                    .sockets
                    .iter()
                    .find(|s| s.socket == *socket)
                    .map(|s| &s.offset_m)
                else {
                    return Err(WorldActorLowerError::UnknownSocket {
                        actor: Some(actor.actor),
                        owner: *carrier,
                        socket: *socket,
                    });
                };
                ActorMotion::Carried {
                    carrier: lower_actor(*carrier),
                    socket: AnchorSocket {
                        actor: lower_actor(*carrier),
                        socket: *socket,
                        offset_m: required(offset, "offset_m")?,
                    },
                }
            }
        };
        actors.push(WorldActorSpec {
            actor: id,
            kind: match actor.kind {
                cs_content::world_actors::DeclaredWorldActorKind::Rail => WorldActorKind::Rail,
                cs_content::world_actors::DeclaredWorldActorKind::Road => WorldActorKind::Road,
                cs_content::world_actors::DeclaredWorldActorKind::Water => WorldActorKind::Water,
                cs_content::world_actors::DeclaredWorldActorKind::Kinematic => {
                    WorldActorKind::Kinematic
                }
            },
            faction: required(&actor.faction, "faction")?,
            objective: actor.objective.map(lower_symbol),
            motion,
        });
    }

    let support = declared
        .support()
        .iter()
        .map(|edge| (lower_actor(edge.supporter), lower_actor(edge.dependent)))
        .collect();

    let mut pickups = Vec::with_capacity(declared.pickups().len());
    for pickup in declared.pickups() {
        let Some(target) = declared.actor(pickup.target) else {
            return Err(WorldActorLowerError::UnknownPickupTarget {
                symbol: pickup.symbol,
                target: pickup.target,
            });
        };
        let Some(target_socket) = target.sockets.iter().find(|s| s.socket == pickup.socket) else {
            return Err(WorldActorLowerError::UnknownSocket {
                actor: Some(pickup.target),
                owner: pickup.target,
                socket: pickup.socket,
            });
        };
        let taker = match pickup.taker {
            DeclaredTaker::WorldActor(taker) => {
                if declared.actor(taker).is_none() {
                    return Err(WorldActorLowerError::UnknownPickupTaker {
                        symbol: pickup.symbol,
                        taker,
                    });
                }
                LoweredTaker::WorldActor(lower_actor(taker))
            }
            DeclaredTaker::External => LoweredTaker::External,
        };
        let completion = match &pickup.completion {
            DeclaredPickupCompletion::Latch => LoweredPickupCompletion::Latch,
            DeclaredPickupCompletion::Attach { socket } => {
                // An `Attach` is only declared with a world-actor taker
                // (the schema refuses it on an external one), so the taker
                // is registered here and its socket resolves by name.
                let DeclaredTaker::WorldActor(taker) = pickup.taker else {
                    unreachable!("the schema refuses Attach on an external taker")
                };
                let owner = declared.actor(taker).expect("checked above");
                let Some(latch) = owner.sockets.iter().find(|s| s.socket == *socket) else {
                    return Err(WorldActorLowerError::UnknownSocket {
                        actor: None,
                        owner: taker,
                        socket: *socket,
                    });
                };
                LoweredPickupCompletion::Attach {
                    socket: AnchorSocket {
                        actor: lower_actor(taker),
                        socket: *socket,
                        offset_m: required(&latch.offset_m, "offset_m")?,
                    },
                }
            }
            DeclaredPickupCompletion::Collect => LoweredPickupCompletion::Collect,
        };
        pickups.push(LoweredPickup {
            symbol: lower_symbol(pickup.symbol),
            target: lower_actor(pickup.target),
            socket: AnchorSocket {
                actor: lower_actor(pickup.target),
                socket: pickup.socket,
                offset_m: required(&target_socket.offset_m, "offset_m")?,
            },
            envelope: PickupEnvelope {
                max_distance_m: required(&pickup.envelope.max_distance_m, "max_distance_m")?,
                max_relative_speed_m_s: required(
                    &pickup.envelope.max_relative_speed_m_s,
                    "max_relative_speed_m_s",
                )?,
            },
            taker,
            completion,
        });
    }

    let mut transitions: Vec<LoweredGateTransition> = declared
        .transitions()
        .iter()
        .map(|t| LoweredGateTransition {
            at: t.at,
            gate: lower_actor(t.gate),
            open: t.open,
        })
        .collect();
    transitions.sort_by_key(|t| t.at);

    Ok(LoweredWorldActors {
        ticks_per_second,
        actors,
        subjects,
        sockets,
        support,
        pickups,
        transitions,
    })
}

// ---------------------------------------------------------------------------
// The session: producer -> runtime -> consumers
// ---------------------------------------------------------------------------

/// Why a session could not launch (or a retry could not rebuild).
#[derive(Clone, Debug, PartialEq)]
pub enum WorldActorLaunchError {
    /// The set refused the tick rate — unreachable after a successful
    /// lowering, reported rather than assumed.
    TickRate(WorldActorError),
    /// The runtime refused one registration; the actor that failed is
    /// named, not the index it sat at.
    Actor {
        /// The actor whose spec was refused.
        actor: ActorId,
        /// The runtime's refusal.
        source: WorldActorError,
    },
    /// The support graph refused an edge.
    Support {
        /// The declared supporter.
        supporter: ActorId,
        /// The declared dependent.
        dependent: ActorId,
        /// The graph's refusal.
        source: WorldActorError,
    },
    /// A retry asked for the live generation. Every artifact the session
    /// hands out carries its generation so a stale one can never be
    /// confused with the current session's — which only holds when the
    /// generations differ.
    SameGeneration {
        /// The generation passed to a session already running it.
        session: SessionGeneration,
    },
}

impl std::fmt::Display for WorldActorLaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TickRate(source) => write!(f, "the runtime refused the tick rate: {source:?}"),
            Self::Actor { actor, source } => {
                write!(f, "actor {actor:?} was refused at registration: {source:?}")
            }
            Self::Support {
                supporter,
                dependent,
                source,
            } => write!(
                f,
                "support edge {supporter:?} -> {dependent:?} was refused: {source:?}"
            ),
            Self::SameGeneration { session } => write!(
                f,
                "a retry must advance the generation, not rebuild {session:?} in place"
            ),
        }
    }
}

impl std::error::Error for WorldActorLaunchError {}

/// A producer's order for the tick: the mission script's interface to the
/// world-actor set.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldActorCommand {
    /// Destroy an actor and cascade its dependents.
    Destroy {
        /// The actor.
        actor: ActorId,
    },
    /// A scripted gate transition: open or close a passage without
    /// destroying the gate.
    Gate {
        /// The gate actor.
        gate: ActorId,
        /// Whether the passage opens (`true`) or closes (`false`).
        open: bool,
    },
    /// Let a carried actor go, drifting on its socket's velocity plus the
    /// authored ejection.
    Detach {
        /// The carried actor.
        actor: ActorId,
        /// The authored ejection in the carrier's frame, m/s.
        eject_m_s: [f64; 3],
    },
    /// Release a fresh payload from a carrier's declared socket: the actor
    /// registers drifting on the socket's velocity plus the authored
    /// ejection.
    Release {
        /// The carrier the socket belongs to.
        carrier: ActorId,
        /// The declared socket id on the carrier.
        socket: u16,
        /// What the carrier hands over.
        spec: PayloadSpec,
        /// The kind the payload registers as.
        kind: WorldActorKind,
    },
}

/// One external taker's probe for a tick: the pose of the craft trying a
/// `DeclaredTaker::External` pickup, keyed by the pickup it courts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TakerProbe {
    /// The pickup this pose courts.
    pub pickup: SymbolId,
    /// The taker's world position.
    pub position_m: [f64; 3],
    /// The taker's world velocity.
    pub velocity_m_s: [f64; 3],
}

/// The whole producer surface one step consumes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldActorTick {
    /// The tick to advance to; never backwards.
    pub to: Tick,
    /// The orders to apply *now*, before the first stepped tick — so a
    /// destroy or a scripted gate is already in effect for every tick the
    /// advance covers.
    pub commands: Vec<WorldActorCommand>,
    /// The external taker poses, evaluated at the target tick.
    pub probes: Vec<TakerProbe>,
}

/// Who took a latched pickup.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SessionTaker {
    /// A registered world actor.
    WorldActor(ActorId),
    /// An external taker.
    External,
}

/// What a latched pickup did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SessionCompletion {
    /// The taker bound to the anchor; its own binding is its consumer's.
    Bound,
    /// The target now rides a socket on the taker.
    Attached,
    /// The target was collected out of the world.
    Collected,
}

/// One fact one step produced, in the order it was produced.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldActorSessionEvent {
    /// A runtime transition: `HeldAtGate`, `ResumedFromGate` or
    /// `RouteCompleted`, emitted on the tick it happened.
    Runtime(WorldActorEvent),
    /// A scripted gate transition took effect — declared schedule or
    /// command — and the open state actually changed.
    Gate {
        /// The gate actor.
        gate: ActorId,
        /// The new state.
        open: bool,
        /// The tick it took effect on.
        at: Tick,
    },
    /// A pickup's envelope was satisfied and its completion applied; the
    /// anchor is the judged socket sample — the same value the renderer
    /// reads for the socket.
    Pickup {
        /// The pickup's symbol.
        pickup: SymbolId,
        /// The actor that was the pickup point.
        target: ActorId,
        /// Who took it.
        taker: SessionTaker,
        /// What the latch did.
        completion: SessionCompletion,
        /// The tick it latched.
        at: Tick,
        /// The judged anchor sample.
        anchor: AnchorSample,
    },
    /// A carried actor detached onto its release velocity.
    Detached {
        /// The release kinematics.
        payload: ReleasedPayload,
    },
    /// A fresh payload released at a carrier socket.
    Released {
        /// The release kinematics.
        payload: ReleasedPayload,
    },
    /// An actor was destroyed; `cascade` is every actor lost with it.
    Destroyed {
        /// The destroyed actor.
        actor: ActorId,
        /// The cascade in graph order.
        cascade: Vec<ActorId>,
        /// The tick it was destroyed on.
        at: Tick,
    },
}

/// A request the step declined, lifted into one typed list.
///
/// A refused order never corrupts the tick: the command or transition is
/// named with the runtime's refusal and the world still advances.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldActorRefusal {
    /// A producer command was refused.
    Command {
        /// The refused order.
        command: WorldActorCommand,
        /// Why the runtime refused it.
        error: WorldActorError,
    },
    /// A scheduled gate transition named a gate the set cannot apply it
    /// to.
    Gate {
        /// The gate actor.
        gate: ActorId,
        /// The state it asked for.
        open: bool,
        /// Why the runtime refused it.
        error: WorldActorError,
    },
    /// A pickup's envelope was satisfied but its completion was refused —
    /// a carriage cycle, a destroyed side, an already-collected target.
    Completion {
        /// The pickup.
        pickup: SymbolId,
        /// Why the runtime refused it.
        error: WorldActorError,
    },
    /// A `Release` command named a socket the carrier never declared.
    UnknownSocket {
        /// The carrier.
        carrier: ActorId,
        /// The missing socket id.
        socket: u16,
    },
}

/// What [`WorldActorSession::step`] answered with.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldActorSessionTick {
    /// The generation this answer belongs to.
    pub session: SessionGeneration,
    /// The tick the set now stands at.
    pub tick: Tick,
    /// The ordered event stream — commands first, then per-tick gate
    /// transitions and runtime events, then the pickups judged at the
    /// target tick.
    pub events: Vec<WorldActorSessionEvent>,
    /// Every refusal the step produced, named.
    pub refusals: Vec<WorldActorRefusal>,
}

/// What a retry's teardown reports: everything the old session still owned.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldActorTeardown {
    /// The generation that was torn down.
    pub session: SessionGeneration,
    /// Every registered actor — the entities the world must despawn.
    pub actors: Vec<ActorId>,
    /// The actors an external taker had collected and still holds.
    pub collected: Vec<ActorId>,
    /// The pickups that had already latched.
    pub latched_pickups: Vec<SymbolId>,
    /// The scheduled gate transitions that had not fired yet.
    pub pending_transitions: Vec<LoweredGateTransition>,
}

/// The wired world-actor session of one mission generation: the
/// producer→runtime→consumer path the stage exists to wire.
///
/// One session owns one [`WorldActorSet`] and every piece of per-session
/// consumer state — the pending gate schedule, the latched pickups, the
/// collected-actor ledger. [`step`](Self::step) is the only way in;
/// [`retry`](Self::retry) is the only way out that keeps the session
/// object. The canonical pose read [`anchor_pose`](Self::anchor_pose) is
/// the single path the renderer and the pickup judge share (AC01).
#[derive(Debug)]
pub struct WorldActorSession {
    /// The lowered program this session launches and relaunches from.
    lowered: LoweredWorldActors,
    /// The live set of the current generation.
    set: WorldActorSet,
    /// The session generation.
    session: SessionGeneration,
    /// The declared gate schedule still to fire.
    pending: VecDeque<LoweredGateTransition>,
    /// The pickups that already completed; a pickup latches once.
    latched: BTreeSet<SymbolId>,
    /// The actors an external taker collected this generation.
    collected: BTreeSet<ActorId>,
}

impl WorldActorSession {
    /// Builds a runtime set from the lowered program: every actor
    /// registered in authored order, each refusal named by the actor that
    /// caused it, then the declared support edges.
    fn build_set(lowered: &LoweredWorldActors) -> Result<WorldActorSet, WorldActorLaunchError> {
        let mut set = WorldActorSet::new(lowered.ticks_per_second)
            .map_err(WorldActorLaunchError::TickRate)?;
        for spec in &lowered.actors {
            set.register(spec.clone())
                .map_err(|source| WorldActorLaunchError::Actor {
                    actor: spec.actor,
                    source,
                })?;
        }
        for &(supporter, dependent) in &lowered.support {
            set.declare_support(supporter, dependent)
                .map_err(|source| WorldActorLaunchError::Support {
                    supporter,
                    dependent,
                    source,
                })?;
        }
        Ok(set)
    }

    /// Launches one session of the lowered program.
    ///
    /// # Errors
    ///
    /// [`WorldActorLaunchError`] naming the first declaration the runtime
    /// refused.
    pub fn launch(
        lowered: LoweredWorldActors,
        session: SessionGeneration,
    ) -> Result<Self, WorldActorLaunchError> {
        let set = Self::build_set(&lowered)?;
        Ok(Self {
            pending: lowered.transitions.iter().copied().collect(),
            lowered,
            set,
            session,
            latched: BTreeSet::new(),
            collected: BTreeSet::new(),
        })
    }

    /// The session generation this session owns.
    #[must_use]
    pub const fn session(&self) -> SessionGeneration {
        self.session
    }

    /// The lowered program this session runs.
    #[must_use]
    pub const fn program(&self) -> &LoweredWorldActors {
        &self.lowered
    }

    /// The live runtime set, for read-only inspection.
    #[must_use]
    pub const fn set(&self) -> &WorldActorSet {
        &self.set
    }

    /// The tick the set stands at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.set.tick()
    }

    /// The canonical read the renderer and the pickup judge share: the
    /// socket's anchor sample on `actor`'s canonical pose at the session's
    /// current tick. There is no second pose source to disagree with
    /// (AC01).
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    pub fn anchor_pose(
        &self,
        actor: ActorId,
        socket: &AnchorSocket,
    ) -> Result<AnchorSample, WorldActorError> {
        let pose = self.set.pose(actor)?;
        Ok(anchor_sample(self.set.tick(), &pose, socket))
    }

    /// Applies one tick's producer surface and dispatches the results.
    ///
    /// Order, once per call: the input's commands apply *now* — before the
    /// first stepped tick — then each stepped tick fires the declared gate
    /// schedule entries due on it, advances the set and judges the
    /// world-actor-taker pickups on that tick's canonical poses; finally
    /// every still-live pickup is judged at the target tick, which is where
    /// an external taker's probe pose is defined. A refusal anywhere lands
    /// in [`WorldActorSessionTick::refusals`]; it never aborts the tick and
    /// never changes another outcome.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::NonMonotonicTick`] when `to` is behind the set's
    /// tick and [`WorldActorError::TickOverflow`] — the only failures the
    /// session propagates as `Err`, because they mean the answer would not
    /// be the asked-for state at all.
    pub fn step(
        &mut self,
        input: &WorldActorTick,
    ) -> Result<WorldActorSessionTick, WorldActorError> {
        if input.to < self.set.tick() {
            return Err(WorldActorError::NonMonotonicTick {
                current: self.set.tick(),
                requested: input.to,
            });
        }
        let mut events = Vec::new();
        let mut refusals = Vec::new();
        let before = self.set.tick();

        for command in &input.commands {
            match command {
                WorldActorCommand::Destroy { actor } => match self.set.destroy(*actor) {
                    Ok(cascade) => events.push(WorldActorSessionEvent::Destroyed {
                        actor: *actor,
                        cascade,
                        at: self.set.tick(),
                    }),
                    Err(error) => refusals.push(WorldActorRefusal::Command {
                        command: command.clone(),
                        error,
                    }),
                },
                WorldActorCommand::Gate { gate, open } => match self.set_gate(*gate, *open) {
                    Ok(changed) => {
                        if changed {
                            events.push(WorldActorSessionEvent::Gate {
                                gate: *gate,
                                open: *open,
                                at: self.set.tick(),
                            });
                        }
                    }
                    Err(error) => refusals.push(WorldActorRefusal::Command {
                        command: command.clone(),
                        error,
                    }),
                },
                WorldActorCommand::Detach { actor, eject_m_s } => {
                    match self.set.detach(*actor, *eject_m_s) {
                        Ok(payload) => {
                            self.collected.remove(actor);
                            events.push(WorldActorSessionEvent::Detached { payload });
                        }
                        Err(error) => refusals.push(WorldActorRefusal::Command {
                            command: command.clone(),
                            error,
                        }),
                    }
                }
                WorldActorCommand::Release {
                    carrier,
                    socket,
                    spec,
                    kind,
                } => match self.lowered.socket(*carrier, *socket) {
                    Some(socket) => match self.anchor_pose(*carrier, &socket) {
                        Ok(anchor) => match self.set.release(&anchor, spec.clone(), *kind) {
                            Ok(payload) => {
                                events.push(WorldActorSessionEvent::Released { payload })
                            }
                            Err(error) => refusals.push(WorldActorRefusal::Command {
                                command: command.clone(),
                                error,
                            }),
                        },
                        Err(error) => refusals.push(WorldActorRefusal::Command {
                            command: command.clone(),
                            error,
                        }),
                    },
                    None => refusals.push(WorldActorRefusal::UnknownSocket {
                        carrier: *carrier,
                        socket: *socket,
                    }),
                },
            }
        }

        while self.set.tick() < input.to {
            let next = Tick(self.set.tick().0 + 1);
            while let Some(transition) = self.pending.front()
                && transition.at <= next
            {
                let transition = self.pending.pop_front().expect("front checked");
                match self.set_gate(transition.gate, transition.open) {
                    Ok(changed) => {
                        if changed {
                            events.push(WorldActorSessionEvent::Gate {
                                gate: transition.gate,
                                open: transition.open,
                                at: next,
                            });
                        }
                    }
                    Err(error) => refusals.push(WorldActorRefusal::Gate {
                        gate: transition.gate,
                        open: transition.open,
                        error,
                    }),
                }
            }
            for event in self.set.step()? {
                events.push(WorldActorSessionEvent::Runtime(event));
            }
            // A world-actor taker's canonical pose exists on every stepped
            // tick, so it is judged per tick: a carrier that passes through
            // a pickup envelope mid-advance latches on the tick it crossed,
            // not whenever the step happened to land.
            for index in 0..self.lowered.pickups.len() {
                if matches!(self.lowered.pickups[index].taker, LoweredTaker::External) {
                    continue;
                }
                let pickup = self.lowered.pickups[index].clone();
                self.judge_pickup(&pickup, None, &mut events, &mut refusals);
            }
        }

        // At the target tick every still-live pickup is judged: an external
        // taker's probe pose is only defined at `to`, and a world-actor
        // taker is judged here only when the call stepped no ticks — the
        // per-tick pass already judged it at `to` otherwise.
        let stepped = self.set.tick() > before;
        for index in 0..self.lowered.pickups.len() {
            let pickup = self.lowered.pickups[index].clone();
            match pickup.taker {
                LoweredTaker::External => {
                    let probe = input
                        .probes
                        .iter()
                        .find(|probe| probe.pickup == pickup.symbol);
                    self.judge_pickup(&pickup, probe, &mut events, &mut refusals);
                }
                LoweredTaker::WorldActor(_) if !stepped => {
                    self.judge_pickup(&pickup, None, &mut events, &mut refusals);
                }
                LoweredTaker::WorldActor(_) => {}
            }
        }

        Ok(WorldActorSessionTick {
            session: self.session,
            tick: self.set.tick(),
            events,
            refusals,
        })
    }

    /// Applies a scripted gate transition through the runtime.
    ///
    /// # Errors
    ///
    /// [`WorldActorError::UnknownActor`].
    fn set_gate(&mut self, gate: ActorId, open: bool) -> Result<bool, WorldActorError> {
        self.set.set_gate_open(gate, open)
    }

    /// Judges one pickup against the set's current tick.
    ///
    /// Skips silently when the pickup already latched, its target left the
    /// world, the taker is not a live world actor, or no probe poses an
    /// external taker; skips without a refusal when the taker fails the
    /// envelope. A satisfied envelope applies the declared completion:
    /// `Latch` emits the judged [`AnchorSample`] for the taker's own
    /// binding transaction, `Attach` rides the target on the taker's
    /// socket and `Collect` takes it out of the world — and a refused
    /// completion lands as a named [`WorldActorRefusal::Completion`].
    fn judge_pickup(
        &mut self,
        pickup: &LoweredPickup,
        probe: Option<&TakerProbe>,
        events: &mut Vec<WorldActorSessionEvent>,
        refusals: &mut Vec<WorldActorRefusal>,
    ) {
        if self.latched.contains(&pickup.symbol)
            || self.set.presence(pickup.target) != Some(Presence::Intact)
        {
            return;
        }
        let taker_pose = match pickup.taker {
            LoweredTaker::WorldActor(taker) => {
                if self.set.presence(taker) != Some(Presence::Intact) {
                    return;
                }
                match self.set.pose(taker) {
                    Ok(pose) => pose,
                    // A registered taker always poses; an unknown one is a
                    // lowering defect, not a runtime case.
                    Err(_) => return,
                }
            }
            LoweredTaker::External => match probe {
                Some(probe) => Pose {
                    position_m: probe.position_m,
                    orientation: Quat::IDENTITY,
                    velocity_m_s: probe.velocity_m_s,
                    angular_velocity_rad_s: [0.0; 3],
                },
                None => return,
            },
        };
        let Ok(target_pose) = self.set.pose(pickup.target) else {
            return;
        };
        let Ok(anchor) = pickup_eligible_pose(
            self.set.tick(),
            &target_pose,
            &pickup.socket,
            taker_pose.position_m,
            taker_pose.velocity_m_s,
            pickup.envelope,
        ) else {
            return;
        };
        let taker = match pickup.taker {
            LoweredTaker::WorldActor(actor) => SessionTaker::WorldActor(actor),
            LoweredTaker::External => SessionTaker::External,
        };
        match pickup.completion {
            LoweredPickupCompletion::Latch => {
                self.latched.insert(pickup.symbol);
                events.push(WorldActorSessionEvent::Pickup {
                    pickup: pickup.symbol,
                    target: pickup.target,
                    taker,
                    completion: SessionCompletion::Bound,
                    at: self.set.tick(),
                    anchor,
                });
            }
            LoweredPickupCompletion::Attach { socket } => {
                let LoweredTaker::WorldActor(carrier) = pickup.taker else {
                    unreachable!("Attach lowers only with a world-actor taker")
                };
                match self.set.attach(pickup.target, carrier, socket) {
                    Ok(_bound) => {
                        self.latched.insert(pickup.symbol);
                        events.push(WorldActorSessionEvent::Pickup {
                            pickup: pickup.symbol,
                            target: pickup.target,
                            taker,
                            completion: SessionCompletion::Attached,
                            at: self.set.tick(),
                            anchor,
                        });
                    }
                    Err(error) => refusals.push(WorldActorRefusal::Completion {
                        pickup: pickup.symbol,
                        error,
                    }),
                }
            }
            LoweredPickupCompletion::Collect => match self.set.collect(pickup.target) {
                Ok(_pose) => {
                    self.latched.insert(pickup.symbol);
                    self.collected.insert(pickup.target);
                    events.push(WorldActorSessionEvent::Pickup {
                        pickup: pickup.symbol,
                        target: pickup.target,
                        taker,
                        completion: SessionCompletion::Collected,
                        at: self.set.tick(),
                        anchor,
                    });
                }
                Err(error) => refusals.push(WorldActorRefusal::Completion {
                    pickup: pickup.symbol,
                    error,
                }),
            },
        }
    }

    /// Tears the current generation down and relaunches the same program
    /// under a new session generation.
    ///
    /// The [`WorldActorTeardown`] is built *first* and names everything the
    /// old session still owned — every registered actor the world must
    /// despawn, the actors an external taker collected, the pickups that
    /// had latched and the gate transitions that never fired. The fresh
    /// set is built before the old one is released, so a launch defect can
    /// never leave the session half-torn-down.
    ///
    /// # Errors
    ///
    /// [`WorldActorLaunchError::SameGeneration`] when `session` is the live
    /// generation, or the `Actor`/`Support` variants when the program that
    /// launched before cannot be registered again — unreachable for a
    /// lowered program, reported rather than assumed.
    pub fn retry(
        &mut self,
        session: SessionGeneration,
    ) -> Result<WorldActorTeardown, WorldActorLaunchError> {
        if session == self.session {
            return Err(WorldActorLaunchError::SameGeneration {
                session: self.session,
            });
        }
        let fresh = Self::build_set(&self.lowered)?;
        let report = WorldActorTeardown {
            session: self.session,
            actors: self.set.actors().collect(),
            collected: self.collected.iter().copied().collect(),
            latched_pickups: self.latched.iter().copied().collect(),
            pending_transitions: self.pending.iter().copied().collect(),
        };
        self.set = fresh;
        self.session = session;
        self.pending = self.lowered.transitions.iter().copied().collect();
        self.latched.clear();
        self.collected.clear();
        Ok(report)
    }
}

/// Component: marks an entity as the visual/physical face of one world
/// actor.
///
/// `actor` is the actor's [`ActorId`], `subject` the catalog subject it was
/// built from and `generation` the scene generation that spawned the
/// binding — so a reload stamps new bindings and stale ones are identified
/// by mismatch, never by surviving pointers (the `STATE-TRANSACTIONS`
/// session-generation discipline).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct WorldActorBinding {
    /// The actor this entity presents.
    pub actor: ActorId,
    /// The catalog subject the actor was built from.
    pub subject: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}
