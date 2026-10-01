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
