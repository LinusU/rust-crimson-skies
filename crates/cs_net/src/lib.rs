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
//! Not here yet: the pinned transport and its codec (F54-B), connection and
//! lifecycle wiring (F54-C), lobby state (`lobby.rs`, F55-A) and session
//! threat/reconnect rules (`lobby.rs`, F58-A). The F57-B interpolation buffer
//! and bounded local prediction consume [`snapshot`] rather than extending it.

pub mod authority;
pub mod bounds;
pub mod compat;
pub mod fixture;
pub mod message;
pub mod snapshot;
