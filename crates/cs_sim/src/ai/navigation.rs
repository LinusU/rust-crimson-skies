//! AI navigation: route graph, maneuver envelope and bounded route following
//! (F31-A).
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Stage **F31-A** defines the typed contract and a minimal synthetic fixture;
//! it is not the whole runtime. The consumer half of the route contract lives
//! here; the provenance-carrying producer record a route importer will emit is
//! `cs_content::routes`. The conversion boundary between them is F31-C.
//!
//! [`navigation`] declares:
//!
//! * [`RouteGraph`], the Bevy-free normalized route the follower consumes —
//!   nodes with stable [`RouteNodeId`]s **and** authored sequences, an
//!   arrival radius per node and the route's minimum [`clearance`].
//! * [`ManeuverEnvelope`], the aircraft's bounded turn/climb/speed
//!   capabilities. Every lookahead, pursuit and avoidance command is bounded
//!   by it (spec non-negotiable behavior 2); the type names its turn radius so
//!   a caller can prove a route is flyable before committing to it.
//! * [`RouteTermination`], what a route does after its last node. A loop
//!   re-arms the whole marker sequence instead of ending.
//! * [`RouteProgress`], the monotonic progression state that only ever moves
//!   forward, so a follower can never skip a mandatory marker and rewind
//!   (spec non-negotiable behavior 1). On a loop route the target wraps back to
//!   node 0 after the last node, a lap is recorded, and the monotonic node
//!   count keeps climbing.
//! * [`Blocker`], the declared obstacle geometry a committed swept segment is
//!   tested against. A decision that would cross a blocker is never issued:
//!   the follower either deviates within its envelope or reports
//!   [`AvoidanceState::Blocked`] and stays put. There is no teleport unsticking
//!   (spec non-negotiable behavior 4).
//! * [`Navigator::decide`], a pure function of one tick's typed request. The
//!   same request — including the same seed-independent state — produces the
//!   same decision regardless of ECS entity order (spec non-negotiable
//!   behavior 5).
//!
//! Stage **F31-B** adds the stateful production path on top of that contract:
//! [`NavigationSet`] owns one [`PursuitState`] per actor (its remembered
//! [`RouteProgress`], the side its last bounded deviation committed to, and a
//! stall counter), so a displaced or repeatedly blocked follower keeps
//! pursuing the next authored marker instead of restarting; and it derives a
//! per-actor tie-break stream from the mission seed and the stable
//! session-qualified [`ActorId`], so the local decision sequence is a pure
//! function of `(actor, its own ticks)` and never of the order the ECS
//! presented the actors in (spec non-negotiable behaviors 2 and 5, acceptance
//! case AC02). [`NavigationSet::decide_all`] additionally emits its decisions
//! in ascending actor-id order.
//!
//! Arrival is a **swept** condition: a step is tested as the segment it sweeps
//! through, so a fast aircraft cannot pass a waypoint between ticks and miss
//! it, and a stationary position being exactly equal to a node is not what
//! fires it (spec non-negotiable behavior 3).
//!
//! # Designed vocabulary, not original data
//!
//! The original route encoding, trigger shape, arrival rule and maneuver
//! limits are **unmeasured**: F13 locates mission programs but recovers no
//! route layout, and nothing here is derived from original bytes. Every
//! constant, bounds value and fixture in this module is newly authored
//! **project design**. The provenance record that will say where real route
//! values come from is `cs_content::routes`, and F31-D is the stage that can
//! make an original-data claim.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`); the AI command it emits is the same
//! [`FlightInput`] a player's controls produce, so the flight model sees one
//! command boundary.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

use std::collections::BTreeMap;
use std::fmt;

use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::random::SplitMix64;

use crate::damage::ActorId;
use crate::flight::FlightInput;

/// Positions within this distance are treated as equal for heading selection,
/// so a zero-length horizontal direction never produces a NaN heading. It is a
/// numerical guard, not a minimum airspeed or a scale.
pub const DIRECTION_EPSILON_M: f64 = 1e-9;

/// How far ahead the vertical guidance aims when converting a height error
/// into a climb/dive rate, in seconds. A designed time constant, not a
/// measured original rule.
pub const CLIMB_APPROACH_S: f64 = 2.0;

/// How many bounded yaw offsets to try on each side when the direct step is
/// blocked, in multiples of one tick's maximum yaw step.
pub const AVOIDANCE_CANDIDATE_STEPS: u32 = 8;

/// The fewest nodes a loop-terminated route may declare.
///
/// A one-node loop has no wrap edge: re-arming it would re-target the node the
/// follower already occupies, so the route could never progress. This is the
/// smallest node count with a real last-to-first edge, i.e. project design, not
/// a measured original rule.
pub const MIN_LOOP_NODES: usize = 2;

/// The domain constant the F31 behavior tie-break uses
/// (`docs/contracts/CLI-EVIDENCE.md`, `--seed`). F31-B subdivides this domain
/// per actor and per tick, so the tie-break a follower sees is a pure function
/// of `(mission seed, actor, tick)`; cosmetic randomness can never move the
/// route (spec non-negotiable behavior 5).
pub const AI_NAVIGATION_DOMAIN: u64 = 0x4149_4E41_565F_4631; // "AINA V_F1"

// -------------------------------------------------------------- route ------

/// The stable identity of one route node.
///
/// The numeric id is the record's own key, distinct from its authored
/// `sequence`: reordering a container must not rename a marker, and
/// renumbering a marker must not silently reorder progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RouteNodeId(pub u32);

impl RouteNodeId {
    /// The id as a list index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for RouteNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What a route does after its last node.
///
/// The runtime mirrors the content contract's
/// `cs_content::routes::RouteTermination` so the producer's declared
/// termination survives projection instead of being refused (F31-C). Whether
/// the original 2000 route encoding expresses a loop at all is **unmeasured**:
/// F13 recovers no route layout, and F31-D measured only that every mission
/// directory carries the `aiv.zrd` control member, not what is inside it; the
/// route decode is filed as **#455** (`F31-ROUTE-ENCODING`). This enum is
/// therefore project design — it lets a declared record be followed honestly,
/// and it never asserts that the original authored one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RouteTermination {
    /// The route ends at its last node: progress runs off the end and the
    /// follower holds station.
    #[default]
    End,
    /// The route returns to its first node: after the last node is reached,
    /// progress wraps back to node 0 and the whole marker sequence is armed
    /// again, so a followed route's target never leaves the node list and it
    /// does not run off its end.
    Loop,
}

impl RouteTermination {
    /// Whether reaching the last node re-arms the route instead of ending it.
    #[must_use]
    pub const fn is_loop(self) -> bool {
        matches!(self, Self::Loop)
    }
}

/// Where a route's node positions live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteFrame {
    /// World-anchored positions.
    World,
    /// Positions relative to the moving anchor with this stable id (a carrier,
    /// train or escort). The caller supplies the anchor's [`ReferenceFrameSample`].
    Moving {
        /// The anchor's stable id.
        anchor: u64,
    },
}

/// One normalized route node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteNode {
    /// The stable node id.
    pub id: RouteNodeId,
    /// The authored sequence number.
    pub sequence: u32,
    /// Whether skipping this node is forbidden (a mandatory marker).
    pub mandatory: bool,
    /// The node's position in the route's frame, in canonical meters.
    pub position_m: [f64; 3],
    /// The radius of the swept arrival volume, in meters. A step that passes
    /// within this radius of the node has arrived, even if neither endpoint is
    /// inside it.
    pub arrival_radius_m: f64,
}

/// The normalized route the follower consumes.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteGraph {
    /// The frame the node positions live in.
    pub frame: RouteFrame,
    /// What the route does after its last node.
    pub termination: RouteTermination,
    /// The minimum clearance to keep from every blocker, in meters.
    pub clearance_m: f64,
    /// The nodes, in authored sequence order.
    pub nodes: Vec<RouteNode>,
}

impl RouteGraph {
    /// The number of nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// What the route does after its last node.
    #[must_use]
    pub const fn termination(&self) -> RouteTermination {
        self.termination
    }

    /// Validates the graph before it is followed.
    ///
    /// # Errors
    ///
    /// [`RouteGraphError`] for an empty graph, a non-finite position, a
    /// non-positive arrival radius or clearance, a duplicate node id, a
    /// sequence that does not strictly increase, or a single-node loop.
    pub fn validate(&self) -> Result<(), RouteGraphError> {
        if self.nodes.is_empty() {
            return Err(RouteGraphError::EmptyNodes);
        }
        // A one-node loop has no wrap edge: re-arming it would re-target the
        // node the follower already occupies forever, so progress could never
        // advance and the follower would never leave. That is an authoring
        // error, not a route the runtime can honestly follow.
        if self.termination.is_loop() && self.nodes.len() < MIN_LOOP_NODES {
            return Err(RouteGraphError::LoopNeedsMultipleNodes {
                nodes: self.nodes.len(),
            });
        }
        if !self.clearance_m.is_finite() {
            return Err(RouteGraphError::NonFinite {
                field: "clearance_m",
            });
        }
        if self.clearance_m < 0.0 {
            return Err(RouteGraphError::NonPositive {
                field: "clearance_m",
                value: self.clearance_m,
            });
        }
        for (index, node) in self.nodes.iter().enumerate() {
            for (component, value) in node.position_m.into_iter().enumerate() {
                if !value.is_finite() {
                    return Err(RouteGraphError::NonFinite {
                        field: position_field(component),
                    });
                }
            }
            if !node.arrival_radius_m.is_finite() {
                return Err(RouteGraphError::NonFinite {
                    field: "arrival_radius_m",
                });
            }
            if node.arrival_radius_m <= 0.0 {
                return Err(RouteGraphError::NonPositive {
                    field: "arrival_radius_m",
                    value: node.arrival_radius_m,
                });
            }
            if self.nodes[..index].iter().any(|other| other.id == node.id) {
                return Err(RouteGraphError::DuplicateNodeId { id: node.id.0 });
            }
            if let Some(previous) = index.checked_sub(1).map(|i| self.nodes[i].sequence)
                && node.sequence <= previous
            {
                return Err(RouteGraphError::SequenceNotIncreasing {
                    index,
                    previous,
                    current: node.sequence,
                });
            }
        }
        Ok(())
    }

    /// Builds a route graph from already-collected parts, validating it before
    /// it can be followed (F31-C).
    ///
    /// The termination defaults to [`RouteTermination::End`]; a producer that
    /// declares a loop passes it explicitly.
    ///
    /// # Errors
    ///
    /// [`RouteGraphError`] under the same rules as [`Self::validate`]. Nothing
    /// is clamped or repaired, so a producer cannot hand the follower an
    /// invalid graph.
    pub fn try_new(
        frame: RouteFrame,
        clearance_m: f64,
        nodes: Vec<RouteNode>,
    ) -> Result<Self, RouteGraphError> {
        Self::try_new_terminated(frame, RouteTermination::End, clearance_m, nodes)
    }

    /// Builds a route graph that declares what it does after its last node.
    ///
    /// # Errors
    ///
    /// [`RouteGraphError`] under the same rules as [`Self::validate`].
    pub fn try_new_terminated(
        frame: RouteFrame,
        termination: RouteTermination,
        clearance_m: f64,
        nodes: Vec<RouteNode>,
    ) -> Result<Self, RouteGraphError> {
        let graph = Self {
            frame,
            termination,
            clearance_m,
            nodes,
        };
        graph.validate()?;
        Ok(graph)
    }

    /// The node at `index`, when it exists.
    #[must_use]
    pub fn node(&self, index: usize) -> Option<&RouteNode> {
        self.nodes.get(index)
    }
}

fn position_field(component: usize) -> &'static str {
    const FIELDS: [&str; 3] = ["position_m[0]", "position_m[1]", "position_m[2]"];
    FIELDS[component]
}

/// Why a route graph was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteGraphError {
    /// The graph has no nodes.
    EmptyNodes,
    /// A named field was not finite.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A named field was not strictly positive.
    NonPositive {
        /// The offending field.
        field: &'static str,
        /// The offending value.
        value: f64,
    },
    /// Two nodes share one id.
    DuplicateNodeId {
        /// The duplicated id.
        id: u32,
    },
    /// Node sequences are not strictly increasing.
    SequenceNotIncreasing {
        /// The offending node's index.
        index: usize,
        /// The previous node's sequence.
        previous: u32,
        /// The offending node's sequence.
        current: u32,
    },
    /// A loop route declared fewer nodes than it needs to wrap.
    LoopNeedsMultipleNodes {
        /// How many nodes the loop route declared.
        nodes: usize,
    },
}

impl fmt::Display for RouteGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyNodes => write!(f, "a route graph must declare at least one node"),
            Self::NonFinite { field } => write!(f, "route {field} must be finite"),
            Self::NonPositive { field, value } => {
                write!(f, "route {field} must be positive, got {value}")
            }
            Self::DuplicateNodeId { id } => write!(f, "route node id {id} is used more than once"),
            Self::SequenceNotIncreasing {
                index,
                previous,
                current,
            } => write!(
                f,
                "route node {index} has sequence {current}, not greater than the previous {previous}"
            ),
            Self::LoopNeedsMultipleNodes { nodes } => write!(
                f,
                "a loop route needs at least {MIN_LOOP_NODES} nodes to wrap, got {nodes}"
            ),
        }
    }
}

impl std::error::Error for RouteGraphError {}

// ----------------------------------------------------------- progress ------

/// How far a follower has progressed along a route.
///
/// Progression is monotonic **by construction**: the only mutator,
/// [`advanced`](Self::advanced), moves the target forward by exactly one
/// authored node, so a follower can never rewind onto a marker it has already
/// passed out of order and, because the target is always the next sequence, it
/// can never skip a mandatory one.
///
/// # Looping
///
/// A [`RouteTermination::Loop`] route does not run off its end: reaching its
/// last node wraps [`next_index`](Self::next_index) back to node 0 and
/// increments [`laps`](Self::laps), which re-arms **every** node of the route —
/// mandatory markers included — in authored sequence order. Three properties
/// follow, and they are what makes a loop a real route rather than a rewind:
///
/// * **Progress stays monotonic.** [`reached`](Self::reached) counts the nodes
///   reached across all laps and only ever increases; the wrap moves the
///   *target*, never the count. A caller can therefore compare progress
///   between ticks exactly as it does on an ending route.
/// * **The wrap edge has no special geometry.** The wrap is the ordinary
///   last-to-first edge, so arrival at node 0 after a wrap is the same swept
///   test against node 0's own authored `arrival_radius_m` as arrival on any
///   other edge. No extra wrap-edge radius is invented.
/// * **A mandatory marker is re-armed, not remembered.** Each lap re-targets
///   every node in order, so a lap cannot skip a mandatory marker; there is no
///   "already fired this lap" shortcut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteProgress {
    next_index: usize,
    laps: u32,
    reached_total: usize,
}

impl RouteProgress {
    /// Progress before any node has been reached.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            next_index: 0,
            laps: 0,
            reached_total: 0,
        }
    }

    /// Progress that has already reached the first `reached` nodes.
    ///
    /// Used to resume a route whose leading nodes (such as the spawn point)
    /// are already occupied. The resume is always on the first lap.
    #[must_use]
    pub const fn reached_nodes(reached: usize) -> Self {
        Self {
            next_index: reached,
            laps: 0,
            reached_total: reached,
        }
    }

    /// The index of the node currently being targeted.
    ///
    /// For a loop route this wraps back to 0 after the last node, so it names
    /// the live target rather than a total. Use [`reached`](Self::reached) for
    /// a monotonic count.
    #[must_use]
    pub const fn next_index(self) -> usize {
        self.next_index
    }

    /// How many nodes have been reached in total, across every lap.
    ///
    /// Monotonic by construction: it never decreases, not even across a wrap.
    #[must_use]
    pub const fn reached(self) -> usize {
        self.reached_total
    }

    /// How many times the route has been re-armed by wrapping.
    #[must_use]
    pub const fn laps(self) -> u32 {
        self.laps
    }

    /// Whether every node has been reached.
    ///
    /// A followed [`RouteTermination::Loop`] route is never complete: the wrap
    /// keeps [`next_index`](Self::next_index) inside the node list, so the bound
    /// below is never tripped and the route re-arms instead of ending.
    ///
    /// The bound is still checked for a loop, because
    /// [`reached_nodes`](Self::reached_nodes) is a caller-supplied count that is
    /// not validated against any route (`NavigationSet::register_resuming` takes
    /// no route). A progress seeded past the end holds station — no target, no
    /// teleport — rather than running off the node list, exactly as it does on
    /// an ending route.
    #[must_use]
    pub const fn is_complete(self, route: &RouteGraph) -> bool {
        self.next_index >= route.nodes.len()
    }

    /// Advances by exactly one node of `route`. There is deliberately no way to
    /// set it back or jump.
    ///
    /// On a loop route, reaching the last node wraps the target to node 0 and
    /// records another lap; on an ending route the target runs off the end and
    /// [`is_complete`](Self::is_complete) becomes true.
    ///
    /// Both counters saturate, so the documented monotonicity of
    /// [`reached`](Self::reached) and [`laps`](Self::laps) cannot be broken by
    /// a counter overflowing on a route that never ends.
    #[must_use]
    fn advanced(self, route: &RouteGraph) -> Self {
        let reached_total = self.reached_total.saturating_add(1);
        if route.termination.is_loop() && self.next_index + 1 >= route.nodes.len() {
            Self {
                next_index: 0,
                laps: self.laps.saturating_add(1),
                reached_total,
            }
        } else {
            Self {
                next_index: self.next_index + 1,
                laps: self.laps,
                reached_total,
            }
        }
    }
}

// ------------------------------------------------------------ geometry -----

/// The sampled pose of the frame a route's positions are expressed in.
///
/// A world route uses [`ReferenceFrameSample::IDENTITY`]. A route authored
/// against a moving anchor is sampled per tick, so the same local node
/// position becomes the anchor's current world position (spec non-negotiable
/// behavior 3: relative coordinates for carriers, trains and escorts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReferenceFrameSample {
    /// The frame origin in world meters.
    pub origin_m: [f64; 3],
    /// The frame's yaw about canonical +Y, in radians.
    pub yaw_rad: f64,
}

impl ReferenceFrameSample {
    /// The identity frame: no translation, no rotation.
    pub const IDENTITY: Self = Self {
        origin_m: [0.0, 0.0, 0.0],
        yaw_rad: 0.0,
    };

    /// Maps a frame-local position into world meters.
    ///
    /// Uses the canonical right-handed rotation about +Y, the same convention
    /// as `cs_types::space` (`docs/contracts/FLIGHT-PHYSICS.md`).
    #[must_use]
    pub fn world_position(&self, local_m: [f64; 3]) -> [f64; 3] {
        let (sin, cos) = self.yaw_rad.sin_cos();
        [
            self.origin_m[0] + local_m[0] * cos + local_m[2] * sin,
            self.origin_m[1] + local_m[1],
            self.origin_m[2] - local_m[0] * sin + local_m[2] * cos,
        ]
    }
}

/// One obstacle a committed swept segment must not cross.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlockerShape {
    /// An axis-aligned box in canonical world meters.
    AxisAlignedBox {
        /// The box centre.
        center_m: [f64; 3],
        /// The box half extents, per axis.
        half_extents_m: [f64; 3],
    },
    /// A sphere in canonical world meters.
    Sphere {
        /// The sphere centre.
        center_m: [f64; 3],
        /// The sphere radius.
        radius_m: f64,
    },
}

/// A declared obstacle. Blockers are world geometry (walls, terrain), not a
/// router's ground navmesh applied blindly to aircraft.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blocker {
    /// The obstacle's shape.
    pub shape: BlockerShape,
}

impl Blocker {
    /// An axis-aligned box blocker.
    #[must_use]
    pub const fn axis_aligned_box(center_m: [f64; 3], half_extents_m: [f64; 3]) -> Self {
        Self {
            shape: BlockerShape::AxisAlignedBox {
                center_m,
                half_extents_m,
            },
        }
    }

    /// A sphere blocker.
    #[must_use]
    pub const fn sphere(center_m: [f64; 3], radius_m: f64) -> Self {
        Self {
            shape: BlockerShape::Sphere { center_m, radius_m },
        }
    }

    /// Validates the blocker's geometry.
    ///
    /// # Errors
    ///
    /// [`BlockerError`] for a non-finite centre or a non-positive extent.
    pub fn validate(&self) -> Result<(), BlockerError> {
        match self.shape {
            BlockerShape::AxisAlignedBox {
                center_m,
                half_extents_m,
            } => {
                check_finite(center_m, CENTER_FIELDS)?;
                for (component, extent) in half_extents_m.into_iter().enumerate() {
                    if !extent.is_finite() {
                        return Err(BlockerError::NonFinite {
                            field: extent_field(component),
                        });
                    }
                    if extent <= 0.0 {
                        return Err(BlockerError::NonPositive {
                            field: extent_field(component),
                            value: extent,
                        });
                    }
                }
            }
            BlockerShape::Sphere { center_m, radius_m } => {
                check_finite(center_m, CENTER_FIELDS)?;
                if !radius_m.is_finite() {
                    return Err(BlockerError::NonFinite { field: "radius_m" });
                }
                if radius_m <= 0.0 {
                    return Err(BlockerError::NonPositive {
                        field: "radius_m",
                        value: radius_m,
                    });
                }
            }
        }
        Ok(())
    }

    /// Whether the segment `from`..`to` intersects the blocker, grown by
    /// `clearance` meters on every side.
    #[must_use]
    pub fn segment_intersects_with_clearance(
        &self,
        from_m: [f64; 3],
        to_m: [f64; 3],
        clearance_m: f64,
    ) -> bool {
        match self.shape {
            BlockerShape::AxisAlignedBox {
                center_m,
                half_extents_m,
            } => {
                let mut min = [0.0; 3];
                let mut max = [0.0; 3];
                for axis in 0..3 {
                    min[axis] = center_m[axis] - half_extents_m[axis] - clearance_m;
                    max[axis] = center_m[axis] + half_extents_m[axis] + clearance_m;
                }
                segment_hits_aabb(from_m, to_m, min, max)
            }
            BlockerShape::Sphere { center_m, radius_m } => {
                segment_hits_sphere(from_m, to_m, center_m, radius_m + clearance_m)
            }
        }
    }

    /// Whether the segment `from`..`to` intersects the blocker, ignoring
    /// clearance. Used to detect a raw crossing independently of the route's
    /// declared clearance.
    #[must_use]
    pub fn segment_intersects(&self, from_m: [f64; 3], to_m: [f64; 3]) -> bool {
        self.segment_intersects_with_clearance(from_m, to_m, 0.0)
    }
}

const CENTER_FIELDS: [&str; 3] = ["center_m[0]", "center_m[1]", "center_m[2]"];

fn frame_origin_field(component: usize) -> &'static str {
    const FIELDS: [&str; 3] = [
        "frame.origin_m[0]",
        "frame.origin_m[1]",
        "frame.origin_m[2]",
    ];
    FIELDS[component]
}

fn extent_field(component: usize) -> &'static str {
    const FIELDS: [&str; 3] = [
        "half_extents_m[0]",
        "half_extents_m[1]",
        "half_extents_m[2]",
    ];
    FIELDS[component]
}

/// Why a blocker was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockerError {
    /// A named field was not finite.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A named field was not strictly positive.
    NonPositive {
        /// The offending field.
        field: &'static str,
        /// The offending value.
        value: f64,
    },
}

impl fmt::Display for BlockerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "blocker {field} must be finite"),
            Self::NonPositive { field, value } => {
                write!(f, "blocker {field} must be positive, got {value}")
            }
        }
    }
}

impl std::error::Error for BlockerError {}

// ----------------------------------------------------------- envelope ------

/// The bounded turn, climb and speed capabilities of the aircraft a route is
/// flown by.
///
/// Every pursuit and avoidance command is bounded by this envelope (spec
/// non-negotiable behavior 2). The values are the aircraft's **designed**
/// capability limits, not a measurement of the original game; the layout
/// table that maps an airframe into them is F31-C's conversion boundary, and
/// F31-D is where an original value could be evidenced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ManeuverEnvelope {
    /// Maximum heading change rate, in radians per second.
    pub max_yaw_rate_radps: f64,
    /// Maximum climb rate, in meters per second.
    pub max_climb_rate_mps: f64,
    /// Maximum dive rate, in meters per second.
    pub max_dive_rate_mps: f64,
    /// Minimum sustainable speed, in meters per second.
    pub min_speed_mps: f64,
    /// Maximum speed, in meters per second.
    pub max_speed_mps: f64,
    /// Cruise speed, in meters per second.
    pub cruise_speed_mps: f64,
    /// Maximum acceleration, in meters per second squared.
    pub max_accel_mps2: f64,
    /// Maximum deceleration, in meters per second squared.
    pub max_decel_mps2: f64,
    /// Maximum commanded bank, in radians (`0 < bank <= pi/2`).
    pub max_bank_rad: f64,
}

impl ManeuverEnvelope {
    /// Validates the envelope before it bounds any command.
    ///
    /// # Errors
    ///
    /// [`ManeuverEnvelopeError`] for a non-finite, non-positive or
    /// out-of-order value. Nothing is clamped or repaired.
    pub fn validate(&self) -> Result<(), ManeuverEnvelopeError> {
        for (field, value) in [
            ("max_yaw_rate_radps", self.max_yaw_rate_radps),
            ("max_climb_rate_mps", self.max_climb_rate_mps),
            ("max_dive_rate_mps", self.max_dive_rate_mps),
            ("min_speed_mps", self.min_speed_mps),
            ("max_speed_mps", self.max_speed_mps),
            ("cruise_speed_mps", self.cruise_speed_mps),
            ("max_accel_mps2", self.max_accel_mps2),
            ("max_decel_mps2", self.max_decel_mps2),
            ("max_bank_rad", self.max_bank_rad),
        ] {
            if !value.is_finite() {
                return Err(ManeuverEnvelopeError::NonFinite { field });
            }
        }
        for (field, value) in [
            ("max_yaw_rate_radps", self.max_yaw_rate_radps),
            ("max_climb_rate_mps", self.max_climb_rate_mps),
            ("max_dive_rate_mps", self.max_dive_rate_mps),
            ("min_speed_mps", self.min_speed_mps),
            ("max_speed_mps", self.max_speed_mps),
            ("max_accel_mps2", self.max_accel_mps2),
            ("max_decel_mps2", self.max_decel_mps2),
        ] {
            if value <= 0.0 {
                return Err(ManeuverEnvelopeError::NotPositive { field, value });
            }
        }
        if !(self.min_speed_mps <= self.cruise_speed_mps
            && self.cruise_speed_mps <= self.max_speed_mps)
        {
            return Err(ManeuverEnvelopeError::SpeedOrder {
                min: self.min_speed_mps,
                cruise: self.cruise_speed_mps,
                max: self.max_speed_mps,
            });
        }
        if !(0.0 < self.max_bank_rad && self.max_bank_rad <= std::f64::consts::FRAC_PI_2) {
            return Err(ManeuverEnvelopeError::BankOutOfRange {
                value: self.max_bank_rad,
            });
        }
        Ok(())
    }

    /// The minimum turn radius at `speed_mps`, in meters.
    ///
    /// `None` when the envelope is invalid. A route segment shorter than this
    /// radius cannot be reversed in one maneuver, so a caller can refuse an
    /// unflyable route before the follower oscillates.
    #[must_use]
    pub fn turn_radius_m(&self, speed_mps: f64) -> Option<f64> {
        let rate = self.max_yaw_rate_radps;
        if speed_mps.is_finite() && speed_mps >= 0.0 && rate.is_finite() && rate > 0.0 {
            Some(speed_mps / rate)
        } else {
            None
        }
    }
}

/// Why a maneuver envelope was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ManeuverEnvelopeError {
    /// A named field was not finite.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A named field was not strictly positive.
    NotPositive {
        /// The offending field.
        field: &'static str,
        /// The offending value.
        value: f64,
    },
    /// The min/cruise/max speeds are not ordered.
    SpeedOrder {
        /// The declared minimum.
        min: f64,
        /// The declared cruise.
        cruise: f64,
        /// The declared maximum.
        max: f64,
    },
    /// The bank limit is outside `(0, pi/2]`.
    BankOutOfRange {
        /// The offending value.
        value: f64,
    },
}

impl fmt::Display for ManeuverEnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "envelope {field} must be finite"),
            Self::NotPositive { field, value } => {
                write!(f, "envelope {field} must be positive, got {value}")
            }
            Self::SpeedOrder { min, cruise, max } => write!(
                f,
                "envelope speeds must satisfy min <= cruise <= max, got {min} <= {cruise} <= {max}"
            ),
            Self::BankOutOfRange { value } => {
                write!(f, "envelope max_bank_rad {value} is outside (0, pi/2]")
            }
        }
    }
}

impl std::error::Error for ManeuverEnvelopeError {}

// ------------------------------------------------------------- cadence -----

/// The fixed cadence AI decisions run on (spec non-negotiable behavior 5).
///
/// A decision is only produced on a tick the cadence admits; between
/// decisions the last command holds. The cadence is integer ticks, so the
/// decision rate is independent of the render frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavigationCadence {
    ticks_per_decision: u32,
}

impl NavigationCadence {
    /// Builds a cadence of `ticks_per_decision` ticks.
    ///
    /// # Errors
    ///
    /// [`NavigationError::NonPositive`] when `ticks_per_decision` is zero.
    pub fn new(ticks_per_decision: u32) -> Result<Self, NavigationError> {
        if ticks_per_decision == 0 {
            return Err(NavigationError::NonPositive {
                field: "cadence.ticks_per_decision",
                value: 0.0,
            });
        }
        Ok(Self { ticks_per_decision })
    }

    /// The designed default: one decision per simulation tick.
    #[must_use]
    pub const fn designed_default() -> Self {
        Self {
            ticks_per_decision: 1,
        }
    }

    /// The cadence length in ticks.
    #[must_use]
    pub const fn ticks_per_decision(self) -> u32 {
        self.ticks_per_decision
    }

    /// Whether `tick` is a decision tick for this cadence.
    #[must_use]
    pub const fn is_decision_tick(self, tick: Tick) -> bool {
        tick.0.is_multiple_of(self.ticks_per_decision as u64)
    }
}

// ------------------------------------------------------------ request ------

/// The world-space state of the aircraft the follower commands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavState {
    /// World position, in canonical meters.
    pub position_m: [f64; 3],
    /// World heading (yaw) in radians; `0` faces canonical forward `-Z` and a
    /// positive value turns nose-left, matching
    /// `docs/contracts/FLIGHT-PHYSICS.md`.
    pub heading_rad: f64,
    /// Forward speed, in meters per second.
    pub speed_mps: f64,
    /// Vertical speed, in meters per second (positive is climbing).
    pub climb_mps: f64,
}

/// One tick's typed navigation input.
#[derive(Clone, Copy, Debug)]
pub struct NavigationRequest<'a> {
    /// The simulation tick this decision belongs to.
    pub tick: Tick,
    /// The actor's generation; a command never crosses a generation boundary.
    pub generation: u64,
    /// The aircraft's current state.
    pub state: NavState,
    /// The route being followed.
    pub route: &'a RouteGraph,
    /// How far along the route the follower already is.
    pub progress: RouteProgress,
    /// The sampled pose of the route's frame.
    pub frame: ReferenceFrameSample,
    /// The world blockers to avoid.
    pub blockers: &'a [Blocker],
    /// The fixed simulation step, in seconds.
    pub dt_s: f64,
}

/// One committed, bounded motion step: the swept segment the decision allows
/// and the world heading and speeds it holds at the end of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouteStep {
    /// Where the step starts (the aircraft's current position).
    pub from_m: [f64; 3],
    /// Where the step ends.
    pub to_m: [f64; 3],
    /// The heading held during the step.
    pub heading_rad: f64,
    /// The horizontal speed held during the step.
    pub speed_mps: f64,
    /// The vertical speed held during the step.
    pub climb_mps: f64,
}

/// How the decision relates to the route and to blockers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvoidanceState {
    /// The step follows the route inside the envelope and clears every blocker.
    OnRoute,
    /// The direct step was blocked, so a bounded deviation was chosen.
    Deviating,
    /// Every bounded step would cross a blocker; the follower holds position
    /// and commands neutral rather than crossing or teleporting (spec
    /// non-negotiable behavior 4).
    Blocked,
    /// The route is complete.
    Arrived,
}

/// Why one decision was made, for traces and tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationDiagnostics {
    /// Distance from the aircraft to the targeted node, in meters.
    pub distance_to_target_m: f64,
    /// The heading error before it was bounded, in radians.
    pub heading_error_rad: f64,
    /// The clearance the committed step was tested with, in meters.
    pub clearance_m: f64,
    /// How many blockers were considered.
    pub blockers_considered: usize,
}

/// One tick's typed navigation output.
///
/// The `command` is the same [`FlightInput`] a player's controls produce, so
/// the flight model consumes one command boundary either way. The `step` is
/// the bounded swept motion the follower committed to and reasoned about; F31-B
/// closes the loop between it and the integrated flight state.
#[derive(Clone, Debug, PartialEq)]
pub struct NavigationDecision {
    /// The decision's simulation tick.
    pub tick: Tick,
    /// The actor generation the command belongs to.
    pub generation: u64,
    /// The bounded flight command for this tick.
    pub command: FlightInput,
    /// The committed swept step.
    pub step: RouteStep,
    /// The (monotonic) progress after this decision.
    pub progress: RouteProgress,
    /// The node being targeted, or `None` when the route is complete.
    pub target: Option<RouteNodeId>,
    /// How the decision relates to the route and blockers.
    pub avoidance: AvoidanceState,
    /// Diagnostic detail.
    pub diagnostics: NavigationDiagnostics,
}

/// Why a navigation decision was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum NavigationError {
    /// A named field was not finite.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A named field was not strictly positive.
    NonPositive {
        /// The offending field.
        field: &'static str,
        /// The offending value.
        value: f64,
    },
    /// The maneuver envelope is invalid.
    Envelope(ManeuverEnvelopeError),
    /// The route graph is invalid.
    Route(RouteGraphError),
    /// A blocker is invalid; `index` locates it in the request.
    Blocker {
        /// The blocker's index.
        index: usize,
        /// The underlying error.
        source: BlockerError,
    },
    /// A request named an actor of another session generation, or carried a
    /// command generation the set does not own.
    ForeignSession {
        /// The set's session generation.
        expected: u64,
        /// The generation the request carried.
        found: u64,
    },
    /// A request named an actor the set does not own.
    UnknownActor {
        /// The actor that was named.
        actor: ActorId,
    },
    /// An actor was registered, or presented for one tick, more than once.
    DuplicateActor {
        /// The actor that was repeated.
        actor: ActorId,
    },
}

impl fmt::Display for NavigationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "navigation {field} must be finite"),
            Self::NonPositive { field, value } => {
                write!(f, "navigation {field} must be positive, got {value}")
            }
            Self::Envelope(error) => write!(f, "navigation envelope: {error}"),
            Self::Route(error) => write!(f, "navigation route: {error}"),
            Self::Blocker { index, source } => {
                write!(f, "navigation blocker {index}: {source}")
            }
            Self::ForeignSession { expected, found } => write!(
                f,
                "navigation request belongs to generation {found}, but this set owns {expected}"
            ),
            Self::UnknownActor { actor } => {
                write!(f, "navigation has no pursuit state for {actor}")
            }
            Self::DuplicateActor { actor } => {
                write!(f, "{actor} was presented to navigation more than once")
            }
        }
    }
}

impl std::error::Error for NavigationError {}

impl From<ManeuverEnvelopeError> for NavigationError {
    fn from(error: ManeuverEnvelopeError) -> Self {
        Self::Envelope(error)
    }
}

impl From<RouteGraphError> for NavigationError {
    fn from(error: RouteGraphError) -> Self {
        Self::Route(error)
    }
}

// ----------------------------------------------------------- navigator -----

/// The bounded route follower.
///
/// `decide` is a pure function of one request and the validated envelope and
/// cadence: it reads no clock, no renderer and no ECS order, so reordering
/// entities cannot change the decision sequence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Navigator {
    envelope: ManeuverEnvelope,
    cadence: NavigationCadence,
}

impl Navigator {
    /// Builds a navigator, validating the envelope and cadence.
    ///
    /// # Errors
    ///
    /// [`NavigationError`] when the envelope is invalid or the cadence is zero.
    pub fn new(
        envelope: ManeuverEnvelope,
        cadence: NavigationCadence,
    ) -> Result<Self, NavigationError> {
        envelope.validate()?;
        Ok(Self { envelope, cadence })
    }

    /// The envelope this navigator bounds every command by.
    #[must_use]
    pub const fn envelope(&self) -> &ManeuverEnvelope {
        &self.envelope
    }

    /// The cadence decisions run on.
    #[must_use]
    pub const fn cadence(&self) -> NavigationCadence {
        self.cadence
    }

    /// Produces one tick's bounded decision.
    ///
    /// # Errors
    ///
    /// [`NavigationError`] when the request, the route or a blocker is
    /// malformed. Nothing is clamped or repaired.
    pub fn decide(
        &self,
        request: &NavigationRequest<'_>,
    ) -> Result<NavigationDecision, NavigationError> {
        self.decide_with_tie_break(request, 0.0)
    }

    /// Produces one tick's bounded decision, breaking an exactly tied bounded
    /// deviation with `tie_break`.
    ///
    /// `tie_break` is a value in `[0, 1)`. When the direct step would cross a
    /// blocker and the two symmetric bounded deviations clear it equally well,
    /// a draw below `0.5` prefers the positive (nose-left) side and a draw at
    /// or above `0.5` the negative one. It is read only at that exact tie, so
    /// a caller that does not want a seeded tie-break uses [`Self::decide`],
    /// which is `decide_with_tie_break(request, 0.0)`.
    ///
    /// # Errors
    ///
    /// [`NavigationError`] when the request, the route or a blocker is
    /// malformed. Nothing is clamped or repaired.
    pub fn decide_with_tie_break(
        &self,
        request: &NavigationRequest<'_>,
        tie_break: f64,
    ) -> Result<NavigationDecision, NavigationError> {
        self.validate_request(request)?;
        let route = request.route;
        let state = request.state;

        if request.progress.is_complete(route) {
            return Ok(self.stationary_decision(
                request,
                AvoidanceState::Arrived,
                state.heading_rad,
            ));
        }

        let target_index = request.progress.next_index();
        let node = route.nodes[target_index];
        let target_world = request.frame.world_position(node.position_m);
        let distance_to_target_m = distance(state.position_m, target_world);

        let horizontal = [
            target_world[0] - state.position_m[0],
            target_world[2] - state.position_m[2],
        ];
        let horizontal_distance =
            (horizontal[0] * horizontal[0] + horizontal[1] * horizontal[1]).sqrt();
        let desired_heading = if horizontal_distance > DIRECTION_EPSILON_M {
            heading_from_direction(horizontal[0], horizontal[1])
        } else {
            state.heading_rad
        };
        let heading_error_rad = wrap_pi(desired_heading - state.heading_rad);
        let max_yaw_step = self.envelope.max_yaw_rate_radps * request.dt_s;
        let yaw_step = heading_error_rad.clamp(-max_yaw_step, max_yaw_step);

        let desired_speed = self.envelope.cruise_speed_mps;
        let speed = approach(
            state.speed_mps,
            desired_speed,
            self.envelope.max_accel_mps2 * request.dt_s,
            self.envelope.max_decel_mps2 * request.dt_s,
        );

        let vertical_error = target_world[1] - state.position_m[1];
        let climb = (vertical_error / CLIMB_APPROACH_S).clamp(
            -self.envelope.max_dive_rate_mps,
            self.envelope.max_climb_rate_mps,
        );

        let desired = step_for(
            state.position_m,
            wrap_pi(state.heading_rad + yaw_step),
            speed,
            climb,
            request.dt_s,
        );

        let (step, avoidance) = if clears(request, &desired) {
            (desired, AvoidanceState::OnRoute)
        } else {
            match self.deviate(request, yaw_step, max_yaw_step, speed, climb, tie_break) {
                Some(step) => (step, AvoidanceState::Deviating),
                None => (
                    stationary_step(state.position_m, state.heading_rad),
                    AvoidanceState::Blocked,
                ),
            }
        };

        let mut progress = request.progress;
        if !matches!(avoidance, AvoidanceState::Blocked)
            && segment_hits_sphere(
                state.position_m,
                step.to_m,
                target_world,
                node.arrival_radius_m,
            )
        {
            progress = progress.advanced(route);
        }

        let command = self.command_for(state, &step);
        Ok(NavigationDecision {
            tick: request.tick,
            generation: request.generation,
            command,
            step,
            progress,
            target: Some(node.id),
            avoidance,
            diagnostics: NavigationDiagnostics {
                distance_to_target_m,
                heading_error_rad,
                clearance_m: route.clearance_m,
                blockers_considered: request.blockers.len(),
            },
        })
    }

    fn stationary_decision(
        &self,
        request: &NavigationRequest<'_>,
        avoidance: AvoidanceState,
        heading_rad: f64,
    ) -> NavigationDecision {
        NavigationDecision {
            tick: request.tick,
            generation: request.generation,
            command: FlightInput::NEUTRAL,
            step: stationary_step(request.state.position_m, heading_rad),
            progress: request.progress,
            target: None,
            avoidance,
            diagnostics: NavigationDiagnostics {
                distance_to_target_m: 0.0,
                heading_error_rad: 0.0,
                clearance_m: request.route.clearance_m,
                blockers_considered: request.blockers.len(),
            },
        }
    }

    fn deviate(
        &self,
        request: &NavigationRequest<'_>,
        yaw_step: f64,
        max_yaw_step: f64,
        speed: f64,
        climb: f64,
        tie_break: f64,
    ) -> Option<RouteStep> {
        for multiplier in 1..=AVOIDANCE_CANDIDATE_STEPS {
            for sign in tie_break_signs(tie_break) {
                let candidate = (yaw_step + sign * f64::from(multiplier) * max_yaw_step)
                    .clamp(-max_yaw_step, max_yaw_step);
                if (candidate - yaw_step).abs() <= f64::EPSILON {
                    continue;
                }
                let step = step_for(
                    request.state.position_m,
                    wrap_pi(request.state.heading_rad + candidate),
                    speed,
                    climb,
                    request.dt_s,
                );
                if clears(request, &step) {
                    return Some(step);
                }
            }
        }
        None
    }

    fn command_for(&self, state: NavState, step: &RouteStep) -> FlightInput {
        let max_yaw_step = self.envelope.max_yaw_rate_radps * step_dt(step);
        let yaw_applied = wrap_pi(step.heading_rad - state.heading_rad);
        let turn_fraction = if max_yaw_step > 0.0 {
            (yaw_applied / max_yaw_step).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        // A positive yaw step turns the nose left (canonical +Y, positive
        // heading), which needs a left bank: `FlightInput.roll` is positive
        // right-wing-down, so the bank that turns left is negative. See
        // `docs/contracts/FLIGHT-PHYSICS.md` and
        // `accept_f24_a_control_axes_map_to_their_body_axes`.
        let roll = -turn_fraction * (self.envelope.max_bank_rad / std::f64::consts::FRAC_PI_2);
        let pitch = (step.climb_mps / self.envelope.max_climb_rate_mps).clamp(-1.0, 1.0);
        let throttle = (step.speed_mps / self.envelope.max_speed_mps).clamp(0.0, 1.0);
        FlightInput::try_new(pitch, roll, 0.0, throttle, false)
            .expect("every bounded command lies inside the declared input ranges")
    }

    fn validate_request(&self, request: &NavigationRequest<'_>) -> Result<(), NavigationError> {
        if !request.dt_s.is_finite() {
            return Err(NavigationError::NonFinite { field: "dt_s" });
        }
        if request.dt_s <= 0.0 {
            return Err(NavigationError::NonPositive {
                field: "dt_s",
                value: request.dt_s,
            });
        }
        for (component, value) in request.state.position_m.into_iter().enumerate() {
            if !value.is_finite() {
                return Err(NavigationError::NonFinite {
                    field: position_field(component),
                });
            }
        }
        if !request.state.heading_rad.is_finite() {
            return Err(NavigationError::NonFinite {
                field: "state.heading_rad",
            });
        }
        if !request.state.speed_mps.is_finite() {
            return Err(NavigationError::NonFinite {
                field: "state.speed_mps",
            });
        }
        if request.state.speed_mps < 0.0 {
            return Err(NavigationError::NonPositive {
                field: "state.speed_mps",
                value: request.state.speed_mps,
            });
        }
        if !request.state.climb_mps.is_finite() {
            return Err(NavigationError::NonFinite {
                field: "state.climb_mps",
            });
        }
        for (component, value) in request.frame.origin_m.into_iter().enumerate() {
            if !value.is_finite() {
                return Err(NavigationError::NonFinite {
                    field: frame_origin_field(component),
                });
            }
        }
        if !request.frame.yaw_rad.is_finite() {
            return Err(NavigationError::NonFinite {
                field: "frame.yaw_rad",
            });
        }
        request.route.validate()?;
        for (index, blocker) in request.blockers.iter().enumerate() {
            blocker
                .validate()
                .map_err(|source| NavigationError::Blocker { index, source })?;
        }
        Ok(())
    }
}

/// Whether every blocker clears the swept step at the route's clearance.
fn clears(request: &NavigationRequest<'_>, step: &RouteStep) -> bool {
    !request.blockers.iter().any(|blocker| {
        blocker.segment_intersects_with_clearance(step.from_m, step.to_m, request.route.clearance_m)
    })
}

fn step_for(
    from_m: [f64; 3],
    heading_rad: f64,
    speed_mps: f64,
    climb_mps: f64,
    dt_s: f64,
) -> RouteStep {
    let forward = forward_from_heading(heading_rad);
    RouteStep {
        from_m,
        to_m: [
            from_m[0] + forward[0] * speed_mps * dt_s,
            from_m[1] + climb_mps * dt_s,
            from_m[2] + forward[2] * speed_mps * dt_s,
        ],
        heading_rad,
        speed_mps,
        climb_mps,
    }
}

fn stationary_step(position_m: [f64; 3], heading_rad: f64) -> RouteStep {
    RouteStep {
        from_m: position_m,
        to_m: position_m,
        heading_rad,
        speed_mps: 0.0,
        climb_mps: 0.0,
    }
}

/// The swept deviation signs to try, preferred side first. A draw below
/// `0.5` prefers the positive (nose-left) side; this is the one place the
/// seeded tie-break reaches the decision.
fn tie_break_signs(tie_break: f64) -> [f64; 2] {
    if tie_break < 0.5 {
        [1.0, -1.0]
    } else {
        [-1.0, 1.0]
    }
}

/// The elapsed time a step covers, recovered from its length and speed, or
/// zero for a stationary step.
fn step_dt(step: &RouteStep) -> f64 {
    let horizontal = distance_xz(step.from_m, step.to_m);
    if step.speed_mps > 0.0 {
        horizontal / step.speed_mps
    } else {
        0.0
    }
}

// ---------------------------------------------------------------- math -----

fn check_finite(values: [f64; 3], fields: [&'static str; 3]) -> Result<(), BlockerError> {
    for (value, field) in values.into_iter().zip(fields) {
        if !value.is_finite() {
            return Err(BlockerError::NonFinite { field });
        }
    }
    Ok(())
}

fn distance(from: [f64; 3], to: [f64; 3]) -> f64 {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn distance_xz(from: [f64; 3], to: [f64; 3]) -> f64 {
    let dx = to[0] - from[0];
    let dz = to[2] - from[2];
    (dx * dx + dz * dz).sqrt()
}

/// Normalizes an angle to `(-pi, pi]`.
#[must_use]
pub fn wrap_pi(angle_rad: f64) -> f64 {
    let two_pi = std::f64::consts::TAU;
    let mut wrapped = angle_rad % two_pi;
    if wrapped > std::f64::consts::PI {
        wrapped -= two_pi;
    } else if wrapped <= -std::f64::consts::PI {
        wrapped += two_pi;
    }
    wrapped
}

/// The canonical unit forward direction of a heading: `0` faces `-Z` and a
/// positive heading turns nose-left about `+Y`
/// (`docs/contracts/FLIGHT-PHYSICS.md`).
#[must_use]
pub fn forward_from_heading(heading_rad: f64) -> [f64; 3] {
    let (sin, cos) = heading_rad.sin_cos();
    [-sin, 0.0, -cos]
}

/// The heading whose forward direction points along `(dx, dz)`.
#[must_use]
pub fn heading_from_direction(dx: f64, dz: f64) -> f64 {
    (-dx).atan2(-dz)
}

/// Moves `current` toward `target` by at most `accel`/`decel`, in that
/// direction.
fn approach(current: f64, target: f64, accel: f64, decel: f64) -> f64 {
    if target > current {
        (current + accel).min(target)
    } else {
        (current - decel).max(target)
    }
}

/// Whether the segment `from`..`to` comes within `radius` of `center`.
#[must_use]
pub fn segment_hits_sphere(from: [f64; 3], to: [f64; 3], center: [f64; 3], radius: f64) -> bool {
    let direction = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let offset = [
        from[0] - center[0],
        from[1] - center[1],
        from[2] - center[2],
    ];
    let a = dot(direction, direction);
    if a <= f64::EPSILON {
        return dot(offset, offset) <= radius * radius;
    }
    let t = (-dot(offset, direction) / a).clamp(0.0, 1.0);
    let closest = [
        from[0] + direction[0] * t,
        from[1] + direction[1] * t,
        from[2] + direction[2] * t,
    ];
    let delta = [
        closest[0] - center[0],
        closest[1] - center[1],
        closest[2] - center[2],
    ];
    dot(delta, delta) <= radius * radius
}

/// Whether the segment `from`..`to` intersects the axis-aligned box
/// `min`..`max` (slab method, inclusive of the box surface).
#[must_use]
pub fn segment_hits_aabb(from: [f64; 3], to: [f64; 3], min: [f64; 3], max: [f64; 3]) -> bool {
    let mut t_min = 0.0_f64;
    let mut t_max = 1.0_f64;
    for (axis, (&from_axis, &to_axis)) in from.iter().zip(to.iter()).enumerate() {
        let direction = to_axis - from_axis;
        if direction.abs() <= f64::EPSILON {
            if from_axis < min[axis] || from_axis > max[axis] {
                return false;
            }
        } else {
            let mut near = (min[axis] - from_axis) / direction;
            let mut far = (max[axis] - from_axis) / direction;
            if near > far {
                std::mem::swap(&mut near, &mut far);
            }
            t_min = t_min.max(near);
            t_max = t_max.min(far);
            if t_min > t_max {
                return false;
            }
        }
    }
    true
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

// ------------------------------------------------------------ pursuit ------

/// The side a bounded deviation committed to, from the aircraft's own frame:
/// `Left` is a positive heading change (nose-left about canonical `+Y`),
/// `Right` a negative one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviationSide {
    /// Nose-left, the positive yaw direction.
    Left,
    /// Nose-right, the negative yaw direction.
    Right,
}

impl DeviationSide {
    /// The sign of the yaw step this side applies.
    #[must_use]
    pub const fn sign(self) -> f64 {
        match self {
            Self::Left => 1.0,
            Self::Right => -1.0,
        }
    }

    /// The side a signed yaw step chose; `0.0` counts as `Left`.
    #[must_use]
    fn of_yaw_step(yaw_step: f64) -> Self {
        if yaw_step < 0.0 {
            Self::Right
        } else {
            Self::Left
        }
    }
}

/// One actor's persistent navigation state (F31-B).
///
/// The state is the memory the F31-A follower deliberately did not carry:
/// the monotonic [`RouteProgress`] the actor has reached, the side its last
/// bounded deviation committed to, and how many consecutive decision ticks it
/// made no headway. It is data only — the update rules live in
/// [`NavigationSet::decide`], so a caller cannot rewind progress or forge a
/// deviation side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PursuitState {
    progress: RouteProgress,
    deviation: Option<DeviationSide>,
    stalled_ticks: u32,
}

impl PursuitState {
    /// The state of an actor that has not yet reached any node.
    #[must_use]
    pub const fn start() -> Self {
        Self {
            progress: RouteProgress::start(),
            deviation: None,
            stalled_ticks: 0,
        }
    }

    /// The state of an actor resuming a route whose first `reached` nodes are
    /// already occupied (for example the spawn point).
    #[must_use]
    pub const fn resuming(reached: usize) -> Self {
        Self {
            progress: RouteProgress::reached_nodes(reached),
            deviation: None,
            stalled_ticks: 0,
        }
    }

    /// The progress the actor has reached.
    #[must_use]
    pub const fn progress(self) -> RouteProgress {
        self.progress
    }

    /// The side of the actor's current bounded deviation, if it is deviating.
    #[must_use]
    pub const fn deviation(self) -> Option<DeviationSide> {
        self.deviation
    }

    /// How many consecutive decision ticks the actor made no headway. It is a
    /// bounded counter used for diagnostics and for a caller (a mission, a
    /// debug overlay) to notice a wedged follower — never a teleport trigger
    /// (spec non-negotiable behavior 4).
    #[must_use]
    pub const fn stalled_ticks(self) -> u32 {
        self.stalled_ticks
    }

    /// Whether every node of `route` has been reached.
    #[must_use]
    pub const fn is_complete(self, route: &RouteGraph) -> bool {
        self.progress.is_complete(route)
    }
}

/// One actor's typed navigation input for one tick (F31-B).
///
/// It is [`NavigationRequest`] minus `progress`: [`NavigationSet`] owns the
/// actor's progress, so a caller cannot reset it on an origin shift and a
/// replay re-enters at exactly the stored node.
#[derive(Clone, Copy, Debug)]
pub struct PursuitRequest<'a> {
    /// The session-qualified actor this decision belongs to.
    pub actor: ActorId,
    /// The simulation tick this decision belongs to.
    pub tick: Tick,
    /// The actor generation this command belongs to; it must equal the set's
    /// session, so a command never crosses a generation boundary.
    pub generation: u64,
    /// The aircraft's current state.
    pub state: NavState,
    /// The route being followed.
    pub route: &'a RouteGraph,
    /// The sampled pose of the route's frame.
    pub frame: ReferenceFrameSample,
    /// The world blockers to avoid.
    pub blockers: &'a [Blocker],
    /// The fixed simulation step, in seconds.
    pub dt_s: f64,
}

/// One actor's decision and the persistent state it left behind (F31-B).
#[derive(Clone, Debug, PartialEq)]
pub struct PursuitDecision {
    /// The actor this decision belongs to.
    pub actor: ActorId,
    /// The bounded navigation decision, identical in shape to the F31-A one.
    pub decision: NavigationDecision,
    /// The actor's persistent pursuit state after this decision.
    pub state: PursuitState,
}

impl PursuitDecision {
    /// The side of this decision's bounded deviation, if it deviated.
    #[must_use]
    pub const fn deviation(&self) -> Option<DeviationSide> {
        self.state.deviation()
    }
}

/// The per-session navigation authority (F31-B).
///
/// One set owns one [`PursuitState`] per registered actor for one session
/// generation, mirroring [`crate::targeting::TargetStore`]'s session
/// confinement. Actors are keyed by their stable session-qualified
/// [`ActorId`], so:
///
/// * the set never iterates in ECS order — [`Self::decide_all`] sorts by actor
///   id and [`Self::decide`] touches exactly the actor named; and
/// * the seeded tie-break an actor sees is derived from the mission seed, the
///   actor id and the tick (see [`Self::tie_break`]), never from a shared
///   stream advanced in roster order.
///
/// Together those make each actor's local decision sequence a function of its
/// own ticks alone, so reordering the ECS entities that present the actors
/// cannot change a single decision (acceptance case AC02).
#[derive(Clone, Debug)]
pub struct NavigationSet {
    session: u64,
    mission_seed: u64,
    navigator: Navigator,
    actors: BTreeMap<ActorId, PursuitState>,
}

impl NavigationSet {
    /// A set for session `session` whose tie-breaks derive from `mission_seed`.
    #[must_use]
    pub fn new(session: u64, mission_seed: u64, navigator: Navigator) -> Self {
        Self {
            session,
            mission_seed,
            navigator,
            actors: BTreeMap::new(),
        }
    }

    /// The session generation this set owns.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The mission seed the per-actor tie-breaks derive from.
    #[must_use]
    pub const fn mission_seed(&self) -> u64 {
        self.mission_seed
    }

    /// The navigator every actor in the set is bounded by.
    #[must_use]
    pub const fn navigator(&self) -> &Navigator {
        &self.navigator
    }

    /// The number of registered actors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.actors.len()
    }

    /// Whether no actor is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }

    /// Registers `actor` with fresh pursuit state.
    ///
    /// # Errors
    ///
    /// [`NavigationError::ForeignSession`] when the actor belongs to another
    /// session, [`NavigationError::DuplicateActor`] when it is already
    /// registered.
    pub fn register(&mut self, actor: ActorId) -> Result<(), NavigationError> {
        self.check_session(actor.session.get())?;
        if self.actors.contains_key(&actor) {
            return Err(NavigationError::DuplicateActor { actor });
        }
        self.actors.insert(actor, PursuitState::start());
        Ok(())
    }

    /// Registers `actor` resuming a route whose first `reached` nodes are
    /// already occupied (the spawn point is not a marker).
    ///
    /// # Errors
    ///
    /// The same as [`Self::register`].
    pub fn register_resuming(
        &mut self,
        actor: ActorId,
        reached: usize,
    ) -> Result<(), NavigationError> {
        self.check_session(actor.session.get())?;
        if self.actors.contains_key(&actor) {
            return Err(NavigationError::DuplicateActor { actor });
        }
        self.actors.insert(actor, PursuitState::resuming(reached));
        Ok(())
    }

    /// Removes `actor` and its pursuit state; `false` when it was not
    /// registered. A removed actor restarts at the route's first node if it is
    /// registered again.
    pub fn unregister(&mut self, actor: ActorId) -> bool {
        self.actors.remove(&actor).is_some()
    }

    /// Whether `actor` is registered.
    #[must_use]
    pub fn contains(&self, actor: ActorId) -> bool {
        self.actors.contains_key(&actor)
    }

    /// One actor's current pursuit state.
    #[must_use]
    pub fn state(&self, actor: ActorId) -> Option<PursuitState> {
        self.actors.get(&actor).copied()
    }

    /// Every registered actor, in ascending stable id order.
    pub fn actors(&self) -> impl Iterator<Item = ActorId> + '_ {
        self.actors.keys().copied()
    }

    /// The seeded tie-break draw for `actor` at `tick`, in `[0, 1)`.
    ///
    /// It is a pure function of `(mission seed, actor id, tick)` — see
    /// [`tie_break_draw`] — so it never depends on the roster's size or
    /// iteration order, and a replayed tick sees the same draw. The
    /// acceptance tests read it to show that two actors are given independent
    /// streams from one mission seed.
    #[must_use]
    pub fn tie_break(&self, actor: ActorId, tick: Tick) -> f64 {
        tie_break_draw(self.mission_seed, actor, tick)
    }

    /// Evaluates one actor's request, updating its stored pursuit state.
    ///
    /// # Errors
    ///
    /// [`NavigationError::ForeignSession`] when the actor or command
    /// generation is not the set's, [`NavigationError::UnknownActor`] when the
    /// actor was never registered, and every error [`Navigator::decide`] can
    /// raise.
    pub fn decide(
        &mut self,
        request: &PursuitRequest<'_>,
    ) -> Result<PursuitDecision, NavigationError> {
        self.check_session(request.actor.session.get())?;
        self.check_session(request.generation)?;
        let mut state =
            self.actors
                .get(&request.actor)
                .copied()
                .ok_or(NavigationError::UnknownActor {
                    actor: request.actor,
                })?;

        // The remembered side of a contiguous deviation biases the tie-break
        // so the follower does not flip-flop between two clearing sides; the
        // seed decides only the first side of a fresh deviation.
        let tie_break = match state.deviation() {
            Some(side) => {
                if side == DeviationSide::Left {
                    0.0
                } else {
                    1.0
                }
            }
            None => self.tie_break(request.actor, request.tick),
        };

        let navigation = NavigationRequest {
            tick: request.tick,
            generation: request.generation,
            state: request.state,
            route: request.route,
            progress: state.progress(),
            frame: request.frame,
            blockers: request.blockers,
            dt_s: request.dt_s,
        };
        let decision = self
            .navigator
            .decide_with_tie_break(&navigation, tie_break)?;

        Self::update_state(&mut state, request.state, &decision);
        self.actors.insert(request.actor, state);
        Ok(PursuitDecision {
            actor: request.actor,
            decision,
            state,
        })
    }

    /// Evaluates every request for one tick and returns the decisions in
    /// ascending actor-id order.
    ///
    /// The input order is deliberately ignored: the requests are sorted by
    /// their stable actor id before any decision runs, so the same roster
    /// presented in a different ECS order produces the same output. An actor
    /// named twice in one call is refused rather than stepped twice.
    ///
    /// # Errors
    ///
    /// [`NavigationError::DuplicateActor`] when an actor appears more than
    /// once, and every error [`Self::decide`] can raise.
    pub fn decide_all(
        &mut self,
        requests: &[PursuitRequest<'_>],
    ) -> Result<Vec<PursuitDecision>, NavigationError> {
        let mut ordered: Vec<&PursuitRequest<'_>> = requests.iter().collect();
        ordered.sort_by_key(|request| request.actor);
        if let Some(pair) = ordered
            .windows(2)
            .find(|pair| pair[0].actor == pair[1].actor)
        {
            return Err(NavigationError::DuplicateActor {
                actor: pair[0].actor,
            });
        }
        ordered
            .into_iter()
            .map(|request| self.decide(request))
            .collect()
    }

    /// The state transition of one decision: adopt the (monotonic) progress,
    /// remember the side of a deviation, forget it once the route is clear,
    /// and count consecutive no-headway ticks.
    fn update_state(state: &mut PursuitState, before: NavState, decision: &NavigationDecision) {
        state.progress = decision.progress;
        let moved = distance_xz(before.position_m, decision.step.to_m) > 0.0;
        if moved || matches!(decision.avoidance, AvoidanceState::Arrived) {
            state.stalled_ticks = 0;
        } else {
            state.stalled_ticks = state.stalled_ticks.saturating_add(1);
        }
        match decision.avoidance {
            AvoidanceState::Deviating => {
                let yaw = wrap_pi(decision.step.heading_rad - before.heading_rad);
                state.deviation = Some(DeviationSide::of_yaw_step(yaw));
            }
            AvoidanceState::OnRoute | AvoidanceState::Arrived => state.deviation = None,
            // A blocked hold keeps the last committed side so a follower that
            // clears the blocker on the same side keeps going that way.
            AvoidanceState::Blocked => {}
        }
    }

    fn check_session(&self, generation: u64) -> Result<(), NavigationError> {
        if generation != self.session {
            return Err(NavigationError::ForeignSession {
                expected: self.session,
                found: generation,
            });
        }
        Ok(())
    }
}

/// The seeded tie-break draw for `(mission_seed, actor, tick)`, in `[0, 1)`.
///
/// It is the domain-separated mission AI stream
/// ([`AI_NAVIGATION_DOMAIN`], `docs/contracts/CLI-EVIDENCE.md` `--seed`)
/// subdivided by a stable mix of the actor id and the tick, so two actors —
/// or two ticks of one actor — draw from independent streams, and no value
/// moves when the roster or the ECS order changes.
#[must_use]
pub fn tie_break_draw(mission_seed: u64, actor: ActorId, tick: Tick) -> f64 {
    let domain = AI_NAVIGATION_DOMAIN
        ^ actor_stream_domain(actor)
        ^ tick.0.wrapping_mul(0xD1B5_4A32_D192_ED03);
    SplitMix64::for_domain(mission_seed, domain).unit_f64()
}

/// A stable, dependency-free mix of an actor's session and serial into a
/// domain offset. It is a domain label, not an identity: two distinct actors
/// that ever collide would merely share a tie-break stream, never a state.
fn actor_stream_domain(actor: ActorId) -> u64 {
    let mut value = actor.serial ^ actor.session.get().rotate_left(32);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

// ------------------------------------------------------- follow driver -----

/// The outcome of one production route-follow run (F31-C).
#[derive(Clone, Debug, PartialEq)]
pub struct FollowOutcome {
    /// Every decision, in tick order.
    pub decisions: Vec<PursuitDecision>,
    /// The progress the actor reached at the end of the run.
    pub progress: RouteProgress,
    /// Whether every node was reached.
    pub complete: bool,
    /// The tick of the first `Blocked` hold, if any.
    pub blocked_at: Option<u64>,
}

impl FollowOutcome {
    /// The index of the actor's first reached mandatory marker, if any.
    #[must_use]
    pub fn first_mandatory_reached(&self, route: &RouteGraph) -> Option<usize> {
        route
            .nodes
            .iter()
            // A loop route's node count is reached on every lap, so one lap's
            // worth is the whole list here; `reached()` only widens the window.
            .take(self.progress.reached().min(route.nodes.len()))
            .position(|node| node.mandatory)
    }
}

/// The static configuration of one production route-follow run (F31-C):
/// the graph to fly and the environment it is flown in.
///
/// Bundling these keeps [`follow_route`]'s inputs to the set, the actor, this
/// plan and the per-tick frame sampler, rather than a long positional list.
#[derive(Clone, Copy, Debug)]
pub struct FollowPlan<'a> {
    /// The projected route to fly.
    pub route: &'a RouteGraph,
    /// Static obstacles evaluated by every decision.
    pub blockers: &'a [Blocker],
    /// The fixed decision step, in seconds.
    pub dt_s: f64,
    /// The actor's initial kinematic state.
    pub start: NavState,
    /// The hard cap on decisions, so a wedged follower can never loop forever.
    pub max_ticks: usize,
}

/// Drives one already-registered actor along `plan.route` from `plan.start`,
/// advancing its state from each committed step (the kinematic closure the
/// synthetic probes use) for up to `plan.max_ticks` decisions (F31-C).
///
/// `frame_at` supplies the sampled pose of the route's frame on each tick, so a
/// route authored against a moving anchor is followed in its relative
/// coordinates. The run stops early when the route completes or the actor is
/// held [`AvoidanceState::Blocked`]. A [`RouteTermination::Loop`] route never
/// completes, so it runs the whole `plan.max_ticks` unless the actor is held,
/// and [`FollowOutcome::progress`] then names the laps it flew. The set keeps
/// every actor's progress, so a displaced start does not reset it and an origin
/// shift cannot restart the route (spec non-negotiable behaviors 1, 3 and 4).
///
/// # Errors
///
/// Every [`NavigationError`] [`NavigationSet::decide`] can raise, including a
/// request for an actor the set does not own and a stale command generation.
pub fn follow_route<F>(
    set: &mut NavigationSet,
    actor: ActorId,
    plan: FollowPlan<'_>,
    mut frame_at: F,
) -> Result<FollowOutcome, NavigationError>
where
    F: FnMut(Tick) -> ReferenceFrameSample,
{
    let FollowPlan {
        route,
        blockers,
        dt_s,
        start,
        max_ticks,
    } = plan;
    let mut state = start;
    let mut decisions = Vec::new();
    let mut blocked_at = None;
    let mut complete = false;
    for index in 0..max_ticks {
        let tick = Tick(index as u64);
        let request = PursuitRequest {
            actor,
            tick,
            generation: set.session(),
            state,
            route,
            frame: frame_at(tick),
            blockers,
            dt_s,
        };
        let decision = set.decide(&request)?;
        let blocked = matches!(decision.decision.avoidance, AvoidanceState::Blocked);
        state = NavState {
            position_m: decision.decision.step.to_m,
            heading_rad: decision.decision.step.heading_rad,
            speed_mps: decision.decision.step.speed_mps,
            climb_mps: decision.decision.step.climb_mps,
        };
        let finished = decision.state.is_complete(route);
        decisions.push(decision);
        if blocked {
            blocked_at = Some(tick.0);
            break;
        }
        if finished {
            complete = true;
            break;
        }
    }
    let progress = set
        .state(actor)
        .map_or_else(RouteProgress::start, PursuitState::progress);
    Ok(FollowOutcome {
        decisions,
        progress,
        complete,
        blocked_at,
    })
}

// ------------------------------------------------------------- fixture -----

/// The designed clearance of the synthetic arch route, in meters.
pub const SYNTHETIC_ARCH_CLEARANCE_M: f64 = 2.0;

/// The designed fixed step of the synthetic arch probe, in seconds.
pub const SYNTHETIC_ARCH_PROBE_DT_S: f64 = 1.0 / 60.0;

/// The designed start speed of the synthetic arch probe, in m/s.
pub const SYNTHETIC_ARCH_START_SPEED_MPS: f64 = 40.0;

/// The designed maneuver envelope of the synthetic arch fixture.
///
/// Chosen so the 40 m turn radius at cruise is small relative to the route's
/// node spacing, so the fixture is flyable rather than marginal. The values are
/// the F31 follower's **designed command contract**, not a model of any
/// particular airframe: every command the follower emits is bounded by them
/// (spec non-negotiable behavior 2), and the kinematic `follow_route` probe
/// flies exactly that bound.
///
/// The F24 synthetic airframe (`cs_sim::flight::synthetic_fixed_wing`) is
/// deliberately **not** this envelope's subject. That airframe is a fidelity
/// model: its roll channel is a rate command with no bank holding, and it
/// cannot hold altitude on a zero-pitch command without its cruise trim angle
/// of attack. So a follower bounded by this envelope flies the *kinematic*
/// route closure, not the F24 body; making the integrated Avian loop rejoin
/// laterally needs new follower state (measured bank) or an assisted airframe.
/// See `docs/findings/2026-10-02-t451-bank-sign-and-envelope-subject.md`
/// (task #451) for the measured evidence and the filed follow-up, #526.
#[must_use]
pub fn synthetic_maneuver_envelope() -> ManeuverEnvelope {
    ManeuverEnvelope {
        max_yaw_rate_radps: 1.0,
        max_climb_rate_mps: 20.0,
        max_dive_rate_mps: 25.0,
        min_speed_mps: 20.0,
        max_speed_mps: 90.0,
        cruise_speed_mps: 40.0,
        max_accel_mps2: 10.0,
        max_decel_mps2: 12.0,
        max_bank_rad: std::f64::consts::FRAC_PI_3,
    }
}

/// The minimal synthetic route: a chain that funnels from a start behind the
/// left wall, through a narrow arch opening and on to a goal behind the right
/// side of the same wall.
///
/// Mirrors `cs_content::routes::declared_synthetic_arch_route`; this stage
/// only consumes and validates it, and F31-C asserts the two agree.
#[must_use]
pub fn synthetic_arch_route() -> RouteGraph {
    RouteGraph {
        frame: RouteFrame::World,
        termination: RouteTermination::End,
        clearance_m: SYNTHETIC_ARCH_CLEARANCE_M,
        nodes: vec![
            RouteNode {
                id: RouteNodeId(0),
                sequence: 0,
                mandatory: false,
                position_m: [0.0, 0.0, -30.0],
                arrival_radius_m: 3.0,
            },
            RouteNode {
                id: RouteNodeId(1),
                sequence: 1,
                mandatory: true,
                position_m: [60.0, 5.0, -4.0],
                arrival_radius_m: 5.0,
            },
            RouteNode {
                id: RouteNodeId(2),
                sequence: 2,
                mandatory: true,
                position_m: [100.0, 5.0, 0.0],
                arrival_radius_m: 5.0,
            },
            RouteNode {
                id: RouteNodeId(3),
                sequence: 3,
                mandatory: true,
                position_m: [140.0, 5.0, -4.0],
                arrival_radius_m: 5.0,
            },
            RouteNode {
                id: RouteNodeId(4),
                sequence: 4,
                mandatory: true,
                position_m: [260.0, 5.0, -30.0],
                arrival_radius_m: 8.0,
            },
        ],
    }
}

/// The wall the synthetic arch cuts through: a thin slab at x=100 with a box
/// left of `z=-12`, a box right of `z=+12` and a lintel above `y=15`. The
/// opening is the gap those leave, and the run of the route funnels through it.
#[must_use]
pub fn synthetic_arch_blockers() -> Vec<Blocker> {
    vec![
        Blocker::axis_aligned_box([100.0, 30.0, -36.0], [1.0, 30.0, 24.0]),
        Blocker::axis_aligned_box([100.0, 30.0, 36.0], [1.0, 30.0, 24.0]),
        Blocker::axis_aligned_box([100.0, 37.5, 0.0], [1.0, 22.5, 12.0]),
    ]
}

/// The aircraft's starting state for the synthetic arch fixture: at the start
/// node, already pointed at the funnel node at cruise speed.
#[must_use]
pub fn synthetic_arch_start() -> NavState {
    NavState {
        position_m: [0.0, 0.0, -30.0],
        heading_rad: heading_from_direction(60.0, 26.0),
        speed_mps: SYNTHETIC_ARCH_START_SPEED_MPS,
        climb_mps: 0.0,
    }
}

// ------------------------------------------------ F31-B pursuit fixture ---

/// The designed fixed step of the F31-B pursuit fixture, in seconds.
pub const SYNTHETIC_PURSUIT_DT_S: f64 = 1.0 / 60.0;

/// The mission seed the F31-B synthetic fixture's tie-breaks derive from.
/// Newly authored fixture data, not a measured original seed.
pub const SYNTHETIC_PURSUIT_SEED: u64 = 0x4633_3142_5055_5253; // "F31B PURS"

/// The session generation the F31-B synthetic fixture's actors belong to.
pub const SYNTHETIC_PURSUIT_SESSION: u64 = 7;

/// [`SYNTHETIC_PURSUIT_SESSION`] as the shared nonzero session type.
pub const SYNTHETIC_PURSUIT_SESSION_ID: SessionId = match SessionId::new(SYNTHETIC_PURSUIT_SESSION)
{
    Some(id) => id,
    None => unreachable!(),
};

/// The stable actor id of the `serial`-th F31-B fixture actor.
#[must_use]
pub const fn synthetic_pursuit_actor(serial: u64) -> ActorId {
    ActorId {
        session: SYNTHETIC_PURSUIT_SESSION_ID,
        serial,
    }
}

/// The straight synthetic pursuit route: from the spawn node at the origin
/// along `-Z` to a mandatory marker 120 m ahead and two more beyond.
#[must_use]
pub fn synthetic_pursuit_route() -> RouteGraph {
    RouteGraph {
        frame: RouteFrame::World,
        termination: RouteTermination::End,
        clearance_m: 0.0,
        nodes: vec![
            RouteNode {
                id: RouteNodeId(0),
                sequence: 0,
                mandatory: false,
                position_m: [0.0, 0.0, 0.0],
                arrival_radius_m: 2.0,
            },
            RouteNode {
                id: RouteNodeId(1),
                sequence: 1,
                mandatory: true,
                position_m: [0.0, 0.0, -120.0],
                arrival_radius_m: 6.0,
            },
            RouteNode {
                id: RouteNodeId(2),
                sequence: 2,
                mandatory: true,
                position_m: [0.0, 0.0, -260.0],
                arrival_radius_m: 6.0,
            },
            RouteNode {
                id: RouteNodeId(3),
                sequence: 3,
                mandatory: true,
                position_m: [0.0, 0.0, -400.0],
                arrival_radius_m: 8.0,
            },
        ],
    }
}

/// A small sphere centred exactly on the pursuit route's first leg, close
/// enough that one bounded yaw step clears it symmetrically: the direct step
/// sweeps through it, and both bounded deviations clear it equally, so the
/// seeded tie-break is the only thing that chooses a side.
#[must_use]
pub fn synthetic_pursuit_tie_blocker() -> Blocker {
    Blocker::sphere([0.0, 0.0, -0.3], 0.004)
}

/// The synthetic looping patrol: a three-node closed circuit that returns to
/// its first node.
///
/// The three nodes form a triangle in the XZ plane, so the wrap edge (node 2
/// back to node 0) is a real leg the follower must fly rather than a teleport
/// back to the spawn. Node 1 is a mandatory marker, so a lap that skipped it
/// would be observable; node 0 deliberately is **not** mandatory, so the wrap
/// itself has no marker requirement. Node 0 declares the widest arrival radius
/// of the three, which makes the wrap edge's arrival volume observable: the
/// wrap must be tested against node 0's own authored radius, not against the
/// last node's tighter one and not against an invented wrap-edge radius.
///
/// All positions and radii are newly authored project design: the original
/// route encoding is unmeasured (F13; F31-D measured only the `aiv.zrd`
/// carrier), and the decode is filed as **#455** (`F31-ROUTE-ENCODING`).
#[must_use]
pub fn synthetic_loop_route() -> RouteGraph {
    RouteGraph {
        frame: RouteFrame::World,
        termination: RouteTermination::Loop,
        clearance_m: 0.0,
        nodes: vec![
            RouteNode {
                id: RouteNodeId(0),
                sequence: 0,
                mandatory: false,
                position_m: [0.0, 0.0, 0.0],
                arrival_radius_m: 12.0,
            },
            RouteNode {
                id: RouteNodeId(1),
                sequence: 1,
                mandatory: true,
                position_m: [0.0, 0.0, -160.0],
                arrival_radius_m: 8.0,
            },
            RouteNode {
                id: RouteNodeId(2),
                sequence: 2,
                mandatory: true,
                position_m: [-160.0, 0.0, -80.0],
                arrival_radius_m: 6.0,
            },
        ],
    }
}

/// The aircraft state the F31-B pursuit fixture starts from: on the spawn
/// node at cruise speed, pointed straight down the first leg.
#[must_use]
pub fn synthetic_pursuit_start() -> NavState {
    NavState {
        position_m: [0.0, 0.0, 0.0],
        heading_rad: 0.0,
        speed_mps: SYNTHETIC_ARCH_START_SPEED_MPS,
        climb_mps: 0.0,
    }
}

/// A set of `count` fixture actors, each resuming past the spawn node (which
/// is not a marker), under the designer's synthetic envelope and seed.
#[must_use]
pub fn synthetic_pursuit_set(count: u64) -> NavigationSet {
    let navigator = Navigator::new(
        synthetic_maneuver_envelope(),
        NavigationCadence::designed_default(),
    )
    .expect("the synthetic envelope and cadence are valid");
    let mut set = NavigationSet::new(SYNTHETIC_PURSUIT_SESSION, SYNTHETIC_PURSUIT_SEED, navigator);
    for serial in 1..=count {
        set.register_resuming(synthetic_pursuit_actor(serial), 1)
            .expect("fresh fixture actors register");
    }
    set
}

/// The result of one synthetic arch traversal.
#[derive(Clone, Debug, PartialEq)]
pub struct SyntheticTraversal {
    /// Every decision, in tick order.
    pub decisions: Vec<NavigationDecision>,
    /// How many leading nodes were reached.
    pub reached: usize,
    /// The total node count.
    pub total_nodes: usize,
    /// Whether the route was completed.
    pub complete: bool,
    /// The tick index a `Blocked` decision first appeared at, if any.
    pub blocked_at: Option<usize>,
    /// The index of the first blocker a committed segment crossed, if any.
    /// The production follower must keep this `None`.
    pub crossed_blocker: Option<usize>,
}

impl SyntheticTraversal {
    /// The committed swept segments, in tick order.
    pub fn segments(&self) -> impl Iterator<Item = ([f64; 3], [f64; 3])> + '_ {
        self.decisions
            .iter()
            .map(|decision| (decision.step.from_m, decision.step.to_m))
    }
}

/// The synthetic arch probe: it drives the production [`Navigator`] over the
/// synthetic route kinematically, one designed step per tick, and records
/// whether the follower ever crossed a blocker.
///
/// It is a fixture, not an integration: F31-B wires the follower to the
/// integrated flight state. It starts at progress 1 because the aircraft
/// spawns on the start node, which is not a marker.
#[derive(Clone, Debug)]
pub struct SyntheticArchProbe {
    navigator: Navigator,
    route: RouteGraph,
    blockers: Vec<Blocker>,
    state: NavState,
    progress: RouteProgress,
}

impl SyntheticArchProbe {
    /// Builds the fixture navigator, route, blockers and start state.
    ///
    /// # Errors
    ///
    /// [`NavigationError`] if a designed fixture value is refused.
    pub fn new() -> Result<Self, NavigationError> {
        Ok(Self {
            navigator: Navigator::new(
                synthetic_maneuver_envelope(),
                NavigationCadence::designed_default(),
            )?,
            route: synthetic_arch_route(),
            blockers: synthetic_arch_blockers(),
            state: synthetic_arch_start(),
            progress: RouteProgress::reached_nodes(1),
        })
    }

    /// The route being flown.
    #[must_use]
    pub fn route(&self) -> &RouteGraph {
        &self.route
    }

    /// The blockers the route must clear.
    #[must_use]
    pub fn blockers(&self) -> &[Blocker] {
        &self.blockers
    }

    /// The navigator under test.
    #[must_use]
    pub fn navigator(&self) -> &Navigator {
        &self.navigator
    }

    /// The current state.
    #[must_use]
    pub const fn state(&self) -> NavState {
        self.state
    }

    /// The run up to `max_ticks` decisions or completion.
    ///
    /// # Errors
    ///
    /// [`NavigationError`] if a decision refuses its request.
    pub fn run(&mut self, max_ticks: usize) -> Result<SyntheticTraversal, NavigationError> {
        let mut decisions = Vec::new();
        let mut blocked_at = None;
        let mut crossed_blocker = None;
        let mut complete = false;
        for tick in 0..max_ticks {
            let request = NavigationRequest {
                tick: Tick(tick as u64),
                generation: 1,
                state: self.state,
                route: &self.route,
                progress: self.progress,
                frame: ReferenceFrameSample::IDENTITY,
                blockers: &self.blockers,
                dt_s: SYNTHETIC_ARCH_PROBE_DT_S,
            };
            let decision = self.navigator.decide(&request)?;
            if crossed_blocker.is_none() {
                crossed_blocker = self.blockers.iter().position(|blocker| {
                    blocker.segment_intersects(decision.step.from_m, decision.step.to_m)
                });
            }
            let blocked = matches!(decision.avoidance, AvoidanceState::Blocked);
            self.state.position_m = decision.step.to_m;
            self.state.heading_rad = decision.step.heading_rad;
            self.state.speed_mps = decision.step.speed_mps;
            self.state.climb_mps = decision.step.climb_mps;
            self.progress = decision.progress;
            decisions.push(decision);
            if blocked {
                blocked_at = Some(tick);
                break;
            }
            if self.progress.is_complete(&self.route) {
                complete = true;
                break;
            }
        }
        Ok(SyntheticTraversal {
            decisions,
            reached: self.progress.reached(),
            total_nodes: self.route.node_count(),
            complete,
            blocked_at,
            crossed_blocker,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request<'a>(
        state: NavState,
        route: &'a RouteGraph,
        blockers: &'a [Blocker],
    ) -> NavigationRequest<'a> {
        NavigationRequest {
            tick: Tick(0),
            generation: 1,
            state,
            route,
            progress: RouteProgress::start(),
            frame: ReferenceFrameSample::IDENTITY,
            blockers,
            dt_s: 1.0 / 60.0,
        }
    }

    /// The swept primitives see a crossing even when both endpoints are
    /// outside the shape, and the AABB case is inclusive of the surface.
    #[test]
    fn accept_f31_a_swept_tests_detect_a_mid_segment_crossing() {
        assert!(segment_hits_sphere(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, -50.0],
            [0.0, 0.0, -10.0],
            2.0
        ));
        assert!(!segment_hits_sphere(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, -5.0],
            [0.0, 0.0, -10.0],
            2.0
        ));
        assert!(segment_hits_aabb(
            [0.0, 0.0, 0.0],
            [50.0, 0.0, 0.0],
            [10.0, -10.0, -10.0],
            [20.0, 10.0, 10.0]
        ));
        assert!(!segment_hits_aabb(
            [0.0, 20.0, 0.0],
            [50.0, 20.0, 0.0],
            [10.0, -10.0, -10.0],
            [20.0, 10.0, 10.0]
        ));
    }

    /// The maneuver envelope refuses a corrupt value by name and reports the
    /// turn radius it implies.
    #[test]
    fn accept_f31_a_envelope_refuses_corrupt_values() {
        let envelope = synthetic_maneuver_envelope();
        assert_eq!(envelope.validate(), Ok(()));
        assert!(
            (envelope.turn_radius_m(40.0).expect("valid envelope") - 40.0).abs() < 1e-9,
            "turn radius is speed / rate"
        );

        let mut corrupt = envelope;
        corrupt.max_yaw_rate_radps = 0.0;
        assert_eq!(
            corrupt.validate(),
            Err(ManeuverEnvelopeError::NotPositive {
                field: "max_yaw_rate_radps",
                value: 0.0,
            })
        );

        let mut unordered = envelope;
        unordered.cruise_speed_mps = 200.0;
        assert_eq!(
            unordered.validate(),
            Err(ManeuverEnvelopeError::SpeedOrder {
                min: 20.0,
                cruise: 200.0,
                max: 90.0,
            })
        );
    }

    /// The route graph refuses a duplicate id and a non-increasing sequence.
    #[test]
    fn accept_f31_a_route_graph_refuses_duplicates_and_out_of_order_sequences() {
        let route = synthetic_arch_route();
        assert_eq!(route.validate(), Ok(()));

        let mut duplicate = route.clone();
        duplicate.nodes[1].id = duplicate.nodes[0].id;
        assert_eq!(
            duplicate.validate(),
            Err(RouteGraphError::DuplicateNodeId { id: 0 })
        );

        let mut unordered = route;
        unordered.nodes[1].sequence = 0;
        assert_eq!(
            unordered.validate(),
            Err(RouteGraphError::SequenceNotIncreasing {
                index: 1,
                previous: 0,
                current: 0,
            })
        );
    }

    /// The cadence admits exactly its declared ticks.
    #[test]
    fn accept_f31_a_cadence_admits_declared_ticks() {
        assert_eq!(
            NavigationCadence::new(0),
            Err(NavigationError::NonPositive {
                field: "cadence.ticks_per_decision",
                value: 0.0,
            })
        );
        let cadence = NavigationCadence::new(4).expect("nonzero cadence");
        assert!(cadence.is_decision_tick(Tick(0)));
        assert!(!cadence.is_decision_tick(Tick(3)));
        assert!(cadence.is_decision_tick(Tick(4)));
    }

    /// A blocked request is refused by name rather than silently repaired.
    #[test]
    fn accept_f31_a_request_refuses_corrupt_state_and_blockers() {
        let navigator = Navigator::new(
            synthetic_maneuver_envelope(),
            NavigationCadence::designed_default(),
        )
        .expect("valid navigator");
        let route = synthetic_arch_route();
        let blockers = synthetic_arch_blockers();

        let mut bad_dt = request(synthetic_arch_start(), &route, &blockers);
        bad_dt.dt_s = 0.0;
        assert_eq!(
            navigator.decide(&bad_dt),
            Err(NavigationError::NonPositive {
                field: "dt_s",
                value: 0.0,
            })
        );

        let mut bad_blocker = synthetic_arch_blockers();
        bad_blocker[0] = Blocker::axis_aligned_box([f64::NAN, 0.0, 0.0], [1.0, 1.0, 1.0]);
        assert_eq!(
            navigator.decide(&request(synthetic_arch_start(), &route, &bad_blocker)),
            Err(NavigationError::Blocker {
                index: 0,
                source: BlockerError::NonFinite {
                    field: "center_m[0]",
                },
            })
        );
    }

    /// The explicit tie-break reaches the decision: at the symmetric tie the
    /// low draw turns nose-left and the high draw nose-right, both bounded by
    /// the envelope's per-tick yaw step.
    #[test]
    fn accept_f31_b_explicit_tie_break_chooses_the_requested_deviation_side() {
        let navigator = Navigator::new(
            synthetic_maneuver_envelope(),
            NavigationCadence::designed_default(),
        )
        .expect("valid navigator");
        let route = synthetic_pursuit_route();
        let blockers = [synthetic_pursuit_tie_blocker()];
        let state = synthetic_pursuit_start();
        let decide = |tie_break: f64| {
            navigator
                .decide_with_tie_break(
                    &NavigationRequest {
                        tick: Tick(0),
                        generation: 1,
                        state,
                        route: &route,
                        progress: RouteProgress::reached_nodes(1),
                        frame: ReferenceFrameSample::IDENTITY,
                        blockers: &blockers,
                        dt_s: SYNTHETIC_PURSUIT_DT_S,
                    },
                    tie_break,
                )
                .expect("valid request")
        };

        let left = decide(0.0);
        let right = decide(1.0);
        assert_eq!(left.avoidance, AvoidanceState::Deviating);
        assert_eq!(right.avoidance, AvoidanceState::Deviating);
        let left_yaw = wrap_pi(left.step.heading_rad - state.heading_rad);
        let right_yaw = wrap_pi(right.step.heading_rad - state.heading_rad);
        assert!(
            left_yaw > 0.0,
            "draw 0.0 must prefer nose-left, got {left_yaw}"
        );
        assert!(
            right_yaw < 0.0,
            "draw 1.0 must prefer nose-right, got {right_yaw}"
        );
        let max_step = synthetic_maneuver_envelope().max_yaw_rate_radps * SYNTHETIC_PURSUIT_DT_S;
        assert!(left_yaw.abs() <= max_step + 1e-12);
        assert!(right_yaw.abs() <= max_step + 1e-12);
        assert!(
            blockers
                .iter()
                .all(|blocker| !blocker.segment_intersects(left.step.from_m, left.step.to_m))
        );
    }

    /// The seeded tie-break is a pure function of `(mission seed, actor,
    /// tick)`: a different actor or a different root seed partitions it, and
    /// every draw is a unit value.
    #[test]
    fn accept_f31_b_seeded_tie_break_is_per_actor_and_seed_deterministic() {
        let set = synthetic_pursuit_set(3);
        let first = synthetic_pursuit_actor(1);
        let second = synthetic_pursuit_actor(2);

        for tick in 0..64 {
            assert_eq!(
                set.tie_break(first, Tick(tick)),
                tie_break_draw(SYNTHETIC_PURSUIT_SEED, first, Tick(tick)),
                "the set must not alter the documented draw"
            );
        }

        let from_first: Vec<f64> = (0..64)
            .map(|tick| set.tie_break(first, Tick(tick)))
            .collect();
        let from_second: Vec<f64> = (0..64)
            .map(|tick| set.tie_break(second, Tick(tick)))
            .collect();
        assert_ne!(
            from_first, from_second,
            "actor ids must subdivide the stream"
        );

        let other = NavigationSet::new(
            SYNTHETIC_PURSUIT_SESSION,
            SYNTHETIC_PURSUIT_SEED ^ 0xDEAD_BEEF,
            *set.navigator(),
        );
        let from_other: Vec<f64> = (0..64)
            .map(|tick| other.tie_break(first, Tick(tick)))
            .collect();
        assert_ne!(
            from_first, from_other,
            "the mission seed must move the stream"
        );
        assert!(from_first.iter().all(|draw| (0.0..1.0).contains(draw)));
    }

    /// The set refuses a foreign session, a duplicate registration and a
    /// request for an actor it does not own.
    #[test]
    fn accept_f31_b_set_refuses_foreign_unknown_and_duplicate_actors() {
        let mut set = synthetic_pursuit_set(1);
        let actor = synthetic_pursuit_actor(1);
        assert_eq!(
            set.register(actor),
            Err(NavigationError::DuplicateActor { actor })
        );

        let foreign = ActorId {
            session: SessionId::new(SYNTHETIC_PURSUIT_SESSION + 1)
                .expect("a nonzero session generation"),
            serial: 9,
        };
        assert_eq!(
            set.register(foreign),
            Err(NavigationError::ForeignSession {
                expected: SYNTHETIC_PURSUIT_SESSION,
                found: SYNTHETIC_PURSUIT_SESSION + 1,
            })
        );

        let route = synthetic_pursuit_route();
        let unknown = ActorId {
            session: SYNTHETIC_PURSUIT_SESSION_ID,
            serial: 42,
        };
        let request = PursuitRequest {
            actor: unknown,
            tick: Tick(0),
            generation: SYNTHETIC_PURSUIT_SESSION,
            state: synthetic_pursuit_start(),
            route: &route,
            frame: ReferenceFrameSample::IDENTITY,
            blockers: &[],
            dt_s: SYNTHETIC_PURSUIT_DT_S,
        };
        assert_eq!(
            set.decide(&request),
            Err(NavigationError::UnknownActor { actor: unknown })
        );

        let stale = PursuitRequest {
            generation: SYNTHETIC_PURSUIT_SESSION + 1,
            ..request
        };
        assert_eq!(
            set.decide(&stale),
            Err(NavigationError::ForeignSession {
                expected: SYNTHETIC_PURSUIT_SESSION,
                found: SYNTHETIC_PURSUIT_SESSION + 1,
            })
        );
    }

    /// Task #451: a positive heading step (a target to the left; canonical
    /// `+Y`, positive heading) must command the bank that turns toward it. A
    /// left turn needs a left bank, and `FlightInput.roll` is positive
    /// right-wing-down, so the roll command must be **negative**; a target to
    /// the right is the mirror. The pre-#451 code used the opposite sign and
    /// banked away from the target.
    #[test]
    fn accept_t451_a_heading_error_commands_the_bank_that_turns_toward_it() {
        let navigator = Navigator::new(
            synthetic_maneuver_envelope(),
            NavigationCadence::designed_default(),
        )
        .expect("valid navigator");
        let route = synthetic_pursuit_route();

        let decide = |position_m: [f64; 3]| {
            navigator
                .decide(&NavigationRequest {
                    tick: Tick(0),
                    generation: 1,
                    state: NavState {
                        position_m,
                        heading_rad: 0.0, // forward -Z
                        speed_mps: 40.0,
                        climb_mps: 0.0,
                    },
                    route: &route,
                    progress: RouteProgress::reached_nodes(1),
                    frame: ReferenceFrameSample::IDENTITY,
                    blockers: &[],
                    dt_s: SYNTHETIC_PURSUIT_DT_S,
                })
                .expect("valid request")
        };

        // Node 1 of the pursuit route is at `[0, 0, -120]`, so an actor at
        // `+X` has the marker to its left and one at `-X` to its right.
        let left = decide([40.0, 0.0, 0.0]);
        let right = decide([-40.0, 0.0, 0.0]);

        let left_yaw = wrap_pi(left.step.heading_rad - 0.0);
        let right_yaw = wrap_pi(right.step.heading_rad - 0.0);
        assert!(left_yaw > 0.0, "the left target needs a nose-left step");
        assert!(right_yaw < 0.0, "the right target needs a nose-right step");
        assert!(
            left.command.roll < 0.0,
            "a nose-left step must command a left bank (negative roll), got {}",
            left.command.roll
        );
        assert!(
            right.command.roll > 0.0,
            "a nose-right step must command a right bank (positive roll), got {}",
            right.command.roll
        );
        assert!(left.command.roll.abs() <= 1.0);
        assert!(right.command.roll.abs() <= 1.0);
    }
}
