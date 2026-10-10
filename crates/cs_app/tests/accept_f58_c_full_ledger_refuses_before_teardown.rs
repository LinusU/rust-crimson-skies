//! Acceptance scenario F58-C (failure case): the departure ledger's bound is
//! paid **before** any state moves.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-C`, non-negotiable 1 ("resource exhaustion" refused with a
//! bounded reason, never a panic) and the once-only rule behind AC03
//! ("authoritative state resolves once"). Contract:
//! `docs/contracts/UI-NETWORK.md` ("Wire ids are stable typed numeric/string
//! keys with bounded lengths"). Task test prefix: `accept_f58_c_`.
//!
//! The bound itself is pinned by
//! `crates/cs_net/tests/accept_f58_c_departure_ledger.rs`; these tests pin
//! what [`RecoveryFlow::depart`] does when the bound is reached **through the
//! flow**: the refusal arrives while the session is exactly as it was — no
//! aircraft released, no membership forgotten, nothing recorded — and a
//! duplicate report is still answered as one. Every value is synthetic;
//! nothing here is original data.
//!
//! [`RecoveryFlow::depart`]: cs_app::network::recovery::RecoveryFlow::depart

use cs_app::network::recovery::{FlowError, RecoveryFlow};
use cs_net::bounds::MAX_SESSION_PEERS;
use cs_net::recovery::{DepartureError, DisconnectCause, Settlement};
use cs_types::Tick;
use cs_types::net::{ActorId, PeerId, SessionId};

const SESSION: u64 = 404;

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session")
}

fn peer(number: u16) -> PeerId {
    PeerId::new(number).expect("a nonzero peer number")
}

/// Fills the flow's ledger with exactly `MAX_SESSION_PEERS` records — every
/// peer id the session's own `PeerAllocator` can hand out, which is the bound
/// the ledger copies — leaving `keep` unsettleable.
fn fill_ledger(flow: &mut RecoveryFlow, keep: u16) {
    for number in 1..=(MAX_SESSION_PEERS as u16 + 1) {
        if number == keep {
            continue;
        }
        flow.depart(peer(number), DisconnectCause::Timeout, Tick(3), None)
            .expect("room inside the peer space");
    }
    assert_eq!(
        flow.departures().len(),
        MAX_SESSION_PEERS,
        "the ledger is exactly full, never above its bound"
    );
}

/// A full ledger refuses by name, and the refusal leaves the live peer's
/// session untouched — the bound is paid before the teardown or the
/// settlement runs, so nothing is half-applied and unrecorded.
#[test]
fn accept_f58_c_a_full_ledger_refuses_before_anything_is_torn_down() {
    let mut flow = RecoveryFlow::new(session());
    fill_ledger(&mut flow, 1);

    // Peer 1 is still live: admitted and flying its aircraft.
    let aircraft = ActorId {
        session: session(),
        serial: 5,
    };
    flow.receiver_mut().admit_peer(peer(1));
    flow.receiver_mut()
        .bind_actor(peer(1), aircraft)
        .expect("the aircraft is free");

    // The next departure has no room, and the refusal names the bound.
    let error = flow
        .depart(peer(1), DisconnectCause::Voluntary, Tick(4), None)
        .expect_err("a full ledger refuses the next departure");
    assert!(
        matches!(
            &error,
            FlowError::Ledger(DepartureError::LedgerFull { max }) if *max == MAX_SESSION_PEERS
        ),
        "the refusal names the layer and the cap: {error}"
    );
    assert!(
        error.to_string().contains("could not be recorded"),
        "the error reports itself for a caller that only has Display: {error}"
    );

    // Nothing moved: the peer is still a member, still owns its aircraft and
    // is not settled, so the session is exactly as it was.
    assert!(
        flow.receiver().gate().is_member(peer(1)),
        "the bound is paid before the teardown, not after it"
    );
    assert_eq!(flow.receiver().actor_of(peer(1)), Some(aircraft));
    assert!(
        !flow.departures().is_settled(peer(1)),
        "a refused departure records nothing, so a later report can still settle it"
    );
    assert_eq!(flow.departures().len(), MAX_SESSION_PEERS);
}

/// A full ledger never turns a duplicate report into an error: the peer is
/// already of record, so the flow answers [`Settlement::Duplicate`] and
/// applies nothing — the once-only rule holds at the bound itself.
#[test]
fn accept_f58_c_a_full_ledger_still_answers_a_duplicate_report() {
    let mut flow = RecoveryFlow::new(session());
    fill_ledger(&mut flow, 1);

    // Peer 2 departed at tick 3 above; the ledger is now full.
    assert!(flow.departures().is_settled(peer(2)));
    let again = flow
        .depart(peer(2), DisconnectCause::Voluntary, Tick(9), None)
        .expect("a duplicate report is not an error, even when the ledger is full");
    assert_eq!(
        again.settlement,
        Settlement::Duplicate { tick: Tick(3) },
        "the duplicate names the tick that settled, not the report's own"
    );
    assert_eq!(
        again.cause,
        DisconnectCause::Timeout,
        "and the cause of record is the first report's"
    );
    assert!(
        again.dropped.is_empty(),
        "a duplicate report applies nothing, full ledger or not"
    );
    assert_eq!(flow.departures().len(), MAX_SESSION_PEERS);
}
