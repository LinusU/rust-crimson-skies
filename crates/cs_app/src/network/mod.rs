//! Networked session state in the running app (F54/F57).
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`
//! and `specs/F57-networked-aircraft-prediction-interpolation-and-projectiles.md`.
//! Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! [`physics`] is the F57-A receiver boundary: it turns one decoded
//! [`cs_net::snapshot::Snapshot`] and the session's live world-origin epoch into
//! the [`physics::RemoteMirror`] of remote aircraft the app presents. The server
//! side of the same schema — the per-session authority, actor generations, the
//! once-per-generation destruction gate and input acknowledgment — is
//! [`cs_sim::net_state`]; the wire schema, quantizers and declared error budgets
//! are [`cs_net::snapshot`].
//!
//! Not here yet: the wiring of a [`cs_net::lifecycle`] session into the Bevy
//! schedule and a [`cs_sim`] ledger — F54-C built and tested the session owners
//! and their bounded queues, but nothing here constructs one or pumps it yet —
//! plus the bounded interpolation buffer and local prediction (F57-B) and the
//! wiring of reconciliation, projectile confirmation and origin epochs into the
//! running app (F57-C).
//!
//! F58-A adds [`recovery`]: the host-side receive boundary that admits a
//! decoded client packet through the session identity gate and turns an
//! admitted fire packet into the F27 requests weapon acceptance consumes.

pub mod physics;
pub mod recovery;
