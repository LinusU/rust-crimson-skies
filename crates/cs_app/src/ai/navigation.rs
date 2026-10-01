//! Mission-ECS wiring for AI route navigation (task #446, the F31-C ECS and
//! moving-anchor follow-up).
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! # What this module owns
//!
//! F31-C produced the Bevy-free runtime follower
//! ([`cs_sim::ai::navigation::NavigationSet`]) and the conversion boundary in
//! `tools/cs_inspect`, but nothing in a running mission owned the set, fed it
//! the integrated flight state or bound an authored moving anchor to a runtime
//! actor id. This module is that wiring, all in `cs_app` because `cs_sim` may
//! not depend on `cs_content` (`docs/01-ARCHITECTURE.md`):
//!
//! * [`bind_route`] is the content-to-runtime conversion (`ResolvedRoute` ->
//!   [`BoundRoute`]). It keeps the authored `sequence` as the runtime node key,
//!   carries the reference frame and the declared
//!   [`cs_sim::ai::navigation::RouteTermination`] across, resolves a moving
//!   anchor through an [`AnchorBinding`] and refuses an unbound anchor by
//!   name. It also keeps the authored string id -> runtime id map, so a
//!   mission event bound by authored name survives an ECS reorder.
//! * [`MovingAnchor`] is the component on the live carrier/train/escort entity.
//!   Its world `Position`/`Rotation` are sampled **every fixed tick** into the
//!   `ReferenceFrameSample` the follower transforms a route node's local
//!   position with (spec non-negotiable behavior 3).
//! * [`RoutePursuit`] is the component on an AI aircraft: its session-qualified
//!   actor id, its bound route and its blockers.
//! * [`AiNavigation`] is the session resource that owns the `NavigationSet` and
//!   registers/unregisters actors as [`RoutePursuit`] entities appear and
//!   despawn.
//! * [`AiNavigationPlugin`] adds the fixed-tick systems. They run in
//!   `FixedPreUpdate`, before the F24-B `drive_flight_aircraft`, so:
//!   1. the add/despawn reconciliation register step first,
//!   2. the decision step reads each aircraft's authoritative Avian state and
//!      the moving anchor's live transform, and
//!   3. the apply step writes the follower's [`FlightInput`] into the same
//!      `FlightAircraft` command boundary the player input session uses.
//!
//! # Designed vocabulary, not original data
//!
//! Every fixture value (route ids, node positions, arrival radii, anchor kind,
//! seed, session) is newly authored project design and the envelope is
//! [`cs_sim::ai::navigation::synthetic_maneuver_envelope`]. The original route
//! encoding, AI cadence and the way the original binds a moving anchor to its
//! runtime object are unmeasured; original-data coverage is **F31-D**.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{App, Component, Entity, FixedPreUpdate, Plugin, Query, Res, ResMut, Resource, Vec3},
    time::{Fixed, Time},
};

use cs_content::routes::{
    AnchorKind, ReferenceFrame, ResolvedRoute, RouteDefinition, RouteDraft, RouteNode,
    RouteNodeId as ContentRouteNodeId, RouteTermination,
};
use cs_sim::ai::navigation::{
    Blocker, NavState, NavigationCadence, NavigationError, NavigationSet, Navigator,
    PursuitDecision, PursuitRequest, ReferenceFrameSample, RouteFrame, RouteGraph, RouteGraphError,
    RouteNode as NavRouteNode, RouteNodeId, RouteTermination as NavRouteTermination,
    heading_from_direction, synthetic_maneuver_envelope,
};
use cs_sim::damage::ActorId;
use cs_sim::flight::FlightInput;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use crate::physics::flight::{FlightAircraft, FlightAircraftError};

// -------------------------------------------------- content -> runtime ------

/// A moving anchor's authored [`ContentId`] bound to the runtime actor id the
/// route frame addresses.
///
/// The route record names an authored anchor; the runtime follower addresses an
/// actor id. The binding is the one place that translation happens, so a
/// moving route whose anchor is not in the binding table is refused rather than
/// silently addressed at an invented id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorBinding {
    /// The authored anchor content id.
    pub anchor: ContentId,
    /// The runtime actor id the anchor's live ECS entity carries.
    pub runtime_id: u64,
    /// The declared kind (carrier, train, escort, other).
    pub kind: AnchorKind,
}

impl AnchorBinding {
    /// Binds an authored anchor to `runtime_id`.
    #[must_use]
    pub fn new(anchor: ContentId, runtime_id: u64, kind: AnchorKind) -> Self {
        Self {
            anchor,
            runtime_id,
            kind,
        }
    }
}

/// Why a resolved declared route could not become a runtime [`RouteGraph`].
#[derive(Clone, Debug, PartialEq)]
pub enum RouteBindingError {
    /// A moving route named an anchor that no [`AnchorBinding`] covers.
    UnboundAnchor {
        /// The authored anchor content id.
        anchor: String,
    },
    /// The projected graph failed the runtime's own validation.
    Graph(RouteGraphError),
}

impl fmt::Display for RouteBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnboundAnchor { anchor } => {
                write!(
                    f,
                    "moving route anchor {anchor:?} is not bound to a runtime actor"
                )
            }
            Self::Graph(error) => write!(f, "the projected route graph is invalid: {error}"),
        }
    }
}

impl std::error::Error for RouteBindingError {}

/// One declared route projected into the runtime follower with its authored
/// node id -> runtime node id map preserved.
///
/// The runtime node key is the authored `sequence` (never a list index), so a
/// container or ECS reorder cannot rename a marker. [`BoundRoute::runtime_node_id`]
/// is the mapping a mission event binding by authored name consumes.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundRoute {
    graph: RouteGraph,
    node_ids: BTreeMap<String, RouteNodeId>,
    anchor_kinds: BTreeMap<u64, AnchorKind>,
}

impl BoundRoute {
    /// The runtime graph the follower consumes.
    #[must_use]
    pub const fn graph(&self) -> &RouteGraph {
        &self.graph
    }

    /// What the route does after its last node, as the runtime expresses it.
    ///
    /// This is the *declared* termination, carried across from the content
    /// record by [`bind_route`] rather than defaulted: a `Loop` record reaches
    /// the follower as a route that re-arms its marker sequence, and an `End`
    /// record as one that runs off the end. A caller can therefore ask the
    /// bound route what it will do instead of re-reading the declaration.
    #[must_use]
    pub const fn termination(&self) -> NavRouteTermination {
        self.graph.termination()
    }

    /// The largest leading-node count this route can be resumed past.
    ///
    /// The set's registration API (`NavigationSet::register_resuming`) takes a
    /// count and **no route**, so it cannot check the count against the node
    /// list, and `RouteProgress::is_complete` treats any target index past the
    /// last node as complete for *every* termination. That makes the two
    /// terminations differ:
    ///
    /// * an **ending** route may legitimately resume at `node_count`: the
    ///   follower is then on the route's finished state and holds station, which
    ///   is a real state a mission can place an aircraft in; and
    /// * a **loop** route cannot. A loop only ever reaches the node past the
    ///   end through a caller-supplied count, so a count at or beyond
    ///   `node_count` is not "finished", it is a follower with no live target
    ///   that can never wrap. Resuming a loop is therefore bounded by
    ///   `node_count() - 1`: the last node, from which the wrap re-arms node 0.
    ///
    /// [`RoutePursuit::resume_past_headroom`] reports a pursuit that exceeds it
    /// and the driver refuses it by name, so the wedge is never silent.
    #[must_use]
    pub fn max_resume_reached(&self) -> usize {
        match self.graph.termination() {
            NavRouteTermination::End => self.graph.node_count(),
            NavRouteTermination::Loop => self.graph.node_count().saturating_sub(1),
        }
    }

    /// The runtime node id for an authored node id, when the route has one.
    #[must_use]
    pub fn runtime_node_id(&self, authored: &str) -> Option<RouteNodeId> {
        self.node_ids.get(authored).copied()
    }

    /// The authored declarations for runtime `id`, when known.
    #[must_use]
    pub fn authored_node_id(&self, id: RouteNodeId) -> Option<&str> {
        self.node_ids
            .iter()
            .find(|(_, runtime)| **runtime == id)
            .map(|(authored, _)| authored.as_str())
    }

    /// Every authored node id -> runtime node id pair, in authored order.
    pub fn node_ids(&self) -> impl Iterator<Item = (&str, RouteNodeId)> + '_ {
        self.node_ids
            .iter()
            .map(|(authored, runtime)| (authored.as_str(), *runtime))
    }

    /// The declared kind of the moving anchor the frame addresses, when the
    /// route is authored in a moving frame.
    #[must_use]
    pub fn anchor_kind(&self, runtime_id: u64) -> Option<AnchorKind> {
        self.anchor_kinds.get(&runtime_id).copied()
    }
}

/// Projects a resolved declared route into the runtime follower's
/// [`RouteGraph`], resolving its moving anchor through `anchors`.
///
/// The authored `sequence` becomes the runtime node key; positions, arrival
/// radii, clearance and the declared termination are carried across; only an
/// unbound moving anchor is refused by name. A `Loop` record binds as a route
/// that re-arms its marker sequence ([`BoundRoute::termination`]), exactly as
/// `tools/cs_inspect`'s `project_route` does at the conversion boundary, so
/// the two projections can never disagree about what a declared route means.
///
/// # Errors
///
/// [`RouteBindingError::UnboundAnchor`] or [`RouteBindingError::Graph`].
pub fn bind_route(
    route: &ResolvedRoute,
    anchors: &[AnchorBinding],
) -> Result<BoundRoute, RouteBindingError> {
    let mut anchor_kinds = BTreeMap::new();
    let frame = match route.frame() {
        ReferenceFrame::World => RouteFrame::World,
        ReferenceFrame::Moving(moving) => {
            let binding = anchors
                .iter()
                .find(|binding| binding.anchor == moving.anchor)
                .ok_or_else(|| RouteBindingError::UnboundAnchor {
                    anchor: moving.anchor.as_str().to_owned(),
                })?;
            anchor_kinds.insert(binding.runtime_id, binding.kind);
            RouteFrame::Moving {
                anchor: binding.runtime_id,
            }
        }
    };
    let termination = match route.termination() {
        RouteTermination::End => NavRouteTermination::End,
        RouteTermination::Loop => NavRouteTermination::Loop,
    };
    let mut node_ids = BTreeMap::new();
    let nodes = route
        .nodes()
        .iter()
        .map(|node| {
            let runtime = RouteNodeId(node.sequence);
            node_ids.insert(node.id.as_str().to_owned(), runtime);
            NavRouteNode {
                id: runtime,
                sequence: node.sequence,
                mandatory: node.mandatory,
                position_m: node.position_m.value,
                arrival_radius_m: node.arrival_radius_m.value,
            }
        })
        .collect();
    let graph =
        RouteGraph::try_new_terminated(frame, termination, route.clearance_m().value, nodes)
            .map_err(RouteBindingError::Graph)?;
    Ok(BoundRoute {
        graph,
        node_ids,
        anchor_kinds,
    })
}

// ------------------------------------------------------------ components ---

/// The ECS entity that carries a moving route anchor's live world transform.
///
/// It is the runtime realization of an authored [`AnchorKind`] — a carrier,
/// train or escort. [`bind_route`] addresses it by [`MovingAnchor::runtime_id`];
/// the fixed tick samples its [`Position`] and [`Rotation`] into the frame the
/// follower transforms the route's local node positions with.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct MovingAnchor {
    /// The authored anchor content id this entity realizes.
    anchor: ContentId,
    /// The declared kind.
    kind: AnchorKind,
    /// The runtime actor id route frames address.
    runtime_id: u64,
}

impl MovingAnchor {
    /// Declares a moving anchor entity.
    #[must_use]
    pub fn new(anchor: ContentId, runtime_id: u64, kind: AnchorKind) -> Self {
        Self {
            anchor,
            kind,
            runtime_id,
        }
    }

    /// The authored anchor content id.
    #[must_use]
    pub const fn anchor(&self) -> &ContentId {
        &self.anchor
    }

    /// The declared kind.
    #[must_use]
    pub const fn kind(&self) -> AnchorKind {
        self.kind
    }

    /// The runtime actor id the route frame addresses.
    #[must_use]
    pub const fn runtime_id(&self) -> u64 {
        self.runtime_id
    }
}

/// An AI aircraft pursuing a projected route in the mission ECS.
///
/// The component is the mission-side producer/consumer record:
/// [`AiNavigationPlugin`] registers [`RoutePursuit::actor`] with the session's
/// `NavigationSet`, feeds the fixed tick the aircraft's authoritative Avian
/// state, and writes the resulting [`FlightInput`] into the entity's
/// [`FlightAircraft`].
#[derive(Component, Clone, Debug, PartialEq)]
pub struct RoutePursuit {
    actor: ActorId,
    route: BoundRoute,
    blockers: Vec<Blocker>,
    resume_reached: usize,
}

impl RoutePursuit {
    /// Pursues `route` as `actor`, with progress resumed past `resume_reached`
    /// leading nodes (the spawn node a mission places the aircraft on).
    ///
    /// `resume_reached` is a leading-node count, never an index into the node
    /// list, and it is bounded by [`BoundRoute::max_resume_reached`]: a loop
    /// route cannot be resumed at or past its node count, because such a
    /// progress has no live target and can never wrap.
    /// [`resume_past_headroom`](Self::resume_past_headroom) reports a pursuit
    /// that exceeds the bound, and the driver refuses it by name.
    #[must_use]
    pub fn new(actor: ActorId, route: BoundRoute, resume_reached: usize) -> Self {
        Self {
            actor,
            route,
            blockers: Vec::new(),
            resume_reached,
        }
    }

    /// Adds the static blockers the follower must clear.
    #[must_use]
    pub fn with_blockers(mut self, blockers: Vec<Blocker>) -> Self {
        self.blockers = blockers;
        self
    }

    /// The session-qualified actor the set owns.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// The bound route.
    #[must_use]
    pub const fn route(&self) -> &BoundRoute {
        &self.route
    }

    /// The static blockers.
    #[must_use]
    pub fn blockers(&self) -> &[Blocker] {
        &self.blockers
    }

    /// How many leading nodes the actor resumes past at registration.
    #[must_use]
    pub const fn resume_reached(&self) -> usize {
        self.resume_reached
    }

    /// Whether the resume count is past the bound this route can be resumed
    /// from, i.e. whether registering it would leave the follower with no live
    /// target.
    ///
    /// `NavigationSet::register_resuming` takes a count and no route, so it
    /// cannot make this check itself; the driver makes it here, where the bound
    /// route is in hand, and refuses by name instead of registering a pursuit
    /// that can never fly.
    #[must_use]
    pub fn resume_past_headroom(&self) -> bool {
        self.resume_reached > self.route.max_resume_reached()
    }
}

// ------------------------------------------------------------- session ------

/// The session's navigation authority: the one `NavigationSet` a mission owns.
///
/// The set is confined to the session generation it was built for (F31-B), so
/// a command or actor from another generation is refused rather than followed.
/// A fresh session builds a fresh set, so no pursuit state can be inherited.
#[derive(Resource, Debug)]
pub struct AiNavigation {
    set: NavigationSet,
}

impl AiNavigation {
    /// A set for `session` whose tie-breaks derive from `mission_seed`.
    #[must_use]
    pub fn new(session: u64, mission_seed: u64, navigator: Navigator) -> Self {
        Self {
            set: NavigationSet::new(session, mission_seed, navigator),
        }
    }

    /// The session generation this set owns.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.set.session()
    }

    /// Read-only access to the underlying follower.
    #[must_use]
    pub const fn set(&self) -> &NavigationSet {
        &self.set
    }

    /// Mutable access to the underlying follower.
    pub fn set_mut(&mut self) -> &mut NavigationSet {
        &mut self.set
    }

    /// Whether `actor` currently has pursuit state.
    #[must_use]
    pub fn is_registered(&self, actor: ActorId) -> bool {
        self.set.contains(actor)
    }

    /// How many route nodes `actor` has reached, if registered.
    ///
    /// This is a monotonic **total**, not a position: on a loop-terminated
    /// route [`cs_sim::ai::navigation::RouteProgress::reached`] counts the
    /// nodes reached across every lap and never rewinds, so the count keeps
    /// climbing past the route's node count. Use [`Self::laps`] as the
    /// lap-independent companion and the route's own `next_index` as the live
    /// target; do not index the route with this count on a loop.
    #[must_use]
    pub fn reached(&self, actor: ActorId) -> Option<usize> {
        self.set
            .state(actor)
            .map(|state| state.progress().reached())
    }

    /// How many times `actor`'s route has re-armed, if registered.
    ///
    /// Zero until the follower reaches a loop route's last node; it is the
    /// count a caller reads instead of dividing [`Self::reached`] by a node
    /// count, which would also count the resumed leading nodes.
    #[must_use]
    pub fn laps(&self, actor: ActorId) -> Option<u32> {
        self.set.state(actor).map(|state| state.progress().laps())
    }

    /// Whether `actor` has completed `route`, if it is registered.
    ///
    /// The set owns progress but not the route, so completion is only
    /// answerable with the route the caller bound the actor to.
    #[must_use]
    pub fn is_complete(&self, actor: ActorId, route: &RouteGraph) -> bool {
        self.set
            .state(actor)
            .is_some_and(|state| state.is_complete(route))
    }
}

/// Why one AI tick produced no command.
#[derive(Clone, Debug, PartialEq)]
pub enum NavigationRefusalReason {
    /// The actor belongs to another session generation; it is never inherited.
    ForeignSession {
        /// The session the set owns.
        expected: u64,
        /// The generation the actor carried.
        found: u64,
    },
    /// Two entities claimed the same actor in one tick.
    DuplicateActor {
        /// The repeated actor.
        actor: ActorId,
    },
    /// A moving route's anchor has no live entity this tick.
    MissingAnchor {
        /// The runtime anchor id that could not be sampled.
        anchor: u64,
    },
    /// The live entity's declared anchor kind does not match the route frame.
    AnchorKindMismatch {
        /// The runtime anchor id.
        anchor: u64,
        /// The kind the route declared.
        route: AnchorKind,
        /// The kind the live entity declared.
        entity: AnchorKind,
    },
    /// The pursuit's resume count is past the bound its route can be resumed
    /// from, so registering it would leave the follower with no live target.
    ResumePastRouteEnd {
        /// The resume count the pursuit declared.
        resume_reached: usize,
        /// The largest resume count this route can be flown from.
        max_resume_reached: usize,
    },
    /// The follower refused the decision.
    Decision(NavigationError),
    /// The bounded command could not be written into the flight record.
    Command(FlightAircraftError),
}

impl fmt::Display for NavigationRefusalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => write!(
                f,
                "an actor of generation {found} cannot join a set of generation {expected}"
            ),
            Self::DuplicateActor { actor } => write!(f, "{actor} was presented twice in one tick"),
            Self::MissingAnchor { anchor } => {
                write!(f, "moving route anchor {anchor} has no live entity")
            }
            Self::AnchorKindMismatch {
                anchor,
                route,
                entity,
            } => write!(
                f,
                "moving anchor {anchor} is a {} but the route declared a {}",
                anchor_kind_label(*entity),
                anchor_kind_label(*route)
            ),
            Self::ResumePastRouteEnd {
                resume_reached,
                max_resume_reached,
            } => write!(
                f,
                "a pursuit resumed past {resume_reached} nodes cannot be flown: its route \
                 accepts at most {max_resume_reached}, because a target index past the last \
                 node leaves the follower with nothing to fly to"
            ),
            Self::Decision(error) => write!(f, "the follower refused the decision: {error}"),
            Self::Command(error) => write!(f, "the command could not be applied: {error}"),
        }
    }
}

impl std::error::Error for NavigationRefusalReason {}

/// One refused AI tick.
#[derive(Clone, Debug, PartialEq)]
pub struct NavigationRefusal {
    /// The fixed tick the refusal happened on.
    pub tick: u64,
    /// The aircraft entity that was refused, when it is known.
    pub entity: Option<Entity>,
    /// The actor that was refused.
    pub actor: ActorId,
    /// Why it was refused.
    pub reason: NavigationRefusalReason,
}

/// The navigation driver's per-session accounting.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct NavigationTickReport {
    /// Fixed ticks the driver ran.
    pub ticks: u64,
    /// Aircraft-ticks that produced a bounded follower command.
    pub decided: u64,
    /// Aircraft-ticks whose command reached the flight record.
    pub applied: u64,
    /// Aircraft-ticks refused for a recorded reason.
    pub refused: u64,
    /// The most recent refusal.
    pub last_refusal: Option<NavigationRefusal>,
}

fn record_refusal(
    report: &mut NavigationTickReport,
    tick: u64,
    entity: Option<Entity>,
    actor: ActorId,
    reason: NavigationRefusalReason,
) {
    report.refused += 1;
    report.last_refusal = Some(NavigationRefusal {
        tick,
        entity,
        actor,
        reason,
    });
}

/// Commands produced by the decision step, consumed by the apply step in the
/// same fixed tick. Private to the plugin.
#[derive(Resource, Default)]
struct PendingNavigationCommands {
    candidates: Vec<(ActorId, FlightInput)>,
}

// ------------------------------------------------------------------ plugin --

/// A label for diagnostics.
fn anchor_kind_label(kind: AnchorKind) -> &'static str {
    match kind {
        AnchorKind::Carrier => "carrier",
        AnchorKind::Train => "train",
        AnchorKind::Escort => "escort",
        AnchorKind::Other => "other",
    }
}

/// Registers the AI navigation session and its fixed-tick systems.
///
/// The systems run in `FixedPreUpdate`, so they observe the authoritative
/// state of the previous tick and write the flight command before the F24-B
/// fixed-wing driver runs in `FixedUpdate`.
#[derive(Clone, Copy, Debug)]
pub struct AiNavigationPlugin {
    session: u64,
    mission_seed: u64,
}

impl AiNavigationPlugin {
    /// Declares the plugin for `session`, drawing tie-breaks from
    /// `mission_seed`.
    #[must_use]
    pub const fn new(session: u64, mission_seed: u64) -> Self {
        Self {
            session,
            mission_seed,
        }
    }
}

impl Plugin for AiNavigationPlugin {
    fn build(&self, app: &mut App) {
        let navigator = Navigator::new(
            synthetic_maneuver_envelope(),
            NavigationCadence::designed_default(),
        )
        .expect("the designed synthetic envelope and cadence are valid");
        app.insert_resource(AiNavigation::new(
            self.session,
            self.mission_seed,
            navigator,
        ));
        app.init_resource::<NavigationTickReport>();
        app.init_resource::<PendingNavigationCommands>();
        app.add_systems(
            FixedPreUpdate,
            (decide_navigation, apply_navigation_commands).chain(),
        );
    }
}

/// The live pose of one moving anchor for this tick.
fn sample_anchor(position: &Position, rotation: &Rotation) -> ReferenceFrameSample {
    let forward = rotation.0 * Vec3::NEG_Z;
    ReferenceFrameSample {
        origin_m: position.0.to_array().map(f64::from),
        yaw_rad: heading_from_direction(f64::from(forward.x), f64::from(forward.z)),
    }
}

/// The aircraft's authoritative state as the follower consumes it.
///
/// The heading is the body forward axis about canonical `+Y`
/// (`docs/contracts/FLIGHT-PHYSICS.md`); speed is the horizontal component and
/// climb the vertical one, matching the step the follower commits.
fn nav_state(position: &Position, rotation: &Rotation, velocity: &LinearVelocity) -> NavState {
    let forward = rotation.0 * Vec3::NEG_Z;
    let v = velocity.0;
    NavState {
        position_m: position.0.to_array().map(f64::from),
        heading_rad: heading_from_direction(f64::from(forward.x), f64::from(forward.z)),
        speed_mps: f64::from((v.x * v.x + v.z * v.z).sqrt()),
        climb_mps: f64::from(v.y),
    }
}

/// Registers/de-registers actors from the live [`RoutePursuit`] roster and
/// decides one bounded command per aircraft.
fn decide_navigation(
    time: Res<Time<Fixed>>,
    mut navigation: ResMut<AiNavigation>,
    mut pending: ResMut<PendingNavigationCommands>,
    mut report: ResMut<NavigationTickReport>,
    anchors: Query<(&MovingAnchor, &Position, &Rotation)>,
    pursuers: Query<(Entity, &RoutePursuit, &Position, &Rotation, &LinearVelocity)>,
) {
    report.ticks += 1;
    let tick = Tick(report.ticks - 1);
    let dt_s = time.timestep().as_secs_f64();
    pending.candidates.clear();

    // Teardown and registration: the set's roster must exactly match the live
    // pursuit entities, so a despawn removes its state and a fresh entity with
    // an unknown/foreign actor is refused rather than inheriting one.
    let present: Vec<(Entity, &RoutePursuit)> = pursuers
        .iter()
        .map(|(entity, pursuit, ..)| (entity, pursuit))
        .collect();
    let present_actors: BTreeSet<ActorId> =
        present.iter().map(|(_, pursuit)| pursuit.actor()).collect();
    let stale: Vec<ActorId> = navigation
        .set
        .actors()
        .filter(|actor| !present_actors.contains(actor))
        .collect();
    for actor in stale {
        navigation.set.unregister(actor);
    }
    for (entity, pursuit) in &present {
        let actor = pursuit.actor();
        if navigation.set.contains(actor) {
            continue;
        }
        // The registration API below takes a count and no route, so it cannot
        // tell a flyable resume from one that leaves the follower with no live
        // target. This is where the bound route is in hand, so the check is
        // made here and refused by name instead of registering a pursuit that
        // holds station forever.
        if pursuit.resume_past_headroom() {
            record_refusal(
                &mut report,
                tick.0,
                Some(*entity),
                actor,
                NavigationRefusalReason::ResumePastRouteEnd {
                    resume_reached: pursuit.resume_reached(),
                    max_resume_reached: pursuit.route().max_resume_reached(),
                },
            );
            continue;
        }
        if let Err(error) = navigation
            .set
            .register_resuming(actor, pursuit.resume_reached())
        {
            report.refused += 1;
            report.last_refusal = Some(NavigationRefusal {
                tick: tick.0,
                entity: Some(*entity),
                actor,
                reason: match error {
                    NavigationError::ForeignSession { expected, found } => {
                        NavigationRefusalReason::ForeignSession { expected, found }
                    }
                    NavigationError::DuplicateActor { actor } => {
                        NavigationRefusalReason::DuplicateActor { actor }
                    }
                    other => NavigationRefusalReason::Decision(other),
                },
            });
        }
    }

    // Live anchor samples, keyed by runtime actor id.
    let mut anchors_by_id: BTreeMap<u64, (&MovingAnchor, ReferenceFrameSample)> = BTreeMap::new();
    for (anchor, position, rotation) in &anchors {
        anchors_by_id
            .entry(anchor.runtime_id())
            .or_insert_with(|| (anchor, sample_anchor(position, rotation)));
    }

    let decided: Result<Vec<PursuitDecision>, NavigationError> = {
        let mut requests = Vec::new();
        for (entity, pursuit, position, rotation, velocity) in &pursuers {
            let actor = pursuit.actor();
            if !navigation.set.contains(actor) {
                continue;
            }
            let frame = match pursuit.route().graph().frame {
                RouteFrame::World => ReferenceFrameSample::IDENTITY,
                RouteFrame::Moving { anchor } => {
                    let Some((live, sample)) = anchors_by_id.get(&anchor) else {
                        record_refusal(
                            &mut report,
                            tick.0,
                            Some(entity),
                            actor,
                            NavigationRefusalReason::MissingAnchor { anchor },
                        );
                        continue;
                    };
                    if let Some(route_kind) = pursuit.route().anchor_kind(anchor)
                        && route_kind != live.kind()
                    {
                        record_refusal(
                            &mut report,
                            tick.0,
                            Some(entity),
                            actor,
                            NavigationRefusalReason::AnchorKindMismatch {
                                anchor,
                                route: route_kind,
                                entity: live.kind(),
                            },
                        );
                        continue;
                    }
                    *sample
                }
            };
            requests.push(PursuitRequest {
                actor,
                tick,
                generation: navigation.session(),
                state: nav_state(position, rotation, velocity),
                route: pursuit.route().graph(),
                frame,
                blockers: pursuit.blockers(),
                dt_s,
            });
        }
        navigation.set.decide_all(&requests)
    };

    match decided {
        Ok(decisions) => {
            for decision in decisions {
                report.decided += 1;
                pending
                    .candidates
                    .push((decision.actor, decision.decision.command));
            }
        }
        Err(error) => {
            for (_, pursuit) in &present {
                if navigation.set.contains(pursuit.actor()) {
                    record_refusal(
                        &mut report,
                        tick.0,
                        None,
                        pursuit.actor(),
                        NavigationRefusalReason::Decision(error.clone()),
                    );
                }
            }
        }
    }
}

/// Writes the follower's bounded command into the same [`FlightAircraft`]
/// command boundary the player input session uses.
fn apply_navigation_commands(
    mut pending: ResMut<PendingNavigationCommands>,
    mut report: ResMut<NavigationTickReport>,
    mut pursuers: Query<(Entity, &RoutePursuit, &mut FlightAircraft)>,
) {
    let tick = report.ticks.saturating_sub(1);
    for (entity, pursuit, mut aircraft) in &mut pursuers {
        let Some((_, command)) = pending
            .candidates
            .iter()
            .find(|(actor, _)| *actor == pursuit.actor())
        else {
            continue;
        };
        match aircraft.set_command(*command) {
            Ok(()) => report.applied += 1,
            Err(error) => record_refusal(
                &mut report,
                tick,
                Some(entity),
                pursuit.actor(),
                NavigationRefusalReason::Command(error),
            ),
        }
    }
    pending.candidates.clear();
}

// ------------------------------------------------------------- fixtures -----

/// The designed synthetic mission seed for the ECS navigation fixtures. Newly
/// authored fixture data, not a measured original seed.
pub const SYNTHETIC_NAVIGATION_SEED: u64 = 0x4633_3143_4543_5300; // "F31C ECS\0"

/// The session generation the ECS navigation acceptance fixture uses.
pub const SYNTHETIC_NAVIGATION_SESSION: u64 = 20;

/// The `route` content id of [`declared_synthetic_moving_route`].
pub const SYNTHETIC_MOVING_ROUTE_ID: &str = "synthetic.moving-patrol";

/// The authored anchor content id of [`declared_synthetic_moving_route`].
pub const SYNTHETIC_MOVING_ANCHOR_ID: &str = "synthetic.carrier";

/// The designed runtime actor id the synthetic moving anchor is bound to.
pub const SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID: u64 = 41;

/// The declared synthetic moving route: a patrol authored in the frame of a
/// carrier/train/escort anchor, with the same local geometry as the F31-B
/// moving-waypoint fixture.
///
/// `anchor` is the authored anchor content id; [`AnchorBinding`] must bind it
/// to [`SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID`], which the [`MovingAnchor`]
/// component carries. Every value is newly authored project design.
#[must_use]
pub fn declared_synthetic_moving_route(anchor: ContentId, kind: AnchorKind) -> RouteDefinition {
    let designed = || {
        Provenance::designed(
            ClaimId::new("t446.synthetic-moving-route").expect("the fixture claim id is valid"),
        )
    };
    let node =
        |id: &str, sequence: u32, mandatory: bool, position: [f64; 3], radius: f64| RouteNode {
            id: ContentRouteNodeId::try_new(id).expect("the fixture node id is valid"),
            sequence,
            mandatory,
            position_m: Resolved::Known(Known::new(position, designed())),
            arrival_radius_m: Resolved::Known(Known::new(radius, designed())),
            trigger: Resolved::Known(Known::new(None, designed())),
        };
    let edge = |from: &str, to: &str| cs_content::routes::RouteEdge {
        from: ContentRouteNodeId::try_new(from).expect("the fixture node id is valid"),
        to: ContentRouteNodeId::try_new(to).expect("the fixture node id is valid"),
    };
    RouteDefinition::try_new(RouteDraft {
        id: ContentId::from_source(ContentKind::Route, SYNTHETIC_MOVING_ROUTE_ID)
            .expect("the fixture route id is valid"),
        origin: Origin::SyntheticFixture,
        frame: ReferenceFrame::Moving(cs_content::routes::MovingAnchor { anchor, kind }),
        termination: RouteTermination::End,
        clearance_m: Resolved::Known(Known::new(0.0, designed())),
        nodes: vec![
            node("start", 0, false, [0.0, 0.0, 0.0], 6.0),
            node("waypoint", 1, true, [0.0, 0.0, -120.0], 10.0),
            node("goal", 2, true, [0.0, 0.0, -260.0], 12.0),
        ],
        edges: vec![edge("start", "waypoint"), edge("waypoint", "goal")],
        provenance: designed(),
    })
    .expect("the declared synthetic moving route is valid")
}

/// The authored anchor content id of the synthetic moving anchor fixture.
#[must_use]
pub fn synthetic_moving_anchor_id() -> ContentId {
    // A moving carrier/train/escort is a moving vehicle; the engine vocabulary
    // has no dedicated anchor kind, so the fixture names it as an airframe-like
    // object. The kind a route declares (`AnchorKind`) is separate from the
    // content namespace.
    ContentId::from_source(ContentKind::Airframe, SYNTHETIC_MOVING_ANCHOR_ID)
        .expect("the fixture anchor id is valid")
}
