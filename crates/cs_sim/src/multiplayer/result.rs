//! The single final result of a match.
//!
//! # Documented resolution order
//!
//! These rules are **engine design**, not measured original behavior (the
//! original tie, limit and simultaneity rules are unknown; see the F56-A
//! finding). They exist so that every host computes one result:
//!
//! 1. Events are applied in [`EventId`] order (tick, producer, sequence), not
//!    arrival order, and only when their tick is *closed* with
//!    [`MatchResolver::close_tick`]. All events of a tick are applied before
//!    anything is judged, so two kills in the same tick are simultaneous: a
//!    pilot who is killed on the tick he scores still scores.
//! 2. The match ends at the first closed tick where a side holds at least the
//!    score limit **or** the tick reaches the time limit. An event stamped
//!    after the time limit is never scored.
//! 3. The highest score wins; equal top scores are a [`Outcome::Draw`] of
//!    exactly those sides. No tie-break is invented.
//! 4. The result is sealed once: later events are refused with
//!    [`SubmitError::AlreadyFinal`] and [`MatchResolver::close_tick`] keeps
//!    returning the same result.
//! 5. An event is scored at most once per [`EventId`] (a retransmission is
//!    [`Submitted::Duplicate`]), and only inside the [`SessionId`] the
//!    resolver was built for, so a reconnect or a recycled [`PeerId`] cannot
//!    inherit another match's score.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::Tick;
use cs_types::net::{EventId, PeerId, SessionId};

/// Who scores: one pilot in a free-for-all, one team otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// A single participant.
    Participant(PeerId),
    /// A team slot.
    Team(u8),
}

/// The participants of one match and the team each holds.
///
/// Either every participant holds a team or none does; a mixed roster is
/// refused so a result can never compare a pilot with a team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roster {
    teams: BTreeMap<PeerId, Option<u8>>,
}

/// Why a [`Roster`] or [`MatchResolver`] was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// No participant at all.
    EmptyRoster,
    /// A participant appears twice.
    DuplicateParticipant(PeerId),
    /// A team game has fewer than two teams in play.
    TooFewTeams,
    /// Neither limit is set, so the match could never end.
    NoLimit,
    /// A score limit of zero or less ends the match before it starts.
    NonPositiveScoreLimit,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRoster => write!(f, "a match needs at least one participant"),
            Self::DuplicateParticipant(peer) => write!(f, "{peer} is listed twice"),
            Self::TooFewTeams => write!(f, "a team match needs at least two teams in play"),
            Self::NoLimit => write!(f, "a match needs a time limit or a score limit"),
            Self::NonPositiveScoreLimit => write!(f, "the score limit must be positive"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Roster {
    /// A free-for-all: every participant scores for themself.
    ///
    /// # Errors
    ///
    /// [`ConfigError::EmptyRoster`], [`ConfigError::DuplicateParticipant`].
    pub fn free_for_all(peers: &[PeerId]) -> Result<Self, ConfigError> {
        let mut teams = BTreeMap::new();
        for peer in peers {
            if teams.insert(*peer, None).is_some() {
                return Err(ConfigError::DuplicateParticipant(*peer));
            }
        }
        if teams.is_empty() {
            return Err(ConfigError::EmptyRoster);
        }
        Ok(Self { teams })
    }

    /// A team match: each participant with the team slot they hold.
    ///
    /// # Errors
    ///
    /// [`ConfigError::EmptyRoster`], [`ConfigError::DuplicateParticipant`],
    /// [`ConfigError::TooFewTeams`].
    pub fn teams(members: &[(PeerId, u8)]) -> Result<Self, ConfigError> {
        let mut teams = BTreeMap::new();
        for (peer, team) in members {
            if teams.insert(*peer, Some(*team)).is_some() {
                return Err(ConfigError::DuplicateParticipant(*peer));
            }
        }
        if teams.is_empty() {
            return Err(ConfigError::EmptyRoster);
        }
        let distinct: BTreeSet<u8> = members.iter().map(|(_, team)| *team).collect();
        if distinct.len() < 2 {
            return Err(ConfigError::TooFewTeams);
        }
        Ok(Self { teams })
    }

    fn side_of(&self, peer: PeerId) -> Option<Side> {
        self.teams.get(&peer).map(|team| match team {
            Some(team) => Side::Team(*team),
            None => Side::Participant(peer),
        })
    }

    fn sides(&self) -> BTreeSet<Side> {
        self.teams
            .keys()
            .filter_map(|peer| self.side_of(*peer))
            .collect()
    }
}

/// The points one lethal event is worth. Caller-supplied: the original values
/// are unknown, so there is no `Default`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScoreTable {
    /// Credited to the killer's side for downing an opponent.
    pub kill: i32,
    /// Added to the victim's side when a pilot crashes or self-destructs.
    pub crash: i32,
    /// Added to the killer's side for downing a teammate.
    pub team_kill: i32,
}

/// When a match ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The tick at which the match ends; events stamped on it still count.
    pub time_limit: Option<Tick>,
    /// The score at which a side wins outright.
    pub score_limit: Option<i32>,
}

/// How a pilot went down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LethalKind {
    /// Shot down by another participant.
    Kill {
        /// The participant credited.
        killer: PeerId,
    },
    /// Crashed or self-destructed.
    Crash,
}

/// One reliable lethal event, identified for deduplication by its [`EventId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LethalEvent {
    /// The event's identity; its tick is the tick it belongs to.
    pub id: EventId,
    /// The pilot who went down.
    pub victim: PeerId,
    /// How.
    pub kind: LethalKind,
}

/// What the resolver did with a submitted event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submitted {
    /// Queued for its tick.
    Accepted,
    /// The same [`EventId`] was already seen; nothing changed.
    Duplicate,
}

/// Why an event was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmitError {
    /// The match already has its final result.
    AlreadyFinal,
    /// The event belongs to another session generation.
    WrongSession {
        /// The session the event named.
        got: SessionId,
    },
    /// The event's tick was already closed.
    LateEvent {
        /// The event's tick.
        tick: Tick,
    },
    /// The victim or the killer is not in this match's roster.
    UnknownParticipant(PeerId),
    /// A kill credited the victim themself; that is a [`LethalKind::Crash`].
    SelfKill(PeerId),
}

impl fmt::Display for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyFinal => write!(f, "the match already has its final result"),
            Self::WrongSession { got } => write!(f, "the event belongs to {got}"),
            Self::LateEvent { tick } => write!(f, "tick {} is already closed", tick.0),
            Self::UnknownParticipant(peer) => write!(f, "{peer} is not in this match"),
            Self::SelfKill(peer) => write!(f, "{peer} cannot be credited with their own kill"),
        }
    }
}

impl std::error::Error for SubmitError {}

/// Why the match ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// A side reached the score limit.
    ScoreLimit,
    /// The time limit was reached.
    TimeLimit,
    /// Both held at the same tick; one result is still produced.
    ScoreAndTimeLimit,
}

/// Who won.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// One side holds the top score.
    Winner(Side),
    /// These sides share the top score.
    Draw(Vec<Side>),
}

/// The one sealed result of a match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalResult {
    /// The session generation the result belongs to.
    pub session: SessionId,
    /// The tick the match ended on.
    pub decided_at: Tick,
    /// Why.
    pub reason: EndReason,
    /// The winner or the drawn sides.
    pub outcome: Outcome,
    /// Every side's score, highest first, ties by side.
    pub standings: Vec<(Side, i32)>,
}

/// Folds lethal events and limits into one [`FinalResult`] (see the module
/// docs for the resolution order).
#[derive(Clone, Debug)]
pub struct MatchResolver {
    session: SessionId,
    roster: Roster,
    table: ScoreTable,
    limits: Limits,
    scores: BTreeMap<Side, i32>,
    seen: BTreeSet<EventId>,
    queued: BTreeMap<EventId, LethalEvent>,
    closed_through: Option<Tick>,
    result: Option<FinalResult>,
}

impl MatchResolver {
    /// Starts a match with every side at zero.
    ///
    /// # Errors
    ///
    /// [`ConfigError::NoLimit`], [`ConfigError::NonPositiveScoreLimit`].
    pub fn new(
        session: SessionId,
        roster: Roster,
        table: ScoreTable,
        limits: Limits,
    ) -> Result<Self, ConfigError> {
        if limits.time_limit.is_none() && limits.score_limit.is_none() {
            return Err(ConfigError::NoLimit);
        }
        if limits.score_limit.is_some_and(|limit| limit <= 0) {
            return Err(ConfigError::NonPositiveScoreLimit);
        }
        let scores = roster.sides().into_iter().map(|side| (side, 0)).collect();
        Ok(Self {
            session,
            roster,
            table,
            limits,
            scores,
            seen: BTreeSet::new(),
            queued: BTreeMap::new(),
            closed_through: None,
            result: None,
        })
    }

    /// Queues one lethal event for its tick.
    ///
    /// # Errors
    ///
    /// [`SubmitError`]; a refused event changes nothing.
    pub fn submit(&mut self, event: LethalEvent) -> Result<Submitted, SubmitError> {
        if self.result.is_some() {
            return Err(SubmitError::AlreadyFinal);
        }
        if event.id.session != self.session {
            return Err(SubmitError::WrongSession {
                got: event.id.session,
            });
        }
        if self
            .closed_through
            .is_some_and(|tick| event.id.tick <= tick)
        {
            return Err(SubmitError::LateEvent {
                tick: event.id.tick,
            });
        }
        if self.roster.side_of(event.victim).is_none() {
            return Err(SubmitError::UnknownParticipant(event.victim));
        }
        if let LethalKind::Kill { killer } = event.kind {
            if killer == event.victim {
                return Err(SubmitError::SelfKill(killer));
            }
            if self.roster.side_of(killer).is_none() {
                return Err(SubmitError::UnknownParticipant(killer));
            }
        }
        if !self.seen.insert(event.id) {
            return Ok(Submitted::Duplicate);
        }
        self.queued.insert(event.id, event);
        Ok(Submitted::Accepted)
    }

    /// Applies every queued event up to `tick`, then judges the limits once.
    ///
    /// Returns the final result when the match ended on or before this tick.
    /// A tick past the time limit is judged as the time-limit tick: events
    /// stamped after the limit are dropped unscored. Once sealed, the same
    /// result is returned for every later call.
    pub fn close_tick(&mut self, tick: Tick) -> Option<&FinalResult> {
        if self.result.is_some() {
            return self.result.as_ref();
        }
        let judged = match self.limits.time_limit {
            Some(limit) if tick > limit => limit,
            _ => tick,
        };
        if self.closed_through.is_some_and(|closed| judged <= closed) {
            return None;
        }
        let due: Vec<EventId> = self
            .queued
            .keys()
            .copied()
            .take_while(|id| id.tick <= judged)
            .collect();
        for id in due {
            if let Some(event) = self.queued.remove(&id) {
                self.apply(&event);
            }
        }
        self.closed_through = Some(judged);

        let score_hit = self
            .limits
            .score_limit
            .is_some_and(|limit| self.scores.values().any(|score| *score >= limit));
        let time_hit = self.limits.time_limit.is_some_and(|limit| judged >= limit);
        let reason = match (score_hit, time_hit) {
            (true, true) => EndReason::ScoreAndTimeLimit,
            (true, false) => EndReason::ScoreLimit,
            (false, true) => EndReason::TimeLimit,
            (false, false) => return None,
        };
        self.queued.clear();
        self.result = Some(self.seal(judged, reason));
        self.result.as_ref()
    }

    /// The sealed result, once the match has ended.
    pub fn result(&self) -> Option<&FinalResult> {
        self.result.as_ref()
    }

    /// A side's current score.
    pub fn score(&self, side: Side) -> Option<i32> {
        self.scores.get(&side).copied()
    }

    fn apply(&mut self, event: &LethalEvent) {
        let victim_side = self.roster.side_of(event.victim);
        let (side, points) = match event.kind {
            LethalKind::Crash => (victim_side, self.table.crash),
            LethalKind::Kill { killer } => {
                let killer_side = self.roster.side_of(killer);
                let friendly =
                    matches!(killer_side, Some(Side::Team(_))) && killer_side == victim_side;
                (
                    killer_side,
                    if friendly {
                        self.table.team_kill
                    } else {
                        self.table.kill
                    },
                )
            }
        };
        if let Some(score) = side.and_then(|side| self.scores.get_mut(&side)) {
            *score += points;
        }
    }

    fn seal(&self, decided_at: Tick, reason: EndReason) -> FinalResult {
        let mut standings: Vec<(Side, i32)> = self.scores.iter().map(|(s, v)| (*s, *v)).collect();
        standings.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let top = standings.first().map_or(0, |(_, score)| *score);
        let leaders: Vec<Side> = standings
            .iter()
            .take_while(|(_, score)| *score == top)
            .map(|(side, _)| *side)
            .collect();
        let outcome = match leaders.as_slice() {
            [only] => Outcome::Winner(*only),
            _ => Outcome::Draw(leaders),
        };
        FinalResult {
            session: self.session,
            decided_at,
            reason,
            outcome,
            standings,
        }
    }
}
