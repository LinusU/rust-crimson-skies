//! Wire size, count and rate caps (F54-A).
//!
//! Spec F54 non-negotiable behavior 3: "Packets have size/count/rate caps,
//! validated ids and finite numeric fields." Every `Vec`/byte payload the
//! wire vocabulary can carry is bounded by a constant here so a decoded or
//! locally produced packet that exceeds a cap fails validation instead of
//! allocating or relaying unbounded data.
//!
//! All values are newly authored engine design: no original network budget
//! is known, and none is claimed.

/// Largest single wire packet, in bytes. The codec (F54-B/C) must refuse to
/// emit or accept a packet beyond this size.
pub const MAX_PACKET_BYTES: usize = 16 * 1024;

/// Largest peer population of one session. [`cs_types::net::PeerId`] is a
/// nonzero `u16`; this designed cap keeps membership and grant validation
/// bounded well inside that space.
pub const MAX_SESSION_PEERS: usize = 32;

/// Largest enabled-mod list a handshake offer may carry.
pub const MAX_MODS: usize = 64;

/// Most differing mod ids one direction of a handshake's mod-set diff names.
///
/// A [`crate::compat::HandshakeReject::ModSetMismatch`] is a *bounded*
/// representation of the difference: each of its two id lists holds at most
/// this many ids, and the matching omitted-count field says how many more
/// differed. That cap is what keeps the refusal encodable inside
/// [`MAX_PACKET_BYTES`]: at the worst case of [`MAX_MODS`] maximum-length
/// catalog ids on both sides with no overlap, an untruncated diff cannot be
/// encoded at all and the client sees neither a grant nor a reason (F54-B/C
/// finding, task F54-C-followup-mods-diff-cap). The client can therefore
/// always be told *that* its mod set differs, even when it cannot be shown
/// every differing id.
pub const MAX_MOD_DIFF_IDS: usize = 8;

/// Most tick-stamped [`cs_types::input::InputFrame`]s one input packet may
/// carry ("bounded tick-stamped inputs", spec F54 deliverable).
pub const MAX_INPUT_FRAMES_PER_PACKET: usize = 8;

/// Largest tick span one input packet may cover, first to last tick.
pub const MAX_INPUT_BATCH_SPAN_TICKS: u64 = 64;

/// Most one-shot edges one wire input frame may carry.
pub const MAX_EDGES_PER_FRAME: usize = 32;

/// Largest byte payload of one snapshot packet. The quantized actor-record
/// schema inside the payload is owned by F57-A
/// (`crates/cs_net/src/snapshot.rs`); this bound is the envelope-level cap
/// that applies before that schema runs.
pub const MAX_SNAPSHOT_BYTES: usize = 8 * 1024;

/// Most admitted client input packets one host pump hands to the
/// authoritative simulation (`crate::lifecycle::ServerSession`). The host's
/// receive path is otherwise unbounded — a peer may send as fast as the socket
/// delivers — so the pump drains into a queue with this cap and refuses the
/// surplus instead of growing (F54-C; the `ResourceExhaustion` threat class).
pub const MAX_WORK_PER_PUMP: usize = 64;

/// Most reliable-event ids one client remembers for deduplication. The
/// reliable channel can replay an event after a retry, so the client keeps a
/// bounded seen-set and evicts the *oldest* id when it is full (F54-C).
pub const MAX_SEEN_EVENTS: usize = 256;

/// Most sent-but-unacknowledged client input packets one client keeps for
/// verbatim retransmission. Each entry is at most [`MAX_PACKET_BYTES`], so
/// this bounds a client's retransmit memory at
/// `MAX_PACKET_BYTES * MAX_UNACKED_PACKETS` (F54-C).
pub const MAX_UNACKED_PACKETS: usize = 8;
