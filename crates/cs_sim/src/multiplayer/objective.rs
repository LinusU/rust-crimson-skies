//! Objective possession: the server-owned state machine for a claimable
//! objective (F56-B).
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-B`; contract `docs/contracts/UI-NETWORK.md` ("Server owns ...
//! mission program, score and result", "application-level ids deduplicate
//! events"). Non-negotiable 2: objective state is authoritative and singular —
//! possession, dropped, returned and scored cannot occur twice due to packet
//! retransmission.
//!
//! One [`ObjectiveBoard`] holds the match's possessable objectives (the flag
//! objectives a capture-the-flag slot declares; a mode with none simply
//! declares an empty board). A client never holds objective state: it submits
//! a [`ObjectiveAction::Claim`], and the board — the server's arbitration —
//! decides whether that claim applies. The other verbs are the server-side
//! facts the session produces: [`ObjectiveAction::Drop`] when the holder loses
//! it (voluntary or downed), [`ObjectiveAction::Return`] when a dropped
//! objective goes home, [`ObjectiveAction::Score`] when the holder delivers.
//!
//! # Documented arbitration order
//!
//! These rules are **engine design**, not measured original behavior (the
//! original possession, drop and delivery rules are unknown; see the F56-A
//! findings). They exist so every host arbitrates identically:
//!
//! 1. Events queue on [`ObjectiveBoard::submit`] and are applied in [`EventId`]
//!    order (tick, producer, sequence) when [`ObjectiveBoard::close_tick`]
//!    closes their tick — never in arrival order, so two clients claiming the
//!    same objective on one tick always get the same outcome: the lower event
//!    id wins, the later one is [`Verdict::Denied`]. An event is applied at
//!    most once per id; a retransmission is [`Submitted::Duplicate`].
//! 2. An event stamped for another session is [`ObjectiveError::WrongSession`]
//!    and one for an already closed tick is [`ObjectiveError::LateEvent`], so a
//!    reconnect or a recycled id cannot move another match's objectives.
//! 3. A claim names a participant of the match's roster; the objective ids are
//!    allocated by [`ObjectiveBoard::declare`] and never recycled inside the
//!    session.
//! 4. A delivered objective returns [`ObjectiveState::Home`] and records a
//!    [`ScoreRecord`] naming the holder and the event, so each capture is a
//!    distinct, attributable event while the same packet can never score
//!    twice.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::Tick;
use cs_types::net::{EventId, PeerId, SessionId};

use super::result::{Roster, Submitted};

/// One possessable objective of a match, allocated by the server.
///
/// Serials are handed out from 1 by [`ObjectiveBoard::declare`] and never
/// recycled inside the session, so a stale request can never alias a later
/// objective (the same discipline as [`cs_types::net::ActorId`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectiveId(u32);

impl ObjectiveId {
    /// Wraps an allocated serial; zero is refused.
    pub const fn new(serial: u32) -> Option<Self> {
        if serial == 0 {
            None
        } else {
            Some(Self(serial))
        }
    }

    /// The allocated serial.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ObjectiveId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "objective {}", self.0)
    }
}

/// Where one objective stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveState {
    /// Unheld, at its place; claimable.
    Home,
    /// Carried by the participant it names.
    Held(PeerId),
    /// Not held and away from home; claimable (a pickup) and returnable.
    Dropped,
}

impl ObjectiveState {
    /// The holder, when held.
    pub fn holder(self) -> Option<PeerId> {
        match self {
            Self::Held(peer) => Some(peer),
            _ => None,
        }
    }
}

/// The one action an [`ObjectiveEvent`] asks the board to apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveAction {
    /// `claimant` asks to take the objective. Applies while it is unheld
    /// ([`ObjectiveState::Home`] or [`ObjectiveState::Dropped`]); refused by
    /// [`Denial::AlreadyHeld`] otherwise.
    Claim {
        /// The participant asking.
        claimant: PeerId,
    },
    /// `holder` loses the objective (voluntary drop, downed, disconnected):
    /// held to dropped. The named peer must be the holder, so a third party's
    /// request cannot shed someone else's objective
    /// ([`Denial::CarrierMismatch`]).
    Drop {
        /// Who is asserted to lose it.
        holder: PeerId,
    },
    /// The objective goes back to its place; applies only while dropped.
    Return,
    /// The holder delivered the objective; applies only while held, returns
    /// it [`ObjectiveState::Home`] and records a [`ScoreRecord`].
    Score,
}

/// One reliable objective event, identified for deduplication by its
/// [`EventId`] — the same envelope discipline as
/// [`crate::multiplayer::result::LethalEvent`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectiveEvent {
    /// The event's identity; its tick is the tick it belongs to.
    pub id: EventId,
    /// The objective it names.
    pub objective: ObjectiveId,
    /// The requested action.
    pub action: ObjectiveAction,
}

/// Why an event was refused before it could queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveError {
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
    /// The objective id is not one this board allocated.
    UnknownObjective(ObjectiveId),
    /// The claimant or holder is not in this match's roster.
    UnknownParticipant(PeerId),
}

impl fmt::Display for ObjectiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongSession { got } => write!(f, "the event belongs to {got}"),
            Self::LateEvent { tick } => write!(f, "tick {} is already closed", tick.0),
            Self::UnknownObjective(objective) => {
                write!(f, "{objective} is not one this match holds")
            }
            Self::UnknownParticipant(peer) => write!(f, "{peer} is not in this match"),
        }
    }
}

impl std::error::Error for ObjectiveError {}

/// The transition an applied event performed: the four singular verbs of the
/// spec's non-negotiable 2, each happening at most once per event id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// [`ObjectiveAction::Claim`] applied: the objective is held by `holder`.
    Possessed {
        /// The participant now holding it.
        holder: PeerId,
    },
    /// [`ObjectiveAction::Drop`] applied: no longer held, not home.
    Dropped,
    /// [`ObjectiveAction::Return`] applied: back home.
    Returned,
    /// [`ObjectiveAction::Score`] applied: `scorer` delivered it and the
    /// objective is home again.
    Scored {
        /// The participant who held it.
        scorer: PeerId,
    },
}

/// Why the state an event found refused it at adjudication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denial {
    /// Claim: the objective is already held (names the holder, possibly the
    /// claimant themself).
    AlreadyHeld {
        /// Who holds it.
        holder: PeerId,
    },
    /// Drop or score: the objective is not held at all.
    NotHeld,
    /// Drop: the objective is held, but by a different participant.
    CarrierMismatch {
        /// Who actually holds it.
        held_by: PeerId,
    },
    /// Return: the objective is not dropped (it is home or still held).
    NotDropped,
}

impl fmt::Display for Denial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyHeld { holder } => write!(f, "{holder} already holds it"),
            Self::NotHeld => write!(f, "the objective is not held"),
            Self::CarrierMismatch { held_by } => {
                write!(f, "it is held by {held_by}, not the named peer")
            }
            Self::NotDropped => write!(f, "the objective is not dropped"),
        }
    }
}

/// The board's decision on one event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The action applied; the transition is recorded.
    Applied(Transition),
    /// The action was refused by the state it found; nothing changed.
    Denied(Denial),
}

/// The adjudication of one queued event, in application order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ruling {
    /// The event judged.
    pub event: EventId,
    /// The objective it named.
    pub objective: ObjectiveId,
    /// What was decided.
    pub verdict: Verdict,
}

/// One recorded delivery: who scored and on which event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScoreRecord {
    /// The scoring event.
    pub event: EventId,
    /// The participant who held the objective.
    pub scorer: PeerId,
}

/// The match's possessable objectives and the one arbitration path over them
/// (see the module docs for the order).
#[derive(Clone, Debug)]
pub struct ObjectiveBoard {
    session: SessionId,
    roster: Roster,
    next_serial: u32,
    states: BTreeMap<ObjectiveId, ObjectiveState>,
    scores: BTreeMap<ObjectiveId, Vec<ScoreRecord>>,
    seen: BTreeSet<EventId>,
    queued: BTreeMap<EventId, ObjectiveEvent>,
    closed_through: Option<Tick>,
}

impl ObjectiveBoard {
    /// An empty board for `session` over `roster`; objectives are then
    /// allocated with [`ObjectiveBoard::declare`].
    pub fn new(session: SessionId, roster: &Roster) -> Self {
        Self {
            session,
            roster: roster.clone(),
            next_serial: 1,
            states: BTreeMap::new(),
            scores: BTreeMap::new(),
            seen: BTreeSet::new(),
            queued: BTreeMap::new(),
            closed_through: None,
        }
    }

    /// Allocates a fresh objective id, initially [`ObjectiveState::Home`].
    ///
    /// Serials come from the server side of the session and are never reused,
    /// so a declared objective keeps its identity for the whole match.
    pub fn declare(&mut self) -> ObjectiveId {
        let id = ObjectiveId(self.next_serial);
        self.next_serial += 1;
        self.states.insert(id, ObjectiveState::Home);
        self.scores.insert(id, Vec::new());
        id
    }

    /// The allocated objective ids, in order.
    pub fn objective_ids(&self) -> impl Iterator<Item = ObjectiveId> + '_ {
        self.states.keys().copied()
    }

    /// The state of one objective, `None` for an id this board did not
    /// allocate.
    pub fn state(&self, objective: ObjectiveId) -> Option<ObjectiveState> {
        self.states.get(&objective).copied()
    }

    /// Who holds `objective`, when it is held.
    pub fn holder(&self, objective: ObjectiveId) -> Option<PeerId> {
        self.state(objective).and_then(ObjectiveState::holder)
    }

    /// The recorded deliveries of one objective, in event order.
    pub fn scores(&self, objective: ObjectiveId) -> Option<&[ScoreRecord]> {
        self.scores.get(&objective).map(Vec::as_slice)
    }

    /// Queues one objective event for its tick's adjudication.
    ///
    /// Session, tick-freshness, objective and participant checks run here and
    /// are [`ObjectiveError`]s; the arbitration itself happens in
    /// [`ObjectiveBoard::close_tick`], so two claims are judged against the
    /// same state regardless of which arrived first. The `Submitted` return is
    /// the same vocabulary [`super::result::MatchResolver`] reports.
    ///
    /// # Errors
    ///
    /// [`ObjectiveError`]; a refused event changes nothing.
    pub fn submit(&mut self, event: ObjectiveEvent) -> Result<Submitted, ObjectiveError> {
        if event.id.session != self.session {
            return Err(ObjectiveError::WrongSession {
                got: event.id.session,
            });
        }
        if self
            .closed_through
            .is_some_and(|tick| event.id.tick <= tick)
        {
            return Err(ObjectiveError::LateEvent {
                tick: event.id.tick,
            });
        }
        if !self.states.contains_key(&event.objective) {
            return Err(ObjectiveError::UnknownObjective(event.objective));
        }
        let actor = match event.action {
            ObjectiveAction::Claim { claimant } => Some(claimant),
            ObjectiveAction::Drop { holder } => Some(holder),
            ObjectiveAction::Return | ObjectiveAction::Score => None,
        };
        if let Some(peer) = actor
            && !self.roster.contains(peer)
        {
            return Err(ObjectiveError::UnknownParticipant(peer));
        }
        if !self.seen.insert(event.id) {
            return Ok(Submitted::Duplicate);
        }
        self.queued.insert(event.id, event);
        Ok(Submitted::Accepted)
    }

    /// Applies every queued event up to `tick`, in [`EventId`] order, and
    /// returns each event's ruling.
    ///
    /// Events for later ticks stay queued; a tick at or before the last closed
    /// one changes nothing and returns an empty list.
    pub fn close_tick(&mut self, tick: Tick) -> Vec<Ruling> {
        if self.closed_through.is_some_and(|closed| tick <= closed) {
            return Vec::new();
        }
        let due: Vec<EventId> = self
            .queued
            .keys()
            .copied()
            .take_while(|id| id.tick <= tick)
            .collect();
        let mut rulings = Vec::with_capacity(due.len());
        for id in due {
            if let Some(event) = self.queued.remove(&id) {
                rulings.push(Ruling {
                    event: id,
                    objective: event.objective,
                    verdict: self.apply(&event),
                });
            }
        }
        self.closed_through = Some(tick);
        rulings
    }

    fn apply(&mut self, event: &ObjectiveEvent) -> Verdict {
        let state = self.states[&event.objective];
        match (state, event.action) {
            (
                ObjectiveState::Home | ObjectiveState::Dropped,
                ObjectiveAction::Claim { claimant },
            ) => {
                self.states
                    .insert(event.objective, ObjectiveState::Held(claimant));
                Verdict::Applied(Transition::Possessed { holder: claimant })
            }
            (ObjectiveState::Held(holder), ObjectiveAction::Claim { .. }) => {
                Verdict::Denied(Denial::AlreadyHeld { holder })
            }
            (ObjectiveState::Held(holder), ObjectiveAction::Drop { holder: named }) => {
                if named == holder {
                    self.states.insert(event.objective, ObjectiveState::Dropped);
                    Verdict::Applied(Transition::Dropped)
                } else {
                    Verdict::Denied(Denial::CarrierMismatch { held_by: holder })
                }
            }
            (ObjectiveState::Home | ObjectiveState::Dropped, ObjectiveAction::Drop { .. }) => {
                Verdict::Denied(Denial::NotHeld)
            }
            (ObjectiveState::Dropped, ObjectiveAction::Return) => {
                self.states.insert(event.objective, ObjectiveState::Home);
                Verdict::Applied(Transition::Returned)
            }
            (ObjectiveState::Home | ObjectiveState::Held(_), ObjectiveAction::Return) => {
                Verdict::Denied(Denial::NotDropped)
            }
            (ObjectiveState::Held(scorer), ObjectiveAction::Score) => {
                self.states.insert(event.objective, ObjectiveState::Home);
                self.scores
                    .get_mut(&event.objective)
                    .expect("declared objectives have a score ledger")
                    .push(ScoreRecord {
                        event: event.id,
                        scorer,
                    });
                Verdict::Applied(Transition::Scored { scorer })
            }
            (ObjectiveState::Home | ObjectiveState::Dropped, ObjectiveAction::Score) => {
                Verdict::Denied(Denial::NotHeld)
            }
        }
    }
}
