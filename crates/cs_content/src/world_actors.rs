//! The declared world-actor schema: provenance-carrying rail, road, water
//! and kinematic mission actors with their sockets, carriage, pickups and
//! scripted gate transitions (F34-C).
//!
//! Spec: `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`,
//! stage `### F34-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module is the **content half** of the world-actor contract — the
//! normalized record a mission-content importer produces and the catalog
//! consumes. Its runtime counterpart is `cs_sim::world_actors::runtime`
//! (the `WorldActorSet`), and the lowering boundary plus the session
//! producer→consumer wiring is `cs_app::world_actors`. This crate cannot
//! depend on `cs_sim` or `cs_script`, so the declared record keeps its own
//! typed vocabulary and reuses the program-level identities of
//! [`crate::objectives`]: [`ProgramActor`] maps one-to-one onto
//! `cs_script::ir::ActorId` and [`ProgramSymbol`] onto `SymbolId` at the
//! lowering boundary.
//!
//! # Records
//!
//! A [`DeclaredWorldActorProgram`] is the whole world-actor surface of one
//! mission: its tick rate, every [`DeclaredWorldActor`] in authored order,
//! the declared support edges between them, the declared [`DeclaredPickup`]s
//! and the declared [`DeclaredGateTransition`] schedule a script would
//! issue. Each actor is a catalog-driven kind — rail, road, water or
//! mission machinery — with a faction, an optional objective identity, a
//! [`DeclaredMotion`] and named [`DeclaredSocket`] attachment points.
//!
//! Every load-bearing value is a [`Resolved`], so an unmeasured position,
//! speed, envelope or tick rate stays an explicit unknown with its claim id
//! and reason instead of a silent default (F14 non-negotiable behavior 3).
//!
//! # Designed vocabulary, not original data
//!
//! The original 2000 PC game's world-actor encoding is **not decoded**: no
//! original actor-kind record, socket table, gate schedule or pickup
//! declaration has been measured
//! (`docs/findings/2026-10-01-f34-a-world-actor-motion-and-dependency.md`).
//! Every id grammar, kind name, motion shape and fixture value here is
//! **newly authored project design** carrying `Origin::Designed` /
//! `Origin::SyntheticFixture` provenance. Nothing in this module is a
//! measurement of the original game.
//!
//! `try_new` validates only what the record itself guarantees: unique actor
//! and socket ids, unique pickup symbols and finite known values. Every
//! cross-reference — a carried actor's carrier and socket, a route's gate,
//! a pickup's target, a transition's gate — resolves at the lowering
//! boundary or is refused by the runtime at launch, never guessed.

use std::collections::HashSet;
use std::fmt;

use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use crate::objectives::{ProgramActor, ProgramSymbol};

/// The catalog kind a world actor declares as.
///
/// Designed vocabulary: the original's actor-type enumeration is
/// unmeasured, so the schema distinguishes only the motion domains the spec
/// names — rail, road, water and mission machinery. Kind is identity, not
/// a motion capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclaredWorldActorKind {
    /// Rail-bound stock: trains and stock cars.
    Rail,
    /// Road vehicles: trucks, convoy cars.
    Road,
    /// Watercraft: boats and barges.
    Water,
    /// Mission machinery: gates, generators, elevators and similar.
    Kinematic,
}

impl DeclaredWorldActorKind {
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

/// A named attachment point declared on an actor, offset in the actor's
/// frame. The id is stable so a pickup or a carriage declaration names the
/// point, not its index in a list.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredSocket {
    /// The socket's id on its actor.
    pub socket: u16,
    /// Offset from the actor origin, in the actor's frame, or an explicit
    /// unknown.
    pub offset_m: Resolved<[f64; 3]>,
}

/// One authored keyframe: the pose at an integer tick.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredKeyframe {
    /// The key's tick.
    pub tick: u64,
    /// The pose's position, or an explicit unknown.
    pub position_m: Resolved<[f64; 3]>,
    /// The pose's orientation quaternion `[x, y, z, w]`, or an unknown.
    pub orientation: Resolved<[f64; 4]>,
}

/// An authored tick-indexed path: timetabled trains, elevators and other
/// machinery whose schedule cannot wait.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredPath {
    /// The authored keys.
    pub keyframes: Vec<DeclaredKeyframe>,
    /// The tick rate the keys index and sampled velocities derive from, or
    /// an explicit unknown.
    pub ticks_per_second: Resolved<u32>,
}

/// One declared gate passage on a route.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredRouteGate {
    /// The actor whose presence and scripted open state control the
    /// passage. It may name an actor declared anywhere in the same program;
    /// resolution is the lowering's, not the schema's.
    pub gate: ProgramActor,
    /// Where the gate stands along the route as arc length, or an unknown.
    pub at_m: Resolved<f64>,
    /// How far before `at_m` a follower must hold, or an unknown.
    pub stop_before_m: Resolved<f64>,
}

/// A declared gate-aware polyline a route follower drives at one authored
/// cruise speed.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredRoute {
    /// The ordered waypoints in world meters, or an explicit unknown.
    pub points: Resolved<Vec<[f64; 3]>>,
    /// The authored cruise speed in m/s, or an unknown.
    pub speed_m_s: Resolved<f64>,
    /// The arc length the follower starts at, or an unknown.
    pub start_progress_m: Resolved<f64>,
    /// The declared gate passages. Their ascending order and bounds are the
    /// runtime's validation, propagated through the lowering.
    pub gates: Vec<DeclaredRouteGate>,
}

/// How one declared actor moves.
#[derive(Clone, Debug, PartialEq)]
pub enum DeclaredMotion {
    /// A fixed pose: gates, generators and parked machinery.
    Held {
        /// World position, or an explicit unknown.
        position_m: Resolved<[f64; 3]>,
        /// World orientation, or an unknown.
        orientation: Resolved<[f64; 4]>,
    },
    /// An authored tick-indexed path.
    Path(DeclaredPath),
    /// A gate-aware route follower.
    Route(DeclaredRoute),
    /// A payload drifting on its own velocity.
    Free {
        /// World position, or an unknown.
        position_m: Resolved<[f64; 3]>,
        /// World velocity, or an unknown.
        velocity_m_s: Resolved<[f64; 3]>,
        /// World orientation, or an unknown.
        orientation: Resolved<[f64; 4]>,
    },
    /// Cargo riding a carrier's declared socket: a boat on a deck, a crate
    /// on a truck bed. The pose derives from the carrier every tick until a
    /// release or a collection ends the carriage.
    Carried {
        /// The actor whose socket this one rides.
        carrier: ProgramActor,
        /// The socket id on the carrier's own declared sockets.
        socket: u16,
    },
}

/// One declared actor of a world-actor program.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredWorldActor {
    /// Its program-level identity; maps onto `ActorId` at the boundary.
    pub actor: ProgramActor,
    /// The catalog subject the actor is built from.
    pub subject: ContentId,
    /// Its catalog kind.
    pub kind: DeclaredWorldActorKind,
    /// Its allegiance, or an explicit unknown.
    pub faction: Resolved<ContentId>,
    /// The objective this actor counts for, if any.
    pub objective: Option<ProgramSymbol>,
    /// How it moves.
    pub motion: DeclaredMotion,
    /// Its declared attachment sockets.
    pub sockets: Vec<DeclaredSocket>,
}

/// A declared support edge: `dependent` is lost when `supporter` is
/// destroyed. Edges are data, never name checks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeclaredSupport {
    /// The actor whose destruction cascades.
    pub supporter: ProgramActor,
    /// The actor lost with it.
    pub dependent: ProgramActor,
}

/// The declared pickup envelope: the bounds a taker must satisfy against
/// the target's anchor.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredPickupEnvelope {
    /// The largest taker-to-anchor distance, in meters, or an unknown.
    pub max_distance_m: Resolved<f64>,
    /// The largest relative speed, in m/s, or an unknown.
    pub max_relative_speed_m_s: Resolved<f64>,
}

/// Who drives a pickup to its latch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclaredTaker {
    /// A world actor of the same program: a carrier winching a boat aboard.
    WorldActor(ProgramActor),
    /// Something outside the world-actor set — the player's own craft —
    /// whose pose the session is probed with each tick.
    External,
}

/// What a satisfied pickup envelope does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclaredPickupCompletion {
    /// The taker binds onto the target's anchor itself — the plane latching
    /// to a moving train's pickup point. The latched anchor sample is the
    /// binding's input; attaching the taker's own body is its consumer's
    /// (F36-B).
    Latch,
    /// The target actor attaches to a socket on a world-actor taker and
    /// rides it: a carrier winching a boat aboard. Only a
    /// [`DeclaredTaker::WorldActor`] can own the socket.
    Attach {
        /// The socket id on the taker's declared sockets.
        socket: u16,
    },
    /// The target actor is collected out of the world by the taker: its
    /// world motion ends and the taker owns it from the latch on.
    Collect,
}

/// One declared pickup: a stable symbol, the actor and socket that are the
/// pickup point, the envelope a taker must satisfy, who takes it and what
/// the latch does.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredPickup {
    /// Its program-level identity; maps onto `SymbolId` at the boundary.
    pub symbol: ProgramSymbol,
    /// The actor whose socket is the pickup point.
    pub target: ProgramActor,
    /// The socket id on the target's declared sockets.
    pub socket: u16,
    /// The bounds a taker must satisfy.
    pub envelope: DeclaredPickupEnvelope,
    /// Who may drive the pickup.
    pub taker: DeclaredTaker,
    /// What a satisfied envelope does.
    pub completion: DeclaredPickupCompletion,
}

/// One scripted gate transition: at tick `at` the gate `gate` opens or
/// closes without being destroyed. Unlike destruction the close may
/// re-impose a passage on a follower that has not crossed — never pull one
/// back that has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeclaredGateTransition {
    /// The tick the transition takes effect on.
    pub at: Tick,
    /// The gate actor. It may name an actor declared anywhere in the same
    /// program; resolution is the lowering's or the session's, and a gate
    /// that never registers is reported when the transition fires.
    pub gate: ProgramActor,
    /// Whether the passage opens (`true`) or closes (`false`).
    pub open: bool,
}

/// Why a declared world-actor program was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldActorSchemaError {
    /// Two actors declared the same program identity.
    DuplicateActor {
        /// The duplicated identity.
        actor: ProgramActor,
    },
    /// One actor declared two sockets with the same id.
    DuplicateSocket {
        /// The actor.
        actor: ProgramActor,
        /// The duplicated socket id.
        socket: u16,
    },
    /// Two pickups declared the same symbol.
    DuplicatePickupSymbol {
        /// The duplicated symbol.
        symbol: ProgramSymbol,
    },
    /// A known value was NaN or infinite.
    NonFinite {
        /// The actor it belongs to, when it is actor-scoped.
        actor: Option<ProgramActor>,
        /// The offending field.
        field: &'static str,
    },
    /// A known envelope bound was negative.
    NegativeEnvelope {
        /// Which field.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// A pickup's taker is its own target: nothing can take itself.
    PickupOnSelf {
        /// The pickup.
        symbol: ProgramSymbol,
    },
    /// An `Attach` completion names an external taker, which owns no socket.
    AttachOnExternalTaker {
        /// The pickup.
        symbol: ProgramSymbol,
    },
    /// The declared tick rate was a known zero.
    ZeroTickRate,
    /// A declared path has no keyframes.
    EmptyKeyframes {
        /// The actor carrying the empty path.
        actor: ProgramActor,
    },
}

impl fmt::Display for WorldActorSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateActor { actor } => write!(f, "duplicate actor {actor}"),
            Self::DuplicateSocket { actor, socket } => {
                write!(f, "duplicate socket {socket} on {actor}")
            }
            Self::DuplicatePickupSymbol { symbol } => {
                write!(f, "duplicate pickup symbol {symbol}")
            }
            Self::NonFinite { actor, field } => match actor {
                Some(actor) => write!(f, "{actor}'s {field} must be finite"),
                None => write!(f, "{field} must be finite"),
            },
            Self::NegativeEnvelope { field, value } => {
                write!(f, "envelope {field} {value} must not be negative")
            }
            Self::PickupOnSelf { symbol } => {
                write!(f, "pickup {symbol} names its own target as taker")
            }
            Self::AttachOnExternalTaker { symbol } => write!(
                f,
                "pickup {symbol} attaches to an external taker, which owns no socket"
            ),
            Self::ZeroTickRate => write!(f, "the tick rate must be greater than zero"),
            Self::EmptyKeyframes { actor } => {
                write!(f, "{actor}'s path has no keyframes")
            }
        }
    }
}

impl std::error::Error for WorldActorSchemaError {}

fn check_f64(
    value: &Resolved<f64>,
    actor: Option<ProgramActor>,
    field: &'static str,
) -> Result<(), WorldActorSchemaError> {
    if let Resolved::Known(known) = value
        && !known.value.is_finite()
    {
        return Err(WorldActorSchemaError::NonFinite { actor, field });
    }
    Ok(())
}

fn check_values(
    value: &Resolved<impl AsRef<[f64]>>,
    actor: ProgramActor,
    field: &'static str,
) -> Result<(), WorldActorSchemaError> {
    if let Resolved::Known(known) = value
        && !known.value.as_ref().iter().all(|v| v.is_finite())
    {
        return Err(WorldActorSchemaError::NonFinite {
            actor: Some(actor),
            field,
        });
    }
    Ok(())
}

/// The whole world-actor surface of one mission: the tick rate, the
/// declared actors, support edges, pickups and the scripted gate schedule.
///
/// Construction is [`DeclaredWorldActorProgram::try_new`] only, which
/// refuses duplicate identities and non-finite known values once. Every
/// cross-reference resolves later: the lowering names the carried carrier,
/// socket, gate or pickup target a program does not declare, and the
/// runtime refuses what registration cannot place.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredWorldActorProgram {
    subject: ContentId,
    origin: Origin,
    provenance: Provenance,
    ticks_per_second: Resolved<u32>,
    actors: Vec<DeclaredWorldActor>,
    support: Vec<DeclaredSupport>,
    pickups: Vec<DeclaredPickup>,
    transitions: Vec<DeclaredGateTransition>,
}

/// The parts of a [`DeclaredWorldActorProgram`], collected so the
/// validating constructor takes one record.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredWorldActorParts {
    /// The program's tick rate, or an explicit unknown.
    pub ticks_per_second: Resolved<u32>,
    /// The declared actors, in authored order.
    pub actors: Vec<DeclaredWorldActor>,
    /// The declared support edges.
    pub support: Vec<DeclaredSupport>,
    /// The declared pickups.
    pub pickups: Vec<DeclaredPickup>,
    /// The scripted gate schedule.
    pub transitions: Vec<DeclaredGateTransition>,
}

impl DeclaredWorldActorProgram {
    /// Validates and assembles a declared world-actor program.
    ///
    /// # Errors
    ///
    /// [`WorldActorSchemaError`] naming the first violated rule.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        provenance: Provenance,
        parts: DeclaredWorldActorParts,
    ) -> Result<Self, WorldActorSchemaError> {
        let DeclaredWorldActorParts {
            ticks_per_second,
            actors,
            support,
            pickups,
            transitions,
        } = parts;

        if let Resolved::Known(known) = &ticks_per_second
            && known.value == 0
        {
            return Err(WorldActorSchemaError::ZeroTickRate);
        }

        let mut actor_ids = HashSet::new();
        for actor in &actors {
            if !actor_ids.insert(actor.actor) {
                return Err(WorldActorSchemaError::DuplicateActor { actor: actor.actor });
            }
            let mut socket_ids = HashSet::new();
            for socket in &actor.sockets {
                if !socket_ids.insert(socket.socket) {
                    return Err(WorldActorSchemaError::DuplicateSocket {
                        actor: actor.actor,
                        socket: socket.socket,
                    });
                }
                check_values(&socket.offset_m, actor.actor, "offset_m")?;
            }
            match &actor.motion {
                DeclaredMotion::Held {
                    position_m,
                    orientation,
                } => {
                    check_values(position_m, actor.actor, "position_m")?;
                    check_values(orientation, actor.actor, "orientation")?;
                }
                DeclaredMotion::Path(path) => {
                    if path.keyframes.is_empty() {
                        return Err(WorldActorSchemaError::EmptyKeyframes { actor: actor.actor });
                    }
                    for key in &path.keyframes {
                        check_values(&key.position_m, actor.actor, "position_m")?;
                        check_values(&key.orientation, actor.actor, "orientation")?;
                    }
                }
                DeclaredMotion::Route(route) => {
                    if let Resolved::Known(known) = &route.points {
                        for point in &known.value {
                            if !point.iter().all(|v| v.is_finite()) {
                                return Err(WorldActorSchemaError::NonFinite {
                                    actor: Some(actor.actor),
                                    field: "points",
                                });
                            }
                        }
                    }
                    check_f64(&route.speed_m_s, Some(actor.actor), "speed_m_s")?;
                    check_f64(
                        &route.start_progress_m,
                        Some(actor.actor),
                        "start_progress_m",
                    )?;
                    for gate in &route.gates {
                        check_f64(&gate.at_m, Some(actor.actor), "at_m")?;
                        check_f64(&gate.stop_before_m, Some(actor.actor), "stop_before_m")?;
                    }
                }
                DeclaredMotion::Free {
                    position_m,
                    velocity_m_s,
                    orientation,
                } => {
                    check_values(position_m, actor.actor, "position_m")?;
                    check_values(velocity_m_s, actor.actor, "velocity_m_s")?;
                    check_values(orientation, actor.actor, "orientation")?;
                }
                DeclaredMotion::Carried { .. } => {}
            }
        }

        let mut pickup_symbols = HashSet::new();
        for pickup in &pickups {
            if !pickup_symbols.insert(pickup.symbol) {
                return Err(WorldActorSchemaError::DuplicatePickupSymbol {
                    symbol: pickup.symbol,
                });
            }
            if matches!(pickup.taker, DeclaredTaker::WorldActor(t) if t == pickup.target) {
                return Err(WorldActorSchemaError::PickupOnSelf {
                    symbol: pickup.symbol,
                });
            }
            if matches!(pickup.completion, DeclaredPickupCompletion::Attach { .. })
                && matches!(pickup.taker, DeclaredTaker::External)
            {
                return Err(WorldActorSchemaError::AttachOnExternalTaker {
                    symbol: pickup.symbol,
                });
            }
            for (field, value) in [
                ("max_distance_m", &pickup.envelope.max_distance_m),
                (
                    "max_relative_speed_m_s",
                    &pickup.envelope.max_relative_speed_m_s,
                ),
            ] {
                check_f64(value, None, field)?;
                if let Resolved::Known(known) = value
                    && known.value < 0.0
                {
                    return Err(WorldActorSchemaError::NegativeEnvelope {
                        field,
                        value: known.value,
                    });
                }
            }
        }

        Ok(Self {
            subject,
            origin,
            provenance,
            ticks_per_second,
            actors,
            support,
            pickups,
            transitions,
        })
    }

    /// The catalog subject the program was authored under.
    #[must_use]
    pub const fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The record's provenance.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The program's tick rate, or an explicit unknown.
    #[must_use]
    pub const fn ticks_per_second(&self) -> &Resolved<u32> {
        &self.ticks_per_second
    }

    /// The declared actors, in authored order.
    #[must_use]
    pub fn actors(&self) -> &[DeclaredWorldActor] {
        &self.actors
    }

    /// One declared actor by its program identity.
    #[must_use]
    pub fn actor(&self, actor: ProgramActor) -> Option<&DeclaredWorldActor> {
        self.actors.iter().find(|a| a.actor == actor)
    }

    /// The declared support edges.
    #[must_use]
    pub fn support(&self) -> &[DeclaredSupport] {
        &self.support
    }

    /// The declared pickups.
    #[must_use]
    pub fn pickups(&self) -> &[DeclaredPickup] {
        &self.pickups
    }

    /// The scripted gate schedule.
    #[must_use]
    pub fn transitions(&self) -> &[DeclaredGateTransition] {
        &self.transitions
    }
}

// ---------------------------------------------------------------------------
// The synthetic fixture
// ---------------------------------------------------------------------------

/// The fixture's gate: stands over the convoy route at the 50 m mark.
pub const SYNTHETIC_GATE: ProgramActor = ProgramActor(1);
/// The fixture's fast convoy: 100 m along +X at 10 m/s.
pub const SYNTHETIC_CONVOY: ProgramActor = ProgramActor(2);
/// The fixture's bridge: the convoy's support edge hangs on it.
pub const SYNTHETIC_BRIDGE: ProgramActor = ProgramActor(3);
/// The fixture's boat: carried on the carrier's deck socket until released
/// or collected.
pub const SYNTHETIC_BOAT: ProgramActor = ProgramActor(4);
/// The fixture's carrier: a water actor moving +Y at 8 m/s with a deck and
/// a stern socket.
pub const SYNTHETIC_CARRIER: ProgramActor = ProgramActor(5);
/// The fixture's train: 100 m along +X at 10 m/s on its own timetable.
pub const SYNTHETIC_TRAIN: ProgramActor = ProgramActor(6);
/// The fixture's crate: a held pickup target the carrier collects into its
/// stern socket.
pub const SYNTHETIC_CRATE: ProgramActor = ProgramActor(8);
/// The fixture's slow convoy: 100 m along +X at 4 m/s, still short of the
/// gate when the scripted close lands.
pub const SYNTHETIC_TRUCK: ProgramActor = ProgramActor(9);

/// The fixture's cargo pickup: an external taker collects the boat off the
/// carrier's deck.
pub const SYNTHETIC_PICKUP_BOAT: ProgramSymbol = ProgramSymbol(10);
/// The fixture's winch pickup: the carrier attaches the crate to its stern
/// socket.
pub const SYNTHETIC_PICKUP_CRATE: ProgramSymbol = ProgramSymbol(11);
/// The fixture's train-roof latch: an external taker binds to the anchor.
pub const SYNTHETIC_PICKUP_TRAIN: ProgramSymbol = ProgramSymbol(12);

/// The carrier's deck socket, which the boat rides.
pub const SYNTHETIC_DECK_SOCKET: u16 = 1;
/// The carrier's stern socket, which a won crate rides.
pub const SYNTHETIC_STERN_SOCKET: u16 = 2;
/// The pickup socket on the boat, crate and train.
pub const SYNTHETIC_PICKUP_SOCKET: u16 = 1;

fn designed<T>(value: T, provenance: &Provenance) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance.clone()))
}

/// The minimal synthetic world-actor program in declared form: a kinematic
/// gate standing over the convoy route with a scripted open at tick 60 and
/// a scripted close at tick 80; a fast convoy held at the gate, a slow
/// truck still short of it when the close lands; a water carrier moving
/// +Y carrying a boat on its deck socket; a held crate the carrier's
/// `Attach` pickup wenches to its stern socket; a rail train whose roof
/// socket an external taker latches to; and one support edge hanging the
/// convoy on the bridge.
///
/// Every pose, speed, envelope and schedule value is newly authored
/// project design under `Origin::SyntheticFixture` with designed
/// provenance — it can never be mistaken for retail content, and the
/// original's actor records are unmeasured either way.
#[must_use]
pub fn declared_synthetic_world_actors() -> DeclaredWorldActorProgram {
    let provenance =
        Provenance::designed(ClaimId::new("f34c.synthetic-harbor").expect("valid claim id"));
    let subject =
        |key: &str| ContentId::from_source(ContentKind::SceneNode, key).expect("valid subject id");
    let raiders = ContentId::from_source(ContentKind::Faction, "synthetic.raiders")
        .expect("valid faction id");
    let faction = || designed(raiders.clone(), &provenance);
    let pose = |position_m: [f64; 3]| DeclaredMotion::Held {
        position_m: designed(position_m, &provenance),
        orientation: designed([0.0, 0.0, 0.0, 1.0], &provenance),
    };
    let route = |speed_m_s: f64| {
        DeclaredMotion::Route(DeclaredRoute {
            points: designed(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]], &provenance),
            speed_m_s: designed(speed_m_s, &provenance),
            start_progress_m: designed(0.0, &provenance),
            gates: vec![DeclaredRouteGate {
                gate: SYNTHETIC_GATE,
                at_m: designed(50.0, &provenance),
                stop_before_m: designed(5.0, &provenance),
            }],
        })
    };
    let socket = |socket: u16, offset_m: [f64; 3]| DeclaredSocket {
        socket,
        offset_m: designed(offset_m, &provenance),
    };

    DeclaredWorldActorProgram::try_new(
        ContentId::from_source(ContentKind::Mission, "synthetic.f34c.harbor")
            .expect("valid mission id"),
        Origin::SyntheticFixture,
        provenance.clone(),
        DeclaredWorldActorParts {
            ticks_per_second: designed(10, &provenance),
            actors: vec![
                DeclaredWorldActor {
                    actor: SYNTHETIC_GATE,
                    subject: subject("synthetic.f34c.gate"),
                    kind: DeclaredWorldActorKind::Kinematic,
                    faction: faction(),
                    objective: None,
                    motion: pose([50.0, 0.0, 0.0]),
                    sockets: vec![],
                },
                DeclaredWorldActor {
                    actor: SYNTHETIC_BRIDGE,
                    subject: subject("synthetic.f34c.bridge"),
                    kind: DeclaredWorldActorKind::Kinematic,
                    faction: faction(),
                    objective: None,
                    motion: pose([30.0, 0.0, 0.0]),
                    sockets: vec![],
                },
                DeclaredWorldActor {
                    actor: SYNTHETIC_CARRIER,
                    subject: subject("synthetic.f34c.carrier"),
                    kind: DeclaredWorldActorKind::Water,
                    faction: faction(),
                    objective: None,
                    motion: DeclaredMotion::Free {
                        position_m: designed([200.0, 0.0, 0.0], &provenance),
                        velocity_m_s: designed([0.0, 8.0, 0.0], &provenance),
                        orientation: designed([0.0, 0.0, 0.0, 1.0], &provenance),
                    },
                    sockets: vec![
                        socket(SYNTHETIC_DECK_SOCKET, [0.0, 3.0, 0.0]),
                        socket(SYNTHETIC_STERN_SOCKET, [-5.0, 3.0, 0.0]),
                    ],
                },
                DeclaredWorldActor {
                    actor: SYNTHETIC_CONVOY,
                    subject: subject("synthetic.f34c.convoy"),
                    kind: DeclaredWorldActorKind::Road,
                    faction: faction(),
                    objective: None,
                    motion: route(10.0),
                    sockets: vec![],
                },
                DeclaredWorldActor {
                    actor: SYNTHETIC_TRUCK,
                    subject: subject("synthetic.f34c.truck"),
                    kind: DeclaredWorldActorKind::Road,
                    faction: faction(),
                    objective: None,
                    motion: route(4.0),
                    sockets: vec![],
                },
                DeclaredWorldActor {
                    actor: SYNTHETIC_BOAT,
                    subject: subject("synthetic.f34c.boat"),
                    kind: DeclaredWorldActorKind::Water,
                    faction: faction(),
                    objective: Some(ProgramSymbol(21)),
                    motion: DeclaredMotion::Carried {
                        carrier: SYNTHETIC_CARRIER,
                        socket: SYNTHETIC_DECK_SOCKET,
                    },
                    sockets: vec![socket(SYNTHETIC_PICKUP_SOCKET, [0.0, 0.0, 0.0])],
                },
                DeclaredWorldActor {
                    actor: SYNTHETIC_CRATE,
                    subject: subject("synthetic.f34c.crate"),
                    kind: DeclaredWorldActorKind::Kinematic,
                    faction: faction(),
                    objective: None,
                    motion: pose([200.0, 40.0, 0.0]),
                    sockets: vec![socket(SYNTHETIC_PICKUP_SOCKET, [0.0, 0.0, 0.0])],
                },
                DeclaredWorldActor {
                    actor: SYNTHETIC_TRAIN,
                    subject: subject("synthetic.f34c.train"),
                    kind: DeclaredWorldActorKind::Rail,
                    faction: faction(),
                    objective: None,
                    motion: DeclaredMotion::Path(DeclaredPath {
                        keyframes: vec![
                            DeclaredKeyframe {
                                tick: 0,
                                position_m: designed([0.0, 0.0, -100.0], &provenance),
                                orientation: designed([0.0, 0.0, 0.0, 1.0], &provenance),
                            },
                            DeclaredKeyframe {
                                tick: 100,
                                position_m: designed([100.0, 0.0, -100.0], &provenance),
                                orientation: designed([0.0, 0.0, 0.0, 1.0], &provenance),
                            },
                        ],
                        ticks_per_second: designed(10, &provenance),
                    }),
                    sockets: vec![socket(SYNTHETIC_PICKUP_SOCKET, [0.0, 3.0, 0.0])],
                },
            ],
            support: vec![DeclaredSupport {
                supporter: SYNTHETIC_BRIDGE,
                dependent: SYNTHETIC_CONVOY,
            }],
            pickups: vec![
                DeclaredPickup {
                    symbol: SYNTHETIC_PICKUP_BOAT,
                    target: SYNTHETIC_BOAT,
                    socket: SYNTHETIC_PICKUP_SOCKET,
                    envelope: DeclaredPickupEnvelope {
                        max_distance_m: designed(5.0, &provenance),
                        max_relative_speed_m_s: designed(12.0, &provenance),
                    },
                    taker: DeclaredTaker::External,
                    completion: DeclaredPickupCompletion::Collect,
                },
                DeclaredPickup {
                    symbol: SYNTHETIC_PICKUP_CRATE,
                    target: SYNTHETIC_CRATE,
                    socket: SYNTHETIC_PICKUP_SOCKET,
                    envelope: DeclaredPickupEnvelope {
                        max_distance_m: designed(2.0, &provenance),
                        max_relative_speed_m_s: designed(12.0, &provenance),
                    },
                    taker: DeclaredTaker::WorldActor(SYNTHETIC_CARRIER),
                    completion: DeclaredPickupCompletion::Attach {
                        socket: SYNTHETIC_STERN_SOCKET,
                    },
                },
                DeclaredPickup {
                    symbol: SYNTHETIC_PICKUP_TRAIN,
                    target: SYNTHETIC_TRAIN,
                    socket: SYNTHETIC_PICKUP_SOCKET,
                    envelope: DeclaredPickupEnvelope {
                        max_distance_m: designed(5.0, &provenance),
                        max_relative_speed_m_s: designed(12.0, &provenance),
                    },
                    taker: DeclaredTaker::External,
                    completion: DeclaredPickupCompletion::Latch,
                },
            ],
            transitions: vec![
                DeclaredGateTransition {
                    at: Tick(60),
                    gate: SYNTHETIC_GATE,
                    open: true,
                },
                DeclaredGateTransition {
                    at: Tick(80),
                    gate: SYNTHETIC_GATE,
                    open: false,
                },
            ],
        },
    )
    .expect("the synthetic world-actor program is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> DeclaredWorldActorProgram {
        declared_synthetic_world_actors()
    }

    #[test]
    fn accept_f34_c_the_declared_program_is_self_contained_and_typed() {
        let program = program();
        assert_eq!(program.subject().kind(), ContentKind::Mission);
        assert_eq!(program.origin(), &Origin::SyntheticFixture);
        assert_eq!(
            program.ticks_per_second(),
            &Resolved::Known(Known::new(
                10,
                Provenance::designed(
                    ClaimId::new("f34c.synthetic-harbor").expect("valid claim id")
                )
            ))
        );
        assert_eq!(program.actors().len(), 8);
        assert_eq!(program.support().len(), 1);
        assert_eq!(program.pickups().len(), 3);
        assert_eq!(program.transitions().len(), 2);
        assert!(
            program.actor(SYNTHETIC_BOAT).is_some() && program.actor(ProgramActor(77)).is_none()
        );
    }

    #[test]
    fn accept_f34_c_unknown_fields_stay_unknown_with_provenance() {
        let base = program();
        let mut actors = base.actors().to_vec();
        let convoy = actors
            .iter_mut()
            .find(|a| a.actor == SYNTHETIC_CONVOY)
            .expect("declared");
        convoy.motion = DeclaredMotion::Route(DeclaredRoute {
            points: designed(vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]], base.provenance()),
            speed_m_s: Resolved::unknown(
                ClaimId::new("f34c.test.unmeasured-speed").expect("valid claim id"),
                "the original convoy speed is unmeasured",
            )
            .expect("a reasoned unknown"),
            start_progress_m: designed(0.0, base.provenance()),
            gates: vec![DeclaredRouteGate {
                gate: SYNTHETIC_GATE,
                at_m: designed(50.0, base.provenance()),
                stop_before_m: designed(5.0, base.provenance()),
            }],
        });
        let program = DeclaredWorldActorProgram::try_new(
            base.subject().clone(),
            base.origin().clone(),
            base.provenance().clone(),
            DeclaredWorldActorParts {
                ticks_per_second: base.ticks_per_second().clone(),
                actors,
                support: base.support().to_vec(),
                pickups: base.pickups().to_vec(),
                transitions: base.transitions().to_vec(),
            },
        )
        .expect("an unknown speed is a legal declaration");
        let DeclaredMotion::Route(route) =
            &program.actor(SYNTHETIC_CONVOY).expect("declared").motion
        else {
            panic!("the convoy is a route follower");
        };
        assert!(matches!(route.speed_m_s, Resolved::Unknown { .. }));
    }

    #[test]
    fn accept_f34_c_try_new_refuses_what_the_record_must_guarantee() {
        let base = program();
        let rebuild = |actors, support, pickups, transitions| {
            DeclaredWorldActorProgram::try_new(
                base.subject().clone(),
                base.origin().clone(),
                base.provenance().clone(),
                DeclaredWorldActorParts {
                    ticks_per_second: base.ticks_per_second().clone(),
                    actors,
                    support,
                    pickups,
                    transitions,
                },
            )
        };

        // Two actors cannot share an identity.
        let mut actors = base.actors().to_vec();
        actors.push(actors[0].clone());
        assert!(matches!(
            rebuild(actors, vec![], vec![], vec![]),
            Err(WorldActorSchemaError::DuplicateActor { .. })
        ));

        // Nor two sockets on one actor.
        let mut actors = base.actors().to_vec();
        let carrier = actors
            .iter_mut()
            .find(|a| a.actor == SYNTHETIC_CARRIER)
            .expect("declared");
        carrier.sockets.push(carrier.sockets[0].clone());
        assert!(matches!(
            rebuild(actors, vec![], vec![], vec![]),
            Err(WorldActorSchemaError::DuplicateSocket {
                actor: SYNTHETIC_CARRIER,
                ..
            })
        ));

        // A pickup cannot name its own target as taker.
        let mut pickups = base.pickups().to_vec();
        pickups[0].taker = DeclaredTaker::WorldActor(SYNTHETIC_BOAT);
        assert_eq!(
            rebuild(vec![], vec![], pickups, vec![]),
            Err(WorldActorSchemaError::PickupOnSelf {
                symbol: SYNTHETIC_PICKUP_BOAT,
            })
        );

        // An external taker owns no socket to attach to.
        let mut pickups = base.pickups().to_vec();
        pickups[1].taker = DeclaredTaker::External;
        assert_eq!(
            rebuild(vec![], vec![], pickups, vec![]),
            Err(WorldActorSchemaError::AttachOnExternalTaker {
                symbol: SYNTHETIC_PICKUP_CRATE,
            })
        );

        // A negative envelope bound is refused, not clamped.
        let mut pickups = base.pickups().to_vec();
        pickups[0].envelope.max_distance_m = designed(
            -1.0,
            &Provenance::designed(ClaimId::new("f34c.test.envelope").expect("valid claim id")),
        );
        assert!(matches!(
            rebuild(vec![], vec![], pickups, vec![]),
            Err(WorldActorSchemaError::NegativeEnvelope { .. })
        ));

        // A known-zero tick rate cannot drive a session.
        let zero = DeclaredWorldActorProgram::try_new(
            base.subject().clone(),
            base.origin().clone(),
            base.provenance().clone(),
            DeclaredWorldActorParts {
                ticks_per_second: designed(
                    0,
                    &Provenance::designed(
                        ClaimId::new("f34c.test.zero-rate").expect("valid claim id"),
                    ),
                ),
                actors: vec![],
                support: vec![],
                pickups: vec![],
                transitions: vec![],
            },
        );
        assert_eq!(zero, Err(WorldActorSchemaError::ZeroTickRate));

        // A support edge never names an actor: data, not identities.
        assert_eq!(
            rebuild(
                vec![],
                vec![DeclaredSupport {
                    supporter: SYNTHETIC_CONVOY,
                    dependent: SYNTHETIC_BRIDGE,
                }],
                vec![],
                vec![],
            )
            .expect("a reversed edge is still a valid record")
            .support()[0],
            DeclaredSupport {
                supporter: SYNTHETIC_CONVOY,
                dependent: SYNTHETIC_BRIDGE,
            }
        );
    }
}
