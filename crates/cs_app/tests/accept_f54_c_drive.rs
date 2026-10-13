//! Acceptance scenario F54-C (drive): the Bevy-scheduled `cs_net` session
//! binding against a `cs_sim` ledger.
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! stage `### F54-C`, and `docs/contracts/UI-NETWORK.md` (ownership table).
//! Follow-up task Rally #592; task test prefix: `accept_f54_c_drive_`.
//!
//! # What is under test
//!
//! [`cs_app::network::session::NetSessionDrivePlugin`] installed on a real
//! [`cs_app::physics::PhysicsSession`], so every pump is the production
//! `FixedPostUpdate` run — the same one a shipped session drives — over a
//! real loopback pair: `ServerSession` + `ClientSession` through the pinned
//! `renet2` stack, not a parallel test-side link.
//!
//! The scenario asserts the whole binding the task named:
//!
//! * `PeerInput.frames` fold into the peer's `ControlBuffer` (the
//!   authoritative `cs_sim` control path), and the packet's sequence is
//!   booked in the `NetStateLedger` and acknowledged on the wire.
//! * `PeerInput.fires` become `FireIntent` orders through
//!   `recovery::fire_intent`, waiting for the weapon schedule's tick.
//! * A `Live` host publishes the ledger's snapshot every committed tick, and
//!   the client's mirror is constructed by the drive at grant time and fed
//!   `ClientSession::snapshot()` at most once each.
//! * A peer departure releases its control state — buffer, held axes and
//!   gate ownership together — and a session resource that goes away is
//!   recorded as a teardown by name.
//! * The authorities a run lacks (clock, tick ledger, a Live phase, a
//!   committed tick) are named in `NetSessionReport`, never guessed.
//!
//! The peer's aircraft never gains client authority: the only state the wire
//! moves is what `cs_net` admitted and what `cs_sim` owns. Loopback on one
//! machine is `network_local` per `AGENTS.md`; no original networked
//! behavior is asserted anywhere here.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use bevy::prelude::World;

use cs_app::network::physics::{IngestOutcome, InterpolationConfig, NetSession};
use cs_app::network::session::{
    NetClient, NetDriveMissing, NetHost, NetHostError, NetMirror, NetPublish,
    NetSessionDrivePlugin, NetSessionReport, drive_net_session,
};
use cs_app::origin::{OriginEpoch, WorldOrigin};
use cs_app::physics::PhysicsSession;
use cs_app::weapons::WeaponOrder;
use cs_net::fixture::{synthetic_hello, synthetic_parameters};
use cs_net::lifecycle::{ClientNotice, ClientPhase, ClientSession, ServerNotice, ServerSession};
use cs_net::transport::ConnectWindow;
use cs_sim::net_state::{NetActorState, NetStateError, NetStateLedger};
use cs_types::Tick;
use cs_types::input::FlightCommand;
use cs_types::net::{ActorId, PeerId, SessionId};
use cs_types::space::{Quaternion, WorldPosition};

/// The session generation the fixture runs.
const SESSION: u64 = 71;

/// A generation that is never this fixture's session, for the foreign-epoch
/// mirror replacement check.
const FOREIGN_SESSION: u64 = 72;

/// The serial the fixture's flown aircraft carries.
const PLANE_SERIAL: u64 = 7;

/// The fixed rate the world commits, matching the interpolation config's
/// declared 60 ticks per second.
const FIXED_HZ: u32 = 60;

/// How many committed ticks a wait gets before it fails.
///
/// Each committed tick pumps each session with one fixed timestep of elapsed
/// (~16.7 ms), so the connection layer's window — the fixture asks for 120
/// seconds, the same bound `accept_f54_c_lifecycle` uses so loopback loss
/// under load is tolerated rather than retried — is worth about 7200 rounds
/// and a few hundred is generous. Nothing here sleeps on the wall clock:
/// every wait bounds *committed ticks*, which is what the session's own
/// timers consume.
const MAX_TICKS: u64 = 600;

/// The connection-layer window the fixture asks for, and why it is not the
/// shipped default: see `accept_f54_c_lifecycle.rs`'s `LOOPBACK_WINDOW`.
const LOOPBACK_WINDOW: ConnectWindow = ConnectWindow::new(120);

/// Serializes the tests in this file that open real loopback sockets — the
/// same finding `accept_f54_c_lifecycle` measured applies here: one live
/// pair at a time is what the machine's loopback path carries without loss.
static LOOPBACK: Mutex<()> = Mutex::new(());

fn loopback() -> MutexGuard<'static, ()> {
    LOOPBACK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session id")
}

fn foreign_session_id() -> SessionId {
    SessionId::new(FOREIGN_SESSION).expect("a nonzero session id")
}

fn plane() -> ActorId {
    ActorId {
        session: session_id(),
        serial: PLANE_SERIAL,
    }
}

/// The shared origin epoch: host publisher and client mirror read the same
/// frame, so relative coordinates cross the wire without an epoch refusal.
fn origin() -> WorldOrigin {
    WorldOrigin::new(
        OriginEpoch(1),
        WorldPosition::try_new([50_000.0, 500.0, -80_000.0]).expect("finite"),
    )
}

/// The ledger's one aircraft, parked near the origin so its quantized record
/// is comfortably inside the snapshot's representable span.
fn spawned_ledger() -> NetStateLedger {
    let mut ledger = NetStateLedger::new(session_id());
    ledger
        .spawn(NetActorState::spawn(
            plane(),
            WorldPosition::try_new([50_100.0, 520.0, -80_300.0]).expect("finite"),
            Quaternion::IDENTITY,
            400,
        ))
        .expect("the first spawn succeeds");
    ledger
}

/// One loopback pair carried by one physics world: the `NetHost` and the
/// `NetClient` are both resources of the same session world, so every
/// committed tick is one production `drive_net_session` run serving both —
/// the shape a listen-server host runs.
struct LinkedWorld {
    session: PhysicsSession,
    _loopback: MutexGuard<'static, ()>,
}

impl LinkedWorld {
    /// Builds the world, binds the host, connects the client and installs
    /// both session resources before the first committed tick.
    fn new() -> Self {
        let loopback = loopback();
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let server =
            ServerSession::bind(session_id(), synthetic_parameters(), bind, Duration::ZERO)
                .expect("the host socket binds");
        let addr = server.local_addr().expect("the bound host has an address");
        let client = ClientSession::connect_with_window(
            synthetic_hello(),
            addr,
            0xC1,
            Duration::ZERO,
            LOOPBACK_WINDOW,
        )
        .expect("the client socket binds");

        let mut session = PhysicsSession::production_builder()
            .fixed_hz(FIXED_HZ)
            .configure(|app| {
                app.add_plugins(NetSessionDrivePlugin);
            })
            .build();
        let world = session.world_mut().expect("the session is active");
        world.insert_resource(
            NetHost::new(server, spawned_ledger(), origin()).expect("the epochs agree"),
        );
        world.insert_resource(NetClient::new(
            client,
            origin(),
            InterpolationConfig::default(),
        ));
        Self {
            session,
            _loopback: loopback,
        }
    }

    fn world(&mut self) -> &mut World {
        self.session.world_mut().expect("the session is active")
    }

    /// Commits `ticks` fixed ticks through the production stepper.
    fn step(&mut self, ticks: u64) {
        self.session.step(ticks).expect("the session steps");
    }

    /// The just-committed run's report.
    fn report(&self) -> NetSessionReport {
        self.session
            .world()
            .and_then(|world| world.get_resource::<NetSessionReport>().cloned())
            .unwrap_or_default()
    }

    /// Commits one tick at a time until `done` holds on the resulting report.
    ///
    /// Every exchange is over a real socket, so how many ticks a delivery
    /// takes is the transport's business: the wait bounds committed ticks —
    /// the budget the session timers and the test both count in — and fails
    /// with the last report when the condition stayed unreachable. The
    /// predicate is `FnMut`, so it may keep a running observation across
    /// runs — a notice that landed two runs ago is still a notice the drive
    /// forwarded.
    fn step_until(&mut self, what: &str, mut done: impl FnMut(&NetSessionReport) -> bool) {
        for _ in 0..MAX_TICKS {
            self.step(1);
            let report = self.report();
            if done(&report) {
                return;
            }
        }
        panic!(
            "{what}: {MAX_TICKS} committed ticks elapsed; last report {:?}",
            self.report()
        );
    }
}

#[test]
fn accept_f54_c_drive_the_session_round_trip_binds_every_boundary() {
    let mut linked = LinkedWorld::new();

    // The handshake is the drive's own work: `ClientSession::pump` emits the
    // hello, `ServerSession::pump` answers, and the grant lands without a
    // test ever calling pump itself. `PeerJoined` is a per-run notice, so the
    // wait keeps the observation across the runs the exchange spans.
    let mut host_saw_join = false;
    linked.step_until("the handshake never granted", |report| {
        host_saw_join |= report
            .host_notices
            .iter()
            .any(|notice| matches!(notice, ServerNotice::PeerJoined { .. }));
        host_saw_join && report.client_session == Some(session_id())
    });
    let report = linked.report();
    assert_eq!(report.host_session, Some(session_id()));
    assert_eq!(report.client_session, Some(session_id()));
    // The grant built the mirror: the drive installs it the first run the
    // grant exists, serving exactly the granted epoch.
    assert_eq!(
        linked
            .world()
            .get_resource::<NetMirror>()
            .expect("the drive built the client mirror at grant")
            .session(),
        session_id()
    );
    let peer = linked
        .world()
        .resource::<NetClient>()
        .session()
        .grant()
        .expect("the client holds a grant")
        .peer;

    // A mirror serving another epoch is torn down and replaced, never fed a
    // foreign generation's traffic.
    linked.world().insert_resource(NetMirror(NetSession::new(
        foreign_session_id(),
        origin(),
        InterpolationConfig::default(),
    )));
    linked.step(1);
    let report = linked.report();
    assert!(
        report
            .tore_down
            .iter()
            .any(|down| down.side == "client" && down.session == Some(foreign_session_id())),
        "the foreign mirror was torn down by name: {:?}",
        report.tore_down
    );
    assert_eq!(
        linked
            .world()
            .get_resource::<NetMirror>()
            .expect("the mirror was rebuilt")
            .session(),
        session_id()
    );

    // Launch: the host's Live phase is what publishes snapshots; the client
    // applies the launched notice itself.
    linked
        .world()
        .resource_mut::<NetHost>()
        .session_mut()
        .launch(Tick(100))
        .expect("a gathering session launches");
    linked.step_until("the client never went live", |report| {
        report.client_notices.iter().any(|notice| {
            matches!(notice, ClientNotice::Phase(ClientPhase::Live { .. }))
                || matches!(notice, ClientNotice::Event { .. })
        })
    });
    assert!(
        matches!(
            *linked.world().resource::<NetClient>().session().phase(),
            ClientPhase::Live { .. }
        ),
        "the client is Live"
    );

    // In flight: bind the peer's aircraft through the host's gate, then let
    // the client submit a sample and a fire edge. The packets travel the
    // real wire; the drive drains them into the authoritative paths.
    linked
        .world()
        .resource_mut::<NetHost>()
        .bind_aircraft(peer, plane())
        .expect("the gate binds the peer's aircraft");
    linked
        .world()
        .resource_mut::<NetClient>()
        .session_mut()
        .submit_sample(FlightCommand::Throttle, 0.75, Tick(101))
        .expect("a finite throttle sample queues");
    linked
        .world()
        .resource_mut::<NetClient>()
        .session_mut()
        .submit_edge(FlightCommand::FirePrimary, Tick(102))
        .expect("a fire edge queues");

    linked.step_until("the admitted packet was never consumed", |report| {
        report
            .consumed
            .iter()
            .any(|input| input.peer == peer && input.frames_applied == 2)
    });
    let report = linked.report();
    let consumed = report
        .consumed
        .iter()
        .find(|input| input.peer == peer)
        .expect("the peer's packet was consumed");
    assert!(!consumed.unbound, "the bound peer had a control buffer");
    assert_eq!(
        consumed.sequence, 0,
        "the first in-session packet is sequence 0"
    );
    // A discovered cross-layer numbering boundary, recorded rather than
    // hidden: wire input sequences are zero-based (`next_sequence: 0`), while
    // the ledger's input book starts `acknowledged` at 0 and refuses
    // `sequence <= acknowledged` as a replay — so the wire's legitimate first
    // packet is always booked `StaleInput`. The courtesy and the rule disagree
    // on sequence zero and the report names both sides: the book refused, the
    // wire ack still went out, and the packet was still consumed. Which side
    // owns the numbering decision is a cs_sim/cs_net follow-up, filed
    // separately — `cs_sim` is outside this task's owner paths.
    assert_eq!(
        consumed.book_refusal,
        Some(NetStateError::StaleInput {
            presented: 0,
            acknowledged: 0,
        }),
    );
    assert_eq!(
        consumed.acknowledged,
        Ok(()),
        "the wire acknowledgment went out"
    );
    assert!(
        consumed.frames_refused.is_empty(),
        "in-order frames are never refused"
    );
    assert_eq!(consumed.intents, 1, "the fire edge minted one intent");

    // The authoritative control path: the peer's buffer holds the throttle
    // axis and queues the fire edge — exactly what a local frame would have
    // left, arrived over the wire.
    let host = linked.world().resource::<NetHost>();
    let buffer = host
        .control(peer)
        .expect("the peer's control buffer is open");
    // The wire axis is a quantized fraction (the transport's own 16-bit
    // encoding): what reached the buffer is the same deflection inside the
    // wire's own precision.
    let axis = buffer
        .axis(FlightCommand::Throttle)
        .expect("the wire frame's axis reached the peer's buffer");
    assert!(
        (axis - 0.75).abs() < 0.0001,
        "the wire axis is the submitted deflection: {axis}"
    );
    assert_eq!(
        buffer.pending_edges(),
        1,
        "the fire edge waits at the input boundary"
    );
    assert_eq!(
        host.ledger().acknowledged_input(),
        consumed.sequence,
        "the ledger booked the consumed sequence"
    );

    // The minted intent is a weapon order named for the fire edge's tick:
    // the weapon schedule's own `ForeignTick`/`DuplicateIntent` rules judge
    // it there; this drive only lands it in the right queue.
    let batch = linked
        .world()
        .resource_mut::<NetHost>()
        .take_orders(Tick(102));
    assert!(batch.stale.is_empty());
    let [WeaponOrder::Fire(intent)] = batch.matched.as_slice() else {
        panic!("expected one fire order, got {:?}", batch.matched);
    };
    assert_eq!(intent.id.session, SESSION);
    assert_eq!(intent.id.tick, Tick(102));
    assert_eq!(intent.id.producer, u32::from(peer.get()));
    assert_eq!(intent.shooter.serial, PLANE_SERIAL);

    // While the host is Live the committed tick publishes the ledger: the
    // report names what went on the wire, and the client mirror ingests the
    // newest mailbox slot exactly once.
    linked.step_until("the client never ingested a snapshot", |report| {
        matches!(
            report.ingested.as_ref().map(|ingest| ingest.outcome),
            Some(IngestOutcome::Applied)
        )
    });
    let report = linked.report();
    assert_eq!(
        report.published,
        Some(NetPublish::Sent {
            at: report.at.expect("a committed tick exists"),
            records: 1,
        }),
        "the Live host published the one-actor ledger"
    );
    let shown = linked
        .world()
        .get_resource::<NetMirror>()
        .expect("the mirror still serves")
        .mirror()
        .aircraft(plane())
        .expect("the wire snapshot reached the client mirror");
    assert_eq!(shown.actor, plane());

    // The mailbox is latest-wins and the drive feeds it once each: with no
    // new snapshot arriving, further ticks ingest nothing. Removing the host
    // freezes the mailbox at its last value — and is itself recorded as the
    // session teardown it is.
    let last_fed = linked
        .world()
        .resource::<NetClient>()
        .session()
        .snapshot_tick();
    linked.world().remove_resource::<NetHost>();
    linked.step(1);
    let report = linked.report();
    assert!(
        report
            .tore_down
            .iter()
            .any(|down| down.side == "host" && down.session == Some(session_id())),
        "the host resource's removal was recorded: {:?}",
        report.tore_down
    );
    linked.step(3);
    let report = linked.report();
    assert_eq!(report.ingested, None, "a snapshot already fed is not refed");
    assert_eq!(
        linked
            .world()
            .resource::<NetClient>()
            .session()
            .snapshot_tick(),
        last_fed,
        "the mailbox still holds the last snapshot"
    );

    // Removing the client tears down the mirror the drive built — the only
    // feed that mirror ever had — and names the dead generation.
    linked.world().remove_resource::<NetClient>();
    linked.step(1);
    let report = linked.report();
    assert!(
        report
            .tore_down
            .iter()
            .any(|down| down.side == "client" && down.session == Some(session_id())),
        "the client resource's removal released the mirror: {:?}",
        report.tore_down
    );
    assert!(
        linked.world().get_resource::<NetMirror>().is_none(),
        "no mirror outlives its session"
    );
}

#[test]
fn accept_f54_c_drive_a_departed_peer_leaves_no_control_or_ownership() {
    let mut linked = LinkedWorld::new();
    linked.step_until("the handshake never granted", |report| {
        report.client_session == Some(session_id())
    });
    let peer = linked
        .world()
        .resource::<NetClient>()
        .session()
        .grant()
        .expect("the client holds a grant")
        .peer;
    linked
        .world()
        .resource_mut::<NetHost>()
        .bind_aircraft(peer, plane())
        .expect("the gate binds the peer's aircraft");

    // Give the peer real held state to leave behind: a deflected throttle
    // axis.
    linked
        .world()
        .resource_mut::<NetClient>()
        .session_mut()
        .submit_sample(FlightCommand::Throttle, 0.5, Tick(101))
        .expect("a finite throttle sample queues");
    linked.step_until("the admitted packet was never consumed", |report| {
        report
            .consumed
            .iter()
            .any(|input| input.peer == peer && input.frames_applied == 1)
    });
    assert!(
        linked
            .world()
            .resource::<NetHost>()
            .control(peer)
            .and_then(|buffer| buffer.axis(FlightCommand::Throttle))
            .is_some_and(|axis| (axis - 0.5).abs() < 0.0001),
        "the wire frame's axis reached the peer's buffer"
    );

    // The peer's own farewell is the departure notice: the drive releases
    // the buffer it owned rather than leaving a ghost stick deflected.
    linked
        .world()
        .resource_mut::<NetClient>()
        .session_mut()
        .leave()
        .expect("a live client can leave");
    linked.step_until("the host never saw the departure", |report| {
        !report.released_pilots.is_empty()
    });
    let report = linked.report();
    let released = report
        .released_pilots
        .iter()
        .find(|pilot| pilot.peer == peer)
        .expect("the departed peer's pilot was released");
    assert_eq!(released.actor, plane());
    assert_eq!(
        released.neutralized, 1,
        "the held throttle axis was neutralized"
    );

    assert!(
        linked.world().resource::<NetHost>().control(peer).is_none(),
        "the departed peer's buffer is gone"
    );
    assert_eq!(
        linked
            .world()
            .resource::<NetHost>()
            .session()
            .gate()
            .ownership()
            .owner(plane()),
        None,
        "the gate's ownership was released with it — no ghost may rebind"
    );
    // A successor may bind the freed aircraft: the aircraft is nobody's now.
    linked
        .world()
        .resource_mut::<NetHost>()
        .bind_aircraft(peer, plane())
        .expect("the released aircraft binds again");
    assert_eq!(
        linked.world().resource::<NetHost>().actor_of(peer),
        Some(plane())
    );
}

#[test]
fn accept_f54_c_drive_an_unbound_peer_is_named_not_owned() {
    let mut linked = LinkedWorld::new();
    linked.step_until("the handshake never granted", |report| {
        report.client_session == Some(session_id())
    });
    let peer = linked
        .world()
        .resource::<NetClient>()
        .session()
        .grant()
        .expect("the client holds a grant")
        .peer;

    // No bind_aircraft: the peer is a session member with no aircraft. Its
    // input is admitted — the gate admits members — and the drive names the
    // missing binding instead of folding the frames somewhere unsound.
    linked
        .world()
        .resource_mut::<NetClient>()
        .session_mut()
        .submit_sample(FlightCommand::Throttle, 0.75, Tick(101))
        .expect("a finite throttle sample queues");
    linked.step_until("the admitted packet was never consumed", |report| {
        report.consumed.iter().any(|input| input.peer == peer)
    });
    let report = linked.report();
    let consumed = report
        .consumed
        .iter()
        .find(|input| input.peer == peer)
        .expect("the peer's packet was consumed");
    assert!(consumed.unbound, "the peer held no bound aircraft");
    assert_eq!(consumed.frames_applied, 0);
    assert_eq!(
        consumed.intents, 0,
        "an unbound peer's edges mint no intents"
    );
    assert_eq!(
        report.pending_orders, 0,
        "nothing queued for the weapon schedule"
    );
}

/// A peer moves aircraft at most one at a time: rebinding surrenders the
/// prior gate record, and a record written straight into the gate — outside
/// `bind_aircraft` — still dies with the peer that held it. Neither may
/// leave an aircraft owned by a peer that no longer flies it.
#[test]
fn accept_f54_c_drive_a_rebound_or_gate_only_peer_keeps_no_aircraft() {
    let _loopback = loopback();
    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let server = ServerSession::bind(session_id(), synthetic_parameters(), bind, Duration::ZERO)
        .expect("the host socket binds");
    let mut host = NetHost::new(server, spawned_ledger(), origin()).expect("the epochs agree");
    let peer = PeerId::new(9).expect("a nonzero peer id");
    let other = ActorId {
        session: session_id(),
        serial: PLANE_SERIAL + 1,
    };

    // The rebind: the gate's "one peer flies at most one aircraft" is a rule
    // this binding keeps, so the first aircraft's record cannot outlive the
    // move — and `actor_of`, which the wire's `fire_requests` reads, can only
    // ever name the aircraft the peer actually flies.
    host.bind_aircraft(peer, plane()).expect("the first bind");
    host.bind_aircraft(peer, other).expect("the rebind");
    assert_eq!(host.actor_of(peer), Some(other));
    assert_eq!(
        host.session().gate().ownership().owner(plane()),
        None,
        "the first aircraft was surrendered with the move"
    );
    assert_eq!(
        host.session().gate().ownership().owner(other),
        Some(peer),
        "the second aircraft is the peer's now"
    );

    // The bypass: a record written through `session_mut().gate_mut()`
    // directly — no `ControlBuffer` was ever opened for it — still releases
    // with the peer, named rather than left as a ghost.
    host.session_mut()
        .gate_mut()
        .ownership_mut()
        .bind(peer, plane())
        .expect("the direct gate bind");
    let released = host
        .unbind_aircraft(peer)
        .expect("every record the peer held releases together");
    assert_eq!(released.peer, peer);
    assert_eq!(released.actor, other, "the buffer's actor is reported");
    assert_eq!(released.neutralized, 0);
    assert_eq!(released.dropped_edges, 0);
    assert_eq!(
        host.session().gate().ownership().owner(plane()),
        None,
        "the record that bypassed `bind_aircraft` released with the peer"
    );
    assert_eq!(
        host.session().gate().ownership().owner(other),
        None,
        "the bound record released with the peer"
    );
    assert!(
        host.unbind_aircraft(peer).is_none(),
        "a released peer is bound nowhere"
    );
}

#[test]
fn accept_f54_c_drive_missing_authorities_are_named_not_guessed() {
    let _loopback = loopback();
    let mut world = World::new();
    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let server = ServerSession::bind(session_id(), synthetic_parameters(), bind, Duration::ZERO)
        .expect("the host socket binds");
    world.insert_resource(
        NetHost::new(server, NetStateLedger::new(session_id()), origin())
            .expect("the epochs agree"),
    );

    // No Time<Fixed>, no PhysicsTickLedger: the run still pumps the session
    // — arrived traffic is still delivered — and names both authorities it
    // lacked rather than guessing a step length or a committed tick.
    drive_net_session(&mut world);
    let report = world.resource::<NetSessionReport>();
    assert_eq!(report.ticks, 1);
    assert_eq!(report.at, None);
    assert_eq!(
        report.missing,
        vec![NetDriveMissing::Clock, NetDriveMissing::Timeline]
    );
    assert_eq!(
        report.published,
        Some(NetPublish::Skipped("gathering")),
        "a Gathering session publishes nothing"
    );

    // A Live phase with no committed tick names the missing tick, not a
    // snapshot at a made-up one.
    world
        .resource_mut::<NetHost>()
        .session_mut()
        .launch(Tick(0))
        .expect("a gathering session launches");
    drive_net_session(&mut world);
    let report = world.resource::<NetSessionReport>();
    assert_eq!(report.ticks, 2);
    assert_eq!(
        report.published,
        Some(NetPublish::Skipped("no committed tick")),
        "a Live session with no committed tick names it"
    );

    // The host assembly's own contract: a ledger serving another epoch
    // refuses rather than publishing one generation's state under another's
    // header.
    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let server = ServerSession::bind(session_id(), synthetic_parameters(), bind, Duration::ZERO)
        .expect("the host socket binds");
    let foreign_ledger = NetStateLedger::new(foreign_session_id());
    assert_eq!(
        NetHost::new(server, foreign_ledger, origin())
            .map(|_| ())
            .unwrap_err(),
        NetHostError::ForeignSession {
            session: session_id(),
            ledger: foreign_session_id(),
        },
        "a session and a ledger of different epochs cannot bind"
    );
}
