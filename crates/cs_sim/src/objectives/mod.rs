//! F39-A objective, trigger and spawn semantics
//! (`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-A`).
//!
//! Typed vocabulary and minimal synthetic fixtures only; the continuous
//! runtime, timers and terminal precedence are F39-B and the mission wiring is
//! F39-C. Everything here is designed behavior: no original rule is measured,
//! so nothing is an original-fidelity claim.
//!
//! - [`state`]: the seven [`state::ObjectiveState`]s and the legal transitions.
//! - [`trigger`]: [`trigger::SweptTrigger`], which turns one actor's movement
//!   segment per tick into ordered entry/exit events, and
//!   [`trigger::Movement::Teleport`], which never sweeps.
//! - [`counters`]: [`counters::ActorCounters`], which keeps destroyed,
//!   disabled, captured, escaped and despawned apart.
//! - [`spawn`]: [`spawn::EmissionLedger`], the per-session idempotency ledger
//!   for spawn groups and dialogue cues.

pub mod counters;
pub mod spawn;
pub mod state;
pub mod trigger;
