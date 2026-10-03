//! Protocol, codecs and connection/lobby state.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. Allowed dependency:
//! [`cs_types`]. This crate must never depend on Bevy or Avian, and packets
//! never serialize Bevy `Entity` values.
//!
//! F54-A lands the *definition* slice of the wire protocol:
//!
//! * [`compat`] — the handshake: [`compat::ProtocolVersion`],
//!   [`compat::Compatibility`] (rules/content signatures plus enabled mods),
//!   [`compat::ClientHello`], [`compat::SessionParameters`], the pure
//!   [`compat::evaluate_hello`] gate that rejects an unsupported protocol or
//!   signature mismatch with a named reason before launch, and
//!   [`compat::admit_hello`]/[`compat::PeerAllocator`], the host-side decision
//!   that turns the gate into a [`compat::HelloReply`] grant or rejection.
//! * [`message`] — the in-session vocabulary: the epoch-and-sequence
//!   [`message::MessageHeader`], the directional [`message::ClientMessage`] /
//!   [`message::ServerMessage`] envelopes (server-owned state exists only on
//!   the server side), per-payload [`message::Delivery`] classification and
//!   bound/epoch/origin validation.
//! * [`authority`] — the `UI-NETWORK` ownership table as executable data.
//! * [`bounds`] — every packet size/count cap in one auditable place.
//! * [`fixture`] — the minimal synthetic session fixture for tests.
//! * [`snapshot`] — the F57-A payload schema [`message::SnapshotFrame`] carries:
//!   the quantized actor record, every declared quantization scale and error
//!   budget ([`snapshot::SNAPSHOT_BUDGET`]), the shared origin epoch, and the
//!   codec both [`snapshot::Snapshot::encode`] and
//!   [`snapshot::Snapshot::decode`] run.
//!
//! Wire identity ([`cs_types::net::SessionId`], [`cs_types::net::PeerId`],
//! [`cs_types::net::ActorId`], [`cs_types::net::EventId`]) lives in
//! `cs_types::net` so simulation and content crates can name sessions and
//! actors without depending on this crate.
//!
//! F55-A adds [`lobby`]: the host's authoritative lobby record, the rules
//! revision/digest protocol, readiness bound to a revision, atomic
//! acknowledged launch and distinct join refusals.
//!
//! F56-A adds [`rules`]: the per-mode rule fields a host must have resolved
//! before launch, where an unknown field blocks the mode instead of defaulting,
//! and the start-time check of human count, custom planes and component limit.
//!
//! F58-A adds [`validation`] and [`recovery`]: the threat model, the session
//! epoch/replay gate and the peer-to-actor ownership table ([`validation`]),
//! and the reconnect, fresh-epoch, one-pilot-per-aircraft and award-once rules
//! ([`recovery`]). The rate/resource caps (F58-B) and the runtime
//! disconnect/recovery flow (F58-C) build on them.
//!
//! F54-B adds the pinned transport: [`codec`], the bounded wire format every
//! F54-A record travels in, and [`transport`], the `renet2`/`renet2_netcode`
//! `=0.16.1` UDP path that delivers the hello, returns the grant or the named
//! rejection before launch, and runs every session packet through the
//! [`validation::SessionGate`] before it can authorize fire requests.
//!
//! Not here yet: the F58-B rate caps, the F58-C disconnect/recovery flow, and
//! the `cs_app`/`cs_sim` binding that drives a session from a Bevy schedule and
//! a simulation ledger. The F57-B interpolation buffer and bounded local
//! prediction consume the snapshot this crate hands them rather than extending
//! it.
//!
//! F54-C adds [`lifecycle`]: the session owners that wire the pinned transport
//! to its producers and consumers. [`lifecycle::ServerSession`] owns the
//! launch/finish/teardown phase machine, drains admitted peer input into a
//! bounded queue, publishes reliable lifecycle events, snapshots and input
//! acknowledgments, and hangs up on the peers the declared threat dispositions
//! say to cut off. [`lifecycle::ClientSession`] is the client side: it produces
//! bounded wire input from locally sampled ticks (refusing a non-finite sample
//! before it can become a packet), deduplicates reliable events by `EventId`,
//! keeps only the newest snapshot, tracks the acknowledgment window and
//! retransmits a lost input packet verbatim. Both report every refusal as a
//! named fault or notice.

pub mod authority;
pub mod bounds;
pub mod codec;
pub mod compat;
pub mod fixture;
pub mod lifecycle;
pub mod lobby;
pub mod message;
pub mod recovery;
pub mod rules;
pub mod snapshot;
pub mod transport;
pub mod validation;
