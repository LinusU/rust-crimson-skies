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
//! * Driving `NetSession` further — sampling interpolated poses, reconciling
//!   local prediction, tracer expiry — is F57-C.01 (Rally #657). This drive
//!   feeds it; it does not render from it.
//!
//! All of this is newly authored engine design: no original networked
//! behavior has been measured, and none is asserted here.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use avian3d::prelude::PhysicsSystems;
use bevy::app::{App, Plugin};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::FixedPostUpdate;
use bevy::prelude::World;
use bevy::time::{Fixed, Time};

use cs_net::lifecycle::{
    ClientNotice, ClientSession, PeerInput, ServerNotice, ServerPhase, ServerSession,
};
use cs_net::message::ReliableEvent;
use cs_net::validation::SessionViolation;
use cs_sim::control::{ControlBuffer, ControlError};
use cs_sim::net_state::{NetStateError, NetStateLedger};
use cs_types::Tick;
use cs_types::net::{ActorId, PeerId, SessionId};

use crate::origin::WorldOrigin;
use crate::physics::PhysicsTickLedger;
use crate::weapons::WeaponOrder;

use super::physics::{
    IngestReport, InterpolationConfig, NetSession, PublishError, publish_snapshot,
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

/// Why a [`NetHost`] could not be assembled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

impl fmt::Display for NetHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { session, ledger } => write!(
                f,
                "ledger serves session {ledger}, but the session object is {session}"
            ),
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

    /// Binds `peer` to `actor` through the session gate's ownership table and
    /// opens the peer's control buffer.
    ///
    /// A second binding for the same peer replaces the buffer — the gate is
    /// the authority on the pairing (one peer flies at most one aircraft),
    /// and a peer bound to a new actor starts that actor's control state
    /// fresh rather than inheriting a predecessor's held axes.
    ///
    /// # Errors
    ///
    /// [`SessionViolation::ActorNotOwned`] when another peer owns the actor.
    pub fn bind_aircraft(&mut self, peer: PeerId, actor: ActorId) -> Result<(), SessionViolation> {
        self.session.gate_mut().ownership_mut().bind(peer, actor)?;
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
    /// release discarded. `None` when the peer was never bound.
    ///
    /// The buffer is released the way every control handover is: its held
    /// axes are neutralized and its queued edges dropped first, so a departed
    /// pilot leaves no stick deflected and no press half-delivered. The
    /// gate's ownership of the actor goes with it — the gate is a separate
    /// record that does not know the peer left, so a pilot released here that
    /// still owns its actor surrenders it rather than leaving a ghost no
    /// other peer may bind.
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
    /// edges dropped, and — when the gate still names the peer as the
    /// actor's owner — the ownership released with it. Returns what the
    /// release discarded, or `None` when the peer had no bound aircraft.
    fn release_pilot(&mut self, peer: PeerId) -> Option<ReleasedPilot> {
        let mut pilot = self.pilots.remove(&peer)?;
        if self.session.gate().ownership().owner(pilot.actor) == Some(peer) {
            self.session.gate_mut().ownership_mut().release(pilot.actor);
        }
        Some(ReleasedPilot {
            peer,
            actor: pilot.actor,
            neutralized: pilot.buffer.neutralize_axes().len(),
            dropped_edges: pilot.buffer.drain_pending().len(),
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
        }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleasedPilot {
    /// The peer that left.
    pub peer: PeerId,
    /// The aircraft it was bound to.
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
    pub mirror_refusals: Vec<String>,
    /// Reliable event bodies whose application recorded mirror state this
    /// run — today that is the `ActorRemoved` retirement fact.
    pub events_recorded: usize,
    /// Session resources that went away since the last run.
    pub tore_down: Vec<NetTeardown>,
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
    }
}

/// Resource: the drive's own bookkeeping across ticks.
///
/// `host`/`client` are the session identities the drive last served — what it
/// tears down against. `fed` is the newest `(session, snapshot tick)` pair
/// handed to the mirror, so the mailbox slot is never fed twice and a mirror
/// built for another generation is never mistaken for current.
#[derive(Resource, Clone, Debug, Default)]
struct NetDriveState {
    host: Option<SessionId>,
    client: Option<SessionId>,
    client_present: bool,
    fed: Option<(SessionId, Tick)>,
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
            && let Some(mut mirror) = world.get_resource_mut::<NetMirror>()
            && mirror.session() == last
        {
            let released = mirror.teardown();
            report.tore_down.push(NetTeardown {
                side: "client",
                session: Some(last),
                mirror: Some(released),
            });
            world.remove_resource::<NetMirror>();
            state.fed = None;
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

        if let Some(grant) = client.session.grant() {
            state.client = Some(grant.session);
            report.client_session = Some(grant.session);
            // The mirror serves exactly the granted epoch: an installed one
            // for another generation is torn down and replaced rather than
            // fed a foreign epoch's traffic.
            let current = world
                .get_resource::<NetMirror>()
                .map(|mirror| mirror.session());
            if current != Some(grant.session) {
                if let Some(mut old) = world.remove_resource::<NetMirror>() {
                    report.tore_down.push(NetTeardown {
                        side: "client",
                        session: Some(old.session()),
                        mirror: Some(old.teardown()),
                    });
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

/// Installs the scheduled session drive in `FixedPostUpdate`, after the
/// physics step.
///
/// Ordered after [`PhysicsSystems::StepSimulation`] for the same reason the
/// weapon schedule is: the pump reads the committed tick the step just made
/// and the snapshot publishes the state it left. A world that installs this
/// plugin with no session resources runs the drive every fixed tick and
/// serves nothing — the honest state of a world with no session, recorded in
/// [`NetSessionReport`].
#[derive(Clone, Copy, Debug, Default)]
pub struct NetSessionDrivePlugin;

impl Plugin for NetSessionDrivePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetSessionReport>()
            .init_resource::<NetDriveState>()
            .add_systems(
                FixedPostUpdate,
                drive_net_session.after(PhysicsSystems::StepSimulation),
            );
    }
}
