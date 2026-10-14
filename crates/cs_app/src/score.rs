//! The session's kill-score ledger and the finished run's outcome records
//! (F29-C.3b, task #704).
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C` (scoring). Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md` ("Results, previous targets and
//! delayed callbacks are always generation-qualified", and the outcome
//! transaction's exactly-once rule). F29-C.3 (#519) built the consumer in
//! `cs_sim::net_state::NetStateLedger` and left two wirings to this task;
//! `docs/findings/2026-10-05-f29-c-3-kill-award-scoring.md` records that
//! stage's measurement and this one's design.
//!
//! Three things live here, and nothing else scores anything:
//!
//! * [`SessionScoreLedger`] is the **one** score store of a running session:
//!   a Bevy [`Resource`] around [`NetStateLedger`] (`cs_sim` cannot depend on
//!   Bevy, `docs/01-ARCHITECTURE.md` rule 7), opened for one [`SessionId`]
//!   and replaced — never carried over — when that generation ends.
//! * [`drive_damage_schedule`](crate::damage::drive_damage_schedule) hands
//!   every accepted resolution batch of a tick to the ledger through
//!   [`SessionScoreLedger::apply_kill_awards`], so a kill the
//!   [`DamageResolver`](cs_sim::damage::DamageResolver) resolved during a
//!   session reaches the consumer exactly once. Refusals come back by name in
//!   the [`KillAwardReport`] the damage schedule's report carries.
//! * [`mission_outcome`] / [`mission_result`] read a finished run's score
//!   back off the settled [`MissionTerminal`] and hand it to
//!   [`MissionOutcome::score`] and [`MissionResult::score`] — the two fields
//!   nobody computed before. Both records keep the caller's [`OutcomeId`], so
//!   the exactly-once rule the outcome transaction already enforces
//!   (`campaign::CampaignState::apply_outcome`, `records::RecordBook::submit`)
//!   is what makes a replayed outcome pay nothing the second time.
//!
//! # Designed value, not original data
//!
//! The points come from the ledger's provenance-carrying
//! [`KillScoreValue`](cs_sim::net_state::KillScoreValue) (`f29-c3.designed-kill-score`, one point per kill):
//! the original's per-kill scoring numbers are unmeasured (F29 "Research
//! boundary"), and nothing here substitutes a number for a measurement it
//! does not have. A caller that measures one replaces it through
//! [`NetStateLedger::set_kill_score`].
//!
//! # What is not claimed
//!
//! No original executable ran; `retail` is read access to the owner's
//! installation. Nothing here is `verified_original` or `release_approved`,
//! and no original-data, visual, audible or ordinary-play claim is made: this
//! is synthetic-fixture wiring only, so it can award at most **checked**.

use std::fmt;

use bevy::prelude::Resource;
use cs_script::ir::Outcome;
use cs_sim::campaign::{
    CampaignNodeKey, DifficultyId, MissionOutcome, OutcomeAuthority, OutcomeId,
};
use cs_sim::damage::DamageEvent;
use cs_sim::net_state::{
    ActorGeneration, KillAwardReport, NetActorState, NetStateError, NetStateLedger,
};
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_sim::records::RecordRule;
use cs_types::content::ContentId;
use cs_types::net::SessionId;

use crate::mission_session::MissionTerminal;
use crate::ui::scrapbook::MissionResult;

/// The running session's kill-score ledger: the one place a kill becomes a
/// number, kept as an ECS resource so the mission's fixed-tick schedule can
/// reach it.
///
/// A [`Resource`] wrapper around [`NetStateLedger`] is the whole design — the
/// ledger itself stays Bevy-free (`cs_sim` must not depend on the renderer or
/// Bevy) and stays the single owner of *score*, destruction records and the
/// provenance-carrying [`KillScoreValue`](cs_sim::net_state::KillScoreValue).
/// Nothing else in this crate keeps a
/// tally: this type adds no second store, only the resource handle and the
/// session identity every call is checked against.
///
/// One ledger serves one [`SessionId`]. The mission host opens one for the
/// session it launches, replaces it when a restart mints a fresh generation
/// (so a restarted run starts from zero, `docs/contracts/STATE-TRANSACTIONS`
/// "Retry restores the authored initial state"), and drops it with the
/// composition.
#[derive(Debug, Resource)]
pub struct SessionScoreLedger {
    /// The session generation this score belongs to.
    ledger: NetStateLedger,
}

impl SessionScoreLedger {
    /// The score ledger of the session generation `session`.
    ///
    /// The ledger starts at zero with the engine's designed kill value; a new
    /// ledger — not a reset of the old one — is what a restart opens, so no
    /// tally, destruction record or credited attacker of the torn-down
    /// generation can survive into the next run.
    #[must_use]
    pub fn open(session: SessionId) -> Self {
        Self {
            ledger: NetStateLedger::new(session),
        }
    }

    /// The session generation this ledger scores.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.ledger.session()
    }

    /// The ledger itself, for the read-backs that need its own accessors
    /// ([`NetStateLedger::score_for`], [`NetStateLedger::kill_score`]).
    #[must_use]
    pub const fn ledger(&self) -> &NetStateLedger {
        &self.ledger
    }

    /// The score this session's kills have awarded.
    #[must_use]
    pub const fn score(&self) -> i64 {
        self.ledger.score()
    }

    /// Registers one actor this session owns, so an award naming it can be
    /// scored.
    ///
    /// Admission is the spawn host's duty, exactly as it is for the F57
    /// network session: [`NetStateLedger`] refuses an actor it does not know
    /// (`NetStateError::UnknownActor`), so a kill for an actor nobody
    /// admitted is reported by name in the [`KillAwardReport`] instead of
    /// being scored against a session that never vouched for it. Today no
    /// production path spawns a damage actor in a single-player mission
    /// (VS-M01 #359 owns that); the gap is recorded in
    /// `docs/findings/2026-10-14-f29-c-3b-score-wiring.md`.
    ///
    /// # Errors
    ///
    /// [`NetStateError`] from [`NetStateLedger::spawn`]: a foreign session, a
    /// duplicate registration, an invalid id or an exhausted generation.
    pub fn admit(&mut self, state: NetActorState) -> Result<ActorGeneration, NetStateError> {
        self.ledger.spawn(state)
    }

    /// Delivers one resolution batch's kill awards to this session's score.
    ///
    /// This is the call `drive_damage_schedule` makes per committed fixed
    /// tick; [`NetStateLedger::apply_kill_awards`] decides what scores, what
    /// is absorbed as a replay and what is refused, and its report is handed
    /// back unchanged so the damage schedule can carry it.
    #[must_use]
    pub fn apply_kill_awards(&mut self, events: &[DamageEvent]) -> KillAwardReport {
        self.ledger.apply_kill_awards(events)
    }
}

/// Why a finished run's outcome record could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutcomeRecordError {
    /// The run's recorded score is negative.
    ///
    /// A [`NetStateLedger`] cannot reach one: a negative
    /// [`KillScoreValue`](cs_sim::net_state::KillScoreValue) is refused at its
    /// own boundary by `KillScoreValue::try_new`, and the tally starts at
    /// zero — so this is the value's own boundary reporting an impossible
    /// state by name rather than the record clamping it into a plausible
    /// score (`docs/contracts/STATE-TRANSACTIONS.md`: validate, never
    /// silently coerce).
    Score {
        /// The refused tally.
        score: i64,
    },
    /// The run ended by extraction, and what an extraction records for the
    /// campaign and the scrapbook is **unmeasured**.
    ///
    /// `cs_script::ir::Outcome` has `Succeeded`, `Failed` and `Aborted`, and
    /// nothing has measured which of them the original records for a run that
    /// left the mission area rather than completing in place — so the record
    /// is refused by name instead of being written as an outcome the run did
    /// not have (AGENTS.md rule 4). The affected content is every extracting
    /// run of every mission; the resolving task is filed with `create_tasks`.
    Extraction,
}

impl fmt::Display for OutcomeRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Score { score } => write!(
                f,
                "the run's recorded score {score} is negative; the outcome record keeps no \
                 negative score"
            ),
            Self::Extraction => write!(
                f,
                "the run ended by extraction, and what an extraction records as a campaign \
                 outcome is unmeasured"
            ),
        }
    }
}

impl std::error::Error for OutcomeRecordError {}

/// The score a settled run hands to its outcome records, read back off the
/// terminal the run actually ended on — never recomputed here.
///
/// # Errors
///
/// [`OutcomeRecordError::Score`] for a negative tally.
fn recorded_score(terminal: &MissionTerminal) -> Result<u64, OutcomeRecordError> {
    u64::try_from(terminal.score).map_err(|_| OutcomeRecordError::Score {
        score: terminal.score,
    })
}

/// The record outcome a settled run maps to.
///
/// # Errors
///
/// [`OutcomeRecordError::Extraction`]: the mapping an extraction would need
/// is unmeasured.
fn recorded_outcome(terminal: &MissionTerminal) -> Result<Outcome, OutcomeRecordError> {
    match terminal.outcome {
        TerminalOutcome::Success => Ok(Outcome::Succeeded),
        TerminalOutcome::Failure => Ok(Outcome::Failed),
        TerminalOutcome::Extraction => Err(OutcomeRecordError::Extraction),
    }
}

/// The campaign's record of one finished run: the identity, node and
/// authority the caller owns, and the verdict and **score** the settled run
/// itself carries.
///
/// The score is read from `terminal`, which [`MissionHost::settle`]
/// filled from the session's [`SessionScoreLedger`]
/// (`crate::mission_session::host`); this function never tallies anything.
/// The caller's [`OutcomeId`] is kept verbatim, so the campaign's applied
/// set still decides — once per `OutcomeId` — whether this record pays.
///
/// # Errors
///
/// [`OutcomeRecordError`]; no record is produced for a refusal.
pub fn mission_outcome(
    terminal: &MissionTerminal,
    id: OutcomeId,
    node: CampaignNodeKey,
    authority: OutcomeAuthority,
) -> Result<MissionOutcome, OutcomeRecordError> {
    Ok(MissionOutcome {
        id,
        node,
        outcome: recorded_outcome(terminal)?,
        score: recorded_score(terminal)?,
        authority,
    })
}

/// The scrapbook's record of the same finished run, carrying the same score
/// the campaign's record carries.
///
/// [`record_mission`](crate::ui::scrapbook::record_mission) submits it under
/// the caller's [`OutcomeId`], and `RecordBook::submit` answers
/// `AlreadyApplied` for an id it already holds — a replayed result after a
/// crash writes nothing, which is what makes the score pay at most once.
///
/// # Errors
///
/// [`OutcomeRecordError`]; no record is produced for a refusal.
pub fn mission_result(
    terminal: &MissionTerminal,
    outcome_id: OutcomeId,
    mission: ContentId,
    difficulty: DifficultyId,
    rule: RecordRule,
) -> Result<MissionResult, OutcomeRecordError> {
    Ok(MissionResult {
        outcome_id,
        mission,
        difficulty,
        outcome: recorded_outcome(terminal)?,
        score: recorded_score(terminal)?,
        rule,
    })
}
