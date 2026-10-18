//! Acceptance scenario F58-C follow-up (#1590): the app-side host pump,
//! `drive_net_session` as `NetSessionDrivePlugin` installs it, judges input
//! with a `MatchStage`, so the F58-B caps are live in the running app. Task
//! test prefix: `accept_f58_c_app_pump_stage_`.
//!
//! The host is a real `NetHost` in a production `PhysicsSession`; the peer is
//! a raw `ClientTransport` that completed the production handshake over UDP
//! loopback (`network_local`, never `network_real`). The stage's tick is the
//! committed `PhysicsTickLedger` tick and its phase is the session's
//! `ServerPhase` (`Live` is `InMatch`). Nothing is judged test-side. No
//! original behavior and no measured rate or window is asserted.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::MutexGuard;
use std::time::Duration;

use bevy::ecs::change_detection::Mut;

use cs_app::network::session::{NetHost, NetSessionDrivePlugin, NetSessionReport};
use cs_app::origin::{OriginEpoch, WorldOrigin};
use cs_app::physics::PhysicsSession;
use cs_net::codec::encode_client_message;
use cs_net::fixture::{synthetic_hello, synthetic_parameters};
use cs_net::lifecycle::{ServerNotice, ServerSession};
use cs_net::transport::{CHANNEL_SEQUENCED, ClientTransport, ConnectWindow, DropReason};
use cs_net::validation::{
    IntentRefusal, MAX_INPUT_TICKS_AHEAD, RateLimits, ThreatCase, synthetic_fire_message,
};
use cs_sim::net_state::{NetActorState, NetStateLedger};
use cs_types::Tick;
use cs_types::net::{ActorId, PeerId, SessionId};
use cs_types::space::{Quaternion, WorldPosition};

const SESSION: u64 = 91;
const FIXED_HZ: u32 = 60;
const MAX_TICKS: u64 = 600;
const STEP: Duration = Duration::from_millis(16);
const LOOPBACK_WINDOW: ConnectWindow = ConnectWindow::new(120);

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session id")
}

fn plane() -> ActorId {
    ActorId {
        session: session_id(),
        serial: 1,
    }
}

struct Fixture {
    session: PhysicsSession,
    peer: ClientTransport,
    id: PeerId,
    next_sequence: u32,
    notices: Vec<ServerNotice>,
    consumed: usize,
    _loopback: MutexGuard<'static, ()>,
}

impl Fixture {
    /// A launched (`Live`) host with one joined, aircraft-bound peer.
    fn new() -> Self {
        Self::with_launch(true)
    }

    /// As [`Self::new`], launching the match only when `launch` is set.
    fn with_launch(launch: bool) -> Self {
        let loopback = super::accept_f54_c_drive::loopback();
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let server =
            ServerSession::bind(session_id(), synthetic_parameters(), bind, Duration::ZERO)
                .expect("the host socket binds");
        let addr = server.local_addr().expect("the host has an address");
        let peer = ClientTransport::connect_with_window(
            synthetic_hello(),
            addr,
            0xF58,
            Duration::ZERO,
            LOOPBACK_WINDOW,
        )
        .expect("the peer binds");
        let origin = WorldOrigin::new(
            OriginEpoch(1),
            WorldPosition::try_new([50_000.0, 500.0, -80_000.0]).expect("finite"),
        );
        let mut ledger = NetStateLedger::new(session_id());
        ledger
            .spawn(NetActorState::spawn(
                plane(),
                WorldPosition::try_new([50_100.0, 520.0, -80_300.0]).expect("finite"),
                Quaternion::IDENTITY,
                400,
            ))
            .expect("the first spawn succeeds");
        let mut session = PhysicsSession::production_builder()
            .fixed_hz(FIXED_HZ)
            .configure(|app| {
                app.add_plugins(NetSessionDrivePlugin);
            })
            .build();
        session
            .world_mut()
            .expect("the session is active")
            .insert_resource(NetHost::new(server, ledger, origin).expect("the epochs agree"));
        let mut fx = Self {
            session,
            peer,
            id: PeerId::new(1).expect("nonzero"),
            next_sequence: 0,
            notices: Vec::new(),
            consumed: 0,
            _loopback: loopback,
        };
        let mut granted = None;
        for _ in 0..MAX_TICKS {
            fx.peer.update(STEP);
            fx.step();
            if let Some(grant) = fx.peer.grant() {
                granted = Some(grant.peer);
                break;
            }
        }
        fx.id = granted.expect("the production handshake grants the peer");
        let id = fx.id;
        let mut host = fx.host();
        host.bind_aircraft(id, plane()).expect("the gate binds");
        if launch {
            host.session_mut()
                .launch(Tick(0))
                .expect("a gathering session launches");
        }
        fx
    }

    fn host(&mut self) -> Mut<'_, NetHost> {
        self.session
            .world_mut()
            .expect("the session is active")
            .resource_mut::<NetHost>()
    }

    /// One committed fixed tick, so one production `drive_net_session` run.
    fn step(&mut self) {
        self.session.step(1).expect("the session steps");
        let report = self
            .session
            .world()
            .and_then(|world| world.get_resource::<NetSessionReport>().cloned())
            .unwrap_or_default();
        self.notices.extend(report.host_notices.iter().cloned());
        self.consumed += report
            .consumed
            .iter()
            .filter(|input| input.peer == self.id)
            .count();
    }

    /// The committed tick the host's last run saw.
    fn committed(&self) -> Tick {
        self.session
            .world()
            .and_then(|world| world.get_resource::<NetSessionReport>())
            .and_then(|report| report.at)
            .expect("a committed tick exists")
    }

    /// Sends `count` fire packets stamped `tick`, then commits one tick.
    fn round(&mut self, count: u32, tick: Tick) {
        for _ in 0..count {
            let message = synthetic_fire_message(session_id(), tick, self.next_sequence);
            self.next_sequence += 1;
            let bytes = encode_client_message(&message).expect("a fire packet encodes");
            self.peer.send_encoded(CHANNEL_SEQUENCED, &bytes);
        }
        self.peer.update(STEP);
        self.step();
    }

    fn saw(&self, wanted: impl Fn(&ServerNotice) -> bool) -> bool {
        self.notices.iter().any(wanted)
    }

    fn is_member(&mut self) -> bool {
        let id = self.id;
        self.host().session().members().any(|member| member == id)
    }
}

#[test]
fn accept_f58_c_app_pump_stage_a_tick_outside_the_window_is_refused_and_absorbed() {
    let mut fx = Fixture::new();
    let id = fx.id;
    // Far beyond the committed tick plus the ahead window; the sim cannot
    // catch up with it during the few rounds the packet takes to arrive.
    let far = Tick(fx.committed().0 + MAX_INPUT_TICKS_AHEAD + 1_000);
    for _ in 0..MAX_TICKS {
        if fx.saw(|n| matches!(n, ServerNotice::IntentRefused { .. })) {
            break;
        }
        fx.round(1, far);
    }
    let refusal = fx
        .notices
        .iter()
        .find_map(|n| match n {
            ServerNotice::IntentRefused { peer, reason } if *peer == id => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the out-of-window tick was never refused: {:?}", fx.notices));
    assert!(matches!(
        refusal,
        IntentRefusal::TickOutsideWindow { tick, .. } if tick == far
    ));
    // Absorbed: the peer is kept and nothing reached the sim.
    assert!(fx.is_member(), "an absorbed refusal keeps the peer");
    assert!(!fx.saw(|n| matches!(n, ServerNotice::CutOff { .. })));
    for _ in 0..10 {
        fx.round(0, far);
    }
    assert_eq!(fx.consumed, 0, "a refused packet never reaches the sim");
}

#[test]
fn accept_f58_c_app_pump_stage_a_flooding_peer_is_cut_off_at_the_designed_budget() {
    let mut fx = Fixture::new();
    let limits = RateLimits::DESIGNED;
    let burst = limits.max_intents_per_tick() + 4;
    let id = fx.id;
    for _ in 0..MAX_TICKS {
        if !fx.is_member() {
            break;
        }
        // In-window ticks while `Live`: only the rate can refuse these.
        let tick = fx.committed();
        fx.round(burst, tick);
    }
    assert!(!fx.is_member(), "the flooder was cut off: {:?}", fx.notices);
    assert!(fx.saw(|n| matches!(
        n,
        ServerNotice::Dropped {
            peer: Some(peer),
            reason: DropReason::RateExceeded { limit, .. },
        } if *peer == id && *limit == limits.max_intents_per_window
    )));
    assert!(fx.saw(|n| matches!(
        n,
        ServerNotice::CutOff {
            peer: Some(peer),
            threat: ThreatCase::ImpossibleRate,
        } if *peer == id
    )));
}

#[test]
fn accept_f58_c_app_pump_stage_an_honest_in_match_peer_reaches_the_sim() {
    let mut fx = Fixture::new();
    for _ in 0..MAX_TICKS {
        if fx.consumed > 0 {
            break;
        }
        let tick = fx.committed();
        fx.round(1, tick);
    }
    assert!(
        fx.consumed > 0,
        "the honest in-window packet never reached the sim: {:?}",
        fx.notices
    );
    assert!(fx.is_member());
    assert!(!fx.saw(|n| matches!(
        n,
        ServerNotice::IntentRefused { .. } | ServerNotice::CutOff { .. }
    )));
}

#[test]
fn accept_f58_c_app_pump_stage_input_before_the_launch_is_refused_by_phase() {
    let mut fx = Fixture::with_launch(false);
    let id = fx.id;
    for _ in 0..MAX_TICKS {
        if fx.saw(|n| matches!(n, ServerNotice::IntentRefused { .. })) {
            break;
        }
        let tick = fx.committed();
        fx.round(1, tick);
    }
    assert!(
        fx.saw(|n| matches!(
            n,
            ServerNotice::IntentRefused {
                peer,
                reason: IntentRefusal::WrongPhase { .. },
            } if *peer == id
        )),
        "a gathering session takes no input: {:?}",
        fx.notices
    );
    assert!(fx.is_member(), "the refusal is absorbed");
    assert_eq!(fx.consumed, 0);
}
