//! Acceptance scenario F58-C follow-up (#1221): the host's `MatchStage` reaches
//! the pinned transport pump, so F58-B's rate budget and tick window are
//! enforced on real wire traffic. Task test prefix: `accept_f58_c_pump_stage_`.
//!
//! Every test drives a real `ServerSession` over UDP loopback with a raw
//! `ClientTransport` that completed the production handshake, and pumps the host
//! with `ServerSession::pump_staged`. Nothing is judged test-side. Loopback on
//! one machine is `network_local`; no original data and no `network_real`
//! evidence is involved, and no network rate or tick window is measured here.

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::net::{ActorId, PeerId, SessionAllocator, SessionId};

use cs_net::fixture::{synthetic_hello, synthetic_parameters};
use cs_net::lifecycle::{ServerNotice, ServerSession};
use cs_net::lobby::Phase;
use cs_net::message::ClientMessage;
use cs_net::transport::{CHANNEL_SEQUENCED, ClientTransport, ConnectWindow, DropReason};
use cs_net::validation::{
    IntentRefusal, MAX_INPUT_TICKS_BEHIND, MatchStage, RateLimits, ThreatCase,
    synthetic_fire_message,
};

const STEP: Duration = Duration::from_millis(16);
const ROUNDS: usize = 600;
const SERVER_TICK: Tick = Tick(1_000);

/// A host with one joined, aircraft-bound raw peer.
struct Fixture {
    host: ServerSession,
    peer: ClientTransport,
    id: PeerId,
    session: SessionId,
    banned: BTreeSet<ContentId>,
    notices: Vec<ServerNotice>,
    next_sequence: u32,
    _loopback: std::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    fn new() -> Self {
        let loopback = crate::support::loopback::lock();
        let session = SessionAllocator::new()
            .allocate()
            .expect("an epoch allocates");
        let mut host = ServerSession::bind(
            session,
            synthetic_parameters(),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            Duration::ZERO,
        )
        .expect("the host binds");
        let addr = host.local_addr().expect("the host has an address");
        let mut peer = ClientTransport::connect_with_window(
            synthetic_hello(),
            addr,
            0xF58,
            Duration::ZERO,
            ConnectWindow::new(30),
        )
        .expect("the peer binds");
        let mut id = None;
        for _ in 0..ROUNDS {
            peer.update(STEP);
            host.pump(STEP);
            if let Some(grant) = peer.grant() {
                id = Some(grant.peer);
                break;
            }
        }
        let id = id.expect("the production handshake grants the peer");
        host.gate_mut()
            .ownership_mut()
            .bind(id, ActorId { session, serial: 1 })
            .expect("the host binds the peer's aircraft");
        Self {
            host,
            peer,
            id,
            session,
            banned: BTreeSet::new(),
            notices: Vec::new(),
            next_sequence: 0,
            _loopback: loopback,
        }
    }

    fn message(&mut self, tick: Tick) -> ClientMessage {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        synthetic_fire_message(self.session, tick, sequence)
    }

    /// Sends `count` packets stamped `tick`, then one staged host round.
    fn round(&mut self, count: u32, tick: Tick, phase: Phase) {
        for _ in 0..count {
            let bytes = cs_net::codec::encode_client_message(&self.message(tick))
                .expect("a fire packet encodes");
            self.peer.send_encoded(CHANNEL_SEQUENCED, &bytes);
        }
        self.peer.update(STEP);
        let stage = MatchStage::new(phase, SERVER_TICK, &self.banned);
        let notices = self.host.pump_staged(STEP, stage);
        self.notices.extend(notices);
        self.host.drain_work();
    }

    fn saw(&self, wanted: impl Fn(&ServerNotice) -> bool) -> bool {
        self.notices.iter().any(wanted)
    }

    fn is_member(&self) -> bool {
        self.host.members().any(|member| member == self.id)
    }
}

#[test]
fn accept_f58_c_pump_stage_a_flooding_peer_is_cut_off_at_the_designed_budget_on_the_wire() {
    let mut fx = Fixture::new();
    let limits = fx.host.rate_limits();
    assert_eq!(limits, RateLimits::DESIGNED);
    let burst = limits.max_intents_per_tick() + 4;
    let id = fx.id;
    for _ in 0..ROUNDS {
        if !fx.is_member() {
            break;
        }
        // In-window ticks in an `InMatch` stage: only the rate can refuse these.
        fx.round(burst, SERVER_TICK, Phase::InMatch);
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
fn accept_f58_c_pump_stage_a_tick_outside_the_window_is_refused_and_absorbed() {
    let mut fx = Fixture::new();
    let id = fx.id;
    let stale = Tick(SERVER_TICK.0 - MAX_INPUT_TICKS_BEHIND - 1);
    for _ in 0..ROUNDS {
        if fx.saw(|n| matches!(n, ServerNotice::IntentRefused { .. })) {
            break;
        }
        fx.round(1, stale, Phase::InMatch);
    }
    let refusal = fx
        .notices
        .iter()
        .find_map(|n| match n {
            ServerNotice::IntentRefused { peer, reason } if *peer == id => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the stale tick was never refused: {:?}", fx.notices));
    assert!(matches!(
        refusal,
        IntentRefusal::TickOutsideWindow { tick, .. } if tick == stale
    ));
    // Absorbed: the peer is still a member, nothing was queued for it, and no
    // cut-off was reported.
    assert!(fx.is_member(), "an absorbed refusal keeps the peer");
    assert!(!fx.saw(|n| matches!(n, ServerNotice::Admitted(_) | ServerNotice::CutOff { .. })));
    // A phase that does not take input is absorbed the same way.
    let before = fx.notices.len();
    for _ in 0..ROUNDS {
        if fx.notices[before..].iter().any(|n| {
            matches!(
                n,
                ServerNotice::IntentRefused {
                    reason: IntentRefusal::WrongPhase { .. },
                    ..
                }
            )
        }) {
            break;
        }
        fx.round(1, SERVER_TICK, Phase::Gathering);
    }
    assert!(fx.notices[before..].iter().any(|n| matches!(
        n,
        ServerNotice::IntentRefused {
            reason: IntentRefusal::WrongPhase { .. },
            ..
        }
    )));
    assert!(fx.is_member());
    assert!(!fx.saw(|n| matches!(n, ServerNotice::Admitted(_))));
}

#[test]
fn accept_f58_c_pump_stage_an_honest_peer_inside_the_window_is_unaffected() {
    let mut fx = Fixture::new();
    let id = fx.id;
    // Both edges of the window are honest.
    let oldest = Tick(SERVER_TICK.0 - MAX_INPUT_TICKS_BEHIND);
    for tick in [SERVER_TICK, oldest] {
        let before = fx.notices.len();
        for _ in 0..ROUNDS {
            if fx.notices[before..]
                .iter()
                .any(|n| matches!(n, ServerNotice::Admitted(input) if input.peer == id))
            {
                break;
            }
            fx.round(1, tick, Phase::InMatch);
        }
        assert!(
            fx.notices[before..]
                .iter()
                .any(|n| matches!(n, ServerNotice::Admitted(input) if input.peer == id)),
            "tick {tick:?} was never admitted: {:?}",
            fx.notices
        );
    }
    assert!(fx.is_member());
    assert!(!fx.saw(|n| matches!(
        n,
        ServerNotice::IntentRefused { .. } | ServerNotice::CutOff { .. }
    )));
}
