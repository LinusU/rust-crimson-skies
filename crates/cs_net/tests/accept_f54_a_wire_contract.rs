//! F54-A acceptance: wire messages and authoritative ownership.
//!
//! Minimum scenario (spec F54 AC01): an unsupported protocol or content hash
//! is rejected with a clear reason. The rest of the file pins the contract's
//! invariants the vocabulary must express: epoch rejection of stale packets,
//! directional authority, delivery classification, input/snapshot bounds and
//! session-qualified identity.

use cs_net::authority::{AuthorityDomain, Owner};
use cs_net::bounds::{
    MAX_EDGES_PER_FRAME, MAX_INPUT_BATCH_SPAN_TICKS, MAX_INPUT_FRAMES_PER_PACKET,
    MAX_SNAPSHOT_BYTES,
};
use cs_net::compat::{
    ClientHello, CompatError, HandshakeReject, PROTOCOL_VERSION, ProtocolVersion, evaluate_hello,
};
use cs_net::fixture::{
    SYNTHETIC_CONTENT_SHA256, SYNTHETIC_PEER, SYNTHETIC_SESSION, synthetic_blueprint_id,
    synthetic_hello, synthetic_input_message, synthetic_parameters, synthetic_peer_joined,
};
use cs_net::message::{
    ClientMessage, ClientPayload, Delivery, DisconnectReason, EventBody, FinishReason, InputBatch,
    MessageHeader, Origin, ReliableEvent, ServerMessage, ServerPayload, SessionMessage,
    SnapshotFrame, WireError,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::input::{Action, AxisValue, FlightCommand, InputFrame, UiAction};
use cs_types::net::{ActorAllocator, ActorId, EventId, PeerId, SessionId};

fn hello_with_protocol(version: u16) -> ClientHello {
    let mut hello = synthetic_hello();
    hello.protocol = ProtocolVersion::new(version).expect("nonzero version");
    hello
}

#[test]
fn accept_f54_a_unsupported_protocol_is_rejected_with_reason() {
    let params = synthetic_parameters();
    let hello = hello_with_protocol(PROTOCOL_VERSION.get() + 1);

    let Err(reject) = evaluate_hello(&params, &hello) else {
        panic!("a foreign protocol version must not clear the launch gate");
    };
    assert_eq!(
        reject,
        HandshakeReject::UnsupportedProtocol {
            offered: ProtocolVersion::new(PROTOCOL_VERSION.get() + 1).unwrap(),
            supported: PROTOCOL_VERSION,
        }
    );
    let reason = reject.to_string();
    assert!(
        reason.contains("unsupported protocol"),
        "the reason must say what was rejected: {reason}"
    );
    assert!(
        reason.contains(&PROTOCOL_VERSION.get().to_string()),
        "the reason must name the supported version: {reason}"
    );
}

#[test]
fn accept_f54_a_content_hash_mismatch_is_rejected_with_reason() {
    let params = synthetic_parameters();
    let mut hello = synthetic_hello();
    hello.compatibility.content_sha256 = ContentHash::from_bytes([0xDE; 32]);

    let Err(reject) = evaluate_hello(&params, &hello) else {
        panic!("a foreign content signature must not clear the launch gate");
    };
    assert_eq!(
        reject,
        HandshakeReject::ContentMismatch {
            expected: SYNTHETIC_CONTENT_SHA256,
            offered: ContentHash::from_bytes([0xDE; 32]),
        }
    );
    let reason = reject.to_string();
    assert!(
        reason.contains("content mismatch"),
        "the reason must say which signature differed: {reason}"
    );
    assert!(
        reason.contains(&SYNTHETIC_CONTENT_SHA256.to_hex()),
        "the reason must name the expected signature: {reason}"
    );
}

#[test]
fn accept_f54_a_rules_and_mod_mismatches_are_distinct_rejections() {
    let params = synthetic_parameters();

    let mut rules = synthetic_hello();
    rules.compatibility.rules_sha256 = ContentHash::from_bytes([0x11; 32]);
    assert!(matches!(
        evaluate_hello(&params, &rules),
        Err(HandshakeReject::RulesMismatch { .. })
    ));

    let mut modded = synthetic_hello();
    modded.compatibility.mods = vec![synthetic_blueprint_id()];
    let Err(HandshakeReject::ModSetMismatch {
        missing,
        unexpected,
    }) = evaluate_hello(&params, &modded)
    else {
        panic!("an extra enabled mod must be rejected");
    };
    assert!(missing.is_empty());
    assert_eq!(unexpected, vec![synthetic_blueprint_id()]);

    // The mirror image: the session requires a mod the client lacks.
    let mut required = synthetic_parameters();
    required.compatibility.mods = vec![synthetic_blueprint_id()];
    let Err(HandshakeReject::ModSetMismatch {
        missing,
        unexpected,
    }) = evaluate_hello(&required, &synthetic_hello())
    else {
        panic!("a missing required mod must be rejected");
    };
    assert_eq!(missing, vec![synthetic_blueprint_id()]);
    assert!(unexpected.is_empty());
}

#[test]
fn accept_f54_a_malformed_hello_is_rejected() {
    let params = synthetic_parameters();
    let mut hello = synthetic_hello();
    let id = synthetic_blueprint_id();
    hello.compatibility.mods = vec![id.clone(), id];

    assert_eq!(
        evaluate_hello(&params, &hello),
        Err(HandshakeReject::MalformedHello(CompatError::DuplicateMod {
            id: synthetic_blueprint_id(),
        }))
    );
}

#[test]
fn accept_f54_a_matching_hello_is_accepted_before_launch() {
    // The accept path is pure: it commits nothing, so a later rejection or a
    // crash here can never leave half a session.
    assert_eq!(
        evaluate_hello(&synthetic_parameters(), &synthetic_hello()),
        Ok(())
    );
}

#[test]
fn accept_f54_a_stale_session_epoch_is_rejected() {
    let message = synthetic_input_message(7);
    let live = SessionId::new(2).expect("nonzero session");

    assert_eq!(
        message.expect_session(live),
        Err(WireError::StaleSession {
            expected: live,
            found: SYNTHETIC_SESSION,
        })
    );
    assert_eq!(message.expect_session(SYNTHETIC_SESSION), Ok(()));

    let packet = SessionMessage::ToClient(ServerMessage {
        header: MessageHeader {
            session: SYNTHETIC_SESSION,
            sequence: 0,
        },
        payload: ServerPayload::Disconnect {
            reason: DisconnectReason::SessionEnded,
        },
    });
    assert!(matches!(
        packet.expect_session(live),
        Err(WireError::StaleSession { .. })
    ));
}

#[test]
fn accept_f54_a_server_payloads_never_originate_from_a_client() {
    // A packet that claims server authority but arrived from a client is
    // refused at the codec boundary; a client-bound packet cannot be sent by
    // a client at all (the type has no client-side constructor).
    let spawn = ServerMessage {
        header: MessageHeader {
            session: SYNTHETIC_SESSION,
            sequence: 3,
        },
        payload: ServerPayload::Event(synthetic_peer_joined(SYNTHETIC_PEER, Tick(0), 0)),
    };
    let from_client = SessionMessage::ToClient(spawn);
    assert_eq!(
        from_client.verify_origin(Origin::Client),
        Err(WireError::WrongOrigin {
            claimed: Origin::Client,
            payload: "server-owned",
        })
    );
    assert_eq!(from_client.verify_origin(Origin::Server), Ok(()));

    let input = synthetic_input_message(1);
    let to_server = SessionMessage::ToServer(input);
    assert_eq!(to_server.verify_origin(Origin::Client), Ok(()));
    assert!(matches!(
        to_server.verify_origin(Origin::Server),
        Err(WireError::WrongOrigin { .. })
    ));
}

#[test]
fn accept_f54_a_client_payloads_are_requests_not_authority() {
    // Exhaustive by construction: a new client payload cannot be added
    // without editing this match, so the vocabulary can never silently grow
    // a server-owned client verb.
    for payload in [
        ClientPayload::Input(InputBatch { frames: Vec::new() }),
        ClientPayload::Leave,
    ] {
        match payload {
            // Local input requests: client-owned, sequenced, acked.
            ClientPayload::Input(_) => {}
            // A farewell request: client-owned, reliable.
            ClientPayload::Leave => {}
        }
    }
}

#[test]
fn accept_f54_a_ownership_table_matches_the_contract() {
    for domain in AuthorityDomain::ALL {
        let expected = match domain {
            AuthorityDomain::ActorAllocation
            | AuthorityDomain::PhysicsTruth
            | AuthorityDomain::WeaponAcceptance
            | AuthorityDomain::HitDamage
            | AuthorityDomain::FactionInteraction
            | AuthorityDomain::MissionProgram
            | AuthorityDomain::ScoreResult
            | AuthorityDomain::LobbyRules
            | AuthorityDomain::SessionMembership => Owner::Server,
            AuthorityDomain::LocalInput
            | AuthorityDomain::UiState
            | AuthorityDomain::PredictedCosmetics => Owner::Client,
        };
        assert_eq!(domain.owner(), expected, "{}", domain.label());
    }
}

#[test]
fn accept_f54_a_delivery_classification_matches_reliability_contract() {
    let reliable_server = [
        ServerPayload::Event(synthetic_peer_joined(SYNTHETIC_PEER, Tick(0), 0)),
        ServerPayload::Disconnect {
            reason: DisconnectReason::SessionEnded,
        },
    ];
    for payload in &reliable_server {
        assert_eq!(payload.delivery(), Delivery::Reliable);
    }
    let sequenced_server = [
        ServerPayload::InputAck { through: 3 },
        ServerPayload::Snapshot(SnapshotFrame {
            tick: Tick(4),
            payload: Vec::new(),
        }),
    ];
    for payload in &sequenced_server {
        assert_eq!(payload.delivery(), Delivery::Sequenced);
    }
    assert_eq!(ClientPayload::Leave.delivery(), Delivery::Reliable);
    assert_eq!(
        ClientPayload::Input(InputBatch { frames: Vec::new() }).delivery(),
        Delivery::Sequenced
    );
}

#[test]
fn accept_f54_a_input_batch_bounds_are_enforced() {
    let session = SYNTHETIC_SESSION;
    let message = |batch: InputBatch| ClientMessage {
        header: MessageHeader {
            session,
            sequence: 0,
        },
        payload: ClientPayload::Input(batch),
    };

    // Empty, over-count, out-of-order, over-span and UI-action batches all
    // fail at the boundary.
    assert_eq!(
        message(InputBatch { frames: Vec::new() }).validate(),
        Err(WireError::Empty {
            field: "input.frames"
        })
    );

    let over_count: Vec<InputFrame> = (0..=MAX_INPUT_FRAMES_PER_PACKET as u64)
        .map(|i| InputFrame::new(Tick(i + 1)))
        .collect();
    assert!(matches!(
        message(InputBatch { frames: over_count }).validate(),
        Err(WireError::TooMany {
            field: "input.frames",
            ..
        })
    ));

    let unordered = vec![InputFrame::new(Tick(5)), InputFrame::new(Tick(5))];
    assert_eq!(
        message(InputBatch { frames: unordered }).validate(),
        Err(WireError::TicksOutOfOrder {
            tick: Tick(5),
            previous: Tick(5),
        })
    );

    let overspan = vec![
        InputFrame::new(Tick(1)),
        InputFrame::new(Tick(2 + MAX_INPUT_BATCH_SPAN_TICKS)),
    ];
    assert!(matches!(
        message(InputBatch { frames: overspan }).validate(),
        Err(WireError::SpanTooWide { .. })
    ));

    let mut ui = InputFrame::new(Tick(1));
    ui.push_edge(Action::Ui(UiAction::Pause));
    assert_eq!(
        message(InputBatch { frames: vec![ui] }).validate(),
        Err(WireError::UiActionOnWire {
            action: UiAction::Pause
        })
    );

    let mut storm = InputFrame::new(Tick(1));
    for _ in 0..=MAX_EDGES_PER_FRAME {
        storm.push_edge(Action::Flight(FlightCommand::FirePrimary));
    }
    assert!(matches!(
        message(InputBatch {
            frames: vec![storm]
        })
        .validate(),
        Err(WireError::TooMany {
            field: "input.edges",
            ..
        })
    ));

    // The fixture packet is valid.
    assert_eq!(synthetic_input_message(0).validate(), Ok(()));
}

#[test]
fn accept_f54_a_oversized_snapshot_payload_is_rejected() {
    let snapshot = ServerMessage {
        header: MessageHeader {
            session: SYNTHETIC_SESSION,
            sequence: 0,
        },
        payload: ServerPayload::Snapshot(SnapshotFrame {
            tick: Tick(9),
            payload: vec![0; MAX_SNAPSHOT_BYTES + 1],
        }),
    };
    assert_eq!(
        snapshot.validate(),
        Err(WireError::TooLarge {
            field: "snapshot.payload",
            max: MAX_SNAPSHOT_BYTES,
            len: MAX_SNAPSHOT_BYTES + 1,
        })
    );
}

#[test]
fn accept_f54_a_ids_are_session_qualified_and_never_zero() {
    // Zero is never a live id: a default or truncated field cannot alias a
    // session, peer or actor.
    assert_eq!(SessionId::new(0), None);
    assert_eq!(PeerId::new(0), None);

    // The same producer/sequence/tick in a different session is a different
    // event: reliable-event dedup can never merge across generations.
    let session_b = SessionId::new(2).expect("nonzero session");
    let a = EventId {
        session: SYNTHETIC_SESSION,
        tick: Tick(1),
        producer: 0,
        sequence: 0,
    };
    let b = EventId {
        session: session_b,
        tick: Tick(1),
        producer: 0,
        sequence: 0,
    };
    assert_ne!(a, b);

    // Server actor allocation is monotonic and never recycles a serial.
    let mut alloc = ActorAllocator::new(SYNTHETIC_SESSION);
    let first = alloc.allocate().expect("first serial");
    let second = alloc.allocate().expect("second serial");
    assert_eq!(
        first,
        ActorId {
            session: SYNTHETIC_SESSION,
            serial: 1
        }
    );
    assert_eq!(second.serial, first.serial + 1);

    // An allocator bound to another session mints ids of that session.
    let mut other = ActorAllocator::new(session_b);
    assert_eq!(other.allocate().expect("serial").session, session_b);
}

#[test]
fn accept_f54_a_reliable_events_carry_contract_event_ids() {
    // Reliable semantic events deduplicate on EventId, not transport
    // delivery (UI-NETWORK): two deliveries of one event are the same id,
    // and distinct events of one producer order by sequence.
    let event = synthetic_peer_joined(SYNTHETIC_PEER, Tick(5), 9);
    let redelivery = event.clone();
    assert_eq!(event.id, redelivery.id);

    let next = ReliableEvent {
        id: EventId {
            sequence: 10,
            ..event.id
        },
        body: EventBody::Finished {
            reason: FinishReason::Completed,
        },
    };
    assert!(event.id < next.id);

    let spawn = EventBody::ActorSpawned {
        actor: ActorId {
            session: SYNTHETIC_SESSION,
            serial: 4,
        },
        blueprint: ContentId::from_source(ContentKind::Blueprint, "synthetic_zephyr")
            .expect("valid id"),
        owner: Some(SYNTHETIC_PEER),
    };
    assert!(matches!(spawn, EventBody::ActorSpawned { .. }));
}

#[test]
fn accept_f54_a_axis_values_stay_quantized_on_the_wire() {
    // Input axes are i16-quantized by construction (F22-A): a NaN or
    // out-of-range float can never be put into a wire input packet because
    // no wire field carries a float.
    assert!(AxisValue::from_unit(FlightCommand::Throttle, f32::NAN).is_err());
    assert!(AxisValue::from_unit(FlightCommand::Throttle, 1.5).is_err());
    assert!(AxisValue::from_unit(FlightCommand::FirePrimary, 0.5).is_err());
}
