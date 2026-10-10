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
//! and their bounded queues, but nothing here constructs one or pumps it yet.
//! F57-B and F57-C built the interpolation buffers, local prediction,
//! reconciliation, projectile confirmation and origin epochs as
//! [`physics::NetSession`]; driving it from the Bevy schedule is F57-C.01.
//!
//! F58-A adds [`recovery`]: the host-side receive boundary that admits a
//! decoded client packet through the session identity gate and turns an
//! admitted fire packet into the F27 requests weapon acceptance consumes.
//! F58-B extends it with the intent layer: [`recovery::SessionReceiver`]'s
//! `validate_intent`/`receive_validated` judge every ask against the
//! server-owned actors, loadouts, rate budget, tick window and match phase,
//! and refuse a client-authored damage/score claim outright.
//!
//! F58-C adds [`recovery::RecoveryFlow`]: the disconnect, reconnect and clean
//! host-loss flow over that boundary. Its producer is the receive path
//! (`departure_cause`/`receive_and_depart`), its consumers are the peer
//! teardown and the authoritative state the peer still held (`depart` drops
//! what it carried, once), the mission's in-flight interactions and the
//! clients' return to the menu (`host_lost`), and the retry
//! (`recover` reopens the boundary on the fresh epoch). What it does *not*
//! wire — the pinned transport's own pump, which still admits packets without
//! a [`cs_net::validation::MatchStage`] — is named in
//! [`recovery`]'s module docs and filed as a follow-up.

pub mod physics;
pub mod recovery;
