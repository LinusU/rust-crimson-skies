//! F39 objective, trigger, spawn and timer semantics
//! (`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`).
//!
//! Everything here is **designed** behavior: no original rule has been measured,
//! so nothing in this module is an original-fidelity claim. Which outcomes win a
//! same-tick collision, which deadline a mission declares and which actors it
//! protects are measured in F39-D with `retail` capability.
//!
//! # Stage `### F39-A` — the vocabulary
//!
//! - [`state`]: the seven [`state::ObjectiveState`]s and the legal transitions.
//! - [`trigger`]: [`trigger::SweptTrigger`], which turns one actor's movement
//!   segment per tick into ordered entry/exit events, and
//!   [`trigger::Movement::Teleport`], which never sweeps.
//! - [`counters`]: [`counters::ActorCounters`], which keeps destroyed, disabled,
//!   captured, escaped and despawned apart.
//! - [`spawn`]: [`spawn::EmissionLedger`], the per-session idempotency ledger
//!   for spawn groups and dialogue cues.
//!
//! # Stage `### F39-B` — the continuous runtime
//!
//! - [`runtime`]: [`runtime::ObjectiveRuntime`], which folds one tick's
//!   lifecycle transitions, real movement segments, declared signals, timer
//!   requests and terminal requests into a single event stream ordered by
//!   [`cs_script::runtime::EventKey`], with bounded work, a session generation
//!   of its own and a declared reveal rule per objective.
//! - [`runtime::CountCondition`] / [`runtime::CountReaction`]: a *declared
//!   roster* plus a *declared category* plus a *declared consequence*, which is
//!   what makes "never approximate every objective by `enemy_alive == 0`"
//!   structural rather than a convention.
//! - [`timer`]: [`timer::MissionTimer`], with a declared
//!   [`timer::TimerStart`], a validated gameplay
//!   [`crate::time::TimeDomain`], and exactly one [`timer::TimerAction`] on
//!   expiry — so reaching a waypoint unlocks or resets an objective only
//!   through a declared program action.
//! - [`terminal`]: [`terminal::TerminalOutcome`], the declared
//!   [`terminal::TerminalPrecedence`] that resolves a tick's conflicting
//!   requests, and the [`terminal::TerminalLatch`] that holds one answer.
//!
//! The mission wiring — the authored content form, the Bevy producers and the
//! UI and dialogue consumers — is F39-C.

pub mod counters;
pub mod runtime;
pub mod spawn;
pub mod state;
pub mod terminal;
pub mod timer;
pub mod trigger;
