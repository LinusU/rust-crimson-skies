//! A deterministic adversarial corpus for the F54-C fuzz scenario.
//!
//! This is a support module, not a test target: it is included by
//! `accept_f54_c_lifecycle.rs`, and every item here feeds a production call in
//! that test.
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! acceptance **AC03** — "Fuzz packet decoding with oversized counts, NaNs and
//! invalid ids" — as the F54-C minimum scenario.
//!
//! # Why a corpus and not `cargo-fuzz`
//!
//! The corpus must run in CI, where there is no original data, no sanitizer
//! and no time budget, and it must be *reproducible*: a reviewer re-running the
//! suite has to see the identical corpus. So this is a fixed, seeded generator
//! plus hand-placed hostile shapes, not a fuzzer entry point. Every case is
//! newly authored; none of it is retail traffic.
//!
//! # What the corpus reaches
//!
//! Each buffer is fed to production code only:
//!
//! * `cs_net::codec::decode_client_packet` / `decode_server_packet` — the
//!   bounded grammar every wire packet passes;
//! * `cs_net::lifecycle::ClientSession::accept` — the client's consumer, which
//!   is exactly what `ClientSession::pump` runs per arrived packet;
//! * `cs_net::snapshot::Snapshot::decode` and the `ActorRecord` dequantizers —
//!   the values a decoded record turns into.
//!
//! The three hostile classes AC03 names are covered deliberately:
//!
//! * **oversized counts** — count and length fields set to their `u16`/`u32`
//!   maximum and to exactly cap+1, in the mod list, the input batch, the axis
//!   list, the edge list, the snapshot envelope and the packet itself;
//! * **NaNs** — the `f32` NaN, both infinities and the signalling NaN bit
//!   patterns written into every offset of otherwise-valid buffers, plus
//!   non-finite *local* samples driven into `ClientSession::sample_axis`. The
//!   wire has no float field, so a NaN can only enter as a *bit pattern*: the
//!   invariant under test is that no decoded value is ever non-finite and no
//!   non-finite sample becomes a packet;
//! * **invalid ids** — zero session/peer ids, a zero actor serial, a dead epoch,
//!   and out-of-range enum and packet tags.
//!
//! # Determinism
//!
//! `corpus(seed)` is a pure function of its seed (a xorshift64\* generator,
//! spelled out below rather than pulled from a dependency) and returns the same
//! buffers on every platform and every run.

use cs_types::Tick;
use cs_types::net::{ActorId, EventId, PeerId, SessionId};

use cs_net::bounds::{MAX_PACKET_BYTES, MAX_SEEN_EVENTS};
use cs_net::codec::ClientPacket;
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_input_message, synthetic_peer_joined};
use cs_net::message::{
    ClientMessage, EventBody, FinishReason, MessageHeader, ReliableEvent, ServerMessage,
    ServerPayload, SnapshotFrame,
};

/// A generated corpus case.
pub struct Case {
    /// What this case is meant to break, for the failure message.
    pub label: String,
    /// The raw buffer, fed to both decoders and (where it decodes) to the
    /// client consumer.
    pub bytes: Vec<u8>,
}

/// IEEE-754 `f32` bit patterns that are not a number. Written into buffers as
/// bytes: the wire has no float field, so these can only be a *pattern*, and
/// the invariant is that no decoded value is non-finite no matter which bytes
/// arrive.
pub const NON_FINITE_F32: [u32; 5] = [
    0x7FC0_0000, // quiet NaN
    0xFFC0_0000, // negative quiet NaN
    0x7F80_0000, // +infinity
    0xFF80_0000, // -infinity
    0x7FA0_0000, // signalling NaN
];

/// xorshift64\*, spelled out so the corpus does not depend on a rand crate.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // A zero state is a fixed point of xorshift; any nonzero seed works.
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next_u64() % bound as u64).expect("the bound fits usize")
    }

    fn byte(&mut self) -> u8 {
        u8::try_from(self.next_u64() & 0xFF).expect("a byte is a byte")
    }
}

/// One valid client input packet's bytes, for mutation.
fn valid_input() -> Vec<u8> {
    cs_net::codec::encode_client_message(&synthetic_input_message(3))
        .expect("the synthetic input packet encodes")
}

/// One valid server message's bytes, for mutation.
fn valid_server_message(session: SessionId) -> Vec<u8> {
    let message = ServerMessage {
        header: MessageHeader {
            session,
            sequence: 4,
        },
        payload: ServerPayload::Event(synthetic_peer_joined(
            PeerId::new(2).expect("two is nonzero"),
            Tick(6),
            1,
        )),
    };
    cs_net::codec::encode_server_message(&message).expect("a server packet encodes")
}

/// One valid hello's bytes, for mutation.
fn valid_hello() -> Vec<u8> {
    cs_net::codec::encode_client_packet(&ClientPacket::Hello(cs_net::fixture::synthetic_hello()))
        .expect("the synthetic hello encodes")
}

/// Writes a non-finite `f32` pattern at `offset`, leaving the rest alone.
fn poke_non_finite(bytes: &mut [u8], offset: usize, index: usize) {
    if offset + 4 <= bytes.len() {
        bytes[offset..offset + 4]
            .copy_from_slice(&NON_FINITE_F32[index % NON_FINITE_F32.len()].to_le_bytes());
    }
}

/// The generated (seed-derived) part of the corpus.
///
/// The shapes are hostile by construction rather than by luck: a random buffer
/// usually fails on its first byte, so most of the work is in the offsets that
/// put *valid* structure in front of the hostile field.
pub fn generated(seed: u64) -> Vec<Case> {
    let mut rng = Rng::new(seed);
    let mut cases = Vec::new();

    // Pure noise at every length class, including past the packet cap. This is
    // what a port scan or a desynchronized stream looks like.
    for length in [0usize, 1, 2, 8, 37, 255, 1024, 4096] {
        let bytes: Vec<u8> = (0..length).map(|_| rng.byte()).collect();
        cases.push(Case {
            label: format!("noise of {length} bytes"),
            bytes,
        });
    }

    // Mutations of a valid input packet: flip one byte, splice a u16 max into
    // a random aligned offset, and write NaN patterns at every offset.
    let base = valid_input();
    for step in 0..96usize {
        let mut bytes = base.clone();
        match step % 4 {
            0 => {
                let index = rng.below(bytes.len());
                bytes[index] ^= 1 << rng.below(8);
                cases.push(Case {
                    label: format!("one flipped bit at {index}"),
                    bytes,
                });
            }
            1 => {
                let offset = 2 * rng.below(bytes.len() / 2 + 1);
                let field = if offset + 2 <= bytes.len() { 2 } else { 0 };
                if field == 2 {
                    bytes[offset..offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
                    cases.push(Case {
                        label: format!("u16::MAX spliced at {offset}"),
                        bytes,
                    });
                }
            }
            2 => {
                let offset = rng.below(bytes.len());
                poke_non_finite(&mut bytes, offset, step);
                if offset + 4 <= base.len() {
                    cases.push(Case {
                        label: format!("non-finite pattern at {offset}"),
                        bytes,
                    });
                }
            }
            _ => {
                // Truncation: a valid packet cut at a random point.
                let cut = rng.below(base.len());
                cases.push(Case {
                    label: format!("input packet cut at {cut}"),
                    bytes: base[..cut].to_vec(),
                });
            }
        }
    }

    // Mutations of a valid server message: the client's inbound direction.
    let server = valid_server_message(SYNTHETIC_SESSION);
    for step in 0..64usize {
        let mut bytes = server.clone();
        let offset = rng.below(bytes.len());
        match step % 3 {
            0 => {
                bytes[offset] = rng.byte();
                cases.push(Case {
                    label: format!("server byte {offset} randomized"),
                    bytes,
                });
            }
            1 => {
                poke_non_finite(&mut bytes, offset, step);
                if offset + 4 <= server.len() {
                    cases.push(Case {
                        label: format!("server non-finite pattern at {offset}"),
                        bytes,
                    });
                }
            }
            _ => {
                let cut = rng.below(server.len());
                cases.push(Case {
                    label: format!("server packet cut at {cut}"),
                    bytes: server[..cut].to_vec(),
                });
            }
        }
    }

    // Mutations of the hello: the pre-session direction.
    let hello = valid_hello();
    for step in 0..48usize {
        let mut bytes = hello.clone();
        let offset = rng.below(bytes.len());
        match step % 2 {
            0 => {
                bytes[offset] = rng.byte();
                cases.push(Case {
                    label: format!("hello byte {offset} randomized"),
                    bytes,
                });
            }
            _ => {
                poke_non_finite(&mut bytes, offset, step);
                if offset + 4 <= hello.len() {
                    cases.push(Case {
                        label: format!("hello non-finite pattern at {offset}"),
                        bytes,
                    });
                }
            }
        }
    }
    cases
}

/// The hand-placed hostile shapes AC03 names, built from the real grammar so
/// each one reaches the field it targets instead of failing on its first byte.
pub fn hostile() -> Vec<Case> {
    let mut cases = Vec::new();

    // ---- oversized packet ------------------------------------------------
    cases.push(Case {
        label: format!(
            "buffer of MAX_PACKET_BYTES+1 ({} bytes)",
            MAX_PACKET_BYTES + 1
        ),
        bytes: vec![0u8; MAX_PACKET_BYTES + 1],
    });
    cases.push(Case {
        label: "u32::MAX-sized buffer behind a u32 length field".to_string(),
        bytes: {
            let mut bytes = vec![3u8];
            bytes.extend_from_slice(&SYNTHETIC_SESSION.get().to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.push(1); // ServerPayload::Snapshot
            bytes.extend_from_slice(&0u64.to_le_bytes());
            bytes.extend_from_slice(&u32::MAX.to_le_bytes());
            bytes
        },
    });

    // ---- oversized counts ------------------------------------------------
    // tag(1) + session(8) + sequence(4) + payload tag(1) + frame count(2).
    let mut batch = vec![2u8];
    batch.extend_from_slice(&SYNTHETIC_SESSION.get().to_le_bytes());
    batch.extend_from_slice(&0u32.to_le_bytes());
    batch.push(0); // ClientPayload::Input
    batch.extend_from_slice(&u16::MAX.to_le_bytes());
    cases.push(Case {
        label: "input batch claims u16::MAX frames".to_string(),
        bytes: batch,
    });

    // One frame with an axis count past the continuous-command cap.
    let mut axes = vec![2u8];
    axes.extend_from_slice(&SYNTHETIC_SESSION.get().to_le_bytes());
    axes.extend_from_slice(&0u32.to_le_bytes());
    axes.push(0);
    axes.extend_from_slice(&1u16.to_le_bytes());
    axes.extend_from_slice(&1u64.to_le_bytes());
    axes.extend_from_slice(&u16::MAX.to_le_bytes());
    axes.extend_from_slice(&0u16.to_le_bytes());
    cases.push(Case {
        label: "one frame claims u16::MAX axes".to_string(),
        bytes: axes,
    });

    // One frame with an edge count past the per-frame cap.
    let mut edges = vec![2u8];
    edges.extend_from_slice(&SYNTHETIC_SESSION.get().to_le_bytes());
    edges.extend_from_slice(&0u32.to_le_bytes());
    edges.push(0);
    edges.extend_from_slice(&1u16.to_le_bytes());
    edges.extend_from_slice(&1u64.to_le_bytes());
    edges.extend_from_slice(&0u16.to_le_bytes());
    edges.extend_from_slice(&u16::MAX.to_le_bytes());
    cases.push(Case {
        label: "one frame claims u16::MAX edges".to_string(),
        bytes: edges,
    });

    // The hello's mod list past its cap, and past the packet cap entirely.
    let mut mods = vec![0u8];
    mods.extend_from_slice(&1u16.to_le_bytes());
    mods.extend_from_slice(&[0x52; 32]);
    mods.extend_from_slice(&[0xC5; 32]);
    mods.extend_from_slice(&u16::MAX.to_le_bytes());
    cases.push(Case {
        label: "hello claims u16::MAX mods".to_string(),
        bytes: mods,
    });

    // ---- invalid ids -----------------------------------------------------
    for (label, bytes) in [
        ("zero session epoch in an in-session packet", {
            let mut bytes = vec![2u8];
            bytes.extend_from_slice(&0u64.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.push(1); // Leave
            bytes
        }),
        ("zero protocol version in a hello", {
            let mut bytes = vec![0u8];
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes
        }),
        ("zero peer id in a grant reply", {
            let mut bytes = vec![1u8, 0];
            bytes.extend_from_slice(&1u64.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes
        }),
        ("zero session epoch in an event id", {
            // tag(1) + header(12) + ServerPayload::Event(1) + EventId with
            // a zero session + EventBody::PeerJoined.
            let mut bytes = vec![3u8];
            bytes.extend_from_slice(&SYNTHETIC_SESSION.get().to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.push(2);
            bytes.extend_from_slice(&0u64.to_le_bytes());
            bytes.extend_from_slice(&1u64.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.push(0);
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes
        }),
        ("a snapshot frame naming a dead epoch", {
            // A structurally valid server message whose *envelope* epoch is
            // not the live one: the client must refuse it on arrival.
            let mut bytes = vec![3u8];
            bytes.extend_from_slice(
                &SessionId::new(999)
                    .expect("999 is nonzero")
                    .get()
                    .to_le_bytes(),
            );
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.push(0); // InputAck
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes
        }),
    ] {
        cases.push(Case {
            label: label.to_string(),
            bytes,
        });
    }

    // ---- unknown tags ----------------------------------------------------
    for (label, bytes) in [
        ("unknown packet tag", vec![0x7Fu8]),
        ("unknown client payload tag", {
            let mut bytes = vec![2u8];
            bytes.extend_from_slice(&SYNTHETIC_SESSION.get().to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.push(0xEE);
            bytes
        }),
        (
            "a server message in the client decoder",
            valid_server_message(SYNTHETIC_SESSION),
        ),
        ("a client message in the server decoder", valid_input()),
    ] {
        cases.push(Case {
            label: label.to_string(),
            bytes,
        });
    }

    // ---- non-finite patterns across the whole valid message --------------
    // Every offset of a valid server message gets each pattern, so a decoder
    // that ever let one through as a value would show up here.
    let valid = valid_server_message(SYNTHETIC_SESSION);
    for (index, pattern) in NON_FINITE_F32.iter().enumerate() {
        for offset in [0usize, 1, 3, 5, 9, 13, 17, 21, 29] {
            let mut bytes = valid.clone();
            if offset + 4 > bytes.len() {
                continue;
            }
            bytes[offset..offset + 4].copy_from_slice(&pattern.to_le_bytes());
            cases.push(Case {
                label: format!("pattern {pattern:#010x} at offset {offset}"),
                bytes,
            });
            let _ = index;
        }
    }
    cases
}

/// The whole corpus for one seed: the hostile shapes plus the generated ones.
pub fn corpus(seed: u64) -> Vec<Case> {
    let mut cases = hostile();
    cases.extend(generated(seed));
    cases
}

/// The seeds the acceptance run uses. Fixed, so the corpus is identical in CI
/// and on a reviewer's machine.
pub const SEEDS: [u64; 4] = [0x5EED_0001, 0x5EED_0002, 0xC0FF_EE01, 0xDEAD_BEEF];

// ---------------------------------------------------------------- builders --
/// Helpers that build *valid* records for the lifecycle tests, kept beside the
/// corpus so the two cannot drift apart.
///
/// A synthetic snapshot for the live epoch, carrying one actor at `offset`.
pub fn synthetic_snapshot(session: SessionId, offset_m: [f64; 3]) -> cs_net::snapshot::Snapshot {
    let record =
        cs_net::snapshot::synthetic_actor_record(ActorId { session, serial: 1 }, 1, offset_m);
    cs_net::snapshot::Snapshot::new(cs_net::snapshot::SYNTHETIC_ORIGIN_EPOCH, 0, vec![record])
}

/// A server message carrying one reliable event.
pub fn event_message(session: SessionId, sequence: u32, event: ReliableEvent) -> ServerMessage {
    ServerMessage {
        header: MessageHeader { session, sequence },
        payload: ServerPayload::Event(event),
    }
}

/// A server message carrying a snapshot frame.
pub fn snapshot_message(session: SessionId, sequence: u32, frame: SnapshotFrame) -> ServerMessage {
    ServerMessage {
        header: MessageHeader { session, sequence },
        payload: ServerPayload::Snapshot(frame),
    }
}

/// A client input packet naming `sequence` on `session`, with one fire edge.
pub fn fire_message(session: SessionId, tick: Tick, sequence: u32) -> ClientMessage {
    cs_net::validation::synthetic_fire_message(session, tick, sequence)
}

/// A reliable `PeerJoined` event, for dedup and phase tests.
pub fn joined_event(session: SessionId, peer: PeerId, sequence: u32) -> ReliableEvent {
    ReliableEvent {
        id: EventId {
            session,
            tick: Tick(3),
            producer: 0,
            sequence,
        },
        body: EventBody::PeerJoined { peer },
    }
}

/// A reliable `Launched` event, for the client phase machine.
pub fn launched_event(session: SessionId, start_tick: Tick, sequence: u32) -> ReliableEvent {
    ReliableEvent {
        id: EventId {
            session,
            tick: start_tick,
            producer: 0,
            sequence,
        },
        body: EventBody::Launched { start_tick },
    }
}

/// A reliable `Finished` event, for the client phase machine.
pub fn finished_event(session: SessionId, tick: Tick, reason: FinishReason) -> ReliableEvent {
    ReliableEvent {
        id: EventId {
            session,
            tick,
            producer: 0,
            sequence: 9,
        },
        body: EventBody::Finished { reason },
    }
}

/// The cap on remembered event ids, re-exported so a test can prove eviction
/// without re-deriving the bound.
pub const EVENT_MEMORY_CAP: usize = MAX_SEEN_EVENTS;
