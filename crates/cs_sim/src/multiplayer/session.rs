//! The match session: the one owner of a running match's scoring, objectives
//! and clock (F56-C).
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-C` ("Wire maps, host options, scoring and end-of-match UI …
//! include teardown/retry and error propagation"), minimum scenario "Restart
//! match and ensure no score, pickup or timer leaks". Contract:
//! `docs/contracts/UI-NETWORK.md` ("Server owns ActorId allocation, physics
//! truth, weapon acceptance, hit/damage, faction/interaction, mission program,
//! **score and result**", "Protocol messages carry session epoch … Epoch
//! mismatch rejects stale packets", and the path "lobby join → ready → match
//! → results → lobby").
//!
//! # What this wires together
//!
//! F56-A and F56-B built the pieces — `cs_net::rules` (a mode's rules, the
//! host's options, the resolver's limits) and [`super::result`] /
//! [`super::objective`] (the scoring resolver and the possession board) — but
//! nothing owned them together: every consumer had to hand-roll a resolver, a
//! board and a clock, and nothing proved that a restarted match started from
//! nothing. A [`MatchSession`] is that owner. It is built from one plain
//! [`SessionConfig`], whose fields are exactly what the producers emit:
//!
//! * **map** — `scenario`, the content id of the selected scenario slot. The
//!   host takes it from its lobby's `scenario` and resolves it through
//!   `cs_content::multiplayer::SlotCatalog::get`; this session carries it so
//!   the end-of-match record names the map it was produced on.
//! * **host options** — `limits` and `victory` come from the rules the host
//!   resolved (`cs_net::rules::MatchRules::resolver_limits` and, across the
//!   crate boundary, [`VictoryRule::from_label`] on
//!   `cs_net::rules::Victory::label`); `roster` and `table` are host inputs
//!   whose original values are still unknown, so nothing here invents one.
//! * **scoring** — [`super::result::MatchResolver`] decides the single final
//!   result; [`super::objective::ObjectiveBoard`] arbitrates possession. This
//!   session drives both on the *same* judged tick and refuses events once the
//!   result is sealed.
//! * **end-of-match UI** — [`MatchSession::end_of_match`] renders the sealed
//!   result into the [`EndOfMatch`] record a results screen consumes (map,
//!   why it ended, standings, every recorded delivery) with stable
//!   localization keys, exactly as `cs_net::lobby::RevokeReason::message_key`
//!   serves the lobby's reasons.
//!
//! # Restart, teardown and retry
//!
//! [`MatchSession::restart`] is the teardown: it *builds the next match first*
//! and only swaps it in when the configuration is accepted, so a refused
//! restart changes nothing and the caller may correct the configuration and
//! retry. A restart is a new session generation: restarting into the running
//! [`SessionId`] is refused ([`SessionError::SameGeneration`]), and because
//! the next session is built from scratch it inherits no score
//! ([`super::result::MatchResolver`] starts every side at zero), no pickup
//! (every objective is [`super::objective::ObjectiveState::Home`] with an
//! empty ledger) and no timer (the session clock starts again, so the old
//! generation's closed ticks cannot make the new one's events late).
//!
//! # Documented design, not measured original behavior
//!
//! The rules, states and keys above are engine design: which score the
//! original awards, how a flag capture scores, what its end-of-match screen
//! shows and how a restart behaved are unknown (see
//! `docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`). Nothing in this
//! module supplies a default; every value is an input.

use std::fmt;

use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::net::{EventId, PeerId, SessionId};

use super::objective::{
    ObjectiveBoard, ObjectiveError, ObjectiveEvent, ObjectiveId, ObjectiveState, Ruling,
};
use super::result::{
    ConfigError, EndReason, LethalEvent, Limits, MatchResolver, Outcome, Roster, ScoreTable, Side,
    SubmitError, Submitted, VictoryRule,
};

/// The most possessable objectives one scenario may declare.
///
/// Engine bound: a mis-decoded or hostile catalog must not be able to make a
/// session allocate unbounded state. The retail maximum is far below this and
/// is pinned by the F56-C retail test; the number itself is a budget, not a
/// claim about the original.
pub const MAX_OBJECTIVES: u32 = 64;

/// Everything a host settles before a match starts: the map it selected, the
/// session generation it launches under, who plays, how the two limits and
/// the victory rule read, and how many possessable objectives the scenario
/// declares.
///
/// Each field is produced by an existing producer (see the module docs); this
/// record is the plain `cs_types`-typed boundary between them and the session,
/// so no crate outside `cs_sim` has to name a `cs_sim` type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionConfig {
    /// The selected scenario slot's content id: which map this match runs on.
    pub scenario: ContentId,
    /// The session generation this match belongs to. A restart allocates a
    /// new one; events of another generation are refused.
    pub session: SessionId,
    /// Who plays, and the team each holds.
    pub roster: Roster,
    /// What each lethal event is worth. Original values are unknown, so the
    /// host supplies it and this module never defaults one.
    pub table: ScoreTable,
    /// The time and score limits, normally built from
    /// `cs_net::rules::MatchRules::resolver_limits`.
    pub limits: Limits,
    /// The victory/draw rule the resolver runs, normally built from
    /// `cs_net::rules::Victory::label` through [`VictoryRule::from_label`].
    pub victory: VictoryRule,
    /// How many possessable objectives the scenario declares (`0` for a mode
    /// with none). The board allocates them in record order.
    pub objectives: u32,
}

/// Why a session could not be started, submitted to or restarted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    /// The match could not be configured at all: no participant, no limit, a
    /// non-positive score limit or a duplicate participant.
    Config(ConfigError),
    /// The scenario declares more objectives than one session holds.
    TooManyObjectives {
        /// What the scenario asked for.
        got: u32,
        /// [`MAX_OBJECTIVES`].
        max: u32,
    },
    /// A restart asked for the session generation already running: a restart
    /// is a new generation, so stale packets of the old one cannot be
    /// mistaken for the new match's.
    SameGeneration {
        /// The generation already running.
        session: SessionId,
    },
    /// The match already has its final result; nothing may change it.
    MatchOver,
    /// A lethal event was refused by the resolver.
    Lethal(SubmitError),
    /// An objective event was refused by the board.
    Objective(ObjectiveError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(f, "the match cannot start: {error}"),
            Self::TooManyObjectives { got, max } => {
                write!(
                    f,
                    "a scenario declares {got} objectives, the bound is {max}"
                )
            }
            Self::SameGeneration { session } => {
                write!(f, "a restart must be a new generation, not {session}")
            }
            Self::MatchOver => write!(f, "the match already has its final result"),
            Self::Lethal(error) => write!(f, "the lethal event was refused: {error}"),
            Self::Objective(error) => write!(f, "the objective event was refused: {error}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<ConfigError> for SessionError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<SubmitError> for SessionError {
    fn from(error: SubmitError) -> Self {
        Self::Lethal(error)
    }
}

impl From<ObjectiveError> for SessionError {
    fn from(error: ObjectiveError) -> Self {
        Self::Objective(error)
    }
}

/// What closing one tick did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickClose {
    /// The objective rulings, in application order (empty when the tick had
    /// nothing to judge or the match was already over).
    pub rulings: Vec<Ruling>,
    /// Whether the match sealed its final result on this tick.
    pub ended: bool,
}

/// One recorded delivery: which objective, who delivered it and on which
/// event. The results screen attributes a capture with this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capture {
    /// The delivered objective.
    pub objective: ObjectiveId,
    /// The participant that held it.
    pub scorer: PeerId,
    /// The event that recorded it (its id is what makes a retransmission a
    /// duplicate rather than a second capture).
    pub event: EventId,
}

/// The results screen's record of one finished match.
///
/// Built only from a sealed [`super::result::FinalResult`] plus the session's
/// own delivery ledger, and scoped to the session that produced it: after a
/// restart there is no [`EndOfMatch`] until the new match ends, so a previous
/// match's screen can never be rendered into the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndOfMatch {
    /// The map the match ran on.
    pub scenario: ContentId,
    /// The session generation this result belongs to.
    pub session: SessionId,
    /// The tick the match ended on.
    pub decided_at: Tick,
    /// Why it ended.
    pub reason: EndReason,
    /// Who won, or the drawn sides.
    pub outcome: Outcome,
    /// Every side's score, highest first, ties by side.
    pub standings: Vec<(Side, i32)>,
    /// Every recorded delivery, in the order the board recorded them.
    pub captures: Vec<Capture>,
}

impl EndOfMatch {
    /// The stable localization key a UI looks the end reason up under.
    ///
    /// The same discipline as `cs_net::lobby::RevokeReason::message_key`: the
    /// record carries ids, the UI carries the strings.
    pub const fn reason_key(&self) -> &'static str {
        match self.reason {
            EndReason::ScoreLimit => "match.end.score_limit",
            EndReason::TimeLimit => "match.end.time_limit",
            EndReason::ScoreAndTimeLimit => "match.end.score_and_time_limit",
        }
    }

    /// The stable localization key a UI looks the outcome up under.
    pub const fn outcome_key(&self) -> &'static str {
        match self.outcome {
            Outcome::Winner(_) => "match.outcome.win",
            Outcome::Draw(_) => "match.outcome.draw",
        }
    }

    /// The winning side, when exactly one side won.
    pub fn winner(&self) -> Option<Side> {
        match &self.outcome {
            Outcome::Winner(side) => Some(*side),
            Outcome::Draw(_) => None,
        }
    }
}

/// One running (or finished) match: its resolver, its objective board and its
/// clock, in one owner.
#[derive(Clone, Debug)]
pub struct MatchSession {
    scenario: ContentId,
    session: SessionId,
    roster: Roster,
    limits: Limits,
    objectives: u32,
    clock: Option<Tick>,
    resolver: MatchResolver,
    board: ObjectiveBoard,
}

impl MatchSession {
    /// Starts a match from its launch configuration.
    ///
    /// # Errors
    ///
    /// [`SessionError::Config`] (propagated from the resolver: an empty
    /// roster, no limit, a non-positive score limit, a duplicate participant)
    /// or [`SessionError::TooManyObjectives`]. Nothing is started on error, so
    /// a caller may correct the configuration and retry.
    pub fn start(config: SessionConfig) -> Result<Self, SessionError> {
        if config.objectives > MAX_OBJECTIVES {
            return Err(SessionError::TooManyObjectives {
                got: config.objectives,
                max: MAX_OBJECTIVES,
            });
        }
        let mut board = ObjectiveBoard::new(config.session, &config.roster);
        for _ in 0..config.objectives {
            board.declare();
        }
        let resolver = MatchResolver::with_victory(
            config.session,
            config.roster.clone(),
            config.table,
            config.limits,
            config.victory,
        )?;
        Ok(Self {
            scenario: config.scenario,
            session: config.session,
            roster: config.roster,
            limits: config.limits,
            objectives: config.objectives,
            clock: None,
            resolver,
            board,
        })
    }

    /// Restarts the match: tears this one down and starts `config` in its
    /// place, in one step.
    ///
    /// The next match is built **before** anything is dropped, so a refused
    /// configuration changes nothing and the caller can retry — and the new
    /// match inherits no score (every side starts at zero), no pickup (every
    /// objective is home with an empty ledger) and no timer (the clock starts
    /// again, so ticks closed by the previous generation cannot make this
    /// generation's events late).
    ///
    /// # Errors
    ///
    /// [`SessionError::SameGeneration`] when `config.session` is the
    /// generation already running — a restart is a new session epoch, so
    /// packets of the old one are refused rather than inherited — plus every
    /// [`MatchSession::start`] error, with this match left untouched.
    pub fn restart(&mut self, config: SessionConfig) -> Result<(), SessionError> {
        if config.session == self.session {
            return Err(SessionError::SameGeneration {
                session: self.session,
            });
        }
        let fresh = Self::start(config)?;
        *self = fresh;
        Ok(())
    }

    /// The map this match runs on.
    pub fn scenario(&self) -> ContentId {
        self.scenario.clone()
    }

    /// The session generation this match belongs to.
    pub fn session(&self) -> SessionId {
        self.session
    }

    /// Who plays, and the team each holds.
    pub fn roster(&self) -> &Roster {
        &self.roster
    }

    /// The limits this match runs under.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// How many objectives the scenario declared.
    pub fn objectives(&self) -> u32 {
        self.objectives
    }

    /// The last tick this session closed, `None` before the first close.
    ///
    /// This is the match's clock: it starts at `None` for every new
    /// generation, which is what makes a restart leak-free.
    pub fn clock(&self) -> Option<Tick> {
        self.clock
    }

    /// Whether the match has its final result.
    pub fn is_over(&self) -> bool {
        self.resolver.result().is_some()
    }

    /// The victory/draw rule this match runs.
    pub fn victory(&self) -> VictoryRule {
        self.resolver.victory()
    }

    /// A side's current score.
    pub fn score(&self, side: Side) -> Option<i32> {
        self.resolver.score(side)
    }

    /// The state of one of this session's objectives.
    pub fn objective_state(&self, objective: ObjectiveId) -> Option<ObjectiveState> {
        self.board.state(objective)
    }

    /// The allocated objective ids, in order.
    pub fn objective_ids(&self) -> impl Iterator<Item = ObjectiveId> + '_ {
        self.board.objective_ids()
    }

    /// The recorded deliveries of one objective, in event order.
    pub fn captures_of(&self, objective: ObjectiveId) -> Option<&[super::objective::ScoreRecord]> {
        self.board.scores(objective)
    }

    /// The sealed result, once the match has ended.
    pub fn result(&self) -> Option<&super::result::FinalResult> {
        self.resolver.result()
    }

    /// Queues one lethal event for its tick.
    ///
    /// # Errors
    ///
    /// [`SessionError::MatchOver`] once the result is sealed, otherwise
    /// [`SessionError::Lethal`] carrying the resolver's refusal; a refused
    /// event changes nothing.
    pub fn submit_lethal(&mut self, event: LethalEvent) -> Result<Submitted, SessionError> {
        if self.is_over() {
            return Err(SessionError::MatchOver);
        }
        Ok(self.resolver.submit(event)?)
    }

    /// Queues one objective event for its tick.
    ///
    /// # Errors
    ///
    /// [`SessionError::MatchOver`] once the result is sealed, otherwise
    /// [`SessionError::Objective`] carrying the board's refusal; a refused
    /// event changes nothing.
    pub fn submit_objective(&mut self, event: ObjectiveEvent) -> Result<Submitted, SessionError> {
        if self.is_over() {
            return Err(SessionError::MatchOver);
        }
        Ok(self.board.submit(event)?)
    }

    /// Closes `tick` for both consumers and reports what happened.
    ///
    /// The two halves are judged on the **same** tick: possession first, then
    /// scoring, then the limits once — so a delivery recorded in the closing
    /// tick is in the end-of-match record, and nothing is applied after the
    /// time limit (a tick past it is judged *as* the limit tick, exactly as
    /// the resolver documents). A tick at or before the closed one, or a call
    /// after the match ended, changes nothing.
    pub fn close_tick(&mut self, tick: Tick) -> TickClose {
        if self.is_over() {
            return TickClose {
                rulings: Vec::new(),
                ended: true,
            };
        }
        let judged = match self.limits.time_limit {
            Some(limit) if tick > limit => limit,
            _ => tick,
        };
        if self.clock.is_some_and(|closed| judged <= closed) {
            return TickClose {
                rulings: Vec::new(),
                ended: false,
            };
        }
        let rulings = self.board.close_tick(judged);
        let ended = self.resolver.close_tick(judged).is_some();
        self.clock = Some(judged);
        TickClose { rulings, ended }
    }

    /// The end-of-match record a results screen consumes, once the match has
    /// ended.
    ///
    /// `None` while the match runs — and `None` again after a restart, until
    /// the new generation ends: the record is scoped to the session that
    /// produced it.
    pub fn end_of_match(&self) -> Option<EndOfMatch> {
        let result = self.resolver.result()?;
        let mut captures = Vec::new();
        for objective in self.board.objective_ids() {
            for record in self.board.scores(objective).unwrap_or_default() {
                captures.push(Capture {
                    objective,
                    scorer: record.scorer,
                    event: record.event,
                });
            }
        }
        Some(EndOfMatch {
            scenario: self.scenario.clone(),
            session: result.session,
            decided_at: result.decided_at,
            reason: result.reason,
            outcome: result.outcome.clone(),
            standings: result.standings.clone(),
            captures,
        })
    }
}
