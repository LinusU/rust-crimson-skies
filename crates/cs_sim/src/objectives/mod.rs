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
//!   of its own and a declared reveal rule per objective. Nothing it is asked to
//!   do is dropped in silence: a refused request, a refused movement and a
//!   request naming a declaration that does not exist are each *reported*, so a
//!   consumer can tell "the program asked" from "the world did nothing".
//! - [`runtime::CountCondition`] / [`runtime::CountReaction`]: a *declared
//!   roster* plus a *declared category* plus a *declared consequence*, which is
//!   what makes "never approximate every objective by `enemy_alive == 0`"
//!   structural rather than a convention.
//! - [`timer`]: [`timer::MissionTimer`], with a declared
//!   [`timer::TimerStart`], a validated gameplay
//!   [`crate::time::TimeDomain`], and exactly one [`timer::TimerAction`] on
//!   expiry — so reaching a waypoint unlocks or resets an objective only
//!   through a declared program action. That action belongs to the expiry, not
//!   to the [`timer::TimerState::Expired`] the table then sits in, so a deadline
//!   that ran out does not run again by itself.
//! - [`terminal`]: [`terminal::TerminalOutcome`], the declared
//!   [`terminal::TerminalPrecedence`] that resolves a tick's conflicting
//!   requests, and the [`terminal::TerminalLatch`] that holds one answer.
//!
//! # Stage F39-E5 — completion effects
//!
//! - [`runtime::CompletionEffect`] / [`runtime::CompletionEffectKind`]: what
//!   completing one objective does to *another* — the four spellings the
//!   original's objective records use, applied as declared moves through the
//!   same transition table every other declared action obeys. They are drained
//!   from a queue in their own phase, never called from inside the completion
//!   that raised them (`docs/contracts/SCRIPT-MISSION.md`: "actions do not
//!   directly recurse into callbacks"), no kind moves its target to `Succeeded`
//!   so the queue cannot cascade, and a target named by two *different* effects
//!   is refused rather than ordered.
//! - [`runtime::UnmeasuredNumber`]: the number a nap declaration carries, kept
//!   as data with no unit because what it measures is unmeasured, and never
//!   interpreted.
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
