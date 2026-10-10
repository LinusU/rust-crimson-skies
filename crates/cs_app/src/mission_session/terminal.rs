//! The mission run's one terminal: which runtime settled it, what that means
//! for the process's exit code, and the single report line the run writes
//! (VS-M01-RT-MISSION-HOST.02, Rally #1279).
//!
//! Two runtimes can settle a mission, and the composed entry funnels both
//! into this one record ([`super::host::MissionHost::settle`]):
//!
//! * the **F39 objective session**'s own `SessionTick::outcome` — the source
//!   this stage names first, because it is the runtime whose declared
//!   conditions, timers and count reactions decide an original mission's
//!   ending;
//! * the **declared control program**'s own terminal state — for M01 the only
//!   reachable one, since its measured `INSTANTWIN`/`INSTANTLOSS` directives
//!   lower to `Finish` actions and no original mission yields an F39 program
//!   today (Rally #1219).
//!
//! Whichever settles **first** ends the run, and the report line says which.
//! The funnel never *raises* an outcome of its own: the designed conservative
//! [`TerminalPrecedence`](cs_sim::objectives::terminal::TerminalPrecedence) is
//! reserved by `docs/contracts/SCRIPT-MISSION.md` for synthetic tests, so the
//! host never fills `TickInput::terminal_requests` against a retail-derived
//! program.
//!
//! # The exit is never a swallowed failure
//!
//! [`MissionExit`] is total over the outcome: `0` **only** for
//! [`TerminalOutcome::Success`], `1` for [`TerminalOutcome::Extraction`] and
//! [`TerminalOutcome::Failure`], and the control program's `Aborted` maps to
//! failure rather than being read as a success it never was. The composed
//! entry sends [`MissionExit::app_exit`] as a Bevy message
//! (`World::write_message(AppExit::…)`), so the windowed run ends and a
//! headless test can read the message (`docs/contracts/CLI-EVIDENCE.md`:
//! never return zero after only logging a failure).

use std::fmt;
use std::num::NonZero;

use bevy::app::AppExit;
use cs_script::ir::SymbolId;
use cs_script::runtime::SessionGeneration;
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_types::Tick;

/// Which runtime reported the terminal outcome this run ended on.
///
/// The label is the one the report line prints, so a reader of the line never
/// has to guess which half of the composed entry settled the mission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissionTerminalSource {
    /// The F39 objective session's own `SessionTick::outcome`.
    Objectives,
    /// The declared control program's own terminal state, as
    /// `MissionTick::terminal` and the `EventKind::TerminalRequested` event
    /// that carried it.
    ControlProgram,
}

impl MissionTerminalSource {
    /// Stable label for the report line and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Objectives => "objectives",
            Self::ControlProgram => "control_program",
        }
    }
}

impl fmt::Display for MissionTerminalSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The process exit one settled outcome maps to.
///
/// This is the mapping the composed entry sends as its `AppExit` message and
/// the only place a mission outcome becomes an exit code:
/// [`TerminalOutcome::Success`] is `0`; [`TerminalOutcome::Extraction`] and
/// [`TerminalOutcome::Failure`] are `1`. An extraction is a different ending
/// the player sees differently, so it is never reported as a plain success,
/// and the control program's `Aborted` arrives here as
/// [`TerminalOutcome::Failure`] (see
/// [`super::host`] for the mapping) rather than as a zero exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissionExit(TerminalOutcome);

impl MissionExit {
    /// The exit of a settled `outcome`.
    #[must_use]
    pub const fn of(outcome: TerminalOutcome) -> Self {
        Self(outcome)
    }

    /// The outcome this exit was mapped from.
    #[must_use]
    pub const fn outcome(self) -> TerminalOutcome {
        self.0
    }

    /// The exit code: `0` only for success, `1` for extraction and failure.
    #[must_use]
    pub const fn code(self) -> i32 {
        match self.0 {
            TerminalOutcome::Success => 0,
            TerminalOutcome::Extraction | TerminalOutcome::Failure => 1,
        }
    }

    /// The same mapping as the Bevy message that ends the app.
    #[must_use]
    pub fn app_exit(self) -> AppExit {
        match self.code() {
            0 => AppExit::Success,
            _ => AppExit::Error(NonZero::<u8>::MIN),
        }
    }
}

/// One settled run's whole terminal answer.
///
/// Every field is read off the records that produced it: the outcome and the
/// requesting symbol come from the settling runtime's own tick, the tick is
/// the composed entry's committed host tick, the session is the generation
/// [`super::host::MissionHost`] launched, and the counts are what the
/// objective session and the audio session still owned when the run ended.
/// Nothing here is a default the host invented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionTerminal {
    /// The settled outcome.
    pub outcome: TerminalOutcome,
    /// Which runtime reported it.
    pub source: MissionTerminalSource,
    /// The host tick the settling answer belongs to.
    pub tick: Tick,
    /// The session generation every record this run drove serves.
    pub session: SessionGeneration,
    /// The objective symbol that asked for the outcome, when the settling
    /// record named one: the `EventKey::source` of the
    /// `TerminalRequested`/`OutcomeSettled` event that settled the run.
    pub requested_by: Option<SymbolId>,
    /// How many dialogue cues the objective session still owned when the run
    /// ended — emitted cues the player will never hear, named rather than
    /// dropped (the terminal sequence drains them).
    pub undrained_cues: usize,
    /// How many mission-bound audio loops were still bound at the end. The
    /// router's [`cs_sim::audio_events::EmitterStopReason`] measures no
    /// mission-end reason, so a bound loop is reported under
    /// [`super::host::MissionHostRefusal::MissionAudioStillBound`] rather
    /// than stopped under a reason it did not have.
    pub audio_loops_bound: usize,
    /// The exit this outcome maps to.
    pub exit: MissionExit,
    /// The one line this run writes when it ends.
    pub report_line: String,
}

impl MissionTerminal {
    /// Builds the terminal of one settled run, with the report line that
    /// names every part of it.
    #[must_use]
    #[allow(clippy::too_many_arguments, reason = "one field per settling record")]
    pub fn new(
        outcome: TerminalOutcome,
        source: MissionTerminalSource,
        tick: Tick,
        session: SessionGeneration,
        requested_by: Option<SymbolId>,
        undrained_cues: usize,
        audio_loops_bound: usize,
    ) -> Self {
        let exit = MissionExit::of(outcome);
        let requested = requested_by.map_or_else(
            || "none".to_owned(),
            |symbol| format!("SymbolId({})", symbol.0),
        );
        let report_line = format!(
            "mission terminal: outcome={} source={} tick={} session={} requested_by={} \
             undrained_cues={} audio_loops_bound={} exit_code={}",
            outcome.label(),
            source.label(),
            tick.0,
            session.0,
            requested,
            undrained_cues,
            audio_loops_bound,
            exit.code(),
        );
        Self {
            outcome,
            source,
            tick,
            session,
            requested_by,
            undrained_cues,
            audio_loops_bound,
            exit,
            report_line,
        }
    }
}
