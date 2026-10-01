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
//! Not here yet: the pinned transport and its channels (F54-B), lobby and
//! connection state (F54-C, F55-A), the bounded interpolation buffer and local
//! prediction (F57-B), and the wiring of reconciliation, projectile
//! confirmation and origin epochs into the running app (F57-C).

pub mod physics;
