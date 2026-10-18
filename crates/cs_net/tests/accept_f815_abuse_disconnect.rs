//! Task #815: the wire `DisconnectReason` carries an abuse arm, so a peer cut
//! off for a named threat is told why. Task test prefix: `accept_f815_`.
//! Every value is synthetic.

use cs_net::codec::{CodecError, ServerPacket, decode_server_packet, encode_server_message};
use cs_net::message::{DisconnectReason, MessageHeader, ServerMessage, ServerPayload};
use cs_net::recovery::DisconnectCause;
use cs_net::validation::{IntentRefusal, ThreatCase, ThreatDisposition};
use cs_types::net::SessionId;

fn disconnect(reason: DisconnectReason) -> ServerMessage {
    ServerMessage {
        header: MessageHeader {
            session: SessionId::new(1).expect("nonzero session"),
            sequence: 1,
        },
        payload: ServerPayload::Disconnect { reason },
    }
}

fn round_trip(reason: DisconnectReason) -> DisconnectReason {
    let bytes = encode_server_message(&disconnect(reason)).expect("encodes");
    match decode_server_packet(&bytes).expect("decodes") {
        ServerPacket::Message(ServerMessage {
            payload: ServerPayload::Disconnect { reason },
            ..
        }) => reason,
        other => panic!("not a disconnect: {other:?}"),
    }
}

#[test]
fn accept_f815_a_cut_off_peer_receives_the_abuse_reason_on_the_wire() {
    let refusal = IntentRefusal::RateExceeded {
        limit: 96,
        window: 4,
    };
    assert_eq!(refusal.disposition(), ThreatDisposition::Disconnect);
    let cause = DisconnectCause::Abusive {
        reason: refusal.label(),
    };
    let reason = cause.wire_reason().expect("abuse is expressible");
    assert_eq!(
        reason,
        DisconnectReason::Abuse {
            threat: ThreatCase::ImpossibleRate
        }
    );
    assert_ne!(reason, DisconnectReason::SessionEnded);
    assert_eq!(round_trip(reason), reason);
}

#[test]
fn accept_f815_every_threat_class_round_trips_and_old_tags_stay_put() {
    for threat in ThreatCase::ALL {
        let reason = DisconnectReason::Abuse { threat: *threat };
        assert_eq!(round_trip(reason), reason);
        assert_eq!(ThreatCase::from_label(threat.label()), Some(*threat));
    }
    for reason in [
        DisconnectReason::Voluntary,
        DisconnectReason::Timeout,
        DisconnectReason::SessionEnded,
    ] {
        assert_eq!(round_trip(reason), reason);
    }
    let bytes = encode_server_message(&disconnect(DisconnectReason::SessionEnded)).unwrap();
    assert_eq!(*bytes.last().unwrap(), 2, "SessionEnded keeps tag 2");
}

#[test]
fn accept_f815_an_unknown_reason_or_threat_tag_fails_closed() {
    let good = encode_server_message(&disconnect(DisconnectReason::Abuse {
        threat: ThreatCase::ImpossibleRate,
    }))
    .unwrap();
    // [.. reason tag, threat tag]: poison each in turn.
    let mut bad_threat = good.clone();
    *bad_threat.last_mut().unwrap() = 250;
    assert_eq!(
        decode_server_packet(&bad_threat),
        Err(CodecError::UnknownTag {
            field: "disconnect.threat",
            tag: 250
        })
    );
    let mut bad_reason = good;
    let at = bad_reason.len() - 2;
    bad_reason[at] = 99;
    assert_eq!(
        decode_server_packet(&bad_reason),
        Err(CodecError::UnknownTag {
            field: "disconnect.reason",
            tag: 99
        })
    );
}

#[test]
fn accept_f815_a_label_naming_no_threat_is_not_mislabelled() {
    assert_eq!(
        DisconnectCause::Abusive { reason: "nonsense" }.wire_reason(),
        None
    );
}
