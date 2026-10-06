//! Acceptance scenario F54-C: the wired server/client lifecycle and bounded
//! message processing.
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! stage `### F54-C`. Minimum scenario (spec F54 **AC03**): "Fuzz packet
//! decoding with oversized counts, NaNs and invalid ids." Task test prefix:
//! `accept_f54_c_`.
//!
//! # What is under test
//!
//! Every test drives production code in `cs_net::lifecycle`, `cs_net::transport`,
//! `cs_net::codec`, `cs_net::snapshot` and `cs_net::validation` over real UDP
//! loopback packets. Nothing is simulated in parallel: `ServerSession` and
//! `ClientSession` wrap the same pinned `renet2`/`renet2_netcode` `=0.16.1`
//! stack F54-B froze, and a [`Link`] is a live host/client pair.
//!
//! The acceptance scenario proper is
//! [`accept_f54_c_fuzzed_packets_are_bounded_and_never_produce_a_non_finite_value`]
//! plus [`accept_f54_c_the_client_consumer_survives_the_whole_corpus`]: a
//! deterministic corpus of hostile buffers through the production decoder, the
//! client's consumer and the snapshot dequantizers.
//!
//! A well-behaved peer is a [`ClientSession`]. A peer that needs to put bytes on
//! the wire a correct producer would never emit (a verbatim replay, a flood of
//! hostile buffers) is a raw [`ClientTransport`] that completed the same
//! production handshake — see [`Link::raw_peer`] — so the injection still
//! arrives through the real receive path.
//!
//! Everything here is the newly authored synthetic fixture. Loopback on one
//! machine is `network_local` per `AGENTS.md`; `network_real` two-machine
//! evidence is F54-D, which this stage does not claim.

#[path = "support/f54_c_fuzz.rs"]
mod fuzz;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::input::{AxisValueError, FlightCommand};
use cs_types::net::{ActorId, PeerId, SessionAllocator, SessionId};

use cs_net::bounds::{
    MAX_INPUT_FRAMES_PER_PACKET, MAX_MODS, MAX_PACKET_BYTES, MAX_SEEN_EVENTS, MAX_SNAPSHOT_BYTES,
    MAX_UNACKED_PACKETS, MAX_WORK_PER_PUMP,
};
use cs_net::codec::{
    ClientPacket, CodecError, ServerPacket, decode_client_packet, decode_server_packet,
    encode_client_message, encode_client_packet,
};
use cs_net::compat::{
    Compatibility, HandshakeReject, PROTOCOL_VERSION, ProtocolVersion, SessionParameters,
};
use cs_net::fixture::{
    SYNTHETIC_CONTENT_SHA256, SYNTHETIC_RULES_SHA256, SYNTHETIC_SESSION, synthetic_blueprint_id,
    synthetic_hello, synthetic_parameters,
};
use cs_net::lifecycle::{
    ClientClosure, ClientFault, ClientNotice, ClientPhase, ClientSession, INPUT_RETRY_INTERVAL,
    PeerInput, ServerFault, ServerNotice, ServerPhase, ServerSession,
};
use cs_net::message::{
    ClientPayload, DisconnectReason, EventBody, FinishReason, MessageHeader, ServerMessage,
    ServerPayload, SnapshotFrame, WireError,
};
use cs_net::snapshot::{SYNTHETIC_ORIGIN_EPOCH, Snapshot};
use cs_net::transport::{
    CHANNEL_SEQUENCED, ClientEvent, ClientTransport, ConnectWindow, DEFAULT_CONNECT_WINDOW,
    DropReason,
};
use cs_net::validation::ThreatCase;

/// One exchange round's duration. Loopback needs no real sleep; the updates
/// only have to run often enough to exchange the netcode handshake's packets.
const STEP: Duration = Duration::from_millis(16);

/// The connection-layer window these acceptance tests ask for.
///
/// The pinned layer's own default is fifteen seconds
/// ([`cs_net::transport::DEFAULT_CONNECT_WINDOW`]), and it accumulates that
/// window entirely from the `elapsed` a caller hands to `update`. A test that
/// pumps at [`STEP`] therefore gives itself fifteen seconds of *pump* time in
/// about four milliseconds of wall clock, and a single dropped loopback
/// datagram on a machine that is oversubscribed cannot be retransmitted inside
/// that window: the client disconnects, and the pinned state machine has no
/// way back. See
/// `docs/findings/2026-10-04-f54-x2-loopback-pump-and-socket-determinism.md`
/// for the measurement.
///
/// This is a fixture parameter only. A shipped session runs with the default
/// window, which no code path here changes.
const LOOPBACK_WINDOW: ConnectWindow = ConnectWindow::new(120);

/// How many pump rounds an end-to-end expectation gets before it fails.
///
/// Derived from [`LOOPBACK_WINDOW`] and [`STEP`] together, so neither can
/// drift out from under the other: the round budget is the window expressed in
/// [`STEP`]s, plus a margin, which means a wait may always outlast the
/// connection layer's own patience instead of reporting a timeout the layer had
/// already declared. It is still a bound on rounds, not a sleep — no test waits
/// on the wall clock.
const MAX_ROUNDS: usize =
    (LOOPBACK_WINDOW.const_seconds() as usize * 1_000 / STEP.as_millis() as usize) + 64;

/// Serializes the tests in this file that open a real loopback socket.
///
/// Measured on an 11-core machine deliberately oversubscribed more than
/// twofold: eleven loopback handshakes at once left 435 of 1500 unable to
/// finish inside the connection layer's own window, while a single handshake
/// loop at the same load settled 600 of 600 (same finding, "Concurrency"). The
/// loss is in the machine's loopback UDP path, not in anything a test asserts,
/// so the file runs one live loopback pair at a time instead of racing for it.
/// Each of these tests is a few milliseconds of socket work, so serializing
/// them costs nothing measurable and removes the machine's load from the
/// result.
static LOOPBACK: Mutex<()> = Mutex::new(());

/// Takes the loopback socket lock. Held for as long as the sockets it guards.
fn loopback() -> MutexGuard<'static, ()> {
    LOOPBACK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How many exchange rounds the abusive peer gets to be cut off in.
///
/// The verdict itself is not timed: the host decides the moment it receives a
/// hostile payload, and the peer keeps firing one payload per round for the
/// whole budget. What the budget bounds is *delivery*: every payload travels
/// the droppable channel, so each round is one more independent attempt for
/// one of them to reach the host on a machine whose loopback drops datagrams
/// under load (F54-X2 and F54-X10 measured that loss). It is a number of
/// attempts, not a sleep and not a deadline for a decision the host has
/// already made.
const HOSTILE_ROUNDS: usize = 64;

/// How many times a fixture may be rebuilt when the loopback underneath it
/// died before the awaited thing could ever be observed.
///
/// F54-X10 measured loopback sockets that stop being reachable while they
/// are still in use, when other processes churn the kernel's socket table
/// (`dropped due to no socket`): a fixture on such a path can never produce
/// the awaited observation — no handshake completes, no payload arrives —
/// so the session never gets the chance to decide. Rebuilding costs fresh
/// sockets and a few milliseconds, and is spent only on an attempt whose own
/// waits found the path already declared dead. A delivered answer is a
/// verdict: it is reported exactly as it happened and never retried. Every
/// attempt dying is still a failure, and the report then says what the dead
/// ends saw rather than any session verdict.
///
/// The count is sized against the churn driver, not a quiet host: under
/// `f54x10_fleet`'s measured worst shape roughly half of fresh socket pairs
/// come up orphaned, and a pass of this file chains about fifteen live-path
/// fixtures. `(0.5)^24` puts one fixture's chance of never once binding a
/// live pair at ~6e-8 — a run then fails only when the loopback truly
/// cannot be had, which is the failure to report, not to hide.
///
/// The measurement behind the dead-path signature is in
/// `docs/findings/2026-10-06-f54-c-hostile-peer-flake-loopback-path.md`, and
/// the count's evidence in
/// `docs/findings/2026-10-06-f54-c-loopback-dead-path-verdicts.md`.
const DEAD_PATH_ATTEMPTS: usize = 24;

/// A wait whose condition could never have been observed on this fixture:
/// the loopback path underneath it died, so nothing the scenario asked was
/// ever delivered. [`live_fixture`] rebuilds it; every other end — a
/// refusal, a hang-up, a budget spent while the path stayed up — is a
/// verdict and is reported, not retried.
struct DeadPath(String);

/// Runs `attempt` until one fixture's loopback stays alive for its whole
/// scenario, bounded by [`DEAD_PATH_ATTEMPTS`].
///
/// `Err(DeadPath)` is the only outcome that rebuilds: the attempt's waits
/// found the connection layer had already ended the path, so no session
/// answer was ever observable on it. A verdict is never rebuilt — a wrong
/// answer fails inside `attempt` itself and that panic leaves this loop
/// directly. When every attempt dies, the report names the dead ends it
/// saw, which is a claim about this host's sockets and not about the
/// session.
fn live_fixture<T>(mut attempt: impl FnMut() -> Result<T, DeadPath>) -> T {
    let mut history = Vec::new();
    for tries in 1..=DEAD_PATH_ATTEMPTS {
        match attempt() {
            Ok(done) => return done,
            // The dead attempt's fixture is already dropped here: a `Link`
            // holds the `LOOPBACK` guard for as long as its sockets live,
            // so keeping it around while the next attempt binds would
            // deadlock — the measurement that rule comes from is in the
            // F54-C hostile-peer findings note. And the rebind is deliberately
            // not immediate: a stillborn pair burns only a few milliseconds,
            // so a churn burst can outlive a dozen back-to-back attempts —
            // spacing the sequence spreads it across the bursts rather than
            // racing the same dead patch twenty-four times. The pause is
            // between fixtures, never inside one: it changes which wall-clock
            // the sockets bind into, not what the session answered.
            Err(DeadPath(reason)) => {
                history.push(format!("{tries}: {reason}"));
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    panic!(
        "the loopback never carried this fixture: {}",
        history.join("; ")
    );
}

/// `None` when the session's verdict was delivered and the caller's own
/// assertions judge it. `Some` — the rebuildable [`DeadPath`] — for every
/// end where no verdict could have arrived: the layer declaring the path
/// dead, the connection gone without a reason, the `TransportLost` closure
/// the layer reports, or a connection still nominally up after the whole
/// window. The last is the shape the churn driver leaves behind: the
/// pinned stack holds a connection open for `timeout_seconds` after the
/// *last received packet*, so keepalives that still trickle in keep a dead
/// path's connection marked up indefinitely while nothing the session owes
/// can land.
fn silent_client(client: &ClientSession, what: &str) -> Option<DeadPath> {
    match client.phase().closure() {
        Some(ClientClosure::TransportLost(_)) | None => Some(DeadPath(format!(
            "{what}: the path carried no verdict (client phase {}, connected {}, reason {:?})",
            client.phase(),
            client.transport().is_connected(),
            client.transport().disconnect_reason()
        ))),
        Some(_) => None,
    }
}

/// The shared read of a grantless handshake end for bare clients and,
/// through [`Link::ungranted`], for whole links: a delivered closure that
/// is not the layer's own `TransportLost` is the session's answer and
/// fails the caller that needed the grant on the spot. Every other end is
/// the loopback's [`DeadPath`] — including a connection still marked up,
/// which under churn only proves stray keepalives keep resetting its
/// timeout while no session packet ever lands (see [`silent_client`]).
fn ungranted_client(client: &ClientSession, what: &str) -> DeadPath {
    if let Some(closure) = client.phase().closure() {
        assert!(
            matches!(closure, ClientClosure::TransportLost(_)),
            "{what}: the session answered {closure}, not a grant"
        );
    }
    DeadPath(format!(
        "{what}: the handshake's answer never arrived (client phase {}, connected {}, reason {:?})",
        client.phase(),
        client.transport().is_connected(),
        client.transport().disconnect_reason()
    ))
}

/// The connection layer's own read of whether a raw peer's path is dead:
/// every reason but a hang-up the host sent. `DisconnectedByServer` is the
/// host *acting* — a session verdict in transport clothing — so it is not a
/// dead path, and a peer still connected is simply alive.
fn peer_path_dead(peer: &RawPeer) -> bool {
    matches!(
        peer.transport.disconnect_reason(),
        Some(reason) if reason != renetcode2::DisconnectReason::DisconnectedByServer
    )
}

/// The loopback read of a wait on `peer`'s traffic that ran out. A
/// hang-up the host sent is the session acting — a verdict — so it fails
/// on the spot with what the host did see. Every other end is the
/// rebuildable [`DeadPath`]: a peer the host never got back to saw its
/// awaited observation never exist, and a peer still nominally connected
/// only proves the host's keepalives still trickle in one way — nothing
/// says the peer's own packets ever made the return trip.
fn peer_delivery(peer: &RawPeer, what: &str, notices: &[ServerNotice]) -> DeadPath {
    match peer.transport.disconnect_reason() {
        Some(renetcode2::DisconnectReason::DisconnectedByServer) => panic!(
            "{what}: the host hung the peer up instead of answering; the host saw {notices:?} ({})",
            peer.status()
        ),
        Some(reason) => DeadPath(format!(
            "{what}: the connection layer ended the peer's path ({reason:?}); {}",
            peer.status()
        )),
        None => DeadPath(format!(
            "{what}: the peer's path carried nothing observable (connected {}); {}",
            peer.transport.is_connected(),
            peer.status()
        )),
    }
}

/// The grantless end of a raw peer's handshake: a delivered refusal is the
/// session's verdict and fails the caller that needed the grant on the
/// spot. Everything else is the loopback's [`DeadPath`] — a connection
/// the layer ended, a hang-up that arrived ahead of the reliable refusal
/// it was meant to follow, or a connection still nominally up, which under
/// churn only says stray keepalives still land one way (see
/// [`silent_client`]).
fn ungranted_peer(peer: &RawPeer, what: &str) -> DeadPath {
    if let Some(reason) = peer.transport.rejection() {
        panic!("{what}: the session refused the peer: {reason}");
    }
    DeadPath(format!("{what}: {}", peer.status()))
}

/// A live loopback pair: one bound host session and one connecting client
/// session, plus every notice each side produced.
struct Link {
    host: ServerSession,
    client: ClientSession,
    host_notices: Vec<ServerNotice>,
    client_notices: Vec<ClientNotice>,
    /// Held for as long as the two sockets above are open, so this file never
    /// has two loopback pairs live at once. See [`loopback`].
    _loopback: MutexGuard<'static, ()>,
}

impl Link {
    /// Binds a host for `session`/`params` on loopback and connects a client
    /// offering `hello`.
    fn new(
        session: SessionId,
        params: SessionParameters,
        hello: cs_net::compat::ClientHello,
    ) -> Self {
        let loopback = loopback();
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let host = ServerSession::bind(session, params, bind, Duration::ZERO)
            .expect("the host socket binds");
        let addr = host.local_addr().expect("the bound host has an address");
        let client =
            ClientSession::connect_with_window(hello, addr, 0xC1, Duration::ZERO, LOOPBACK_WINDOW)
                .expect("the client socket binds");
        Self {
            host,
            client,
            host_notices: Vec::new(),
            client_notices: Vec::new(),
            _loopback: loopback,
        }
    }

    /// A link whose handshake completed and whose client holds a grant, as
    /// an outcome: `Err` hands the unfinished link back so the caller can
    /// read which side owned the wait's end — a `Refused` closure is the
    /// session's verdict, everything else is the loopback's.
    ///
    /// No expectation is dropped by asking this way — the caller either
    /// rebuilds the fixture or reports that it never came up — but a test
    /// that retries its fixture needs the difference between "the loopback
    /// could not deliver" and "the session answered".
    fn try_joined(session: SessionId) -> Result<Self, Box<Self>> {
        let mut link = Self::new(session, synthetic_parameters(), synthetic_hello());
        for _ in 0..MAX_ROUNDS {
            if link.client.grant().is_some() {
                return Ok(link);
            }
            if link.client.phase().closure().is_some() {
                break; // A verdict that is not a grant: no round will change it.
            }
            link.round();
        }
        if link.client.grant().is_some() {
            Ok(link)
        } else {
            Err(Box::new(link))
        }
    }

    /// What [`Self::try_joined`]'s `Err` means for a scenario that needed
    /// the grant, settled by [`ungranted_client`].
    fn ungranted(&self, what: &str) -> DeadPath {
        ungranted_client(&self.client, what)
    }

    /// One exchange round: the client pumps (emitting its packets), then the
    /// host pumps (receiving and answering).
    fn round(&mut self) {
        self.client_notices.extend(self.client.pump(STEP));
        self.host_notices.extend(self.host.pump(STEP));
    }

    /// One round that pumps the client alone.
    ///
    /// Where the host must not run another update: a hang-up the previous pump
    /// condemned fires in the next one, and a test that needs the host to stay
    /// exactly where it is can still give the client the rounds it needs to read
    /// what the host has already sent.
    fn client_round(&mut self) {
        self.client_notices.extend(self.client.pump(STEP));
    }

    /// Pumps until `done` holds, as an outcome.
    ///
    /// `Err` when [`MAX_ROUNDS`] ran out and the awaited condition was
    /// never deliverable on this fixture: the connection layer ended the
    /// path, or — the shape the churn driver leaves — the connection is
    /// still nominally up on stray keepalives while nothing the session
    /// owed could land. Only delivered verdicts still panic: the host's
    /// own hang-up, or a session closure that is not the transport's fault.
    ///
    /// Every exchange here is over a real socket, so how many rounds a
    /// delivery takes is the transport's business, not the test's: an
    /// acknowledgment needs a netcode round trip and the connection layer may
    /// lose and resend it. Waiting on the *condition* keeps every expectation
    /// below exactly as strict as a fixed wait, without turning a slow round
    /// trip into a failure.
    fn try_pump_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) -> Result<(), DeadPath> {
        for _ in 0..MAX_ROUNDS {
            if done(self) {
                return Ok(());
            }
            // Once the layer has ended the connection no further round can
            // deliver anything; which of its ends — the host's hang-up, a
            // missed window, a fault — is for the tail of this wait to
            // classify. A session closure alone is *not* a stop: waits whose
            // condition rides the transport itself (a teardown's hang-up is
            // still in flight when `ServerClosed` lands) must see the
            // connection out. Nor is the netcode reason alone a stop: it
            // appears the round the `Disconnect` packet decodes, while the
            // renet-level `is_connected` and the `Disconnected` notice land
            // one pump later, so the break waits until both layers agree the
            // end is real.
            if self.client.transport().disconnect_reason().is_some()
                && !self.client.transport().is_connected()
            {
                break;
            }
            self.round();
        }
        if done(self) {
            return Ok(());
        }
        // A delivered verdict stands: the host's own hang-up, or a session
        // closure that is not the transport's fault. Every other end is a
        // fixture that carried nothing the condition could ride on — the
        // layer-declared dead path, or the nominally-up-but-silent one the
        // churn leaves behind (see [`silent_client`]) — so it is rebuilt.
        match self.client.transport().disconnect_reason() {
            Some(renetcode2::DisconnectReason::DisconnectedByServer) => panic!(
                "{what}: the host hung up instead of the awaited observation; the host saw {:?} and the client saw {:?}",
                self.host_notices, self.client_notices
            ),
            Some(_) => {}
            None => {
                if let Some(closure) = self.client.phase().closure() {
                    assert!(
                        matches!(closure, ClientClosure::TransportLost(_)),
                        "{what}: the session answered {closure} instead of the awaited observation; the host saw {:?} and the client saw {:?}",
                        self.host_notices,
                        self.client_notices
                    );
                }
            }
        }
        Err(DeadPath(format!(
            "{what}: the path carried nothing observable (client phase {}, connected {}, reason {:?}); the host saw {:?} and the client saw {:?}",
            self.client.phase(),
            self.client.transport().is_connected(),
            self.client.transport().disconnect_reason(),
            self.host_notices,
            self.client_notices
        )))
    }

    /// Pumps until the handshake settles: the client holds a grant or a
    /// closure.
    ///
    /// `Err` when the closure is `TransportLost` or the budget ran out on a
    /// dead path — the connection layer's report, not the session's, so the
    /// handshake's answer never existed to be observed. A `Refused` closure
    /// is the session's verdict and returns `Ok` for the caller's
    /// assertions to judge.
    fn try_pump_until_joined(&mut self) -> Result<(), DeadPath> {
        self.try_pump_until("the handshake never settled", |link| {
            link.client.grant().is_some() || link.client.phase().closure().is_some()
        })?;
        if matches!(
            self.client.phase().closure(),
            Some(ClientClosure::TransportLost(_))
        ) {
            return Err(DeadPath(format!(
                "the handshake's answer never arrived; the client saw {:?}",
                self.client_notices
            )));
        }
        Ok(())
    }

    /// Pumps until the client applied the host's `Launched`.
    fn try_pump_until_live(&mut self) -> Result<(), DeadPath> {
        self.try_pump_until("the session never launched", |link| {
            matches!(link.client.phase(), ClientPhase::Live { .. })
        })
    }

    /// Pumps `rounds` further rounds.
    fn pump(&mut self, rounds: usize) {
        for _ in 0..rounds {
            self.round();
        }
    }

    /// Pumps until the host has at least one queued work item.
    fn try_pump_until_work(&mut self) -> Result<(), DeadPath> {
        self.try_pump_until("no admitted input ever arrived", |link| {
            link.host.queued() > 0
        })
    }

    /// Pumps until the host acknowledges input up to `through`.
    fn try_pump_until_acked(&mut self, through: u32) -> Result<(), DeadPath> {
        self.try_pump_until(
            &format!("input was never acknowledged through {through}"),
            |link| {
                link.client
                    .acked_through()
                    .is_some_and(|held| held >= through)
            },
        )
    }

    /// Pumps until the client session is closed *and* its transport
    /// connection is down.
    ///
    /// A teardown observation is only as delivered as its last leg: the
    /// client seeing `ServerClosed` while its connection lingers is half an
    /// observation, so the wait also covers the hang-up. A connection that
    /// then died by anything but the host's hang-up — the client missing
    /// its own window, a fault — means the teardown never arrived, which is
    /// the rebuildable [`DeadPath`], not a verdict.
    fn try_pump_until_closed(&mut self) -> Result<(), DeadPath> {
        self.try_pump_until("the client never observed the teardown", |link| {
            !link.client.phase().open() && !link.client.transport().is_connected()
        })?;
        if !matches!(
            self.client.transport().disconnect_reason(),
            Some(renetcode2::DisconnectReason::DisconnectedByServer)
        ) {
            return Err(DeadPath(format!(
                "the teardown's hang-up never arrived; the connection died by itself ({:?}); the client saw {:?}",
                self.client.transport().disconnect_reason(),
                self.client_notices
            )));
        }
        Ok(())
    }

    /// Pumps `peer` and the host until `done` holds, refiring `payload`'s
    /// sender each round, as an outcome.
    ///
    /// Every injection travels the droppable channel, so a wait that sent
    /// once and then watched could only ever observe the one delivery it
    /// happened to get; refiring keeps the expectation on the session's
    /// answer, not on which datagram survived. `Err(DeadPath)` — settled by
    /// [`peer_delivery`] — is the connection layer declaring the peer's own
    /// path dead; a hang-up the host sent is its answer and panics as the
    /// verdict it is, as does a peer whose path held for the whole window.
    fn try_pump_peer_until(
        &mut self,
        peer: &mut RawPeer,
        what: &str,
        mut refire: impl FnMut(&mut RawPeer),
        done: impl Fn(&Self) -> bool,
    ) -> Result<(), DeadPath> {
        for _ in 0..MAX_ROUNDS {
            if done(self) {
                return Ok(());
            }
            // Once the layer ends the connection no refired payload can ever
            // be delivered; which end — the host's own hang-up or the layer's
            // dead path — is for [`peer_delivery`] below to classify.
            if peer.transport.disconnect_reason().is_some() {
                break;
            }
            refire(peer);
            self.host_notices.extend(peer.round(&mut self.host));
        }
        if done(self) {
            return Ok(());
        }
        Err(peer_delivery(peer, what, &self.host_notices))
    }

    /// Whether the host produced a notice matching `wanted`.
    fn host_has(&self, wanted: impl Fn(&ServerNotice) -> bool) -> bool {
        self.host_notices.iter().any(wanted)
    }

    /// Whether the client produced a notice matching `wanted`.
    fn client_has(&self, wanted: impl Fn(&ClientNotice) -> bool) -> bool {
        self.client_notices.iter().any(wanted)
    }

    /// A second peer that completed the same production handshake but has no
    /// lifecycle owner, so a test can put verbatim bytes on the wire.
    ///
    /// Injection still travels the real receive path — netcode, the channel
    /// layer, the bounded codec and the session gate — so a refusal observed
    /// afterwards is the production refusal and not a test-side filter.
    fn raw_peer(&self, client_id: u64) -> RawPeer {
        let addr = self.host.local_addr().expect("the host has an address");
        RawPeer {
            transport: ClientTransport::connect_with_window(
                synthetic_hello(),
                addr,
                client_id,
                Duration::ZERO,
                LOOPBACK_WINDOW,
            )
            .expect("the raw client socket binds"),
            events: Vec::new(),
        }
    }
}

/// A handshake-complete peer with no lifecycle owner.
struct RawPeer {
    transport: ClientTransport,
    /// Every event this peer produced since it was built.
    ///
    /// The peer has no lifecycle owner to hold its notices, so without this
    /// a failed expectation could not tell "the host cut me off" apart from
    /// "my side stopped being able to talk at all" — which is the difference
    /// between a session decision and a transport fault.
    events: Vec<ClientEvent>,
}

impl RawPeer {
    /// Drives this peer to its grant, pumping `host` alongside it, as an
    /// outcome: `None` when the loopback never carried the handshake inside
    /// [`MAX_ROUNDS`] — or when the connection's own verdict (a refusal the
    /// host sent, or the layer's own disconnect) arrived first — which is a
    /// delivery failure of the fixture, not anything the host decided.
    ///
    /// A caller that needed the grant hands a `None` to [`ungranted_peer`],
    /// which reads the peer's own state to separate the session's answer
    /// from the loopback's dead path.
    fn try_handshake(&mut self, host: &mut ServerSession) -> Option<PeerId> {
        for _ in 0..MAX_ROUNDS {
            if let Some(grant) = self.transport.grant() {
                return Some(grant.peer);
            }
            // A rejection is the session's verdict and a disconnect is the
            // connection layer's; no further round can produce a grant.
            if self.transport.rejection().is_some() || self.transport.disconnect_reason().is_some()
            {
                break;
            }
            self.events.extend(self.transport.update(STEP));
            host.pump(STEP);
        }
        self.transport.grant().map(|grant| grant.peer)
    }

    /// Puts `bytes` on `channel` verbatim.
    fn inject(&mut self, channel: u8, bytes: &[u8]) {
        self.transport.send_encoded(channel, bytes);
    }

    /// Pumps this peer once.
    fn pump(&mut self) -> Vec<ClientEvent> {
        let events = self.transport.update(STEP);
        self.events.extend(events.iter().cloned());
        events
    }

    /// Pumps this peer and the host together.
    fn round(&mut self, host: &mut ServerSession) -> Vec<ServerNotice> {
        self.events.extend(self.transport.update(STEP));
        host.pump(STEP)
    }

    /// This peer's transport state, in the form a failing assertion prints:
    /// whether the connection is still up, why it went down if it did, and
    /// what its event stream has said since it was built.
    fn status(&self) -> String {
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for event in &self.events {
            let kind = match event {
                ClientEvent::Connected => "connected",
                ClientEvent::Granted { .. } => "granted",
                ClientEvent::Rejected { .. } => "rejected",
                ClientEvent::Server(_) => "server packet",
                ClientEvent::Disconnected { .. } => "disconnected",
                ClientEvent::PacketDropped { .. } => "undecodable reply",
                ClientEvent::TransportFault { .. } => "transport fault",
                ClientEvent::SendFault { .. } => "send fault",
            };
            match counts.iter_mut().find(|(name, _)| *name == kind) {
                Some((_, seen)) => *seen += 1,
                None => counts.push((kind, 1)),
            }
        }
        format!(
            "the abusive peer's transport: connected {}, reason {:?}, events {:?}",
            self.transport.is_connected(),
            self.transport.disconnect_reason(),
            counts
        )
    }
}

/// Encodes one fire packet for `session`/`sequence`.
fn fire_bytes(session: SessionId, tick: Tick, sequence: u32) -> Vec<u8> {
    encode_client_message(&fuzz::fire_message(session, tick, sequence))
        .expect("a fire packet encodes")
}

/// Whether the host has cut `peer` off: it is no longer one of its members.
fn cut_off(link: &Link, peer: PeerId) -> bool {
    link.host.members().all(|member| member != peer)
}

/// One run of the hostile-peer scenario: a fresh fixture, the flood, and the
/// wait for the verdict.
struct Attempt {
    /// The fixture the run used: the host, the honest client and every notice.
    link: Link,
    /// The peer that flooded the host.
    hostile: RawPeer,
    /// The id the host allocated to that peer.
    hostile_peer: PeerId,
    /// Whether the host processed any traffic from that peer.
    arrived: bool,
    /// Whether the honest peer's probe input reached the consumer afterwards.
    honest_reached: bool,
}

impl Attempt {
    /// The loopback carried nothing from the abusive peer, so no verdict was
    /// observable: the fixture failed, not the session.
    fn quiet(&self) -> bool {
        !self.arrived && !cut_off(&self.link, self.hostile_peer)
    }
}

/// How many packets from `peer` the host has classified so far — admitted
/// input, a refusal or the cut-off — one count per arrival.
fn peer_arrivals(notices: &[ServerNotice], peer: PeerId) -> usize {
    notices
        .iter()
        .filter(|notice| match notice {
            ServerNotice::Admitted(input) => input.peer == peer,
            ServerNotice::Dropped {
                peer: Some(seen), ..
            }
            | ServerNotice::CutOff {
                peer: Some(seen), ..
            } => *seen == peer,
            _ => false,
        })
        .count()
}

/// Whether the host processed anything `peer` sent: admitted input, a refusal
/// or the cut-off itself. A peer that joined and then produced none of these
/// never reached the host's receive path at all.
fn hostile_traffic(notices: &[ServerNotice], peer: PeerId) -> bool {
    peer_arrivals(notices, peer) > 0
}

/// Builds a fresh fixture, floods the host from a second peer and waits for
/// the verdict the session owes.
///
/// `Err(DeadPath)` means the fixture never came up — the loopback did not
/// carry one of the two handshakes inside [`MAX_ROUNDS`] — and describes
/// which end died, so a caller that rebuilds can still say what it saw when
/// every attempt failed.
fn abusive_peer_attempt() -> Result<Attempt, DeadPath> {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let mut link = Link::try_joined(session)
        .map_err(|link| link.ungranted("the honest client's handshake"))?;
    let good_peer = link.client.grant().expect("a grant exists").peer;
    link.host
        .gate_mut()
        .ownership_mut()
        .bind(good_peer, ActorId { session, serial: 1 })
        .expect("the host binds an aircraft");

    // A second peer that handshakes and then floods the host with the
    // malformed and oversized shapes from the corpus.
    let mut hostile = link.raw_peer(0xC8);
    let hostile_peer = hostile
        .try_handshake(&mut link.host)
        .ok_or_else(|| ungranted_peer(&hostile, "the abusive peer's handshake"))?;
    assert_ne!(hostile_peer, good_peer, "the two peers are distinct");

    // Small shapes keep the loopback socket honest.
    let cases: Vec<Vec<u8>> = fuzz::corpus(fuzz::SEEDS[0])
        .into_iter()
        .filter(|case| case.bytes.len() <= 512)
        .map(|case| case.bytes)
        .collect();
    assert!(!cases.is_empty(), "the corpus carries small hostile shapes");

    // One burst of the whole corpus, sixteen payloads between exchange rounds.
    let mut sent = 0usize;
    for case in &cases {
        hostile.inject(CHANNEL_SEQUENCED, case);
        sent += 1;
        if sent.is_multiple_of(16) {
            link.host_notices.extend(hostile.round(&mut link.host));
        }
    }

    // And then the peer keeps abusing the host while the verdict is waited
    // for: the flood travels the droppable channel, so a datagram the
    // loopback drops (F54-X2/X10 measured real loss on this host) is gone for
    // good, and a wait that sends nothing more can only ever observe a
    // cut-off that one particular delivery already triggered. One hostile
    // payload per round keeps the expectation on the session's answer — the
    // abusive peer is cut off — instead of on which datagrams survived.
    let mut rounds = 0usize;
    while rounds < HOSTILE_ROUNDS && !cut_off(&link, hostile_peer) {
        hostile.inject(CHANNEL_SEQUENCED, &cases[rounds % cases.len()]);
        link.host_notices.extend(hostile.round(&mut link.host));
        rounds += 1;
    }

    let arrived = hostile_traffic(&link.host_notices, hostile_peer);

    // A peer the connection layer ended mid-flood can no longer produce the
    // traffic the verdict is owed for, so the wait's end is the loopback's
    // again — unless the end is the hang-up the host sent, which is the
    // session's own verdict whether or not the notice was read yet.
    if !cut_off(&link, hostile_peer) && peer_path_dead(&hostile) {
        return Err(peer_delivery(
            &hostile,
            "the abusive peer's path died before the verdict",
            &link.host_notices,
        ));
    }

    // The scenario's other half — the honest peer keeps a working session —
    // needs the honest path alive too, and its socket can have died under the
    // same churn, so it is probed here where a dead one is still rebuildable.
    // The wait is for the honest peer's own admission, not the queue's depth:
    // the flood may still be queuing its own packets, and the edge rides the
    // droppable channel whose loss the client's own retransmission covers.
    link.host.drain_work();
    link.client
        .submit_edge(FlightCommand::FirePrimary, Tick(1000))
        .expect("an edge queues");
    link.try_pump_until("the honest peer's probe was never admitted", |link| {
        link.host_has(
            |notice| matches!(notice, ServerNotice::Admitted(input) if input.peer == good_peer),
        )
    })?;
    let honest_reached = link
        .host
        .drain_work()
        .iter()
        .any(|input| input.peer == good_peer);

    Ok(Attempt {
        link,
        hostile,
        hostile_peer,
        arrived,
        honest_reached,
    })
}

/// What a missing verdict looks like from both ends of the link.
///
/// Reached only when the abusive peer was not cut off, so a passing run pumps
/// nothing extra. It reports what the host saw before this message was built,
/// the abusive peer's own transport state, how many connections the host
/// still holds, and — by letting the honest peer speak — whether the host
/// hears *anybody*. The last two separate a session that declined to cut the
/// peer off from a loopback path that carried nothing at all.
fn diagnose_silence(link: &mut Link, hostile: &RawPeer, hostile_peer: PeerId) -> String {
    let seen = link.host_notices.len();
    let reached = hostile_traffic(&link.host_notices[..seen], hostile_peer);

    link.host.drain_work();
    let mut heard = false;
    if link
        .client
        .submit_edge(FlightCommand::FirePrimary, Tick(1000))
        .is_ok()
    {
        for _ in 0..MAX_ROUNDS {
            if link.host.queued() > 0 {
                heard = true;
                break;
            }
            link.round();
        }
    }
    format!(
        "the host saw {:?}; {}; the host holds {} client(s); the abusive peer's traffic reached the host: {reached}; the honest peer still reaches the host: {heard}",
        &link.host_notices[..seen],
        hostile.status(),
        link.host.connected_clients(),
    )
}

// --------------------------------------------------------------- lifecycle --

#[test]
fn accept_f54_c_the_connect_window_is_a_fixture_parameter_over_the_default() {
    let _loopback = loopback();
    // The pinned layer's own unsecure token carries a fifteen-second window;
    // that is what `DEFAULT_CONNECT_WINDOW` is and what every shipped session
    // runs with, so the fixture parameter only ever *widens* what a caller
    // already had.
    assert_eq!(
        DEFAULT_CONNECT_WINDOW.const_seconds(),
        15,
        "the default is the pinned stack's own value, not a number this crate chose"
    );
    assert_eq!(
        ConnectWindow::from_seconds(0),
        None,
        "zero is the pinned stack's 'no timeout at all' development setting and is refused"
    );
    assert_eq!(
        ConnectWindow::from_seconds(-1),
        None,
        "and so is a negative window"
    );
    assert_eq!(ConnectWindow::new(120).const_seconds(), 120);

    // Widening it must not change what the handshake *is*. This is the path the
    // fixture uses, so it runs a real loopback handshake through it: the host
    // still admits the same offer with the same grant...
    let session = SessionAllocator::new()
        .allocate()
        .expect("the first epoch allocates");
    let (host, client) = live_fixture(|| {
        let mut host = ServerSession::bind(
            session,
            synthetic_parameters(),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            Duration::ZERO,
        )
        .expect("the host socket binds");
        let addr = host.local_addr().expect("the bound host has an address");
        let mut client = ClientSession::connect_with_window(
            synthetic_hello(),
            addr,
            0xCE,
            Duration::ZERO,
            ConnectWindow::new(120),
        )
        .expect("the client socket binds");
        for _ in 0..MAX_ROUNDS {
            if client.grant().is_some()
                || client.phase().closure().is_some()
                || client.transport().disconnect_reason().is_some()
            {
                break;
            }
            client.pump(STEP);
            host.pump(STEP);
        }
        if client.grant().is_some() {
            Ok((host, client))
        } else {
            Err(ungranted_client(&client, "the widened-window handshake"))
        }
    });
    let grant = client.grant().expect("the handshake granted a session");
    assert_eq!(
        grant.session, session,
        "and it is the same epoch a default client gets"
    );
    assert_eq!(host.phase(), ServerPhase::Gathering);

    // ...and still refuses the same offers the same way. The window is a
    // timeout, not a way around the compatibility gate.
    let reason = live_fixture(|| {
        let mut refused = ServerSession::bind(
            session,
            synthetic_parameters(),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            Duration::ZERO,
        )
        .expect("a second host binds");
        let refused_addr = refused
            .local_addr()
            .expect("the second host has an address");
        let mut wrong = synthetic_hello();
        wrong.protocol = ProtocolVersion::new(9).expect("nine is nonzero");
        let mut second = ClientSession::connect_with_window(
            wrong,
            refused_addr,
            0xCF,
            Duration::ZERO,
            ConnectWindow::new(120),
        )
        .expect("the second client socket binds");
        let mut reason = None;
        for _ in 0..MAX_ROUNDS {
            second.pump(STEP);
            if let Some(rejected) = refused
                .pump(STEP)
                .into_iter()
                .find_map(|notice| match notice {
                    ServerNotice::PeerRefused { reason } => Some(reason),
                    _ => None,
                })
            {
                reason = Some(rejected);
                break;
            }
            if second.transport().disconnect_reason().is_some() {
                break;
            }
        }
        match reason {
            Some(reason) => Ok(reason),
            // The refusal is produced the round the host decodes the hello,
            // so `reason` staying empty means the host never saw it — and a
            // connection still nominally up says no more than that stray
            // keepalives keep resetting its timeout while nothing the
            // session owes can land (see [`silent_client`]).
            None => Err(DeadPath(format!(
                "the refused handshake never reached the host (client phase {:?}, connected {}, reason {:?})",
                second.phase(),
                second.transport().is_connected(),
                second.transport().disconnect_reason()
            ))),
        }
    });
    assert!(
        matches!(
            reason,
            HandshakeReject::UnsupportedProtocol {
                offered,
                supported,
            } if offered.get() == 9 && supported == PROTOCOL_VERSION
        ),
        "the widened window changed nothing about admission: {reason:?}"
    );

    // And the window is the connection layer's own, measured in the `elapsed`
    // a caller pumps with: a client that asked for one second is dropped by the
    // pinned layer after about a second of pump time, while a client that asked
    // for two minutes is still there. Nothing in `cs_net` decides this — it is
    // read straight out of the connect token the pinned stack decodes.
    for (window, rounds, still_connected) in [
        (ConnectWindow::new(1), 256usize, false),
        (LOOPBACK_WINDOW, 256, true),
    ] {
        live_fixture(|| {
            let epoch = SessionAllocator::new()
                .allocate()
                .expect("an epoch allocates");
            let mut quiet = ServerSession::bind(
                epoch,
                synthetic_parameters(),
                SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                Duration::ZERO,
            )
            .expect("the host socket binds");
            let quiet_addr = quiet.local_addr().expect("the bound host has an address");
            let mut speaker = ClientSession::connect_with_window(
                synthetic_hello(),
                quiet_addr,
                0xD0,
                Duration::ZERO,
                window,
            )
            .expect("the client socket binds");

            for _ in 0..MAX_ROUNDS {
                speaker.pump(STEP);
                quiet.pump(STEP);
                if speaker.grant().is_some() || speaker.transport().disconnect_reason().is_some() {
                    break;
                }
            }
            if speaker.grant().is_none() {
                return Err(ungranted_client(&speaker, "the quiet-window handshake"));
            }

            // The host goes quiet: it is never pumped again, so it stops
            // sending and the connection layer's own silence window is what
            // runs.
            for _ in 0..rounds {
                speaker.pump(STEP);
            }
            if speaker.transport().is_connected() == still_connected {
                return Ok(());
            }
            // A connection still standing past its own one-second window is
            // the window arithmetic under test failing — a verdict. A
            // connection that went *down* inside a two-minute window cannot
            // be the window, so the path underneath is what died.
            assert!(
                still_connected,
                "after {rounds} silent rounds ({window:?} window) the connection layer's verdict is the window's"
            );
            Err(DeadPath(format!(
                "the {window:?} fixture's connection died underneath the quiet wait ({:?})",
                speaker.transport().disconnect_reason()
            )))
        });
    }
}

#[test]
fn accept_f54_c_the_session_runs_connect_launch_finish_and_disconnect() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("the first epoch allocates");
    live_fixture(|| {
        // Every `?` here is a wait the loopback had to carry; an `Err` rebuilds
        // the fixture, while a delivered-but-wrong answer panics and is never
        // retried.
        let mut link =
            Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))?;

        // connect: the handshake completed and both sides agree on the
        // identity.
        let grant = link
            .client
            .grant()
            .expect("the handshake granted a session");
        assert_eq!(grant.session, session);
        assert_eq!(link.host.phase(), ServerPhase::Gathering);
        assert_eq!(*link.client.phase(), ClientPhase::Joined);
        assert!(
            link.host_has(
                |notice| matches!(notice, ServerNotice::PeerJoined { peer } if *peer == grant.peer)
            ),
            "the host saw the join: {:?}",
            link.host_notices
        );

        // launch: the host publishes `Launched` and the client applies it.
        link.host
            .launch(Tick(100))
            .expect("a gathering session launches");
        assert_eq!(
            link.host.phase(),
            ServerPhase::Live {
                start_tick: Tick(100)
            }
        );
        link.try_pump_until_live()?;
        assert_eq!(
            *link.client.phase(),
            ClientPhase::Live {
                start_tick: Tick(100)
            }
        );

        // in flight: the client's local samples reach the simulation as
        // admitted input, once, with the fire request the gate authorized.
        let peer = grant.peer;
        let actor = ActorId { session, serial: 7 };
        link.host
            .gate_mut()
            .ownership_mut()
            .bind(peer, actor)
            .expect("the host binds the client's aircraft");
        link.client
            .submit_sample(FlightCommand::Throttle, 0.75, Tick(101))
            .expect("a finite throttle sample queues");
        link.client
            .submit_edge(FlightCommand::FirePrimary, Tick(102))
            .expect("a fire edge queues");
        assert_eq!(link.client.pending(), 2, "two frames are queued");
        link.try_pump_until_work()?;

        let work: Vec<PeerInput> = link.host.drain_work();
        assert_eq!(work.len(), 1, "one admitted packet reached the consumer");
        assert_eq!(work[0].peer, peer);
        assert_eq!(work[0].frames.frames.len(), 2);
        assert_eq!(
            work[0].fires.len(),
            1,
            "the gate authorized exactly the one fire edge"
        );
        assert_eq!(work[0].fires[0].actor, actor);
        assert_eq!(work[0].fires[0].tick, Tick(102));
        assert!(
            link.host.queued() <= MAX_WORK_PER_PUMP,
            "the work queue stays inside its cap"
        );

        // The host acknowledges the input it consumed and the client retires
        // it.
        link.host
            .acknowledge_input(peer, work[0].sequence)
            .expect("the ack encodes");
        link.try_pump_until_acked(work[0].sequence)?;
        assert_eq!(link.client.acked_through(), Some(work[0].sequence));
        assert_eq!(link.client.unacked(), 0, "the acked packet was retired");

        // finish: the host publishes `Finished` and the client applies it.
        link.host
            .finish(Tick(140), FinishReason::Completed)
            .expect("a live session finishes");
        assert_eq!(
            link.host.phase(),
            ServerPhase::Finished {
                reason: FinishReason::Completed
            }
        );
        link.try_pump_until("the client never applied the finish", |link| {
            matches!(link.client.phase(), ClientPhase::Finished { .. })
        })?;
        assert_eq!(
            *link.client.phase(),
            ClientPhase::Finished {
                reason: FinishReason::Completed
            }
        );

        // disconnect: the host tears the session down and the client observes
        // it.
        let hung_up = link
            .host
            .close(DisconnectReason::SessionEnded)
            .expect("teardown runs");
        assert_eq!(hung_up, 1, "the one member was hung up");
        assert_eq!(link.host.phase(), ServerPhase::Closed);
        assert!(link.host.members().next().is_none());
        link.try_pump_until_closed()?;
        assert!(
            matches!(
                link.client.phase().closure(),
                Some(ClientClosure::ServerClosed(DisconnectReason::SessionEnded))
                    | Some(ClientClosure::TransportLost(_))
            ),
            "the client observed the teardown: {:?}",
            link.client_notices
        );
        assert!(
            !link.client.phase().open(),
            "a closed client session sends nothing more"
        );
        assert!(
            link.client_has(|notice| matches!(notice, ClientNotice::Disconnected { .. }))
                || link.client_has(|notice| matches!(
                    notice,
                    ClientNotice::Phase(ClientPhase::Closed(_))
                )),
            "the close is reported, not silent: {:?}",
            link.client_notices
        );
        Ok(())
    });
}

#[test]
fn accept_f54_c_a_duplicate_packet_reaches_the_consumer_exactly_once() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    live_fixture(|| {
        let mut link =
            Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))?;
        let mut raw = link.raw_peer(0xC2);
        let peer = raw
            .try_handshake(&mut link.host)
            .ok_or_else(|| ungranted_peer(&raw, "the replaying peer's handshake"))?;
        let actor = ActorId { session, serial: 1 };
        link.host
            .gate_mut()
            .ownership_mut()
            .bind(peer, actor)
            .expect("the host binds an aircraft");

        // The exact same bytes three times: the replay the contract's
        // "reliable delivery does not replace application idempotency" exists
        // for. They go on the sequenced channel on purpose — the reliable
        // channel deduplicates at its own layer, which is exactly why
        // application-level idempotency still has to exist for the direction
        // that does not. Every copy is a droppable datagram, so the wait
        // refires the same bytes until all three were accounted for; each
        // extra arrival is only ever another named replay refusal.
        let bytes = fire_bytes(session, Tick(20), 0);
        for _ in 0..3 {
            raw.inject(CHANNEL_SEQUENCED, &bytes);
        }
        link.try_pump_peer_until(
            &mut raw,
            "the host never accounted for all three copies",
            |peer| peer.inject(CHANNEL_SEQUENCED, &bytes),
            |link| peer_arrivals(&link.host_notices, peer) >= 3,
        )?;
        link.pump(4);

        let work = link.host.drain_work();
        assert_eq!(
            work.len(),
            1,
            "a replayed packet is applied once, however often it arrives"
        );
        assert_eq!(work[0].fires.len(), 1, "and it fires exactly once");
        assert_eq!(work[0].fires[0].sequence, 0, "under its first sequence");
        assert!(
            link.host_has(|notice| matches!(
                notice,
                ServerNotice::Dropped {
                    reason: DropReason::Refused(violation),
                    ..
                } if violation.label() == "replayed_request"
            )),
            "the replays were refused and named: {:?}",
            link.host_notices
        );
        let admitted = link
            .host_notices
            .iter()
            .filter(|notice| matches!(notice, ServerNotice::Admitted(_)))
            .count();
        assert_eq!(admitted, 1, "exactly one delivery produced work");
        Ok(())
    });
}

#[test]
fn accept_f54_c_the_phase_machine_refuses_every_illegal_transition() {
    let _loopback = loopback();

    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let mut host = ServerSession::bind(
        session,
        synthetic_parameters(),
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        Duration::ZERO,
    )
    .expect("the host socket binds");

    // finish before launch is refused, and it names the phase it was in.
    assert_eq!(
        host.finish(Tick(1), FinishReason::Aborted),
        Err(ServerFault::WrongPhase {
            action: "finish",
            phase: ServerPhase::Gathering,
        })
    );
    // close is always legal, and is idempotent.
    assert_eq!(host.close(DisconnectReason::SessionEnded), Ok(0));
    assert_eq!(host.close(DisconnectReason::SessionEnded), Ok(0));
    assert_eq!(host.phase(), ServerPhase::Closed);
    // launch after teardown is refused.
    assert_eq!(
        host.launch(Tick(1)),
        Err(ServerFault::WrongPhase {
            action: "launch",
            phase: ServerPhase::Closed,
        })
    );
    // acknowledging input in a spent epoch is refused too.
    assert_eq!(
        host.acknowledge_input(PeerId::new(1).expect("one is nonzero"), 0),
        Err(ServerFault::WrongPhase {
            action: "acknowledge input",
            phase: ServerPhase::Closed,
        })
    );

    // A fresh epoch launches exactly once.
    let retry = SessionAllocator::new()
        .allocate()
        .expect("a retry epoch allocates");
    host.reopen(retry)
        .expect("the session reopens on a fresh epoch");
    assert_eq!(host.session(), retry);
    assert_eq!(host.phase(), ServerPhase::Gathering);
    assert!(host.launch(Tick(50)).is_ok());
    assert!(
        matches!(
            host.launch(Tick(51)),
            Err(ServerFault::WrongPhase {
                action: "launch",
                ..
            })
        ),
        "a launched session cannot launch again, got {:?}",
        host.phase()
    );
    assert!(
        host.finish(Tick(60), FinishReason::Completed).is_ok(),
        "and it can finish once"
    );
}

#[test]
fn accept_f54_c_a_retry_runs_on_a_fresh_epoch_and_the_old_one_is_stale() {
    let mut allocator = SessionAllocator::new();
    let first = allocator.allocate().expect("the first epoch allocates");
    let second = allocator.allocate().expect("the second epoch allocates");

    live_fixture(|| {
        let mut link = Link::try_joined(first)
            .map_err(|link| link.ungranted("the first client's handshake"))?;
        link.host
            .launch(Tick(10))
            .expect("the first epoch launches");
        link.try_pump_until_live()?;

        // The retry: a fresh epoch on the same address. Everything stamped
        // with the first epoch is stale by construction afterwards.
        link.host
            .reopen(second)
            .expect("the retry binds a fresh epoch");
        assert_eq!(link.host.session(), second);
        assert_eq!(link.host.phase(), ServerPhase::Gathering);
        assert!(
            link.host.members().next().is_none(),
            "the retry starts with an empty membership"
        );

        // A new client joins the new epoch and its grant names it.
        let mut returning = link.raw_peer(0xC3);
        let peer = returning
            .try_handshake(&mut link.host)
            .ok_or_else(|| ungranted_peer(&returning, "the returning client's handshake"))?;
        assert!(
            link.host.members().any(|member| member == peer),
            "the returning client is a member of the new epoch"
        );

        // The prior connection's packet, replayed verbatim against the new
        // epoch. The host must absorb it: the sequence is new to this epoch's
        // window, so only the epoch check can refuse it. The packet rides the
        // droppable channel, so the wait refires it until the refusal is
        // observable.
        let stale = fire_bytes(first, Tick(11), 0);
        returning.inject(CHANNEL_SEQUENCED, &stale);
        link.try_pump_peer_until(
            &mut returning,
            "the stale packet was never refused",
            |peer| peer.inject(CHANNEL_SEQUENCED, &stale),
            |link| {
                link.host_has(|notice| {
                    matches!(
                        notice,
                        ServerNotice::Dropped {
                            reason: DropReason::Refused(violation),
                            ..
                        } if violation.label() == "stale_session"
                    )
                })
            },
        )?;
        assert_eq!(
            link.host.drain_work().len(),
            0,
            "a packet stamped with the prior epoch applies to nothing"
        );

        // The retry hung up on the connection the first epoch was serving, so
        // the client that was live a moment ago observes the teardown rather
        // than waiting forever on a session nobody is serving.
        link.try_pump_until_closed()?;
        assert!(
            link.client.phase().closure().is_some(),
            "the prior client observed the retry's teardown: {:?}",
            link.client_notices
        );
        assert!(
            !link.client.transport().is_connected(),
            "and its connection is down"
        );
        Ok(())
    });
}

#[test]
fn accept_f54_c_the_retransmit_window_is_bounded_and_resends_the_exact_bytes() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    live_fixture(|| {
        let mut link =
            Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))?;

        // Sample input every round while the host is never pumped: nothing is
        // acknowledged, so the retransmit window is the only bound that holds.
        for index in 0..(MAX_UNACKED_PACKETS * 3) as u64 {
            link.client
                .submit_edge(FlightCommand::FirePrimary, Tick(200 + index))
                .expect("an edge queues");
            link.client.pump(STEP);
            assert!(
                link.client.unacked() <= MAX_UNACKED_PACKETS,
                "the retransmit window is bounded at {MAX_UNACKED_PACKETS}, saw {}",
                link.client.unacked()
            );
        }
        assert_eq!(
            link.client.unacked(),
            MAX_UNACKED_PACKETS,
            "and it fills to exactly its cap rather than past it"
        );
        assert!(
            link.client.acked_through().is_none(),
            "the host acknowledged nothing"
        );

        // An acknowledgment retires the window. Twenty-four packets went out,
        // so an acknowledgment through 24 covers every one the window still
        // holds.
        let peer = link.client.grant().expect("a grant exists").peer;
        link.host
            .acknowledge_input(peer, 24)
            .expect("the ack encodes");
        link.try_pump_until_acked(24)?;
        assert_eq!(link.client.acked_through(), Some(24));
        assert_eq!(
            link.client.unacked(),
            0,
            "acknowledgment retired the covered packets"
        );
        Ok(())
    });
}

#[test]
fn accept_f54_c_a_lost_input_packet_is_retransmitted_verbatim() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    live_fixture(|| {
        let mut link =
            Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))?;
        link.host
            .gate_mut()
            .ownership_mut()
            .bind(
                link.client.grant().expect("a grant exists").peer,
                ActorId { session, serial: 1 },
            )
            .expect("the host binds an aircraft");

        link.client
            .submit_edge(FlightCommand::FirePrimary, Tick(300))
            .expect("an edge queues");
        // Pump the client alone so the host never sees the packet: it is
        // "lost".
        link.client.pump(STEP);
        assert_eq!(link.client.unacked(), 1, "one packet is unacknowledged");
        assert_eq!(link.host.queued(), 0, "the host has not applied it");

        // With nothing newer to send, the client resends after the retry
        // window. These client-only rounds ask nothing of the loopback.
        let mut retried = None;
        for _ in 0..64 {
            let notices = link.client.pump(INPUT_RETRY_INTERVAL);
            if let Some(sequence) = notices.iter().find_map(|notice| match notice {
                ClientNotice::Retried { sequence } => Some(*sequence),
                _ => None,
            }) {
                retried = Some(sequence);
                break;
            }
        }
        assert_eq!(retried, Some(0), "the lost packet was retransmitted");

        // The host applies it once, under the sequence it was first stamped
        // with, so a further retransmission of the same bytes is still
        // identifiable as a replay. The retry is retransmitted by the client
        // itself, so the wait needs no refire.
        link.try_pump_until_work()?;
        link.pump(4);
        let work = link.host.drain_work();
        assert_eq!(work.len(), 1, "the retry reached the consumer");
        assert_eq!(
            work[0].sequence, 0,
            "the retry kept its sequence, so a replay is still identifiable"
        );
        assert_eq!(
            work[0].fires.len(),
            1,
            "and it authorizes its one fire edge"
        );
        Ok(())
    });
}

#[test]
fn accept_f54_c_the_work_queue_refuses_the_surplus_and_cuts_the_abusive_peer() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    live_fixture(|| {
        let mut link =
            Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))?;
        let mut raw = link.raw_peer(0xC4);
        let peer = raw
            .try_handshake(&mut link.host)
            .ok_or_else(|| ungranted_peer(&raw, "the flooding peer's handshake"))?;
        link.host
            .gate_mut()
            .ownership_mut()
            .bind(peer, ActorId { session, serial: 1 })
            .expect("the host binds an aircraft");

        // One packet more than the queue can hold, all delivered inside a
        // single host pump. The queue is never drained, so arrivals
        // accumulate across pumps: the overflow fires on the cap+1-th packet
        // to land whenever that is. The channel drops datagrams, so the wait
        // refires a fresh burst — fresh sequences, since a repeated one is a
        // named replay that cannot queue — until the surplus is observable.
        let mut next = 0u32;
        let mut burst = |peer: &mut RawPeer| {
            for _ in 0..=MAX_WORK_PER_PUMP {
                peer.inject(
                    CHANNEL_SEQUENCED,
                    &fire_bytes(session, Tick(400 + u64::from(next)), next),
                );
                next = next.wrapping_add(1);
            }
        };
        burst(&mut raw);
        link.try_pump_peer_until(
            &mut raw,
            "the queue never overflowed",
            |peer| burst(peer),
            |link| link.host.overflowed() > 0,
        )?;

        assert_eq!(
            link.host.queued(),
            MAX_WORK_PER_PUMP,
            "the queue never grows past its cap"
        );
        assert!(
            link.host.overflowed() > 0,
            "the surplus was refused, not queued"
        );
        assert!(
            link.host_has(|notice| matches!(
                notice,
                ServerNotice::Dropped {
                    reason: DropReason::QueueOverflow { limit },
                    ..
                } if *limit == MAX_WORK_PER_PUMP
            )),
            "the refusal is named: {:?}",
            link.host_notices
        );
        assert!(
            link.host_has(|notice| matches!(
                notice,
                ServerNotice::CutOff {
                    threat: ThreatCase::ResourceExhaustion,
                    ..
                }
            )),
            "the abusive peer was cut off: {:?}",
            link.host_notices
        );
        assert!(
            link.host.members().all(|member| member != peer),
            "the cut-off peer is no longer a member"
        );
        assert_eq!(
            link.host.drain_work().len(),
            MAX_WORK_PER_PUMP,
            "exactly the capped number of packets were handed over"
        );
        Ok(())
    });
}

// ----------------------------------------------------------- client accept --

/// Builds a client that already holds a grant, without keeping the host: the
/// consumer is a pure function of the decoded message, so the adversarial
/// corpus can drive it directly.
///
/// The handshake is the only delivery this fixture needs: once the grant is
/// in, the host is dropped and `accept` drives the client without another
/// packet. A handshake the loopback never carried is rebuilt by
/// [`live_fixture`]; a delivered verdict still panics on the spot.
fn granted_client(session: SessionId) -> (ClientSession, MutexGuard<'static, ()>) {
    live_fixture(|| {
        let loopback = loopback();
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let mut host = ServerSession::bind(session, synthetic_parameters(), bind, Duration::ZERO)
            .expect("the host socket binds");
        let addr = host.local_addr().expect("the bound host has an address");
        let mut client = ClientSession::connect_with_window(
            synthetic_hello(),
            addr,
            0xC5,
            Duration::ZERO,
            LOOPBACK_WINDOW,
        )
        .expect("the client socket binds");
        for _ in 0..MAX_ROUNDS {
            if client.grant().is_some() {
                return Ok((client, loopback));
            }
            // A settled answer or a declared dead end can only ever stay so.
            if client.phase().closure().is_some()
                || client.transport().disconnect_reason().is_some()
            {
                break;
            }
            client.pump(STEP);
            host.pump(STEP);
        }
        if client.grant().is_some() {
            Ok((client, loopback))
        } else {
            Err(ungranted_client(&client, "the consumer's handshake"))
        }
    })
}

#[test]
fn accept_f54_c_a_replayed_reliable_event_is_applied_once() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);
    let peer = PeerId::new(2).expect("two is nonzero");
    let event = fuzz::joined_event(session, peer, 0);

    let first = client
        .accept(fuzz::event_message(session, 0, event.clone()))
        .expect("a live epoch is accepted");
    assert!(
        first
            .iter()
            .any(|notice| matches!(notice, ClientNotice::Event { .. }))
    );

    // The reliable channel can replay a request after a retry; the id is what
    // stops it taking effect twice.
    let replay = client
        .accept(fuzz::event_message(session, 1, event))
        .expect("the replay decodes");
    assert!(
        replay
            .iter()
            .any(|notice| matches!(notice, ClientNotice::Duplicate { .. })),
        "the replay was recognized: {replay:?}"
    );
    assert!(
        !replay
            .iter()
            .any(|notice| matches!(notice, ClientNotice::Event { .. })),
        "the replay took no effect"
    );
    assert_eq!(client.remembered_events(), 1, "one id is remembered");

    // A different event under the same envelope is new and is applied.
    let other = fuzz::joined_event(session, PeerId::new(3).expect("three is nonzero"), 1);
    let applied = client
        .accept(fuzz::event_message(session, 2, other))
        .expect("a distinct id is accepted");
    assert!(
        applied
            .iter()
            .any(|notice| matches!(notice, ClientNotice::Event { .. }))
    );
    assert_eq!(client.remembered_events(), 2);
}

#[test]
fn accept_f54_c_the_client_phase_machine_follows_the_hosts_events() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);
    assert_eq!(*client.phase(), ClientPhase::Joined);

    let notices = client
        .accept(fuzz::event_message(
            session,
            0,
            fuzz::launched_event(session, Tick(1000), 0),
        ))
        .expect("a live epoch is accepted");
    assert!(
        notices.iter().any(|notice| matches!(
            notice,
            ClientNotice::Phase(ClientPhase::Live {
                start_tick: Tick(1000)
            })
        )),
        "the launch applied: {notices:?}"
    );
    assert_eq!(
        *client.phase(),
        ClientPhase::Live {
            start_tick: Tick(1000)
        }
    );

    // A launch replay under a *new* envelope sequence and a new event id is a
    // second fact the server may legitimately publish; the phase machine takes
    // the newest one and says so.
    let notices = client
        .accept(fuzz::event_message(
            session,
            1,
            fuzz::launched_event(session, Tick(1100), 1),
        ))
        .expect("a live epoch is accepted");
    assert!(
        notices.iter().any(|notice| matches!(
            notice,
            ClientNotice::Phase(ClientPhase::Live {
                start_tick: Tick(1100)
            })
        )),
        "a newer launch is applied: {notices:?}"
    );

    let notices = client
        .accept(fuzz::event_message(
            session,
            2,
            fuzz::finished_event(session, Tick(1200), FinishReason::Aborted),
        ))
        .expect("a live epoch is accepted");
    assert!(
        notices.iter().any(|notice| matches!(
            notice,
            ClientNotice::Phase(ClientPhase::Finished {
                reason: FinishReason::Aborted
            })
        )),
        "the finish applied: {notices:?}"
    );
    assert!(
        client.phase().open(),
        "a finished session may still say farewell"
    );
}

#[test]
fn accept_f54_c_the_event_memory_is_bounded_and_evicts_the_oldest() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);
    let cap = fuzz::EVENT_MEMORY_CAP;

    for index in 0..(cap + 8) {
        let event = fuzz::joined_event(
            session,
            PeerId::new(u16::try_from(index % 30 + 2).expect("small")).expect("nonzero"),
            u32::try_from(index).expect("small"),
        );
        client
            .accept(fuzz::event_message(session, 0, event))
            .expect("a live epoch is accepted");
        assert!(
            client.remembered_events() <= cap,
            "the dedup memory stays at {cap}, saw {}",
            client.remembered_events()
        );
    }
    assert_eq!(client.remembered_events(), cap);
    assert_eq!(cap, MAX_SEEN_EVENTS, "the cap is the declared bound");

    // The oldest id was evicted, so it is applied again rather than silently
    // suppressed forever; the newest is still remembered.
    let oldest = fuzz::joined_event(session, PeerId::new(2).expect("two is nonzero"), 0);
    let notices = client
        .accept(fuzz::event_message(session, 1, oldest))
        .expect("a live epoch is accepted");
    assert!(
        notices
            .iter()
            .any(|notice| matches!(notice, ClientNotice::Event { .. })),
        "the evicted id is no longer suppressed: {notices:?}"
    );
    let newest = fuzz::joined_event(
        session,
        PeerId::new(2).expect("two is nonzero"),
        u32::try_from(cap + 7).expect("small"),
    );
    let notices = client
        .accept(fuzz::event_message(session, 2, newest))
        .expect("a live epoch is accepted");
    assert!(
        notices
            .iter()
            .any(|notice| matches!(notice, ClientNotice::Duplicate { .. })),
        "the newest id is still remembered: {notices:?}"
    );
}

#[test]
fn accept_f54_c_only_the_newest_snapshot_survives() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);

    let newest = SnapshotFrame {
        tick: Tick(500),
        payload: fuzz::synthetic_snapshot(session, [10.0, 20.0, 30.0])
            .encode(session)
            .expect("the synthetic snapshot encodes"),
    };
    client
        .accept(fuzz::snapshot_message(session, 0, newest))
        .expect("a live epoch is accepted");
    assert_eq!(client.snapshot_tick(), Some(Tick(500)));
    let held = client.snapshot().expect("a snapshot is held");
    assert_eq!(held.actors.len(), 1);
    let position = held.actors[0]
        .position_m()
        .expect("the record is inside its declared range");
    assert!(
        position.iter().all(|value| value.is_finite()),
        "a decoded position is finite: {position:?}"
    );
    assert!(
        (position[0] - 10.0).abs() < 0.01 && (position[1] - 20.0).abs() < 0.01,
        "and it is the snapshot that was sent: {position:?}"
    );

    // An older snapshot is sequenced-and-droppable: it must not overwrite the
    // newer frame.
    let older = SnapshotFrame {
        tick: Tick(499),
        payload: fuzz::synthetic_snapshot(session, [-1.0, -2.0, -3.0])
            .encode(session)
            .expect("the synthetic snapshot encodes"),
    };
    let notices = client
        .accept(fuzz::snapshot_message(session, 1, older))
        .expect("a live epoch is accepted");
    assert!(
        notices.iter().any(
            |notice| matches!(notice, ClientNotice::StaleSnapshot { tick } if *tick == Tick(499))
        ),
        "the older snapshot was refused: {notices:?}"
    );
    assert_eq!(client.snapshot_tick(), Some(Tick(500)), "still the newest");
    let held = client.snapshot().expect("a snapshot is still held");
    assert!(
        held.actors[0].position_m().expect("in range")[0] > 0.0,
        "the newer frame was not overwritten"
    );
}

#[test]
fn accept_f54_c_a_stale_epoch_snapshot_is_refused_before_it_is_decoded() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let dead = SessionId::new(session.get() + 1).expect("nonzero");
    let (mut client, _loopback) = granted_client(session);

    let frame = SnapshotFrame {
        tick: Tick(600),
        payload: fuzz::synthetic_snapshot(session, [1.0, 2.0, 3.0])
            .encode(session)
            .expect("the synthetic snapshot encodes"),
    };
    let message = ServerMessage {
        header: MessageHeader {
            session: dead,
            sequence: 0,
        },
        payload: ServerPayload::Snapshot(frame),
    };
    assert!(
        matches!(
            client.accept(message),
            Err(ClientFault::Wire(WireError::StaleSession { .. }))
        ),
        "a packet from a dead epoch is refused by epoch, not by content"
    );
    assert!(client.snapshot().is_none(), "and nothing was stored");
}

#[test]
fn accept_f54_c_a_malformed_snapshot_payload_is_refused_and_names_the_field() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);

    // Bytes that are well formed at the envelope but not a snapshot schema.
    for (label, payload) in [
        ("random bytes", vec![0xFF; 32]),
        ("an empty payload", Vec::new()),
        ("a truncated valid payload", {
            let valid = fuzz::synthetic_snapshot(session, [1.0, 2.0, 3.0])
                .encode(session)
                .expect("the synthetic snapshot encodes");
            valid[..valid.len() - 8].to_vec()
        }),
    ] {
        let outcome = client.accept(fuzz::snapshot_message(
            session,
            0,
            SnapshotFrame {
                tick: Tick(700),
                payload: payload.clone(),
            },
        ));
        assert!(
            matches!(outcome, Err(ClientFault::Snapshot(ref reason)) if !reason.to_string().is_empty()),
            "{label} must be refused with a named reason, saw {outcome:?}"
        );
        assert!(client.snapshot().is_none(), "{label} was not stored");
    }
}

#[test]
fn accept_f54_c_an_oversized_snapshot_payload_is_refused_before_it_is_decoded() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);

    // The envelope's own cap. A client cannot even be handed a payload past
    // `MAX_SNAPSHOT_BYTES`, which is what keeps one hostile peer from making
    // every client allocate.
    let payload = vec![0u8; MAX_SNAPSHOT_BYTES + 1];
    let message = ServerMessage {
        header: MessageHeader {
            session,
            sequence: 0,
        },
        payload: ServerPayload::Snapshot(SnapshotFrame {
            tick: Tick(800),
            payload,
        }),
    };
    assert!(
        matches!(
            client.accept(message),
            Err(ClientFault::Wire(WireError::TooLarge { .. }))
        ),
        "an oversized snapshot payload is refused by the wire bounds"
    );
    assert!(client.snapshot().is_none());

    // And the host refuses to publish one in the first place.
    let mut host = ServerSession::bind(
        session,
        synthetic_parameters(),
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        Duration::ZERO,
    )
    .expect("the host socket binds");
    let mut oversized = fuzz::synthetic_snapshot(session, [0.0, 0.0, 0.0]);
    // An actor record past the population cap is the reachable way to make an
    // unencodable snapshot.
    let record = *oversized
        .actors
        .first()
        .expect("the synthetic snapshot has one record");
    oversized.actors = vec![record; cs_net::snapshot::MAX_ACTORS_PER_SNAPSHOT + 1];
    assert!(
        matches!(
            host.publish_snapshot(Tick(801), &oversized),
            Err(ServerFault::Encode(_))
        ),
        "an over-populated snapshot is refused with a named reason"
    );
    assert!(
        host.launch(Tick(802)).is_ok(),
        "and the failure did not corrupt the session"
    );
}

#[test]
fn accept_f54_c_a_non_acknowledging_host_does_not_grow_the_client() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);

    for index in 0..40u64 {
        client
            .submit_edge(FlightCommand::FirePrimary, Tick(800 + index))
            .expect("an edge queues");
        client.pump(STEP);
        assert!(client.unacked() <= MAX_UNACKED_PACKETS);
    }
    assert_eq!(
        client.unacked(),
        MAX_UNACKED_PACKETS,
        "the retransmit window is bounded no matter what the host does"
    );
    assert!(client.acked_through().is_none(), "nothing was acknowledged");

    // The acknowledgment path retires exactly the packets it covers. Forty
    // packets went out and the window kept the newest eight, so an
    // acknowledgment through 40 must empty it.
    client
        .accept(ServerMessage {
            header: MessageHeader {
                session,
                sequence: 0,
            },
            payload: ServerPayload::InputAck { through: 40 },
        })
        .expect("a live epoch is accepted");
    assert_eq!(client.acked_through(), Some(40));
    assert_eq!(client.unacked(), 0, "the window emptied: 40 covered all");

    // An acknowledgment that does not advance is refused.
    let notices = client
        .accept(ServerMessage {
            header: MessageHeader {
                session,
                sequence: 1,
            },
            payload: ServerPayload::InputAck { through: 1 },
        })
        .expect("a live epoch is accepted");
    assert!(
        notices.iter().any(|notice| matches!(
            notice,
            ClientNotice::StaleAck {
                through: 1,
                acknowledged: 40
            }
        )),
        "a stale acknowledgment is refused: {notices:?}"
    );
    assert_eq!(
        client.acked_through(),
        Some(40),
        "the window did not move back"
    );
}

#[test]
fn accept_f54_c_a_local_sample_that_is_not_finite_never_becomes_a_packet() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    live_fixture(|| {
        let mut link =
            Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))?;

        // Every non-finite f32 the corpus uses, driven straight into the
        // producer.
        for value in [
            f32::NAN,
            -f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::from_bits(0x7FA0_0000),
            f32::from_bits(0xFFC0_0000),
        ] {
            let outcome = link
                .client
                .submit_sample(FlightCommand::Throttle, value, Tick(900));
            assert!(
                matches!(
                    outcome,
                    Err(ClientFault::Axis(AxisValueError::NonFinite { .. }))
                ),
                "a non-finite sample must be refused, got {outcome:?}"
            );
        }
        // Out of the normalized range is refused too, not clamped silently.
        assert!(matches!(
            link.client
                .submit_sample(FlightCommand::Throttle, 1.5, Tick(901)),
            Err(ClientFault::Axis(AxisValueError::OutOfRange { .. }))
        ));
        // An edge command is not an axis.
        assert!(matches!(
            ClientSession::sample_axis(FlightCommand::FirePrimary, 0.0),
            Err(ClientFault::Axis(AxisValueError::NotContinuous { .. }))
        ));

        assert_eq!(
            link.client.pending(),
            0,
            "no refused sample was queued, so none can reach the wire"
        );
        link.pump(4);
        assert_eq!(
            link.host.drain_work().len(),
            0,
            "and nothing arrived at the host"
        );

        // A finite sample still works, so the check is not a blanket refusal.
        link.client
            .submit_sample(FlightCommand::Throttle, 0.5, Tick(902))
            .expect("a finite sample queues");
        link.try_pump_until_work()?;
        let work = link.host.drain_work();
        assert_eq!(work.len(), 1, "the finite sample reached the consumer");
        let axis = work[0].frames.frames[0]
            .axis(FlightCommand::Throttle)
            .expect("the throttle sample crossed the wire");
        assert!(axis.as_unit().is_finite(), "the wire value is finite");
        assert!(
            (axis.as_unit() - 0.5).abs() < 0.001,
            "and it kept its value: {}",
            axis.as_unit()
        );
        Ok(())
    });
}

#[test]
fn accept_f54_c_the_client_refuses_a_backwards_tick_and_a_full_queue() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);

    client
        .submit_edge(FlightCommand::FirePrimary, Tick(50))
        .expect("a first tick queues");
    // A tick before the queued one would break the batch's strict order and
    // fail the wire validation, so the producer refuses it.
    assert_eq!(
        client.submit_edge(FlightCommand::FirePrimary, Tick(49)),
        Err(ClientFault::TickNotNewer {
            tick: Tick(49),
            previous: Tick(50),
        })
    );
    // The same tick merges into the frame already there.
    client
        .submit_sample(FlightCommand::Throttle, 0.25, Tick(50))
        .expect("the same tick extends the frame");
    assert_eq!(client.pending(), 1, "still one frame");

    // Fill the queue to its cap.
    for index in 1..MAX_INPUT_FRAMES_PER_PACKET as u64 {
        client
            .submit_edge(FlightCommand::FirePrimary, Tick(50 + index))
            .expect("a later tick queues");
    }
    assert_eq!(client.pending(), MAX_INPUT_FRAMES_PER_PACKET);
    assert_eq!(
        client.submit_edge(FlightCommand::FirePrimary, Tick(999)),
        Err(ClientFault::InputQueueFull {
            max: MAX_INPUT_FRAMES_PER_PACKET
        }),
        "the queue is capped at the packet's own bound"
    );
}

#[test]
fn accept_f54_c_the_client_leaves_reliably_and_the_host_departs_it() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    live_fixture(|| {
        let mut link =
            Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))?;
        let peer = link.client.grant().expect("a grant exists").peer;

        link.client.leave().expect("the farewell sends");
        assert!(matches!(
            *link.client.phase(),
            ClientPhase::Closed(ClientClosure::LeftVoluntarily)
        ));
        // A second farewell is refused, not sent twice.
        assert!(matches!(
            link.client.leave(),
            Err(ClientFault::Closed { .. })
        ));
        // And no input is accepted after the farewell.
        assert!(matches!(
            link.client.submit_edge(FlightCommand::FirePrimary, Tick(1)),
            Err(ClientFault::Closed { .. })
        ));

        // The departure has to arrive before the host's own netcode window
        // would retire the connection anyway — a `PeerLeft` produced by the
        // window says nothing about the farewell — so the budget stays far
        // short of it. On expiry the client's connection is down by its own
        // leave(), so a member still standing means nothing was delivered.
        for _ in 0..200 {
            link.round();
            if !link.host.members().any(|member| member == peer) {
                break;
            }
        }
        if link.host.members().any(|member| member == peer) {
            assert!(
                !link.client.transport().is_connected(),
                "the host keeps a member whose connection is still up: {:?}",
                link.host_notices
            );
            return Err(DeadPath(format!(
                "the client's connection is down but its farewell never reached the host: {:?}",
                link.host_notices
            )));
        }
        assert!(
            link.host.members().next().is_none(),
            "the host departed the peer that left: {:?}",
            link.host_notices
        );
        assert!(
            link.host_has(|notice| matches!(notice, ServerNotice::PeerLeft { .. })),
            "the departure is reported: {:?}",
            link.host_notices
        );
        assert!(
            !link.host.gate().is_member(peer),
            "and its replay window died with it"
        );
        Ok(())
    });
}

#[test]
fn accept_f54_c_a_refused_client_is_told_why_and_then_hung_up_on() {
    let _loopback = loopback();

    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let mut hello = synthetic_hello();
    hello.compatibility.rules_sha256 = ContentHash::from_bytes([0x00; 32]);
    let (mut host, mut client, mut seen) = live_fixture(|| {
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let mut host = ServerSession::bind(session, synthetic_parameters(), bind, Duration::ZERO)
            .expect("the host socket binds");
        let addr = host.local_addr().expect("the bound host has an address");
        let mut client = ClientSession::connect_with_window(
            hello.clone(),
            addr,
            0xC6,
            Duration::ZERO,
            LOOPBACK_WINDOW,
        )
        .expect("the client socket binds");
        let mut seen: Vec<ServerNotice> = Vec::new();

        for _ in 0..MAX_ROUNDS {
            if client.phase().closure().is_some() {
                break;
            }
            client.pump(STEP);
            seen.extend(host.pump(STEP));
        }
        // The refusal is a delivered verdict and the assert below is its only
        // judge; what may be rebuilt is a handshake whose answer the loopback
        // never carried — which `silent_client` proves from the connection
        // layer's own state.
        if let Some(dead) = silent_client(&client, "the refused handshake") {
            return Err(dead);
        }
        Ok((host, client, seen))
    });
    assert!(
        matches!(
            client.phase().closure(),
            Some(ClientClosure::Refused(
                HandshakeReject::RulesMismatch { .. }
            ))
        ),
        "the client holds the named reason: {:?}",
        client.phase()
    );
    assert!(client.grant().is_none(), "and it never became a peer");
    assert!(
        !client.phase().open(),
        "a refused session sends nothing more"
    );

    // The host hangs up on the refused connection: it holds no peer id, so it
    // must not keep occupying one of the session's netcode slots. The refusal
    // itself is already observed, so everything the host does next is its own
    // bookkeeping — this wait asks nothing more of the loopback.
    for _ in 0..MAX_ROUNDS {
        seen.extend(host.pump(STEP));
        if seen
            .iter()
            .any(|notice| matches!(notice, ServerNotice::HungUp { .. }))
        {
            break;
        }
        client.pump(STEP);
    }
    assert!(
        seen.iter()
            .any(|notice| matches!(notice, ServerNotice::HungUp { .. })),
        "the refused connection was hung up on; the host saw {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|notice| matches!(notice, ServerNotice::PeerRefused { .. })),
        "and the refusal was reported with its reason: {seen:?}"
    );
    assert!(host.members().next().is_none(), "it never became a member");
}

#[test]
fn accept_f54_c_a_handshake_answer_that_cannot_be_sent_is_reported_not_swallowed() {
    let _loopback = loopback();

    // Two disjoint sets of maximum-length mod ids: the named rejection needs
    // 128 catalog ids on the wire, which does not fit a packet. The refusal
    // must surface as a reported fault and a hang-up, and the client must never
    // become a peer.
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let params = SessionParameters {
        compatibility: Compatibility {
            rules_sha256: SYNTHETIC_RULES_SHA256,
            content_sha256: SYNTHETIC_CONTENT_SHA256,
            mods: (0..MAX_MODS).map(long_mod).collect(),
        },
    };
    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));

    // The client's own hello stays inside the packet cap; only the *reply*
    // cannot.
    let mut hello = synthetic_hello();
    hello.compatibility.mods = (MAX_MODS..MAX_MODS * 2).map(long_mod).collect();
    let client_bytes = encode_client_packet(&ClientPacket::Hello(hello.clone()))
        .expect("the client's own hello fits a packet");
    assert!(
        client_bytes.len() <= MAX_PACKET_BYTES,
        "the hello must be sendable, or the test proves nothing"
    );

    let (host, raw, reported) = live_fixture(|| {
        let mut host = ServerSession::bind(session, params.clone(), bind, Duration::ZERO)
            .expect("the host socket binds");
        let addr = host.local_addr().expect("the bound host has an address");
        let mut raw = RawPeer {
            transport: ClientTransport::connect_with_window(
                hello.clone(),
                addr,
                0xC7,
                Duration::ZERO,
                LOOPBACK_WINDOW,
            )
            .expect("the client socket binds"),
            events: Vec::new(),
        };
        let mut reported = false;
        let mut seen: Vec<ServerNotice> = Vec::new();
        for _ in 0..MAX_ROUNDS {
            let notices = host.pump(STEP);
            if notices.iter().any(|notice| {
                matches!(notice, ServerNotice::TransportFault { reason } if reason.contains("is") && reason.contains("bytes, max is"))
            }) {
                reported = true;
            }
            let hung_up = notices
                .iter()
                .any(|notice| matches!(notice, ServerNotice::HungUp { .. }));
            seen.extend(notices);
            if hung_up {
                break;
            }
            if peer_path_dead(&raw) {
                break;
            }
            raw.pump();
        }
        if !seen
            .iter()
            .any(|notice| matches!(notice, ServerNotice::HungUp { .. }))
        {
            return Err(peer_delivery(
                &raw,
                "the unencodable answer's hang-up",
                &seen,
            ));
        }
        Ok((host, raw, reported))
    });
    assert!(
        reported,
        "an unencodable handshake answer is reported, not swallowed; the host saw {host:?}"
    );
    assert!(
        host.members().next().is_none(),
        "and the client never became a peer"
    );
    assert!(
        raw.transport.grant().is_none() && raw.transport.rejection().is_none(),
        "the client holds neither a grant nor a reason, because neither was sent"
    );
}

/// How long the client is pumped before the round under test, expressed as one
/// hop past the window its own connect token carries.
///
/// The pinned layer accumulates its timeout purely from the `elapsed` a caller
/// hands to `update` (see [`LOOPBACK_WINDOW`]), so a single round of one second
/// more than that window *is* the timeout — no wall clock, no sleep, and no
/// dependence on how fast this machine is. One second is the margin: the layer
/// compares `last_received + window < now`, and `now` has just grown by
/// `window + 1`.
fn overdue_round() -> Duration {
    let window = u64::try_from(LOOPBACK_WINDOW.const_seconds())
        .expect("a window is a positive number of seconds");
    Duration::from_secs(window + 1)
}

/// The round every session-level send-path test below shares: pump `link`'s
/// client until it has nothing left to report, then hand it one
/// [`overdue_round`], whose refusal of the round's send is that round's only
/// report and leaves the session open.
///
/// The quiet rounds' notices are kept on `link` the way [`Link::round`] keeps
/// them, so a failing assertion still prints the whole history rather than the
/// round it happened to stop on.
fn quiet_then_overdue(link: &mut Link) -> Vec<ClientNotice> {
    let mut quiet = Vec::new();
    for _ in 0..16 {
        quiet = link.client.pump(STEP);
        if quiet.is_empty() {
            break;
        }
        link.client_notices.extend(quiet.iter().cloned());
    }
    assert!(
        quiet.is_empty(),
        "the fixture went quiet before the round under test: {quiet:?}"
    );
    let notices = link.client.pump(overdue_round());
    link.client_notices.extend(notices.iter().cloned());
    notices
}

#[test]
fn accept_f54_c_a_client_send_that_never_left_is_reported_not_swallowed() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (_link, mut peer) = live_fixture(|| {
        let mut link =
            Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))?;
        let mut peer = link.raw_peer(0xC8);
        peer.try_handshake(&mut link.host)
            .ok_or_else(|| ungranted_peer(&peer, "the raw peer's handshake"))?;
        Ok((link, peer))
    });

    // Leave the socket quiet first, so the round under test has only its own
    // verdict to report. Sixteen steps is a bound, not a wait: the host is
    // never pumped again, so nothing new can arrive, and 16 * 16 ms is far
    // inside the window the timeout below depends on.
    let mut quiet = Vec::new();
    for _ in 0..16 {
        quiet = peer.transport.update(STEP);
        if quiet.is_empty() {
            break;
        }
    }
    assert!(
        quiet.is_empty(),
        "the fixture went quiet before the round under test: {quiet:?}"
    );

    // With the host silent, this round exhausts the connection window *inside*
    // the transport's own update: the netcode layer records the timeout while
    // `update` still reports success, and the send that follows it is refused
    // after the packets it carried were already popped from renet's queues.
    // Nothing else happened this round, so this refusal is its only report —
    // the exact case the swallowed `let _ =` hid.
    let events = peer.transport.update(overdue_round());
    assert!(
        matches!(
            events.as_slice(),
            [ClientEvent::SendFault { reason }] if reason.contains("connection timed out")
        ),
        "the refused send is reported and nothing else happened this round: {events:?}"
    );

    // The connection layer states the same death on the *next* round, where
    // `transport.update` sees it first. So the event above was not a second
    // voice for a fault already reported: it was the first one — and it stays
    // quiet afterwards instead of restating it every round.
    let following = peer.transport.update(STEP);
    assert!(
        matches!(
            following.as_slice(),
            [
                ClientEvent::TransportFault { .. },
                ClientEvent::Disconnected { .. }
            ]
        ),
        "the layer reports the dead connection on the following round: {following:?}"
    );

    assert!(
        peer.transport.disconnect().is_err(),
        "and the hang-up returns the connection layer's verdict instead of discarding it"
    );
}

#[test]
fn accept_f54_c_a_refused_send_reaches_the_session_owner_as_a_notice() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let mut link = live_fixture(|| {
        Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))
    });

    // Same round as `accept_f54_c_a_client_send_that_never_left_is_reported_not_swallowed`,
    // through the session owner an app actually drives.
    let notices = quiet_then_overdue(&mut link);
    assert!(
        matches!(
            notices.as_slice(),
            [ClientNotice::Dropped {
                reason: ClientFault::Transport(text)
            }] if text.contains("connection timed out")
        ),
        "the send path's refusal reaches the caller as a named notice: {notices:?}"
    );
    assert!(
        link.client.phase().open(),
        "a report is not a session decision: the phase is {:?}",
        link.client.phase()
    );

    // Teardown still comes from the connection layer's own verdict, on the
    // round after — reporting the send did not close anything early and did
    // not hide the closure either.
    link.client_round();
    assert!(
        matches!(
            link.client.phase(),
            ClientPhase::Closed(ClientClosure::TransportLost(_))
        ),
        "the layer's verdict still closes the session: {:?}",
        link.client_notices
    );
}

/// The farewell's own report: [`ClientSession::leave`] used to discard the
/// pinned layer's verdict on the hang-up, so a caller could not tell a clean
/// hang-up from one the layer had already refused — and this is the round in
/// which that verdict exists while the session is still open.
#[test]
fn accept_f54_c_leaving_after_the_layer_gave_up_reports_the_hang_up() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let mut link = live_fixture(|| {
        Link::try_joined(session).map_err(|link| link.ungranted("the client's handshake"))
    });

    let notices = quiet_then_overdue(&mut link);
    assert!(
        matches!(notices.as_slice(), [ClientNotice::Dropped { .. }]),
        "the round under test reported the send path: {notices:?}"
    );
    assert!(
        link.client.phase().open(),
        "the session is still open before the farewell: {:?}",
        link.client.phase()
    );

    // The farewell queues what renet has already stopped carrying and then
    // hangs up; the layer's own answer to that hang-up is this call's error.
    let verdict = link.client.leave();
    assert!(
        matches!(
            &verdict,
            Err(ClientFault::Transport(reason)) if reason.contains("connection timed out")
        ),
        "the hang-up carries the layer's verdict instead of discarding it: {verdict:?}"
    );
    assert!(
        matches!(
            link.client.phase(),
            ClientPhase::Closed(ClientClosure::LeftVoluntarily)
        ),
        "and a failed report never leaves the session open: {:?}",
        link.client.phase()
    );
    assert!(
        matches!(link.client.leave(), Err(ClientFault::Closed { .. })),
        "so the session is over for every later call too: {:?}",
        link.client.phase()
    );
}

/// A catalog id whose wire form is as long as the id grammar allows, so that
/// [`MAX_MODS`] of them are individually sendable while two lists of them are
/// not.
fn long_mod(index: usize) -> ContentId {
    ContentId::from_source(
        ContentKind::Blueprint,
        &format!("m{index:04}{}", "x".repeat(120)),
    )
    .expect("the key satisfies the id grammar")
}

// ----------------------------------------------------------------- fuzzing --

/// The acceptance scenario: spec F54 AC03, "Fuzz packet decoding with
/// oversized counts, NaNs and invalid ids".
///
/// The corpus is deterministic (fixed seeds) and every buffer goes through the
/// production decoder. Three properties are under test:
///
/// 1. **nothing panics** — the decoder and the consumer are total;
/// 2. **every refusal is named** — the error's `Display` names the field or the
///    cap, so an unexplained failure is distinguishable from a missing one;
/// 3. **no decoded value is non-finite** — the wire has no float field, so a
///    NaN bit pattern can only ever be a byte pattern; any decoded axis sample,
///    snapshot record or dequantized coordinate must be finite.
///
/// Buffers past the packet cap must be refused before any parsing, which is
/// what makes a hostile peer cheap to absorb.
#[test]
fn accept_f54_c_fuzzed_packets_are_bounded_and_never_produce_a_non_finite_value() {
    let mut total = 0usize;
    let mut decoded_client = 0usize;
    let mut decoded_server = 0usize;
    let mut oversized = 0usize;

    for seed in fuzz::SEEDS {
        for case in fuzz::corpus(seed) {
            total += 1;
            let label = format!("seed {seed:#x}, {}", case.label);

            // The cap is enforced before the grammar runs, so a huge buffer
            // costs nothing to refuse.
            if case.bytes.len() > MAX_PACKET_BYTES {
                oversized += 1;
                assert!(
                    matches!(
                        decode_client_packet(&case.bytes),
                        Err(CodecError::TooLarge { max, .. }) if max == MAX_PACKET_BYTES
                    ),
                    "{label}: a buffer past the cap must be refused"
                );
                assert!(
                    matches!(
                        decode_server_packet(&case.bytes),
                        Err(CodecError::TooLarge { .. })
                    ),
                    "{label}: and in both directions"
                );
                continue;
            }

            match decode_client_packet(&case.bytes) {
                Ok(packet) => {
                    decoded_client += 1;
                    assert_client_packet_is_sane(&packet, &label);
                }
                Err(reason) => assert_named(&reason.to_string(), &label, "client"),
            }
            match decode_server_packet(&case.bytes) {
                Ok(packet) => {
                    decoded_server += 1;
                    assert_server_packet_is_sane(&packet, &label);
                }
                Err(reason) => assert_named(&reason.to_string(), &label, "server"),
            }
        }
    }

    assert!(
        total > 500,
        "the corpus must be substantial, ran {total} cases"
    );
    assert!(
        oversized > 0,
        "the corpus must reach the packet cap, saw {oversized}"
    );
    assert!(
        decoded_server > 0 && decoded_client > 0,
        "the corpus must decode successfully in both directions, saw \
         {decoded_client} client and {decoded_server} server"
    );
}

/// Runs the client's consumer over every buffer the server decoder accepted.
///
/// `ClientSession::accept` is the production consumer: `pump` calls it for each
/// arrived packet, so a refusal here is the refusal a real client would apply.
#[test]
fn accept_f54_c_the_client_consumer_survives_the_whole_corpus() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);
    let mut accepted = 0usize;
    let mut refused = 0usize;

    for seed in fuzz::SEEDS {
        for case in fuzz::corpus(seed) {
            if case.bytes.len() > MAX_PACKET_BYTES {
                continue;
            }
            // Only an in-session message reaches the consumer; a pre-session
            // reply is the transport's business, not the lifecycle's.
            let Ok(ServerPacket::Message(message)) = decode_server_packet(&case.bytes) else {
                continue;
            };
            match client.accept(message) {
                Ok(notices) => {
                    accepted += 1;
                    for notice in &notices {
                        assert!(
                            !notice.to_string().is_empty(),
                            "{}: a notice names what it did",
                            case.label
                        );
                    }
                    // Whatever the client now holds must be finite, live and
                    // bounded.
                    if let Some(snapshot) = client.snapshot() {
                        for record in &snapshot.actors {
                            assert!(
                                record
                                    .position_m()
                                    .expect("in range")
                                    .iter()
                                    .all(|v| v.is_finite()),
                                "{}: a decoded position is finite",
                                case.label
                            );
                            assert!(
                                record
                                    .linear_velocity_mps()
                                    .expect("in range")
                                    .iter()
                                    .all(|v| v.is_finite()),
                                "{}: a decoded velocity is finite",
                                case.label
                            );
                            assert!(
                                record
                                    .angular_velocity_radps()
                                    .expect("in range")
                                    .iter()
                                    .all(|v| v.is_finite()),
                                "{}: a decoded angular velocity is finite",
                                case.label
                            );
                            assert!(
                                record.flight.fractions().iter().all(|v| v.is_finite()),
                                "{}: a decoded flight channel is finite",
                                case.label
                            );
                            assert_ne!(
                                record.actor.serial, 0,
                                "{}: a decoded actor id is never the reserved serial",
                                case.label
                            );
                            assert_eq!(
                                record.actor.session, session,
                                "{}: a record of a dead epoch is never accepted",
                                case.label
                            );
                        }
                    }
                }
                Err(reason) => {
                    refused += 1;
                    assert!(
                        !reason.to_string().is_empty(),
                        "{}: a refused packet names its reason",
                        case.label
                    );
                }
            }
            assert!(client.remembered_events() <= MAX_SEEN_EVENTS);
            assert!(client.unacked() <= MAX_UNACKED_PACKETS);
        }
    }
    assert!(
        accepted > 0,
        "the corpus must reach the consumer, saw {accepted}"
    );
    assert!(
        refused > 0,
        "the corpus must also be refused somewhere, saw {refused}"
    );
}

/// Asserts that a decoded client packet holds only finite, in-bounds values.
fn assert_client_packet_is_sane(packet: &ClientPacket, label: &str) {
    let ClientPacket::Message(message) = packet else {
        return; // A hello is bounded hashes and bounded text.
    };
    assert!(
        message.header.session.get() > 0,
        "{label}: a session id is never zero"
    );
    let ClientPayload::Input(batch) = &message.payload else {
        return; // Leave carries nothing.
    };
    assert!(
        batch.frames.len() <= MAX_INPUT_FRAMES_PER_PACKET,
        "{label}: a decoded batch respects its cap"
    );
    for frame in &batch.frames {
        for axis in frame.axes() {
            assert!(
                axis.as_unit().is_finite(),
                "{label}: a decoded axis sample is finite ({})",
                axis.as_unit()
            );
        }
    }
}

/// Asserts that a decoded server packet holds only finite, in-bounds values.
fn assert_server_packet_is_sane(packet: &ServerPacket, label: &str) {
    let ServerPacket::Message(message) = packet else {
        return; // A reply is hashes, ids and bounded text.
    };
    assert!(
        message.header.session.get() > 0,
        "{label}: a session id is never zero"
    );
    match &message.payload {
        ServerPayload::Snapshot(frame) => {
            assert!(
                frame.payload.len() <= MAX_SNAPSHOT_BYTES,
                "{label}: a decoded snapshot respects its cap"
            );
            // The envelope bounds the payload; the schema decides whether the
            // bytes are a snapshot. Either outcome is acceptable, but a payload
            // that *does* decode must be entirely finite.
            for epoch in [SYNTHETIC_SESSION, message.header.session] {
                if let Ok(snapshot) = Snapshot::decode(&frame.payload, epoch) {
                    assert_eq!(
                        snapshot.origin, SYNTHETIC_ORIGIN_EPOCH,
                        "{label}: a decoded origin epoch is a known one"
                    );
                    for record in &snapshot.actors {
                        assert!(
                            record
                                .position_m()
                                .expect("in range")
                                .iter()
                                .all(|value| value.is_finite()),
                            "{label}: a decoded position is finite"
                        );
                        assert!(
                            record
                                .linear_velocity_mps()
                                .expect("in range")
                                .iter()
                                .all(|value| value.is_finite()),
                            "{label}: a decoded velocity is finite"
                        );
                        assert!(
                            record
                                .angular_velocity_radps()
                                .expect("in range")
                                .iter()
                                .all(|value| value.is_finite()),
                            "{label}: a decoded angular velocity is finite"
                        );
                    }
                }
            }
        }
        ServerPayload::Event(event) => {
            assert!(
                event.id.session.get() > 0,
                "{label}: an event id names a nonzero epoch"
            );
        }
        ServerPayload::InputAck { .. } | ServerPayload::Disconnect { .. } => {}
    }
}

/// A refusal must name what failed, so an unexplained failure is
/// distinguishable from a missing one.
fn assert_named(reason: &str, label: &str, direction: &str) {
    assert!(
        reason.len() > 8,
        "{label}: the {direction} decoder's refusal names its reason, got {reason:?}"
    );
}

// ------------------------------------------------------------------ the wire --

#[test]
fn accept_f54_c_a_hostile_peer_is_cut_off_without_disturbing_the_others() {
    // The verdict is never retried: an attempt in which the host saw the
    // abusive peer's traffic is reported exactly as it happened. What may be
    // rebuilt is a fixture whose loopback path carried *nothing* — neither a
    // handshake nor a single hostile payload — because such a fixture never
    // gave the session the chance to decide. See [`DEAD_PATH_ATTEMPTS`].
    let Attempt {
        mut link,
        hostile,
        hostile_peer,
        honest_reached,
        ..
    } = live_fixture(|| match abusive_peer_attempt() {
        Ok(attempt) if attempt.quiet() => Err(DeadPath(
            "the host saw nothing from the abusive peer".to_string(),
        )),
        done => done,
    });
    let good_peer = link.client.grant().expect("a grant exists").peer;

    // The abusive peer is gone; the honest one is untouched. When no verdict
    // ever came, say what both ends of the link can see — including whether
    // the host still hears the honest peer, which separates a session that
    // did not cut the peer off from a path that carried nothing at all.
    let diagnosis = if cut_off(&link, hostile_peer) {
        String::new()
    } else {
        diagnose_silence(&mut link, &hostile, hostile_peer)
    };
    assert!(
        cut_off(&link, hostile_peer),
        "the abusive peer was cut off; {diagnosis}",
    );
    let state = format!(
        "{}, the host holds {} client(s)",
        hostile.status(),
        link.host.connected_clients()
    );
    assert!(
        link.host.members().any(|member| member == good_peer),
        "the honest peer survived; the host saw {:?} ({state})",
        link.host_notices
    );
    assert!(
        link.host_has(|notice| matches!(notice, ServerNotice::CutOff { .. })),
        "the cut-off is declared, not silent; the host saw {:?} ({state})",
        link.host_notices
    );

    // And the honest peer still has a working session: the probe ran inside
    // the attempt, so a dead path rebuilt the fixture and the verdict here is
    // as delivered.
    assert!(
        honest_reached,
        "the honest peer still reaches the consumer; the host saw {:?} ({state})",
        link.host_notices
    );
}

#[test]
fn accept_f54_c_a_send_before_the_grant_is_refused() {
    let _loopback = loopback();

    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let host = ServerSession::bind(session, synthetic_parameters(), bind, Duration::ZERO)
        .expect("the host socket binds");
    let addr = host.local_addr().expect("the bound host has an address");
    let mut client = ClientSession::connect_with_window(
        synthetic_hello(),
        addr,
        0xC9,
        Duration::ZERO,
        LOOPBACK_WINDOW,
    )
    .expect("the client socket binds");

    // Nothing may be sent before the handshake, and the lifecycle says so.
    assert!(matches!(client.leave(), Err(ClientFault::NotInSession)));
    assert!(matches!(
        client.accept(ServerMessage {
            header: MessageHeader {
                session,
                sequence: 0,
            },
            payload: ServerPayload::InputAck { through: 0 },
        }),
        Err(ClientFault::NotInSession)
    ));
    assert!(matches!(client.phase(), ClientPhase::Connecting));
}

#[test]
fn accept_f54_c_a_client_without_a_grant_refuses_every_server_packet() {
    let _loopback = loopback();

    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let host = ServerSession::bind(session, synthetic_parameters(), bind, Duration::ZERO)
        .expect("the host socket binds");
    let addr = host.local_addr().expect("the bound host has an address");
    let mut client = ClientSession::connect_with_window(
        synthetic_hello(),
        addr,
        0xCA,
        Duration::ZERO,
        LOOPBACK_WINDOW,
    )
    .expect("the client socket binds");

    // A decoded packet cannot be applied before the handshake named an epoch:
    // there is no epoch to check it against.
    assert!(matches!(
        client.accept(ServerMessage {
            header: MessageHeader {
                session,
                sequence: 0,
            },
            payload: ServerPayload::InputAck { through: 0 },
        }),
        Err(ClientFault::NotInSession)
    ));
}

#[test]
fn accept_f54_c_a_wrong_protocol_revision_is_refused_through_the_lifecycle() {
    // The revision rides inside the handshake, so the only way a lifecycle
    // owner can meet another one is a client that offers it. This runs the real
    // handshake with a client offering revision 2: the host must refuse it with
    // the named reason, tell the client, and never make it a peer.
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let mut hello = synthetic_hello();
    let offered = cs_net::compat::ProtocolVersion::new(2).expect("two is nonzero");
    hello.protocol = offered;
    let hello_offered = hello.protocol;
    // The handshake's answer is the verdict under test: a `Refused` closure
    // the client actually holds is asserted exactly as delivered, and only a
    // path that died before carrying it is rebuilt.
    let mut link = live_fixture(|| {
        let mut link = Link::new(session, synthetic_parameters(), hello.clone());
        link.try_pump_until_joined().map(|()| link)
    });

    assert!(
        matches!(
            link.client.phase().closure(),
            Some(ClientClosure::Refused(
                HandshakeReject::UnsupportedProtocol { offered, supported }
            )) if *offered == hello_offered && *supported == PROTOCOL_VERSION
        ),
        "the client holds the named reason: {:?}",
        link.client.phase()
    );
    assert!(link.client.grant().is_none(), "and it never became a peer");
    assert!(
        link.host.members().next().is_none(),
        "the host has no member to run a match for"
    );
    assert!(
        link.host_has(|notice| matches!(
            notice,
            ServerNotice::PeerRefused {
                reason: HandshakeReject::UnsupportedProtocol { .. }
            }
        )),
        "the refusal is reported with its reason: {:?}",
        link.host_notices
    );
    assert!(
        link.host.launch(Tick(1)).is_ok(),
        "and the refusal did not corrupt the session"
    );
}

#[test]
fn accept_f54_c_a_retry_hangs_up_the_connections_that_hold_no_peer() {
    // A refused client keeps a netcode connection but never becomes a peer, so
    // `close` does not reach it and a retry has to: the new epoch does not know
    // that connection, so it may neither keep a session slot for it nor let it
    // back in.
    let mut allocator = SessionAllocator::new();
    let first = allocator.allocate().expect("the first epoch allocates");
    let second = allocator.allocate().expect("the retry epoch allocates");
    let mut hello = synthetic_hello();
    hello.compatibility.rules_sha256 = ContentHash::from_bytes([0x00; 32]);
    live_fixture(|| {
        let mut link = Link::new(first, synthetic_parameters(), hello.clone());

        // Both sides are driven by the same [`STEP`], which is the clock every
        // pump here takes: the pinned connection layer times its own 250 ms
        // keep-alive and its [`LOOPBACK_WINDOW`] disconnect window off the
        // duration each pump is handed, so a round that advances one side by a
        // second races that window instead of the handshake.
        let mut refused = false;
        for _ in 0..MAX_ROUNDS {
            link.host_notices.extend(link.host.pump(STEP));
            if link.host_has(|notice| matches!(notice, ServerNotice::PeerRefused { .. })) {
                refused = true;
                break;
            }
            if link.client.transport().disconnect_reason().is_some() {
                break;
            }
            link.client_round();
        }
        if !refused {
            if let Some(dead) = silent_client(&link.client, "the refused handshake") {
                return Err(dead);
            }
            panic!(
                "the handshake was never refused; the host saw {:?}",
                link.host_notices
            );
        }

        // The client still has to read the verdict, and only its own rounds
        // may run to do it: the host hangs up on a refused client one round
        // later, on purpose, so that renet can flush the refusal first — and
        // the retry below is what has to hang this connection up, so it has
        // to start from one the host still holds.
        for _ in 0..MAX_ROUNDS {
            if link.client.phase().closure().is_some() {
                break;
            }
            link.client_round();
        }
        if let Some(dead) = silent_client(&link.client, "the refusal's delivery") {
            return Err(dead);
        }
        assert!(
            link.client.phase().closure().is_some(),
            "the client never applied the refusal; it saw {:?}",
            link.client_notices
        );
        assert!(
            link.client_has(|notice| matches!(
                notice,
                ClientNotice::Refused {
                    reason: HandshakeReject::RulesMismatch { .. }
                }
            )),
            "the client holds the named refusal: {:?}",
            link.client_notices
        );
        // The retry's precondition is this connection still standing: the
        // host's own hang-up is deferred, so a connection down at this point
        // died without the host acting on it — nothing it could be retried
        // for ever arrived.
        if !link.client.transport().is_connected() || link.host.connected_clients() == 0 {
            return Err(DeadPath(format!(
                "the refused connection died before the retry could hang it up; the client saw {:?}",
                link.client_notices
            )));
        }
        assert!(link.client.grant().is_none(), "and it never became a peer");
        assert_eq!(
            link.host.connected_clients(),
            1,
            "and that connection still holds one netcode slot"
        );
        assert!(
            link.host.members().next().is_none(),
            "the host has no member for it, so a teardown has nothing to reach"
        );

        // The retry's decision, with no clock in it at all: it hung up on the
        // connection that holds no peer id, and the new epoch keeps no slot
        // for it.
        assert_eq!(
            link.host
                .reopen(second)
                .expect("the retry binds a fresh epoch"),
            1,
            "the retry hung up on the connection that held no peer id"
        );
        assert_eq!(link.host.session(), second);
        assert_eq!(
            link.host.connected_clients(),
            0,
            "the fresh epoch holds no connection for it"
        );

        // The refused client stays silent while both sides keep pumping, so
        // the only thing that can take its connection down is the host's
        // hang-up. The wait runs the full window because the packet is never
        // retransmitted: a connection that goes down for any other reason is
        // proof the hang-up never arrived, which is a dead path, while a
        // connection the host's own packet ended is the verdict.
        link.try_pump_until("the retry's hang-up never reached the client", |link| {
            !link.client.transport().is_connected()
        })?;
        if !matches!(
            link.client.transport().disconnect_reason(),
            Some(renetcode2::DisconnectReason::DisconnectedByServer)
        ) {
            return Err(DeadPath(format!(
                "the retry's hang-up never arrived; the connection died by itself ({:?}); the client saw {:?}",
                link.client.transport().disconnect_reason(),
                link.client_notices
            )));
        }
        assert!(
            !link.host_has(|notice| matches!(notice, ServerNotice::HungUp { .. })),
            "the retry hung up on that connection as one decision, so the fresh epoch does not report \
             it again as a per-connection hang-up of the spent epoch: {:?}",
            link.host_notices
        );
        Ok(())
    });
}

#[test]
fn accept_f54_c_a_retry_tells_a_returning_client_its_verdict() {
    // The pinned connection layer keeps its own client table alongside the
    // session's bookkeeping. Before the retry released that table itself, a
    // client reconnecting with the same connection id inside the window
    // where the spent epoch's slot still stood was denied without an answer:
    // the pinned layer silently refuses a request whose client_id occupies a
    // slot and ignores a response that names one, so the returning client
    // could never be told the new epoch's verdict. The retry now hangs those
    // slots up itself, and this test drives the real loopback through it.
    let mut allocator = SessionAllocator::new();
    let first = allocator.allocate().expect("the first epoch allocates");
    let second = allocator.allocate().expect("the retry epoch allocates");
    live_fixture(|| {
        let mut link = Link::try_joined(first)
            .map_err(|link| link.ungranted("the first client's handshake"))?;
        let mut second_member = link.raw_peer(0xC2);
        second_member
            .try_handshake(&mut link.host)
            .ok_or_else(|| ungranted_peer(&second_member, "the second member's handshake"))?;
        assert_eq!(
            link.host.members().count(),
            2,
            "two members held connection-layer slots in the spent epoch"
        );

        // The retry's count is only observable if both connections are still
        // standing when it runs: a slot the loopback already killed leaves the
        // retry nothing to hang up, which is the fixture's failure, not the
        // session's answer.
        if !link.client.transport().is_connected()
            || !second_member.transport.is_connected()
            || link.host.connected_clients() != 2
        {
            return Err(DeadPath(format!(
                "a spent-era connection died before the retry could hang it up: client {}, second member {}",
                link.client.transport().disconnect_reason().is_some(),
                second_member.status()
            )));
        }

        // `close` condemns the members at the reliable layer, but only the
        // retry releases the slots they still occupy at the connection layer —
        // so its count is every connection it hung up there, not just the
        // peer-less connections the session's own bookkeeping still knew
        // about.
        assert_eq!(
            link.host
                .reopen(second)
                .expect("the retry binds a fresh epoch"),
            2,
            "the retry hung up both connections the spent epoch still held"
        );
        assert_eq!(link.host.session(), second);
        assert_eq!(
            link.host.connected_clients(),
            0,
            "no spent-era slot survives at the connection layer"
        );

        // The same connection-layer id returns on a fresh socket and is told
        // the new epoch's grant — the stale slot that would have denied it is
        // gone.
        let mut returning = link.raw_peer(0xC1);
        let peer = returning
            .try_handshake(&mut link.host)
            .ok_or_else(|| ungranted_peer(&returning, "the returning client's handshake"))?;
        assert_eq!(
            returning
                .transport
                .grant()
                .expect("the returning client is told its grant")
                .session,
            second,
            "the grant is the new epoch's"
        );
        assert!(
            link.host.members().any(|member| member == peer),
            "and the returning client is a member of it"
        );

        // The same id with a hello the admission gate refuses is told the
        // named reason rather than left waiting for an answer that cannot
        // come.
        let mut refused_hello = synthetic_hello();
        refused_hello.compatibility.rules_sha256 = ContentHash::from_bytes([0x00; 32]);
        let addr = link.host.local_addr().expect("the host has an address");
        let mut refused = RawPeer {
            transport: ClientTransport::connect_with_window(
                refused_hello,
                addr,
                0xC2,
                Duration::ZERO,
                LOOPBACK_WINDOW,
            )
            .expect("the refused client socket binds"),
            events: Vec::new(),
        };
        let mut told = false;
        for _ in 0..MAX_ROUNDS {
            if refused.transport.rejection().is_some() {
                told = true;
                break;
            }
            if peer_path_dead(&refused) {
                break;
            }
            link.host_notices.extend(refused.round(&mut link.host));
        }
        if !told {
            // A hang-up that outran the reliable refusal left the answer
            // undelivered, which is the path's failure, not the session's.
            return Err(peer_delivery(
                &refused,
                "the refused client was never told its reason",
                &link.host_notices,
            ));
        }
        assert!(
            matches!(
                refused.transport.rejection(),
                Some(HandshakeReject::RulesMismatch { .. })
            ),
            "the refused client holds the named reason: {:?}",
            refused.transport.rejection()
        );

        // The spent epoch stays stale: the returning client's packet stamped
        // with the first epoch is still refused by the gate. The refire keeps
        // the expectation on the refusal rather than one datagram surviving.
        let stale = fire_bytes(first, Tick(11), 0);
        returning.inject(CHANNEL_SEQUENCED, &stale);
        link.try_pump_peer_until(
            &mut returning,
            "the stale packet was never refused",
            |peer| peer.inject(CHANNEL_SEQUENCED, &stale),
            |link| {
                link.host_has(|notice| {
                    matches!(
                        notice,
                        ServerNotice::Dropped {
                            reason: DropReason::Refused(violation),
                            ..
                        } if violation.label() == "stale_session"
                    )
                })
            },
        )?;
        assert_eq!(
            link.host.drain_work().len(),
            0,
            "a packet stamped with the prior epoch applies to nothing"
        );
        Ok(())
    });
}

#[test]
fn accept_f54_c_a_spent_epoch_refuses_to_publish() {
    let _loopback = loopback();

    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let mut host = ServerSession::bind(
        session,
        synthetic_parameters(),
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        Duration::ZERO,
    )
    .expect("the host socket binds");
    let snapshot = fuzz::synthetic_snapshot(session, [0.0, 0.0, 0.0]);
    let actor = ActorId { session, serial: 1 };

    // While the epoch is open every publisher works.
    assert!(host.publish_snapshot(Tick(1), &snapshot).is_ok());
    assert!(
        host.announce_spawn(Tick(1), actor, synthetic_blueprint_id(), None)
            .is_ok()
    );
    host.close(DisconnectReason::SessionEnded)
        .expect("teardown runs");

    // After teardown they are refused by name rather than sent into a session
    // with no members left, where a silent no-op would look like success.
    for outcome in [
        host.publish_snapshot(Tick(2), &snapshot).err(),
        host.announce_spawn(Tick(2), actor, synthetic_blueprint_id(), None)
            .err(),
        host.announce_removal(Tick(2), actor).err(),
        host.announce(
            Tick(2),
            EventBody::Finished {
                reason: FinishReason::Completed,
            },
        )
        .err(),
    ] {
        assert_eq!(
            outcome,
            Some(ServerFault::WrongPhase {
                action: "publish",
                phase: ServerPhase::Closed,
            }),
            "a spent epoch refuses to publish, naming the phase"
        );
    }
}

#[test]
fn accept_f54_c_a_closed_client_session_is_terminal() {
    let session = SessionAllocator::new()
        .allocate()
        .expect("an epoch allocates");
    let (mut client, _loopback) = granted_client(session);

    // The host closed the session.
    let notices = client
        .accept(ServerMessage {
            header: MessageHeader {
                session,
                sequence: 0,
            },
            payload: ServerPayload::Disconnect {
                reason: DisconnectReason::SessionEnded,
            },
        })
        .expect("a live epoch is accepted");
    assert!(
        notices.iter().any(|notice| matches!(
            notice,
            ClientNotice::Phase(ClientPhase::Closed(ClientClosure::ServerClosed(
                DisconnectReason::SessionEnded
            )))
        )),
        "the close applied: {notices:?}"
    );

    // Everything that arrives afterwards changes nothing: not a reliable event
    // the host had already queued, not a snapshot, not an acknowledgment.
    for (label, message) in [
        (
            "a launch published before the close",
            fuzz::event_message(session, 1, fuzz::launched_event(session, Tick(1000), 1)),
        ),
        (
            "a finish published before the close",
            fuzz::event_message(
                session,
                2,
                fuzz::finished_event(session, Tick(1100), FinishReason::Completed),
            ),
        ),
        (
            "a snapshot published before the close",
            fuzz::snapshot_message(
                session,
                3,
                SnapshotFrame {
                    tick: Tick(1200),
                    payload: fuzz::synthetic_snapshot(session, [1.0, 2.0, 3.0])
                        .encode(session)
                        .expect("the synthetic snapshot encodes"),
                },
            ),
        ),
        (
            "an acknowledgment published before the close",
            ServerMessage {
                header: MessageHeader {
                    session,
                    sequence: 4,
                },
                payload: ServerPayload::InputAck { through: 7 },
            },
        ),
    ] {
        assert!(
            matches!(
                client.accept(message),
                Err(ClientFault::Closed {
                    closure: ClientClosure::ServerClosed(DisconnectReason::SessionEnded)
                })
            ),
            "{label} is refused because the session is over"
        );
        assert_eq!(
            *client.phase(),
            ClientPhase::Closed(ClientClosure::ServerClosed(DisconnectReason::SessionEnded)),
            "{label} did not reopen the session"
        );
    }
    assert!(client.snapshot().is_none(), "no late snapshot was stored");
    assert_eq!(client.acked_through(), None, "no late ack was applied");
    assert_eq!(
        client.remembered_events(),
        0,
        "no late event was remembered"
    );
    assert!(!client.phase().open(), "and the session sends nothing more");
}
