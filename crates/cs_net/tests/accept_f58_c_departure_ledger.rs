//! Acceptance scenario F58-C (rules half): the disconnect flow's once-only
//! guard and its bounded reason.
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-C`, minimum scenario "Disconnect during docking or objective
//! carry; authoritative state resolves once"; non-negotiable 3 (a clean host
//! loss, no migration). Contract: `docs/contracts/UI-NETWORK.md`
//! ("Reliable delivery does not replace application idempotency because
//! reconnect/retry can replay requests"). Task test prefix: `accept_f58_c_`.
//!
//! The scenario's *state* half is driven through the app boundary in
//! `crates/cs_app/tests/accept_f58_c_disconnect_settles_carried_objectives.rs`
//! and `..._host_loss_ends_the_match_once.rs`; these tests pin the rules those
//! flows stand on. Every value is synthetic; nothing here is original data.

use cs_net::bounds::MAX_SESSION_PEERS;
use cs_net::message::DisconnectReason;
use cs_net::recovery::{DepartureError, DepartureLedger, DisconnectCause, Settlement};
use cs_types::Tick;
use cs_types::net::PeerId;

fn peer(number: u16) -> PeerId {
    PeerId::new(number).expect("a nonzero peer number")
}

#[test]
fn accept_f58_c_each_peer_departs_once_and_a_duplicate_report_settles_nothing() {
    let mut ledger = DepartureLedger::new();
    assert!(ledger.is_empty());
    assert!(!ledger.is_settled(peer(1)));
    assert_eq!(ledger.get(peer(1)), None);
    assert_eq!(ledger.check(peer(1)), Ok(()));

    let at = Tick(12);
    let first = ledger
        .record(peer(1), DisconnectCause::Voluntary, at)
        .expect("a fresh ledger records the first departure");
    assert!(
        first.applied(),
        "the first report is the one that settles: {first}"
    );
    assert_eq!(first.tick(), at);
    assert_eq!(ledger.len(), 1);
    assert!(ledger.is_settled(peer(1)));
    let record = ledger.get(peer(1)).expect("the departure was recorded");
    assert_eq!(record.cause, DisconnectCause::Voluntary);
    assert_eq!(record.tick, at);

    // The transport reports the same peer again (a retransmitted farewell, or
    // a dead link the client already said goodbye to). The record keeps the
    // first report and the second applies nothing — this is what makes "the
    // authoritative state resolves once" hold across two producers.
    let second = ledger
        .record(peer(1), DisconnectCause::Timeout, Tick(13))
        .expect("a duplicate report is not an error");
    assert!(
        !second.applied(),
        "a duplicate must not settle again: {second}"
    );
    assert_eq!(second, Settlement::Duplicate { tick: at });
    assert_eq!(
        second.tick(),
        at,
        "the duplicate names the tick that settled"
    );
    assert_eq!(
        ledger.len(),
        1,
        "a duplicate report must not grow the ledger"
    );
    assert_eq!(
        ledger.get(peer(1)).map(|record| record.cause),
        Some(DisconnectCause::Voluntary),
        "the first report is the one of record, whatever arrives later"
    );

    // A peer that has not departed is unaffected by its neighbour's record.
    assert_eq!(ledger.check(peer(2)), Ok(()));
    assert!(
        ledger
            .record(peer(2), DisconnectCause::HostLoss, Tick(14))
            .expect("room for a second peer")
            .applied()
    );
    assert_eq!(ledger.len(), 2);
}

#[test]
fn accept_f58_c_the_departure_ledger_is_bounded_by_the_peer_id_space() {
    let mut ledger = DepartureLedger::new();

    // The session can hold at most MAX_SESSION_PEERS members, and
    // `PeerAllocator` never issues an id above that, so the ledger fills at
    // exactly the same bound and never earlier.
    for number in 1..=(MAX_SESSION_PEERS as u16) {
        let departing = peer(number);
        assert_eq!(
            ledger.check(departing),
            Ok(()),
            "the ledger has room until the peer space is spent"
        );
        let settled = ledger
            .record(departing, DisconnectCause::Timeout, Tick(1))
            .expect("room inside the peer space");
        assert!(settled.applied());
    }
    assert_eq!(ledger.len(), MAX_SESSION_PEERS);

    // An unknown peer beyond the bound is refused **by name**, and nothing is
    // inserted: a flood of departure notices cannot grow the ledger.
    let stranger = peer(u16::try_from(MAX_SESSION_PEERS).expect("fits a u16") + 1);
    let full = DepartureError::LedgerFull {
        max: MAX_SESSION_PEERS,
    };
    assert_eq!(ledger.check(stranger), Err(full));
    assert_eq!(
        ledger.record(stranger, DisconnectCause::Timeout, Tick(1)),
        Err(full)
    );
    assert_eq!(
        ledger.len(),
        MAX_SESSION_PEERS,
        "a refused record must not grow the ledger"
    );

    // A peer already of record is still answered when the ledger is full —
    // its duplicate report must not become an error just because the room
    // ran out.
    assert_eq!(
        ledger.record(peer(1), DisconnectCause::Voluntary, Tick(9)),
        Ok(Settlement::Duplicate { tick: Tick(1) })
    );
    assert!(ledger.len() <= MAX_SESSION_PEERS);
}

#[test]
fn accept_f58_c_a_disconnect_cause_carries_only_a_bounded_reason() {
    assert_eq!(DisconnectCause::Voluntary.label(), "voluntary");
    assert_eq!(DisconnectCause::Timeout.label(), "timeout");
    assert_eq!(DisconnectCause::HostLoss.label(), "host_loss");
    assert!(!DisconnectCause::Voluntary.is_abuse());
    assert!(!DisconnectCause::Timeout.is_abuse());
    assert!(!DisconnectCause::HostLoss.is_abuse());

    // The abuse arm names the refusal that cut the peer off — a label the
    // identity and intent layers already bound, never a free-form message
    // that could grow with the traffic that triggered it.
    let abuse = DisconnectCause::Abusive {
        reason: "impossible_rate",
    };
    assert!(abuse.is_abuse());
    assert_eq!(abuse.label(), "abusive");
    assert_eq!(abuse.to_string(), "cut off for abuse: impossible_rate");
    assert_eq!(
        DisconnectCause::HostLoss.to_string(),
        "the host is gone",
        "every cause still reports a bounded sentence"
    );

    // The wire can express three of the four causes.
    assert_eq!(
        DisconnectCause::Voluntary.wire_reason(),
        Some(DisconnectReason::Voluntary)
    );
    assert_eq!(
        DisconnectCause::Timeout.wire_reason(),
        Some(DisconnectReason::Timeout)
    );
    assert_eq!(
        DisconnectCause::HostLoss.wire_reason(),
        Some(DisconnectReason::SessionEnded)
    );
    // …and refuses to lie about the fourth: `DisconnectReason` has no abuse
    // arm (task #815), so an abusive peer is hung up without a reason packet
    // rather than told the session merely "ended".
    assert_eq!(abuse.wire_reason(), None);
}
