//! The wired network session drive: one `cs_net::lifecycle` session owned and
//! pumped by the Bevy schedule (F54-C follow-up, Rally #592).
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! stage `### F54-C`, and `docs/contracts/UI-NETWORK.md` (ownership table:
//! the server owns every actor, every kill and every session fact; the client
//! owns its input and nothing else).
//!
//! F54-C built the session objects and stopped at their boundary: "A Bevy
//! plugin or system that owns and pumps one is left for the next stage" (see
//! `docs/findings/2026-10-03-f54-c-session-lifecycle-and-bounded-processing.md`,
//! "Not wired"). This is that stage. [`NetHost`] owns a
//! [`cs_net::lifecycle::ServerSession`] together with the authoritative
//! [`cs_sim::net_state::NetStateLedger`], [`NetClient`] owns a
//! [`cs_net::lifecycle::ClientSession`], and [`drive_net_session`] pumps each
//! installed one exactly once per committed fixed tick:
//!
//! * **Host.** `ServerSession::pump` produces the tick's
//!   [`cs_net::lifecycle::ServerNotice`]s, forwarded verbatim into
//!   [`NetSessionReport`]; a peer departure releases its control buffer rather
//!   than leaving a ghost stick deflected. `drain_work` hands each admitted
//!   [`cs_net::lifecycle::PeerInput`] to the authoritative paths it could not
//!   name: `frames` fold into the peer's [`cs_sim::control::ControlBuffer`]
//!   (the F22-A input boundary), `fires` mint
//!   [`cs_sim::weapons::FireIntent`]s through
//!   [`crate::network::recovery::fire_intent`] — the conversion `cs_net`
//!   cannot do because it must not depend on `cs_sim` — and the ledger's
//!   input book is observed before the wire acknowledgment is sent. When the
//!   session is `Live`, [`publish_snapshot`] quantizes the ledger into the
//!   shared origin frame and `ServerSession::publish_snapshot` puts it on
//!   the wire.
//! * **Client.** `ClientSession::pump` produces its notices the same way;
//!   reliable event bodies are applied to the [`NetSession`] mirror (the
//!   F57-C typed path), and the newest accepted `ClientSession::snapshot()`
//!   is handed to `NetSession::ingest` at most once each — the session's
//!   snapshot slot is a latest-wins mailbox, so a snapshot already fed is
//!   never fed twice. The [`NetSession`] resource itself is installed by the
//!   drive the moment the handshake grant names the session epoch, built from
//!   the [`NetClient`]'s declared origin and interpolation config; a mirror
//!   for a different generation is torn down and replaced, never fed a
//!   foreign epoch's traffic.
//!
//! # What stays elsewhere
//!
//! * The minted `FireIntent`s go to the *weapon* schedule, not the guns: they
//!   wait in [`NetHost::take_orders`] for the session driver that publishes
//!   [`crate::weapons::WeaponTickInput`]s to fold them into that tick's
//!   `orders` — the same channel authored orders and the cockpit use, and the
//!   resolver's own `ForeignTick`/`DuplicateIntent` refusals stay the shot's
//!   judges. An intent for a tick already committed is drained as stale by
//!   name, not silently kept.
//! * The `SessionReceiver` (`crate::network::recovery`) and the transport
//!   gate's match-stage intent validation are the F58 follow-up recorded as
//!   Rally #1221: this drive consumes the already-admitted [`PeerInput`] the
//!   lifecycle produced; it does not re-run admission.
//!
//! # The F57-C.01 scheduled consumption
//!
//! Rally #657 fills in what the earlier drive deliberately left out: the
//! mirror's own consumers now run on the same committed tick, in one
//! explicitly chained [`NetSessionSet`] in `FixedPostUpdate`, ordered after
//! Avian's [`PhysicsSystems::StepSimulation`]:
//!
//! 1. [`NetSessionSet::Origin`] ([`drive_net_origin`]) — the installed
//!    [`NetOriginDriver`] advances one frame of wall time into whole fixed
//!    ticks, so a policy rebase happens *before* this tick's snapshot is
//!    decoded. The frame the driver produced is adopted by `NetHost` (the
//!    publish frame), `NetClient` (the frame future mirrors build in) and the
//!    live [`NetMirror`] through `NetSession::rebase` — one call per epoch
//!    the driver crossed — and the one [`NetPredictedBody`] pose owner's
//!    `Position` is converted through the same frame change, because a local
//!    `f32` coordinate does not survive an epoch change unconverted.
//! 2. [`NetSessionSet::Drive`] ([`drive_net_session`]) — the F54-C pump: the
//!    lifecycle is pumped, a `Closed` client session's mirror is released by
//!    name, decoded `SnapshotFrame`s drain into `NetSession::ingest` in tick
//!    order (the latest-wins mailbox is fed at most once per snapshot), and
//!    reliable `ActorSpawned` events teach the drive which actor the wire
//!    named this client's own.
//! 3. [`NetSessionSet::Sample`] ([`sample_net_mirror`]) — every mirrored
//!    remote actor is sampled at the committed tick (the schedule's estimate
//!    of the server tick) into [`NetPresentation`]. The local actor is never
//!    interpolated: its pose is the predictor's, not the buffer's.
//! 4. [`NetSessionSet::Correct`] ([`correct_local_pose`]) — the
//!    server-named local actor is attached, the pose owner's committed pose
//!    is recorded, `reconcile_local` is run when this tick's ingest moved
//!    the local record, and `NetSession::next_correction` is written through
//!    the one marked [`NetPredictedBody`] entity — the only place a pose the
//!    session produced reaches the ECS. Two markers are refused by name
//!    (`NetPoseOwner::Ambiguous`), not silently double-applied.
//! 5. [`NetSessionSet::ExpireTracers`] ([`expire_net_tracers`]) — client
//!    tracers past their presentation window are expired.
//!
//! Every refusal the chain hits — `SessionError`, `IngestRefusal`,
//! `PublishError`, `BufferRefusal`, snapshot-decode and lifecycle notices —
//! lands in the one caller-visible channel, [`NetSessionReport`]; nothing in
//! the chain returns or drops a refusal. A session end or lifecycle
//! disconnect runs `NetSession::teardown` by name, so no aircraft, tracer or
//! interpolation track outlives its generation, and a reconnect builds a
//! fresh mirror under the fresh grant's epoch rather than feeding the spent
//! one.
//!
//! All of this is newly authored engine design: no original networked
//! behavior has been measured, and none is asserted here.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use avian3d::prelude::{PhysicsSystems, Position, Rotation};
use bevy::app::{App, Plugin};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::math::{Quat, Vec3};
use bevy::prelude::FixedPostUpdate;
use bevy::prelude::World;
use bevy::time::{Fixed, Time};

use cs_net::lifecycle::{
    ClientNotice, ClientPhase, ClientSession, PeerInput, ServerFault, ServerNotice, ServerPhase,
    ServerSession,
};
use cs_net::message::{EventBody, ReliableEvent};
use cs_net::validation::SessionViolation;
use cs_sim::control::{ControlBuffer, ControlError};
use cs_sim::net_state::{NetStateError, NetStateLedger};
use cs_types::Tick;
use cs_types::net::{ActorId, PeerId, SessionId};
use cs_types::space::{LocalPosition, Quaternion};

use crate::origin::{FixedTickDriver, WorldOrigin};
use crate::physics::{FlightAircraft, PhysicsTickLedger};
use crate::weapons::WeaponOrder;

use super::physics::{
    BufferRefusal, EpochTransition, IngestReport, InterpolatedAircraft, InterpolationConfig,
    NetSession, PoseCorrection, PredictedPose, PredictionConfig, PublishError, ReconcileKind,
    Reconciliation, publish_snapshot,
};
use super::recovery::fire_intent;

/// How many notices of one side one run's report keeps.
///
/// One `pump` emits a bounded handful of notices per admitted packet plus
/// membership churn, so this bound is generous by design — its overflow count
/// is what reports a session that produced more, rather than the log growing
/// without limit.
const MAX_REPORTED_NOTICES: usize = 256;

/// One remote pilot's authoritative inbound control state.
///
/// The buffer is the F22-A `ControlBuffer` the wire frames fold into: axes
/// apply immediately, edges queue for the frame's tick, and the flight
/// driver's `begin_tick` consumes them. Nothing about the buffer is a client
/// authority: it is what the *server* consumed of the peer's input.
#[derive(Clone, Debug)]
struct RemotePilot {
    /// The aircraft the gate bound to the peer.
    actor: ActorId,
    /// The peer's inbound control buffer.
    buffer: ControlBuffer,
}

/// Why a [`NetHost`] could not be assembled or retried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetHostError {
    /// The authoritative ledger serves a different session generation than
    /// the session object — the two authorities must agree or every snapshot
    /// would publish one epoch's state under another's header.
    ForeignSession {
        /// The session object's epoch.
        session: SessionId,
        /// The ledger's epoch.
        ledger: SessionId,
    },
    /// The session's own `reopen` refused the retry; the fault is the
    /// session's words, not a guess.
    Reopen(ServerFault),
}

impl fmt::Display for NetHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { session, ledger } => write!(
                f,
                "ledger serves session {ledger}, but the session object is {session}"
            ),
            Self::Reopen(fault) => write!(f, "the session refused the reopen: {fault}"),
        }
    }
}

impl std::error::Error for NetHostError {}

/// What [`NetHost::take_orders`] drained for the tick `at`.
///
/// `matched` are the orders whose intent tick is `at` — the only ones the
/// resolver at that tick can accept. `stale` are orders whose intent tick
/// already passed: they are named rather than silently kept, the same
/// discipline [`crate::weapons::WeaponTickInputs`] applies to a record
/// published for a committed tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoteOrderBatch {
    /// Fire orders minted for `at`.
    pub matched: Vec<WeaponOrder>,
    /// Fire orders whose intent tick is behind `at`.
    pub stale: Vec<WeaponOrder>,
}

/// Resource: the host side of a networked session.
///
/// One resource owns the `cs_net` session object, the `cs_sim` authoritative
/// ledger it publishes from, the world-origin frame snapshots are quantized
/// in, and the per-peer control state the wire feeds. A session driver (the
/// mission host, a lobby) binds peers to actors through
/// [`Self::bind_aircraft`], publishes actor state into [`Self::ledger_mut`],
/// drains [`Self::take_orders`] into the tick's weapon inputs, and calls
/// `session_mut().launch/finish/close` for the match's own lifecycle.
#[derive(Resource)]
pub struct NetHost {
    session: ServerSession,
    ledger: NetStateLedger,
    origin: WorldOrigin,
    pilots: BTreeMap<PeerId, RemotePilot>,
    orders: VecDeque<WeaponOrder>,
}

impl NetHost {
    /// A host for the already-bound `session`, publishing `ledger`'s state in
    /// `origin`'s frame.
    ///
    /// # Errors
    ///
    /// [`NetHostError::ForeignSession`] when the ledger and the session object
    /// do not serve the same epoch.
    pub fn new(
        session: ServerSession,
        ledger: NetStateLedger,
        origin: WorldOrigin,
    ) -> Result<Self, NetHostError> {
        if ledger.session() != session.session() {
            return Err(NetHostError::ForeignSession {
                session: session.session(),
                ledger: ledger.session(),
            });
        }
        Ok(Self {
            session,
            ledger,
            origin,
            pilots: BTreeMap::new(),
            orders: VecDeque::new(),
        })
    }

    /// The session object, for membership and phase reads.
    #[must_use]
    pub const fn session(&self) -> &ServerSession {
        &self.session
    }

    /// The session object, for the match's own lifecycle calls (`launch`,
    /// `finish`, `close`, `reopen`, `announce`).
    pub fn session_mut(&mut self) -> &mut ServerSession {
        &mut self.session
    }

    /// The authoritative ledger the snapshots publish.
    #[must_use]
    pub const fn ledger(&self) -> &NetStateLedger {
        &self.ledger
    }

    /// The authoritative ledger, for the session driver's per-tick
    /// `NetActorState` publications.
    pub fn ledger_mut(&mut self) -> &mut NetStateLedger {
        &mut self.ledger
    }

    /// The world-origin frame snapshots are quantized in.
    #[must_use]
    pub const fn origin(&self) -> WorldOrigin {
        self.origin
    }

    /// Moves the publish frame to `next`'s origin. Snapshots after the call
    /// publish the new epoch; clients adopt it through the snapshot's own
    /// epoch field (F57-C.01 owns the receiving rebase).
    pub fn set_origin(&mut self, next: WorldOrigin) {
        self.origin = next;
    }

    /// Spends the epoch and mints a fresh one on the same socket — the
    /// session object's own [`ServerSession::reopen`] — then swaps in the
    /// fresh generation's `ledger`. The spent epoch's peer bindings and
    /// queued fire orders die with it: `reopen` hung every peer up, so a
    /// control buffer or an order still naming one would be a ghost nobody
    /// owns. Returns how many hung-up connections the retry reported.
    ///
    /// This is the retry half the F57-C.01 teardown test uses: a client that
    /// reconnects after it gets a grant for the *new* session, and the drive
    /// builds a fresh mirror under it rather than feeding the spent one.
    ///
    /// # Errors
    ///
    /// [`NetHostError::ForeignSession`] when `ledger` does not serve
    /// `session`, or [`NetHostError::Reopen`] when the session object refused
    /// the retry.
    pub fn reopen(
        &mut self,
        session: SessionId,
        ledger: NetStateLedger,
    ) -> Result<usize, NetHostError> {
        if ledger.session() != session {
            return Err(NetHostError::ForeignSession {
                session,
                ledger: ledger.session(),
            });
        }
        let hung_up = self.session.reopen(session).map_err(NetHostError::Reopen)?;
        self.ledger = ledger;
        self.pilots.clear();
        self.orders.clear();
        Ok(hung_up)
    }

    /// Binds `peer` to `actor` through the session gate's ownership table and
    /// opens the peer's control buffer.
    ///
    /// A second binding for the same peer replaces the buffer — the gate is
    /// the authority on the pairing (one peer flies at most one aircraft),
    /// and a peer bound to a new actor starts that actor's control state
    /// fresh rather than inheriting a predecessor's held axes. The prior
    /// aircraft's ownership goes with the move: it is released here, so a
    /// re-bound peer cannot keep the first aircraft bound to it forever.
    ///
    /// # Errors
    ///
    /// [`SessionViolation::ActorNotOwned`] when another peer owns the actor.
    /// A refused bind changes nothing: the prior binding still stands.
    pub fn bind_aircraft(&mut self, peer: PeerId, actor: ActorId) -> Result<(), SessionViolation> {
        self.session.gate_mut().ownership_mut().bind(peer, actor)?;
        if let Some(previous) = self
            .pilots
            .get(&peer)
            .map(|pilot| pilot.actor)
            .filter(|previous| *previous != actor)
            && self.session.gate().ownership().owner(previous) == Some(peer)
        {
            self.session.gate_mut().ownership_mut().release(previous);
        }
        self.pilots.insert(
            peer,
            RemotePilot {
                actor,
                buffer: ControlBuffer::new(),
            },
        );
        Ok(())
    }

    /// Releases the peer's binding and its control buffer, returning what the
    /// release discarded. `None` when the peer was bound nowhere.
    ///
    /// The buffer is released the way every control handover is: its held
    /// axes are neutralized and its queued edges dropped first, so a departed
    /// pilot leaves no stick deflected and no press half-delivered. Every
    /// ownership record the gate holds for the peer goes with it — the gate
    /// is a separate record that does not know the peer left, so a pilot
    /// released here that still owns an actor surrenders it rather than
    /// leaving a ghost no other peer may bind.
    pub fn unbind_aircraft(&mut self, peer: PeerId) -> Option<ReleasedPilot> {
        self.release_pilot(peer)
    }

    /// The control buffer `peer`'s admitted frames fold into.
    #[must_use]
    pub fn control(&self, peer: PeerId) -> Option<&ControlBuffer> {
        self.pilots.get(&peer).map(|pilot| &pilot.buffer)
    }

    /// The control buffer, mutably, for the authoritative consumer that calls
    /// `begin_tick` on the peer's aircraft.
    pub fn control_mut(&mut self, peer: PeerId) -> Option<&mut ControlBuffer> {
        self.pilots.get_mut(&peer).map(|pilot| &mut pilot.buffer)
    }

    /// The actor `peer` is bound to, when the drive bound it here.
    #[must_use]
    pub fn actor_of(&self, peer: PeerId) -> Option<ActorId> {
        self.pilots.get(&peer).map(|pilot| pilot.actor)
    }

    /// How many remote fire orders are waiting for their tick.
    #[must_use]
    pub fn pending_orders(&self) -> usize {
        self.orders.len()
    }

    /// Drains the pending remote orders for the committed tick `at`.
    ///
    /// Orders minted for `at` come back `matched`; orders whose intent tick
    /// already passed come back `stale` (named, not kept); orders for ticks
    /// still ahead stay pending for their own. This is the session driver's
    /// half of the wire-fire path: it folds `matched` into the
    /// [`crate::weapons::WeaponTickInput`] record it publishes for `at`, and
    /// reports `stale` the same way the weapon schedule reports a dropped
    /// input record.
    pub fn take_orders(&mut self, at: Tick) -> RemoteOrderBatch {
        let mut batch = RemoteOrderBatch::default();
        let mut pending = VecDeque::new();
        for order in self.orders.drain(..) {
            let WeaponOrder::Fire(intent) = &order else {
                pending.push_back(order);
                continue;
            };
            if intent.id.tick < at {
                batch.stale.push(order);
            } else if intent.id.tick == at {
                batch.matched.push(order);
            } else {
                pending.push_back(order);
            }
        }
        self.orders = pending;
        batch
    }

    /// Releases a departed peer's control state: axes neutralized, queued
    /// edges dropped, and every ownership record the gate still holds for the
    /// peer released with it — the gate is a separate record that does not
    /// know the peer left, so an owner that goes away keeps its aircraft as a
    /// ghost no other peer may bind unless the release happens here.
    /// `actor_of` only ever names *this* peer's records, so an actor another
    /// peer was moved onto is never stripped. Returns what the release
    /// discarded, or `None` when the peer was bound nowhere.
    fn release_pilot(&mut self, peer: PeerId) -> Option<ReleasedPilot> {
        let mut pilot = self.pilots.remove(&peer);
        let mut actor = pilot.as_ref().map(|pilot| pilot.actor);
        while let Some(owned) = self.session.gate().ownership().actor_of(peer) {
            self.session.gate_mut().ownership_mut().release(owned);
            actor = actor.or(Some(owned));
        }
        let actor = actor?;
        let (neutralized, dropped_edges) = pilot.as_mut().map_or((0, 0), |pilot| {
            (
                pilot.buffer.neutralize_axes().len(),
                pilot.buffer.drain_pending().len(),
            )
        });
        Some(ReleasedPilot {
            peer,
            actor,
            neutralized,
            dropped_edges,
        })
    }
}

/// Resource: the client side of a networked session.
///
/// Owns the `ClientSession` and the two parameters the mirror is built from
/// once the handshake grant names the session epoch: the live world-origin
/// frame and the declared interpolation bounds. A session driver submits the
/// local input through `session_mut().submit_sample/submit_edge`; the drive
/// pumps everything else.
#[derive(Resource)]
pub struct NetClient {
    session: ClientSession,
    origin: WorldOrigin,
    interpolation: InterpolationConfig,
    prediction: PredictionConfig,
}

impl NetClient {
    /// A client wrapping an already-connected `session`, reading snapshots in
    /// `origin`'s frame under `interpolation`'s declared bounds.
    #[must_use]
    pub fn new(
        session: ClientSession,
        origin: WorldOrigin,
        interpolation: InterpolationConfig,
    ) -> Self {
        Self {
            session,
            origin,
            interpolation,
            prediction: PredictionConfig::default(),
        }
    }

    /// Declares the prediction bounds `attach_local` installs when the wire
    /// names this client's actor. A builder because the bounds belong to the
    /// client that owns them, not to a session that was already attached.
    #[must_use]
    pub fn with_prediction(mut self, prediction: PredictionConfig) -> Self {
        self.prediction = prediction;
        self
    }

    /// The local predictor's declared bounds.
    #[must_use]
    pub const fn prediction(&self) -> PredictionConfig {
        self.prediction
    }

    /// The session object, for grant, phase and snapshot reads.
    #[must_use]
    pub const fn session(&self) -> &ClientSession {
        &self.session
    }

    /// The session object, for the input driver's submissions and the
    /// caller's own lifecycle (`disconnect`).
    pub fn session_mut(&mut self) -> &mut ClientSession {
        &mut self.session
    }

    /// The world-origin frame the mirror is built in.
    #[must_use]
    pub const fn origin(&self) -> WorldOrigin {
        self.origin
    }

    /// Moves the frame a *future* mirror is built in. An already-built
    /// [`NetSession`] migrates through its own `rebase`, the F57-C path for
    /// an epoch change — this setter does not pretend to be it.
    pub fn set_origin(&mut self, next: WorldOrigin) {
        self.origin = next;
    }

    /// The interpolation bounds a built mirror runs under.
    #[must_use]
    pub const fn interpolation(&self) -> InterpolationConfig {
        self.interpolation
    }
}

/// Resource: the F57-C client mirror, schedule-owned once it exists.
///
/// `cs_app::network::physics` stays deliberately free of Bevy so the typed
/// boundary carries no schedule, so this newtype — not a `Resource` impl on
/// [`NetSession`] itself — is what the world holds. It dereferences to the
/// session, so `mirror.ingest(...)` and friends read exactly as they do on
/// the typed object.
#[derive(Resource)]
pub struct NetMirror(pub NetSession);

impl std::ops::Deref for NetMirror {
    type Target = NetSession;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for NetMirror {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Resource: the session world's own [`crate::origin::FixedTickDriver`].
///
/// `FixedTickDriver` stays deliberately free of Bevy, so this newtype is what
/// the world holds; it dereferences, so `driver.advance_frame(...)` reads as
/// on the typed object. Install one when this world's frame is driven by the
/// real origin driver — [`drive_net_origin`] advances it one committed-tick
/// step per run and propagates the produced frame to every session owner.
/// A world without one simply keeps the origins its session driver set.
#[derive(Resource)]
pub struct NetOriginDriver(pub FixedTickDriver);

impl std::ops::Deref for NetOriginDriver {
    type Target = FixedTickDriver;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for NetOriginDriver {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Marker: the one entity whose `Position`/`Rotation` own the local
/// aircraft's pose for the networked session.
///
/// `NetSession::next_correction` *returns* a correction — the session itself
/// never mutates a body — so the scheduled [`correct_local_pose`] writes it
/// through the entity carrying this marker, keeping the one-pose-owner rule
/// a property of the schedule rather than a convention. The marker is a unit
/// on purpose: which *actor* is local is the wire's word (an `ActorSpawned`
/// naming this client), not the marker's — a body that claims an actor the
/// server did not grant is exactly the authority inversion the drive must
/// not perform. Marking two entities is refused by name as
/// [`NetPoseOwner::Ambiguous`]; an unposed or absent marker leaves the
/// correction unwritten and the report says which happened.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct NetPredictedBody;

/// Resource: the remote aircraft the interpolation buffers presented this
/// committed tick, keyed by actor.
///
/// Rebuilt every run by [`sample_net_mirror`] — an actor the buffers no
/// longer hold does not linger in presentation. The local actor is never in
/// it: a predicted aircraft's pose is its own body's, and blending the wire
/// copy into the presentation would draw the same plane twice.
#[derive(Resource, Clone, Debug, Default)]
pub struct NetPresentation {
    /// What `NetSession::sample` answered for each mirrored remote actor.
    pub aircraft: BTreeMap<ActorId, InterpolatedAircraft>,
}

/// The ordered stages of the scheduled session drive (F57-C.01).
///
/// Chained in `FixedPostUpdate` after [`PhysicsSystems::StepSimulation`], in
/// exactly this order — origin frame first (a rebase precedes decoding), the
/// lifecycle pump and snapshot ingest, remote sampling, the local
/// pose-owner's correction, tracer expiry last. The chain is the contract:
/// splitting these across unrelated system placements would let a new-epoch
/// snapshot be decoded in the old frame or a correction written before the
/// ingest that produced it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NetSessionSet {
    /// Advance the installed origin driver and adopt its frame.
    Origin,
    /// Pump the lifecycle sessions and ingest the snapshot mailbox.
    Drive,
    /// Sample remote actors at the estimated server tick.
    Sample,
    /// Reconcile the local prediction and write the correction through the
    /// one [`NetPredictedBody`].
    Correct,
    /// Expire client tracers whose presentation window elapsed.
    ExpireTracers,
}

/// What [`correct_local_pose`] found claiming the local pose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetPoseOwner {
    /// No [`NetPredictedBody`] marker exists — or no committed tick named
    /// this run — so nothing was written.
    #[default]
    Unbound,
    /// The one marked entity, whose `Position`/`Rotation` the session's
    /// correction owns.
    Bound(Entity),
    /// The marked entity carries no `Position`/`Rotation` to write through.
    Unposed(Entity),
    /// More than one marker claimed the pose. Writing through all of them
    /// would apply the same correction twice, so none was written — the
    /// count names the ambiguity instead of picking a winner.
    Ambiguous(usize),
}

/// What the [`NetSessionSet::Origin`] stage did this run.
///
/// This is the caller-visible record AC03 names: `rebased` carries the
/// mirror's own [`EpochTransition`] (the shift the frame made versus the
/// largest conversion any record moved), so a world-scale jump is
/// measurable rather than implied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetOriginStep {
    /// Whole fixed ticks the driver committed this run.
    pub advanced: u64,
    /// The mirror's epoch transitions, one per epoch the driver crossed —
    /// empty when no rebase happened or no mirror exists to adopt it.
    pub rebased: Vec<EpochTransition>,
    /// How many [`NetPredictedBody`] poses were converted through the frame
    /// change this run.
    pub shifted_poses: usize,
    /// The driver's refusal (`DriverError`), the mirror's `rebase` refusal or
    /// a pose conversion refusal — named, never retried invisibly.
    pub fault: Option<String>,
}

/// Why a tick the drive ran could not be served.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetDriveMissing {
    /// No [`PhysicsTickLedger`]: the host side cannot name the committed tick
    /// its observations and publications belong to. Input still drains and
    /// acknowledges — it is already gated — but the ledger's input book and
    /// the snapshot publish wait for a tick that can own them.
    Timeline,
    /// No `Time<Fixed>`: the transport is pumped with zero elapsed, which
    /// forwards arrived traffic but advances no timeouts. Named rather than
    /// guessed a step length.
    Clock,
}

/// What happened to one departed peer's control state.
///
/// `neutralized` and `dropped_edges` are honestly zero for a peer the gate
/// bound but no `ControlBuffer` ever served — the ownership release still
/// happened and `actor` still names the freed aircraft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleasedPilot {
    /// The peer that left.
    pub peer: PeerId,
    /// The aircraft it was bound to — the bound buffer's actor when one was
    /// open, otherwise a gate record the release found.
    pub actor: ActorId,
    /// How many held axes the release neutralized.
    pub neutralized: usize,
    /// How many queued edges the release dropped undelivered.
    pub dropped_edges: usize,
}

/// One admitted packet's fate inside the drain.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsumedInput {
    /// The peer it came from.
    pub peer: PeerId,
    /// The admitted sequence.
    pub sequence: u32,
    /// The ledger's input book refused the sequence (`StaleInput`); the
    /// transport's window is the courtesy and this is the rule. `None` when
    /// the book observed it — or when no committed tick existed to record it
    /// under (`missing` says which happened).
    pub book_refusal: Option<NetStateError>,
    /// The wire acknowledgment's outcome, sent unconditionally: the ack is
    /// cumulative "consumed through", so a packet the book already counted is
    /// still acknowledged. `Err` carries the session's own send fault.
    pub acknowledged: Result<(), String>,
    /// Frames folded into the peer's control buffer.
    pub frames_applied: usize,
    /// Per-frame refusals from `ControlBuffer::apply_frame`, with the frame
    /// tick that failed.
    pub frames_refused: Vec<(Tick, ControlError)>,
    /// The peer held no bound aircraft, so its frames had no buffer to fold
    /// into — counted rather than silently discarded.
    pub unbound: bool,
    /// How many fire intents this packet minted.
    pub intents: usize,
}

/// What one tick's snapshot publication did.
#[derive(Clone, Debug, PartialEq)]
pub enum NetPublish {
    /// The snapshot was built and put on the wire for the committed tick.
    Sent {
        /// The committed tick the frame carries.
        at: Tick,
        /// How many actor records it carried.
        records: usize,
    },
    /// The ledger's state could not be expressed as a snapshot.
    Refused(PublishError),
    /// The snapshot built but the session refused the send (a spent epoch,
    /// an encode or transport fault).
    Faulted(String),
    /// The session phase does not publish (not `Live`), or no committed tick
    /// exists to name the frame: named so a quiet wire is distinguishable
    /// from a failed one.
    Skipped(&'static str),
}

/// What the drive released when a session resource went away.
///
/// Only facts still observable are recorded: a dropped `NetHost`'s queue
/// depth died with it, so nothing here counts what was lost — the record
/// names the dead generation and whatever the teardown call itself reported.
#[derive(Clone, Debug, PartialEq)]
pub struct NetTeardown {
    /// Which side tore down.
    pub side: &'static str,
    /// The session generation that went away. `None` for a client that never
    /// held a grant.
    pub session: Option<SessionId>,
    /// What the client mirror's `teardown` released, when one was torn down.
    pub mirror: Option<usize>,
}

/// Resource: what the scheduled session drive did, one fixed tick at a time.
///
/// Every field answers for the run `at` names. `host_notices` and
/// `client_notices` are the `cs_net` notices forwarded verbatim — the "own
/// state" the lifecycle's handoff names — bounded at
/// [`MAX_REPORTED_NOTICES`] with the overflow counted.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct NetSessionReport {
    /// Fixed ticks the drive has run, including this one.
    pub ticks: u64,
    /// The installed host's session, when one is.
    pub host_session: Option<SessionId>,
    /// The client session's granted epoch, once the handshake produced one.
    pub client_session: Option<SessionId>,
    /// The committed tick this run observed; `None` without a
    /// [`PhysicsTickLedger`].
    pub at: Option<Tick>,
    /// The authorities the run needed but the world did not have.
    pub missing: Vec<NetDriveMissing>,
    /// The host pump's notices, bounded.
    pub host_notices: Vec<ServerNotice>,
    /// Notices the bound would not hold.
    pub host_notices_overflowed: u64,
    /// The client pump's notices, bounded.
    pub client_notices: Vec<ClientNotice>,
    /// Notices the bound would not hold.
    pub client_notices_overflowed: u64,
    /// What the host drain did with each admitted packet, in drain order.
    pub consumed: Vec<ConsumedInput>,
    /// Peers whose control buffers a departure released this run.
    pub released_pilots: Vec<ReleasedPilot>,
    /// Remote orders pending after the run (queue depth, surfaced so a driver
    /// that never drains is visible).
    pub pending_orders: usize,
    /// The committed tick's snapshot publication, when the session attempted
    /// one. `Skipped` names the phase or authority that published nothing.
    pub published: Option<NetPublish>,
    /// The mirror the client feed ingested into this run.
    pub mirror_session: Option<SessionId>,
    /// The newest snapshot's ingest report, when one was fed.
    pub ingested: Option<IngestReport>,
    /// Mirror calls that failed this run, each with the session's own words.
    ///
    /// `SessionError`s from `attach_local`, `record_local_pose`,
    /// `reconcile_local` and `expire_tracers`, plus the pose conversions the
    /// correction stage performed for the mirror, all land here — typed
    /// refusals that have a place on the report's other fields stay there
    /// (`ingested`, `sample_refusals`, `published`, `origin`); this vec is
    /// where everything else must still reach a caller.
    pub mirror_refusals: Vec<String>,
    /// Reliable event bodies whose application recorded mirror state this
    /// run — today that is the `ActorRemoved` retirement fact.
    pub events_recorded: usize,
    /// Session resources that went away since the last run.
    pub tore_down: Vec<NetTeardown>,
    /// What the origin stage did this run, when a [`NetOriginDriver`] is
    /// installed. Written through `NetDriveState` because the report's
    /// per-tick fields reset in `Drive`, which the chained set runs *after*
    /// `Origin`.
    pub origin: Option<NetOriginStep>,
    /// Remote actors the sample stage presented this run.
    pub sampled: usize,
    /// Per-actor `BufferRefusal`s the sampler hit this run — the buffer's own
    /// words, with the actor each belongs to.
    pub sample_refusals: Vec<(ActorId, BufferRefusal)>,
    /// The actor the session's `ActorSpawned` events last named this client
    /// the owner of — the wire's word for "this is your aircraft".
    pub local_actor: Option<ActorId>,
    /// The local actor `attach_local` bound this run, when it did.
    pub attached: Option<ActorId>,
    /// What claimed the local pose this run (`correct_local_pose`'s verdict
    /// on the [`NetPredictedBody`] marker).
    pub pose_owner: NetPoseOwner,
    /// The reconciliation the fresh local record produced this run, when the
    /// run reconciled.
    pub reconciled: Option<Reconciliation>,
    /// The correction written through the pose owner this run, when a
    /// non-identity one was pending.
    pub corrected: Option<PoseCorrection>,
    /// Set when the pose owner *adopted* the authoritative pose outright this
    /// run — a reconcile that had no prediction to compare is reported
    /// `Snapped` with an unbounded error, and adopting the record's pose is
    /// the honest enactment of that verdict.
    pub adopted_pose: bool,
    /// Client tracers whose presentation window expired this run.
    pub tracers_expired: usize,
}

impl NetSessionReport {
    /// Opens the report for a new run: every per-tick field clears, the run
    /// count advances.
    fn begin_tick(&mut self) {
        self.ticks += 1;
        self.host_session = None;
        self.client_session = None;
        self.at = None;
        self.missing.clear();
        self.host_notices.clear();
        self.host_notices_overflowed = 0;
        self.client_notices.clear();
        self.client_notices_overflowed = 0;
        self.consumed.clear();
        self.released_pilots.clear();
        self.pending_orders = 0;
        self.published = None;
        self.mirror_session = None;
        self.ingested = None;
        self.mirror_refusals.clear();
        self.events_recorded = 0;
        self.tore_down.clear();
        self.origin = None;
        self.sampled = 0;
        self.sample_refusals.clear();
        self.local_actor = None;
        self.attached = None;
        self.pose_owner = NetPoseOwner::Unbound;
        self.reconciled = None;
        self.corrected = None;
        self.adopted_pose = false;
        self.tracers_expired = 0;
    }
}

/// Resource: the drive's own bookkeeping across ticks.
///
/// `host`/`client` are the session identities the drive last served — what it
/// tears down against. `fed` is the newest `(session, snapshot tick)` pair
/// handed to the mirror, so the mailbox slot is never fed twice and a mirror
/// built for another generation is never mistaken for current.
/// `local_actor` is the wire's word for which actor is this client's own
/// (learned from `ActorSpawned`, reset with the mirror); `attach_refused`
/// keeps an `attach_local` refusal from being re-issued every run — the
/// refusal is reported once and the persistent divergence stays readable in
/// `local_actor` vs `attached`. `origin_step`/`origin_teardown` are the
/// `Origin` stage's record, held because the report's per-tick fields reset
/// in `Drive`, which the chained set runs after `Origin`.
#[derive(Resource, Clone, Debug, Default)]
struct NetDriveState {
    host: Option<SessionId>,
    client: Option<SessionId>,
    client_present: bool,
    fed: Option<(SessionId, Tick)>,
    local_actor: Option<ActorId>,
    attach_refused: Option<ActorId>,
    origin_step: Option<NetOriginStep>,
    origin_teardown: Option<NetTeardown>,
}

/// Forwards notices into the bounded report log, counting what would not fit.
fn log_notices<T>(log: &mut Vec<T>, overflowed: &mut u64, notices: Vec<T>) {
    for notice in notices {
        if log.len() < MAX_REPORTED_NOTICES {
            log.push(notice);
        } else {
            *overflowed = overflowed.saturating_add(1);
        }
    }
}

/// Releases the installed mirror when it serves `session`, by name, and
/// clears the presentation it produced plus the per-session bookkeeping the
/// drive holds for it (`fed`, `local_actor`, `attach_refused`), so whatever
/// is built next starts clean. Returns the [`NetTeardown`] for the caller to
/// record, or `None` when no installed mirror serves `session` — a mirror
/// for another generation was never this session's and is left alone.
fn teardown_client_mirror(
    world: &mut World,
    state: &mut NetDriveState,
    session: SessionId,
) -> Option<NetTeardown> {
    let serving = world
        .get_resource::<NetMirror>()
        .is_some_and(|mirror| mirror.session() == session);
    if !serving {
        return None;
    }
    let released = world
        .remove_resource::<NetMirror>()
        .map(|mut mirror| mirror.teardown());
    if let Some(mut presentation) = world.get_resource_mut::<NetPresentation>() {
        presentation.aircraft.clear();
    }
    state.fed = None;
    state.local_actor = None;
    state.attach_refused = None;
    Some(NetTeardown {
        side: "client",
        session: Some(session),
        mirror: released,
    })
}

/// Consumes one admitted packet: the ledger's input book, the wire
/// acknowledgment, the control frames, then the fire intents — in that
/// order, so a refusal early still names itself before anything after it
/// ran.
fn consume_input(host: &mut NetHost, input: &PeerInput, at: Option<Tick>) -> ConsumedInput {
    let mut consumed = ConsumedInput {
        peer: input.peer,
        sequence: input.sequence,
        book_refusal: None,
        acknowledged: Ok(()),
        frames_applied: 0,
        frames_refused: Vec::new(),
        unbound: false,
        intents: 0,
    };

    // The authoritative input book: a sequence it refuses as replayed is
    // named here even though the transport's window already absorbed it —
    // the window is the courtesy and this is the rule.
    if let Some(tick) = at {
        consumed.book_refusal = host.ledger.observe_input(input.sequence, tick).err();
    }
    consumed.acknowledged = host
        .session
        .acknowledge_input(input.peer, input.sequence)
        .map(|_| ())
        .map_err(|fault| fault.to_string());

    match host.pilots.get_mut(&input.peer) {
        Some(pilot) => {
            for frame in &input.frames.frames {
                match pilot.buffer.apply_frame(frame) {
                    Ok(()) => consumed.frames_applied += 1,
                    Err(refusal) => consumed.frames_refused.push((frame.frame_tick(), refusal)),
                }
            }
        }
        None => consumed.unbound = true,
    }

    for request in &input.fires {
        host.orders
            .push_back(WeaponOrder::Fire(fire_intent(request)));
        consumed.intents += 1;
    }
    consumed
}

/// Drives one committed fixed tick of the network sessions: teardown
/// detection first, then the host pump/drain/publish, then the client
/// pump/feed — in that order — recorded in [`NetSessionReport`].
///
/// Exclusive over the world because the session owners are resources and the
/// mirror may be constructed on the fly; running it anywhere else would be a
/// second schedule for the same sessions. Directly callable, and the same
/// code [`NetSessionDrivePlugin`] installs, so a test drives exactly what a
/// session runs.
pub fn drive_net_session(world: &mut World) {
    let mut report = world
        .remove_resource::<NetSessionReport>()
        .unwrap_or_default();
    report.begin_tick();
    let mut state = world.remove_resource::<NetDriveState>().unwrap_or_default();

    // The Origin stage ran ahead of this one in the chained set and wrote
    // through the state, because the report's per-tick fields just reset.
    if let Some(teardown) = state.origin_teardown.take() {
        report.tore_down.push(teardown);
    }
    report.origin = state.origin_step.take();

    let elapsed = world.get_resource::<Time<Fixed>>().map(|time| time.delta());
    if elapsed.is_none() {
        report.missing.push(NetDriveMissing::Clock);
    }
    let elapsed = elapsed.unwrap_or_default();
    let at = world
        .get_resource::<PhysicsTickLedger>()
        .map(|ledger| Tick(ledger.ticks));
    report.at = at;
    if at.is_none() {
        report.missing.push(NetDriveMissing::Timeline);
    }

    // --- teardown detection --------------------------------------------------
    //
    // A session resource removed or replaced since the last run is a dead
    // generation: its drop already released the socket, so what this records
    // is what it was still holding — a mirror nobody fed further.
    let installed_host = world
        .get_resource::<NetHost>()
        .map(|host| host.session.session());
    report.host_session = installed_host;
    if state.host != installed_host {
        if let Some(previous) = state.host.take() {
            report.tore_down.push(NetTeardown {
                side: "host",
                session: Some(previous),
                mirror: None,
            });
        }
        state.host = installed_host;
    }
    let client_present = world.contains_resource::<NetClient>();
    let installed_client = world
        .get_resource::<NetClient>()
        .and_then(|client| client.session.grant().map(|grant| grant.session));
    report.client_session = installed_client;
    if (!client_present && state.client_present)
        || (installed_client.is_some() && state.client != installed_client)
    {
        // The client that was here is gone — removed, or replaced by one the
        // grant names differently. A mirror still serving its session has no
        // feed left; release it by name. A mirror for another generation was
        // never this client's and is left alone.
        if let Some(last) = state.client
            && let Some(teardown) = teardown_client_mirror(world, &mut state, last)
        {
            report.tore_down.push(teardown);
            state.client = None;
        }
    }
    state.client_present = client_present;

    // --- host ----------------------------------------------------------------
    if let Some(mut host) = world.remove_resource::<NetHost>() {
        let notices = host.session.pump(elapsed);
        // A departed peer's buffer releases with the notice that carried it,
        // in notice order, so a buffer never outlives the membership fact
        // that ended it.
        for notice in &notices {
            let departed = match notice {
                ServerNotice::PeerLeft { peer } => Some(*peer),
                ServerNotice::CutOff { peer, .. } | ServerNotice::HungUp { peer, .. } => *peer,
                _ => None,
            };
            if let Some(peer) = departed
                && let Some(released) = host.release_pilot(peer)
            {
                report.released_pilots.push(released);
            }
        }
        log_notices(
            &mut report.host_notices,
            &mut report.host_notices_overflowed,
            notices,
        );

        for input in host.session.drain_work() {
            report.consumed.push(consume_input(&mut host, &input, at));
        }
        report.pending_orders = host.pending_orders();

        // The committed tick's snapshot exists only while the match is live:
        // Gathering has no simulation to describe and a spent epoch cannot
        // send. `publish_snapshot` is attempted nowhere else, so its refusal
        // classes stay named rather than colliding with a skipped phase.
        report.published = Some(match host.session.phase() {
            ServerPhase::Live { .. } => match at {
                None => NetPublish::Skipped("no committed tick"),
                Some(tick) => match publish_snapshot(&host.ledger, &host.origin) {
                    Ok(snapshot) => {
                        let records = snapshot.actors.len();
                        match host.session.publish_snapshot(tick, &snapshot) {
                            Ok(()) => NetPublish::Sent { at: tick, records },
                            Err(fault) => NetPublish::Faulted(fault.to_string()),
                        }
                    }
                    Err(reason) => NetPublish::Refused(reason),
                },
            },
            phase => NetPublish::Skipped(phase.label()),
        });

        world.insert_resource(host);
    }

    // --- client --------------------------------------------------------------
    if let Some(mut client) = world.remove_resource::<NetClient>() {
        let notices = client.session.pump(elapsed);
        // Reliable event bodies apply to the mirror in notice order; the
        // snapshot mailbox is handed over once, after them.
        let events: Vec<ReliableEvent> = notices
            .iter()
            .filter_map(|notice| match notice {
                ClientNotice::Event { id, body } => Some(ReliableEvent {
                    id: *id,
                    body: body.clone(),
                }),
                _ => None,
            })
            .collect();
        log_notices(
            &mut report.client_notices,
            &mut report.client_notices_overflowed,
            notices,
        );

        // A closed session's mirror is a ghost feed: `Closed` is terminal,
        // so the mailbox can never move again and nothing may still be
        // presented from it. Released by name, once — the spent epoch's
        // grant still answers `grant()`, so the same session keeps serving
        // as the thing being torn down.
        if matches!(client.session.phase(), ClientPhase::Closed(_)) {
            let dead = client
                .session
                .grant()
                .map(|grant| grant.session)
                .or(state.client);
            if let Some(session) = dead
                && let Some(teardown) = teardown_client_mirror(world, &mut state, session)
            {
                report.tore_down.push(teardown);
            }
        }

        if let Some(grant) = client.session.grant()
            && !matches!(client.session.phase(), ClientPhase::Closed(_))
        {
            state.client = Some(grant.session);
            report.client_session = Some(grant.session);
            // The mirror serves exactly the granted epoch: an installed one
            // for another generation is torn down and replaced rather than
            // fed a foreign epoch's traffic.
            let current = world
                .get_resource::<NetMirror>()
                .map(|mirror| mirror.session());
            if current != Some(grant.session) {
                if let Some(old) = current
                    && let Some(teardown) = teardown_client_mirror(world, &mut state, old)
                {
                    report.tore_down.push(teardown);
                }
                world.insert_resource(NetMirror(NetSession::new(
                    grant.session,
                    client.origin,
                    client.interpolation,
                )));
                state.fed = None;
            }
            report.mirror_session = Some(grant.session);

            if let Some(mut mirror) = world.get_resource_mut::<NetMirror>() {
                for event in &events {
                    // A spawn event that names this peer the actor's owner
                    // is the server's word for "this aircraft is yours" —
                    // the fact the Correct stage attaches against. Applying
                    // the event itself still only moves the mirror on a
                    // removal; a respawn re-names the actor and the failed
                    // attach retry is released with it.
                    if let EventBody::ActorSpawned {
                        actor,
                        owner: Some(owner),
                        ..
                    } = &event.body
                        && *owner == grant.peer
                        && state.local_actor != Some(*actor)
                    {
                        state.local_actor = Some(*actor);
                        state.attach_refused = None;
                    }
                    match mirror.apply_event(event) {
                        Ok(applied) => report.events_recorded += usize::from(applied),
                        Err(refusal) => report.mirror_refusals.push(refusal.to_string()),
                    }
                }
                if let (Some(tick), Some(snapshot)) =
                    (client.session.snapshot_tick(), client.session.snapshot())
                {
                    // The mailbox holds only the newest snapshot; feeding it
                    // again would replay one frame as if it had arrived twice.
                    if state.fed != Some((grant.session, tick)) {
                        match mirror.ingest(snapshot, tick) {
                            Ok(ingest) => report.ingested = Some(ingest),
                            Err(refusal) => {
                                report.mirror_refusals.push(refusal.to_string());
                            }
                        }
                        state.fed = Some((grant.session, tick));
                    }
                }
            }
        }

        world.insert_resource(client);
    }

    world.insert_resource(report);
    world.insert_resource(state);
}

/// The origin stage of the scheduled drive (F57-C.01): once per committed
/// tick, before anything reads a frame, the installed [`NetOriginDriver`]
/// advances one frame of wall time into whole fixed ticks and every session
/// owner adopts the frame it produced — `NetHost`'s publish frame,
/// `NetClient`'s next-mirror frame, and the live mirror through
/// `NetSession::rebase`, one call per epoch the driver crossed and each
/// recorded in [`NetOriginStep::rebased`]. The [`NetPredictedBody`] pose
/// owner's `Position` converts through the same frame change: a rebase is a
/// coordinate-frame conversion, so the body's canonical world position is
/// identical before and after — never a displacement of `origin_shift_m`.
///
/// A mirror already *ahead* of the driver's epoch can never go backwards —
/// epochs are monotone — so a driver restart under a fresh frame (the
/// driver's own `retry`) releases that mirror by name rather than letting it
/// refuse every later snapshot as a foreign epoch.
///
/// The stage's record lands in `NetDriveState::origin_step`, because
/// `NetSessionReport`'s per-tick fields reset in `Drive` — which the chained
/// set runs after this. Directly callable, and the same code
/// [`NetSessionDrivePlugin`] installs.
pub fn drive_net_origin(world: &mut World) {
    let Some(mut driver) = world.remove_resource::<NetOriginDriver>() else {
        return;
    };
    let mut state = world.remove_resource::<NetDriveState>().unwrap_or_default();
    let elapsed = world
        .get_resource::<Time<Fixed>>()
        .map(|time| time.delta())
        .unwrap_or_default();

    let mut step = NetOriginStep {
        advanced: 0,
        rebased: Vec::new(),
        shifted_poses: 0,
        fault: None,
    };
    let before = driver.world().origin();
    match driver.advance_frame(elapsed) {
        Ok(ticks) => step.advanced = ticks,
        Err(fault) => step.fault = Some(fault.to_string()),
    }
    let origin = driver.world().origin();
    if let Some(mut host) = world.get_resource_mut::<NetHost>() {
        host.set_origin(origin);
    }
    if let Some(mut client) = world.get_resource_mut::<NetClient>() {
        client.set_origin(origin);
    }

    let mirror_frame = world
        .get_resource::<NetMirror>()
        .map(|mirror| (mirror.session(), mirror.origin().epoch()));
    if let Some((session, epoch)) = mirror_frame {
        if epoch > origin.epoch() {
            // The driver's frame restarted below the mirror's — a retry's
            // fresh world. The mirror can never be fed again (every later
            // snapshot reads as a foreign epoch), so it is released by name.
            if let Some(teardown) = teardown_client_mirror(world, &mut state, session) {
                state.origin_teardown = Some(teardown);
            }
        } else if let Some(mut mirror) = world.get_resource_mut::<NetMirror>() {
            while mirror.origin().epoch() < origin.epoch() {
                match mirror.rebase(origin.position()) {
                    Ok(transition) => step.rebased.push(transition),
                    Err(refusal) => {
                        step.fault = Some(refusal.to_string());
                        break;
                    }
                }
            }
        }
    }

    if origin.epoch() != before.epoch() {
        // The session's pose owner holds a local f32 pose: the frame moved
        // under it, so its coordinates convert through the frame change the
        // same way the mirror's records did — a conversion, so a body the
        // session drives never reads `origin_shift_m` off its own pose.
        let owners: Vec<Entity> = world
            .query_filtered::<Entity, With<NetPredictedBody>>()
            .iter(world)
            .collect();
        for entity in owners {
            let Some(held) = world.get::<Position>(entity).map(|p| p.0) else {
                continue;
            };
            let converted = LocalPosition::try_new(held.to_array())
                .map_err(crate::origin::OriginError::from)
                .and_then(|local| before.world_of(local))
                .and_then(|at| origin.local_of(at));
            match converted {
                Ok(local) => {
                    if let Some(mut position) = world.get_mut::<Position>(entity) {
                        position.0 = Vec3::from_array(local.to_array());
                    }
                    step.shifted_poses += 1;
                }
                Err(fault) => step.fault = Some(fault.to_string()),
            }
        }
    }

    state.origin_step = Some(step);
    world.insert_resource(state);
    world.insert_resource(driver);
}

/// The sample stage of the scheduled drive: once per committed tick, after
/// `Drive`, every mirrored remote actor is sampled at the committed tick —
/// the schedule's estimate of the server tick, which the interpolator's
/// declared `delay_ticks` runs behind — into [`NetPresentation`]. The
/// presentation is rebuilt whole each run, so an actor the buffers dropped
/// or forgot does not linger. The local actor is skipped on purpose: a
/// predicted aircraft's pose is its own body's, and drawing the interpolated
/// wire copy would present the same plane twice. Directly callable, and the
/// same code [`NetSessionDrivePlugin`] installs.
pub fn sample_net_mirror(world: &mut World) {
    let mut report = world
        .remove_resource::<NetSessionReport>()
        .unwrap_or_default();
    let mut presentation = world
        .remove_resource::<NetPresentation>()
        .unwrap_or_default();
    presentation.aircraft.clear();

    let at = world
        .get_resource::<PhysicsTickLedger>()
        .map(|ledger| Tick(ledger.ticks));
    if let (Some(at), Some(mirror)) = (at, world.get_resource::<NetMirror>()) {
        let local = mirror.local_actor();
        for actor in mirror
            .mirror()
            .all_aircraft()
            .map(|aircraft| aircraft.actor)
            .collect::<Vec<_>>()
        {
            if Some(actor) == local {
                continue;
            }
            match mirror.sample(actor, at) {
                Ok(Some(sampled)) => {
                    presentation.aircraft.insert(actor, sampled);
                }
                Ok(None) => {}
                Err(refusal) => report.sample_refusals.push((actor, refusal)),
            }
        }
    }
    report.sampled = presentation.aircraft.len();
    world.insert_resource(presentation);
    world.insert_resource(report);
}

/// The correction stage of the scheduled drive: once per committed tick,
/// after `Sample`, the pose the one [`NetPredictedBody`] entity owns is
/// recorded into the session's predictor at the committed tick, the fresh
/// local record this run's ingest produced is reconciled, and
/// `NetSession::next_correction` is written through that same entity —
/// `Position` translated, `Rotation` pre-multiplied by the correction step.
///
/// The session never mutates a body itself, so this system is the only place
/// a session-produced pose reaches the ECS: the one-pose-owner rule is a
/// property of the schedule, not a convention. A reconcile that had no
/// prediction to compare is reported `Snapped` with an unbounded error; the
/// honest enactment of that verdict is the pose owner adopting the record's
/// pose outright (`adopted_pose` records that it happened). Every session or
/// conversion refusal lands in `mirror_refusals`, and an `attach_local`
/// refusal is issued once per wire-named actor rather than re-issued every
/// run — the divergence stays readable in `local_actor` versus `attached`.
/// Directly callable, and the same code [`NetSessionDrivePlugin`] installs.
pub fn correct_local_pose(world: &mut World) {
    let mut report = world
        .remove_resource::<NetSessionReport>()
        .unwrap_or_default();
    let mut state = world.remove_resource::<NetDriveState>().unwrap_or_default();
    let mut mirror = world.remove_resource::<NetMirror>();

    // The pose owner is declared, not searched: the marker names the entity
    // and the count names the claim, so two markers are an ambiguity the
    // report says, rather than a winner the drive picked.
    let owners: Vec<Entity> = world
        .query_filtered::<Entity, With<NetPredictedBody>>()
        .iter(world)
        .collect();
    report.pose_owner = match owners.as_slice() {
        [] => NetPoseOwner::Unbound,
        [entity] => {
            if world.get::<Position>(*entity).is_some() && world.get::<Rotation>(*entity).is_some()
            {
                NetPoseOwner::Bound(*entity)
            } else {
                NetPoseOwner::Unposed(*entity)
            }
        }
        many => NetPoseOwner::Ambiguous(many.len()),
    };
    report.local_actor = state.local_actor;

    if let Some(mirror) = mirror.as_mut() {
        // Attach the wire-named actor: the mirror must already hold the
        // server's record of it, and a refusal stands once per named actor
        // rather than being re-attempted — and re-reported — every run.
        if let Some(actor) = state.local_actor
            && mirror.local_actor() != Some(actor)
            && state.attach_refused != Some(actor)
            && let Some(record) = mirror.mirror().aircraft(actor).copied()
        {
            let prediction = world
                .get_resource::<NetClient>()
                .map(NetClient::prediction)
                .unwrap_or_default();
            match mirror.attach_local(actor, record.generation, prediction) {
                Ok(()) => report.attached = Some(actor),
                Err(refusal) => {
                    state.attach_refused = Some(actor);
                    report.mirror_refusals.push(refusal.to_string());
                }
            }
        }

        let at = world
            .get_resource::<PhysicsTickLedger>()
            .map(|ledger| Tick(ledger.ticks));
        if let (NetPoseOwner::Bound(entity), Some(tick)) = (report.pose_owner, at)
            && mirror.local_actor().is_some()
        {
            // The recorded pose is canonical world coordinates: the body's
            // local pose re-anchored through the session's live frame.
            let held_position = world.get::<Position>(entity).map(|p| p.0);
            let held_rotation = world.get::<Rotation>(entity).map(|r| r.0);
            let boosting = world
                .get::<FlightAircraft>(entity)
                .is_some_and(|aircraft| aircraft.command().boost);
            let pose = held_position.and_then(|position| {
                held_rotation.map(|rotation| {
                    let position = LocalPosition::try_new(position.to_array())
                        .map_err(crate::origin::OriginError::from)
                        .and_then(|local| mirror.origin().world_of(local));
                    let orientation = Quaternion::try_new([
                        f64::from(rotation.x),
                        f64::from(rotation.y),
                        f64::from(rotation.z),
                        f64::from(rotation.w),
                    ])
                    .map_err(crate::origin::OriginError::from);
                    position.and_then(|position| {
                        orientation.map(|orientation| PredictedPose {
                            position,
                            orientation,
                        })
                    })
                })
            });
            match pose {
                None => {}
                Some(Err(fault)) => report.mirror_refusals.push(fault.to_string()),
                Some(Ok(pose)) => {
                    if let Err(refusal) = mirror.record_local_pose(tick, pose, boosting) {
                        report.mirror_refusals.push(refusal.to_string());
                    }
                    // Reconciliation follows ingestion: only when this run's
                    // feed actually moved the local record is there a new
                    // authoritative word to compare against — every other run
                    // leaves the pending correction advancing on its own.
                    let fresh = mirror.local_actor().is_some_and(|actor| {
                        report.ingested.as_ref().is_some_and(|ingest| {
                            ingest.spawned.contains(&actor) || ingest.updated.contains(&actor)
                        })
                    });
                    if fresh {
                        match mirror.reconcile_local() {
                            Ok(reconciliation) => {
                                report.reconciled = Some(reconciliation);
                                if reconciliation.kind == ReconcileKind::Snapped
                                    && reconciliation.error_m.is_infinite()
                                    && let Some(record) = mirror
                                        .local_actor()
                                        .and_then(|actor| mirror.mirror().aircraft(actor))
                                        .copied()
                                    && let Ok(local) = mirror
                                        .origin()
                                        .local_of(record.position)
                                        .map(|local| local.to_array())
                                {
                                    // No predicted pose existed to compare:
                                    // the predictor reported the snap but
                                    // writes nothing, so the pose owner
                                    // adopts the record's pose here.
                                    let [x, y, z, w] = record.orientation.components();
                                    if let Some(mut position) = world.get_mut::<Position>(entity) {
                                        position.0 = Vec3::from_array(local);
                                    }
                                    if let Some(mut rotation) = world.get_mut::<Rotation>(entity) {
                                        rotation.0 =
                                            Quat::from_xyzw(x as f32, y as f32, z as f32, w as f32);
                                    }
                                    report.adopted_pose = true;
                                }
                            }
                            Err(refusal) => {
                                report.mirror_refusals.push(refusal.to_string());
                            }
                        }
                    }
                    let correction = mirror.next_correction();
                    let is_identity = correction.translation_m.iter().all(|c| *c == 0.0)
                        && correction.rotation == Quaternion::IDENTITY;
                    if !is_identity {
                        let [dx, dy, dz] = correction.translation_m;
                        let [x, y, z, w] = correction.rotation.components();
                        let step = Quat::from_xyzw(x as f32, y as f32, z as f32, w as f32);
                        if let Some(mut position) = world.get_mut::<Position>(entity) {
                            position.0 += Vec3::new(dx as f32, dy as f32, dz as f32);
                        }
                        if let Some(mut rotation) = world.get_mut::<Rotation>(entity) {
                            rotation.0 = step * rotation.0;
                        }
                        report.corrected = Some(correction);
                    }
                }
            }
        }
    }

    if let Some(mirror) = mirror {
        world.insert_resource(mirror);
    }
    world.insert_resource(state);
    world.insert_resource(report);
}

/// The last stage of the scheduled drive: once per committed tick, after the
/// correction write, client tracers whose presentation window elapsed are
/// expired — the bound that keeps a shot nobody ever answered from pinning a
/// tracer forever. Expiring last means one tick's tracers are observable for
/// the whole tick they were spawned in. Directly callable, and the same code
/// [`NetSessionDrivePlugin`] installs.
pub fn expire_net_tracers(world: &mut World) {
    let mut report = world
        .remove_resource::<NetSessionReport>()
        .unwrap_or_default();
    let at = world
        .get_resource::<PhysicsTickLedger>()
        .map(|ledger| Tick(ledger.ticks));
    if let Some(at) = at
        && let Some(mut mirror) = world.get_resource_mut::<NetMirror>()
    {
        match mirror.expire_tracers(at) {
            Ok(expired) => report.tracers_expired = expired,
            Err(refusal) => report.mirror_refusals.push(refusal.to_string()),
        }
    }
    world.insert_resource(report);
}

/// Installs the scheduled session drive in `FixedPostUpdate`, after the
/// physics step.
///
/// Ordered after [`PhysicsSystems::StepSimulation`] for the same reason the
/// weapon schedule is: the pump reads the committed tick the step just made
/// and the snapshot publishes the state it left. The five
/// [`NetSessionSet`]s are chained — origin frame, lifecycle drive, remote
/// sampling, pose-owner correction, tracer expiry — so a rebase always lands
/// before the tick's ingest and the ingest before the correction it feeds.
/// A world that installs this plugin with no session resources runs the
/// drive every fixed tick and serves nothing — the honest state of a world
/// with no session, recorded in [`NetSessionReport`].
#[derive(Clone, Copy, Debug, Default)]
pub struct NetSessionDrivePlugin;

impl Plugin for NetSessionDrivePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetSessionReport>()
            .init_resource::<NetDriveState>()
            .init_resource::<NetPresentation>()
            .configure_sets(
                FixedPostUpdate,
                (
                    NetSessionSet::Origin,
                    NetSessionSet::Drive,
                    NetSessionSet::Sample,
                    NetSessionSet::Correct,
                    NetSessionSet::ExpireTracers,
                )
                    .chain()
                    .after(PhysicsSystems::StepSimulation),
            )
            .add_systems(
                FixedPostUpdate,
                (
                    drive_net_origin.in_set(NetSessionSet::Origin),
                    drive_net_session.in_set(NetSessionSet::Drive),
                    sample_net_mirror.in_set(NetSessionSet::Sample),
                    correct_local_pose.in_set(NetSessionSet::Correct),
                    expire_net_tracers.in_set(NetSessionSet::ExpireTracers),
                ),
            );
    }
}
