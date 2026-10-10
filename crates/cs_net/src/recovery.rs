//! Reconnect, late-join and reward-replay rules (F58-A).
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stages `### F58-A` and non-negotiable behaviors 3 and 4. Contract:
//! `docs/contracts/UI-NETWORK.md` ("Reconnect uses fresh authenticated session
//! identity and a full authoritative snapshot; a client-provided score,
//! health, faction or outcome is never accepted" and "Reliable delivery does
//! not replace application idempotency because reconnect/retry can replay
//! requests").
//!
//! # What F58-A defines
//!
//! The typed inputs, outputs and rules of resuming a match, with the runtime
//! wiring built on top of them by F58-C (see the section below):
//!
//! * [`SessionGenerations`] mints a **fresh** epoch for every session. It is
//!   monotonic and never reuses an id, so a packet stamped with the epoch the
//!   client last knew is stale under [`crate::validation`]'s rule 1.
//! * [`ClientClaim`] is state a returning client may offer — score, damage,
//!   health, faction, outcome, aircraft and rewards. None of it is ever
//!   applied: [`decide_recovery`] lists every claim as refused and résumés the
//!   client from [`ResumeState::FullAuthoritativeSnapshot`] alone.
//! * [`PilotBindings`] is the one-pilot-per-aircraft table. A reconnect may
//!   reclaim the aircraft its own prior peer flew, and is refused any other
//!   pilot's.
//! * [`RewardLedger`] deduplicates a one-shot pickup or capture reward by its
//!   match-stable [`AwardId`], so replaying the delivery after a reconnect can
//!   never award it twice.
//!
//! Host migration is out of scope (spec behavior 3): if the host is gone and
//! the (F55) [`HostLoss`] policy has no migration, the match ends; this module
//! does not pretend seamless recovery. [`HostLoss`] is re-exported only to
//! name that boundary in the decision types.
//!
//! # What F58-C adds
//!
//! The disconnect and clean host-loss vocabulary, plus the once-only guard the
//! settlement runs behind:
//!
//! * [`DisconnectCause`] is the bounded reason a peer left — its own farewell,
//!   a dead link, an abuse cut-off naming the refusal that cut it, or the host
//!   being gone. [`DisconnectCause::wire_reason`] maps it onto
//!   [`crate::message::DisconnectReason`] for the consumer that tells the
//!   client, and deliberately returns `None` for abuse: the wire has no abuse
//!   arm yet, and naming another reason would be a falsehood (task #815 owns
//!   that arm).
//! * [`DepartureLedger`] settles **each peer's departure once**: the first
//!   report is [`Settlement::Applied`], every later report of the same peer is
//!   [`Settlement::Duplicate`] and applies nothing. Its records are bounded by
//!   [`crate::bounds::MAX_SESSION_PEERS`] — the session's own peer-id space —
//!   so a flood of departure notices cannot grow it, and a ledger that is
//!   full refuses by name instead of growing.
//!
//! `cs_app::network::recovery::RecoveryFlow` is where these are wired to their
//! producers (the receive path and the transport's dead-link report) and their
//! consumers (the gate teardown, the authoritative match state, the client's
//! return to the menu).
//!
//! All values are newly authored engine design: no original reconnect,
//! late-join or reward behavior has been measured, and none is asserted.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::Tick;
use cs_types::net::{ActorId, PeerId, SessionId};

use crate::bounds::MAX_SESSION_PEERS;
use crate::compat::{PeerAllocError, PeerAllocator};
use crate::lobby::{HostLoss, LateJoin, TeamId};

/// Why a recovery operation could not proceed at the transport-independent
/// layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryError {
    /// The monotonic session-id space is exhausted, so no fresh epoch can be
    /// issued and no reconnect can be admitted.
    SessionSpaceExhausted,
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionSpaceExhausted => {
                write!(f, "the session-id space is exhausted")
            }
        }
    }
}

impl std::error::Error for RecoveryError {}

/// The host's allocator of fresh session epochs.
///
/// Ids are strictly increasing and never reused, so two live sessions can
/// never share one and a stale packet can never be mistaken for the live
/// epoch. The host creates one allocator per running app, not per session.
#[derive(Clone, Copy, Debug)]
pub struct SessionGenerations {
    next: u64,
}

impl SessionGenerations {
    /// Starts allocation at session 1, so session 0 is never live.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 1 }
    }

    /// Starts allocation at `next`; zero is clamped to 1 so session 0 is never
    /// live. Used by tests and by a resumed app that persists its counter.
    #[must_use]
    pub const fn starting_at(next: u64) -> Self {
        Self {
            next: if next == 0 { 1 } else { next },
        }
    }

    /// The epoch the next successful issue will return.
    #[must_use]
    pub const fn peek(&self) -> u64 {
        self.next
    }

    /// Issues the next fresh epoch.
    ///
    /// # Errors
    ///
    /// [`RecoveryError::SessionSpaceExhausted`] when the counter cannot
    /// advance, which includes the top of the space: the allocator refuses to
    /// issue the last representable id rather than issue one it can never move
    /// past. It then stays exhausted rather than reusing an id.
    pub const fn issue(&mut self) -> Result<SessionId, RecoveryError> {
        let Some(session) = SessionId::new(self.next) else {
            return Err(RecoveryError::SessionSpaceExhausted);
        };
        let Some(next) = self.next.checked_add(1) else {
            return Err(RecoveryError::SessionSpaceExhausted);
        };
        self.next = next;
        Ok(session)
    }
}

impl Default for SessionGenerations {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a returning client was in the match (a reconnect) or is new (a
/// late join).
///
/// The distinction is why a closed [`LateJoin`] policy still admits a
/// reconnect: the returning pilot already has authoritative state, while a
/// newcomer does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecoveryKind {
    /// A pilot that was already in the match and lost its connection.
    Reconnect,
    /// A client joining a match that already launched.
    LateJoin,
}

impl RecoveryKind {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reconnect => "reconnect",
            Self::LateJoin => "late_join",
        }
    }
}

/// State a returning or late client offers.
///
/// The contract is explicit that none of this is accepted: score, damage,
/// health, faction and outcome are server-owned, and a reconnect cannot bind
/// to another pilot's aircraft. Every variant exists so the server can name it
/// in the refusal list instead of silently trusting it.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientClaim {
    /// A score the client says it has.
    Score {
        /// The claimed point total.
        points: i64,
    },
    /// Damage the client says it already dealt.
    Damage {
        /// The aircraft it says it damaged.
        target: ActorId,
        /// The structure it says it removed.
        amount: u32,
    },
    /// A remaining-structure fraction the client says it has.
    Health {
        /// The claimed fraction.
        fraction: f64,
    },
    /// A team the client says it is on.
    Faction {
        /// The claimed team.
        team: TeamId,
    },
    /// A match outcome the client asserts.
    Outcome,
    /// A specific aircraft the client says it flies.
    Aircraft {
        /// The claimed actor.
        actor: ActorId,
    },
    /// Rewards the client says it already earned.
    Rewards {
        /// How many it claims.
        count: u64,
    },
}

impl ClientClaim {
    /// The stable label used in reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Score { .. } => "score",
            Self::Damage { .. } => "damage",
            Self::Health { .. } => "health",
            Self::Faction { .. } => "faction",
            Self::Outcome => "outcome",
            Self::Aircraft { .. } => "aircraft",
            Self::Rewards { .. } => "rewards",
        }
    }
}

impl fmt::Display for ClientClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A returning or late client's typed offer to resume.
#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryRequest {
    /// Whether this is a reconnect or a late join.
    pub kind: RecoveryKind,
    /// The epoch the client last knew. Informational only: the server never
    /// resumes this epoch, it issues a fresh one.
    pub resumed_from: SessionId,
    /// The client-authored state it offers. Every item is refused.
    pub claimed: Vec<ClientClaim>,
}

/// The match-side facts a recovery decision depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryPolicy {
    /// Whether the mode admits members after launch (F55 [`LateJoin`]).
    pub late_join: LateJoin,
    /// Whether the match is still running (a finished match has nothing to
    /// resume).
    pub match_running: bool,
    /// Whether authoritative state still exists to send (F57-A's server
    /// ledger). Without it there is nothing to resume from.
    pub authoritative_state: bool,
    /// What losing the host does (F55 [`HostLoss`]); recorded here so the
    /// recovery decision names the boundary it does not cross.
    pub host_loss: HostLoss,
}

/// Where a resumed client's state comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeState {
    /// The only accepted source: the server's full authoritative snapshot
    /// (the F57-A schema). There is no "resume from the client's word" arm.
    FullAuthoritativeSnapshot,
}

/// Why a recovery request was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryRefusal {
    /// A late join arrived after the mode closed late join.
    LateJoinClosed,
    /// The match already ended or was aborted.
    MatchEnded,
    /// No authoritative state remains to resume from.
    NoAuthoritativeState,
    /// The session already holds its peer cap.
    SessionFull {
        /// The cap.
        max: usize,
    },
    /// No fresh session epoch could be issued.
    SessionSpaceExhausted,
}

impl fmt::Display for RecoveryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LateJoinClosed => write!(f, "this mode does not admit late joins"),
            Self::MatchEnded => write!(f, "the match is over"),
            Self::NoAuthoritativeState => {
                write!(f, "no authoritative state remains to resume from")
            }
            Self::SessionFull { max } => {
                write!(f, "the session already holds its maximum of {max} peers")
            }
            Self::SessionSpaceExhausted => write!(f, "the session-id space is exhausted"),
        }
    }
}

impl std::error::Error for RecoveryRefusal {}

/// What the server does with a [`RecoveryRequest`].
#[derive(Clone, Debug, PartialEq)]
pub enum RecoveryDecision {
    /// Admit under a fresh epoch and server-allocated peer. `refused_claims`
    /// lists every [`ClientClaim`] the client offered, in order; the state the
    /// client receives is the full authoritative snapshot and nothing else.
    Resume {
        /// The fresh epoch, never equal to `request.resumed_from`.
        session: SessionId,
        /// The freshly allocated peer identity.
        peer: PeerId,
        /// The only state source.
        state: ResumeState,
        /// Every claim that was refused (all of them).
        refused_claims: Vec<ClientClaim>,
    },
    /// Refuse with a bounded reason.
    Refuse(RecoveryRefusal),
}

impl RecoveryDecision {
    /// Whether the request was admitted.
    #[must_use]
    pub const fn resumed(&self) -> bool {
        matches!(self, Self::Resume { .. })
    }

    /// The refusal, when there was one.
    #[must_use]
    pub const fn refusal(&self) -> Option<RecoveryRefusal> {
        match self {
            Self::Refuse(refusal) => Some(*refusal),
            Self::Resume { .. } => None,
        }
    }
}

/// Decides a recovery request and, on success, mints a fresh epoch and peer.
///
/// The rules, in order:
///
/// 1. a finished match cannot be resumed ([`RecoveryRefusal::MatchEnded`]);
/// 2. a resume needs authoritative state ([`RecoveryRefusal::NoAuthoritativeState`]);
/// 3. a late join is refused when the mode closed late join; a reconnect never
///    is ([`RecoveryRefusal::LateJoinClosed`]);
/// 4. a [fresh epoch](SessionGenerations::issue) is issued — never the one the
///    client named — and a peer is allocated;
/// 5. every [`ClientClaim`] is carried into `refused_claims` and none is
///    applied.
pub fn decide_recovery(
    policy: &RecoveryPolicy,
    generations: &mut SessionGenerations,
    peers: &mut PeerAllocator,
    request: &RecoveryRequest,
) -> RecoveryDecision {
    if !policy.match_running {
        return RecoveryDecision::Refuse(RecoveryRefusal::MatchEnded);
    }
    if !policy.authoritative_state {
        return RecoveryDecision::Refuse(RecoveryRefusal::NoAuthoritativeState);
    }
    if request.kind == RecoveryKind::LateJoin && policy.late_join == LateJoin::Closed {
        return RecoveryDecision::Refuse(RecoveryRefusal::LateJoinClosed);
    }
    let session = match generations.issue() {
        Ok(session) => session,
        Err(RecoveryError::SessionSpaceExhausted) => {
            return RecoveryDecision::Refuse(RecoveryRefusal::SessionSpaceExhausted);
        }
    };
    let peer = match peers.allocate() {
        Ok(peer) => peer,
        Err(PeerAllocError::Full) => {
            return RecoveryDecision::Refuse(RecoveryRefusal::SessionFull {
                max: MAX_SESSION_PEERS,
            });
        }
    };
    RecoveryDecision::Resume {
        session,
        peer,
        state: ResumeState::FullAuthoritativeSnapshot,
        refused_claims: request.claimed.clone(),
    }
}

/// What binding an actor to a peer did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindOutcome {
    /// The aircraft was free; the peer now flies it.
    Bound,
    /// The aircraft was bound to the peer's own prior identity; the binding
    /// moved to the new peer (a reconnect reclaiming its own aircraft).
    Reclaimed {
        /// The peer identity that previously held it.
        from: PeerId,
    },
    /// The aircraft belongs to a different live pilot; refused.
    RefusedOtherPilot {
        /// The current owner.
        owner: PeerId,
    },
}

/// The server's one-pilot-per-aircraft binding table.
///
/// Reconnect and late join must not bind to another pilot's aircraft (spec
/// behavior 4). A reconnect reclaims the aircraft its own prior peer flew by
/// naming that prior identity ([`PilotBindings::rebind`]); naming anyone
/// else's is refused.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PilotBindings {
    owners: BTreeMap<ActorId, PeerId>,
}

impl PilotBindings {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds a free `actor` to `peer`.
    ///
    /// # Errors
    ///
    /// Returns [`BindOutcome::RefusedOtherPilot`] (and changes nothing) when
    /// the actor already belongs to a peer other than `peer`. Rebinding to
    /// oneself is idempotent and reported as [`BindOutcome::Bound`].
    pub fn bind(&mut self, peer: PeerId, actor: ActorId) -> BindOutcome {
        match self.owners.get(&actor) {
            Some(&owner) if owner != peer => BindOutcome::RefusedOtherPilot { owner },
            _ => {
                self.owners.insert(actor, peer);
                BindOutcome::Bound
            }
        }
    }

    /// Moves a binding from `prior` to `new_peer`, as a reconnect does.
    ///
    /// The move is allowed only when `prior` actually holds the actor, so a
    /// returning pilot can reclaim its own aircraft and no one else's.
    ///
    /// # Errors
    ///
    /// Returns [`BindOutcome::RefusedOtherPilot`] (and changes nothing) when
    /// the actor is held by someone other than `prior`.
    pub fn rebind(&mut self, new_peer: PeerId, actor: ActorId, prior: PeerId) -> BindOutcome {
        match self.owners.get(&actor) {
            Some(&owner) if owner == prior => {
                self.owners.insert(actor, new_peer);
                BindOutcome::Reclaimed { from: prior }
            }
            Some(&owner) => BindOutcome::RefusedOtherPilot { owner },
            None => {
                // Nothing to reclaim: the aircraft was not bound. Bind it
                // fresh, which is what a late join to a spawned aircraft does.
                self.owners.insert(actor, new_peer);
                BindOutcome::Bound
            }
        }
    }

    /// The peer that owns `actor`, if any.
    #[must_use]
    pub fn owner(&self, actor: ActorId) -> Option<PeerId> {
        self.owners.get(&actor).copied()
    }
}

/// A one-shot reward's match-stable identity.
///
/// It is deliberately **not** an `EventId`: an [`cs_types::net::EventId`]
/// carries a [`SessionId`], and a reconnect runs a fresh epoch, so an id that
/// changed with the epoch could not detect a replay across it. The reward
/// domain allocates one `AwardId` per pickup or capture when the match begins
/// or when it occurs, and never reuses it for the match's lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AwardId(u64);

impl AwardId {
    /// Wraps a nonzero award number.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// The award number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// What awarding one [`AwardId`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AwardOutcome {
    /// The first award: this delivery grants it.
    Awarded,
    /// A replay: the award was already granted to `first`. Nothing changes.
    AlreadyAwarded {
        /// The peer the first award went to.
        first: PeerId,
    },
}

impl AwardOutcome {
    /// Whether this delivery granted the reward.
    #[must_use]
    pub const fn awarded(self) -> bool {
        matches!(self, Self::Awarded)
    }
}

/// The server's award-once ledger for pickups and captures.
///
/// A reconnect or late join may replay a reliable delivery (contract:
/// "Reliable delivery does not replace application idempotency"); keying on
/// the match-stable [`AwardId`] makes the second delivery report
/// [`AwardOutcome::AlreadyAwarded`] instead of granting again. The ledger's
/// state is bounded by the awards a match actually grants.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RewardLedger {
    awarded: BTreeMap<AwardId, PeerId>,
}

impl RewardLedger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Grants `id` to `peer` unless it was already granted.
    pub fn award(&mut self, id: AwardId, peer: PeerId) -> AwardOutcome {
        match self.awarded.get(&id) {
            Some(&first) => AwardOutcome::AlreadyAwarded { first },
            None => {
                self.awarded.insert(id, peer);
                AwardOutcome::Awarded
            }
        }
    }

    /// Whether `id` was already granted.
    #[must_use]
    pub fn is_awarded(&self, id: AwardId) -> bool {
        self.awarded.contains_key(&id)
    }

    /// How many distinct awards were granted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.awarded.len()
    }

    /// Whether no award was granted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.awarded.is_empty()
    }
}

// ---------------------------------------------------------------------------
// The disconnect and clean host-loss flow (F58-C)
// ---------------------------------------------------------------------------

/// Why a peer left the live session: the bounded vocabulary the disconnect
/// flow reports to its consumers — the transport that hangs the connection
/// up, the client it tells, the log it writes.
///
/// Every cause is a designed reason of the new engine; no original
/// disconnect, timeout or abuse behavior has been measured and none is
/// asserted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisconnectCause {
    /// The client sent its own farewell (the wire's `ClientPayload::Leave`).
    Voluntary,
    /// The link stopped answering. No packet carries this: it is the
    /// transport's dead-connection report, and the flow settles it the same
    /// way it settles a farewell.
    Timeout,
    /// The session cut the peer off for abuse. `reason` is the bounded label
    /// of the refusal that said so — `SessionViolation::label()` or
    /// `IntentRefusal::label()` — never a free-form message, so the reason a
    /// caller reports cannot grow with the traffic that triggered it.
    Abusive {
        /// The bounded refusal label.
        reason: &'static str,
    },
    /// The host is gone and the host-loss policy ended the match (spec
    /// non-negotiable 3: the match ends cleanly and clients return to menu).
    HostLoss,
}

impl DisconnectCause {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Voluntary => "voluntary",
            Self::Timeout => "timeout",
            Self::Abusive { .. } => "abusive",
            Self::HostLoss => "host_loss",
        }
    }

    /// Whether this cause is an abuse cut-off.
    #[must_use]
    pub const fn is_abuse(self) -> bool {
        matches!(self, Self::Abusive { .. })
    }

    /// The reason the client is told on the wire, when the wire can express
    /// it.
    ///
    /// `None` for [`DisconnectCause::Abusive`]: `message::DisconnectReason`
    /// is still `Voluntary | Timeout | SessionEnded` and has no abuse arm, so
    /// the peer is hung up without a reason packet rather than told a
    /// different one. Task #815 owns that arm; nothing here maps abuse onto
    /// `SessionEnded`.
    #[must_use]
    pub const fn wire_reason(self) -> Option<crate::message::DisconnectReason> {
        use crate::message::DisconnectReason;
        match self {
            Self::Voluntary => Some(DisconnectReason::Voluntary),
            Self::Timeout => Some(DisconnectReason::Timeout),
            Self::HostLoss => Some(DisconnectReason::SessionEnded),
            Self::Abusive { .. } => None,
        }
    }
}

impl fmt::Display for DisconnectCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Voluntary => write!(f, "the peer asked to leave"),
            Self::Timeout => write!(f, "the peer stopped responding"),
            Self::Abusive { reason } => write!(f, "cut off for abuse: {reason}"),
            Self::HostLoss => write!(f, "the host is gone"),
        }
    }
}

/// One recorded departure: the cause the **first** report named and the server
/// tick it was settled at.
///
/// The tick is what makes the settlement's own `EventId` stable: a later
/// report of the same peer can be told apart from the settlement it
/// duplicates without re-running it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepartureRecord {
    /// Why the peer left, as first reported.
    pub cause: DisconnectCause,
    /// The server tick the departure was first reported at.
    pub tick: Tick,
}

/// What reporting one peer's departure did: the once-only guard every
/// settlement runs behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settlement {
    /// The first report: the settlement applies now, at `tick`.
    Applied {
        /// The server tick this departure was first reported at.
        tick: Tick,
    },
    /// A repeat report — a retransmitted farewell, a second notice of the
    /// same dead link, an abuse cut-off the client already saw: the peer was
    /// settled at `tick` and **nothing** is applied again.
    Duplicate {
        /// The server tick the first report settled this departure at.
        tick: Tick,
    },
}

impl Settlement {
    /// Whether this report is the one that settles.
    #[must_use]
    pub const fn applied(self) -> bool {
        matches!(self, Self::Applied { .. })
    }

    /// The tick the departure was first reported at.
    #[must_use]
    pub const fn tick(self) -> Tick {
        match self {
            Self::Applied { tick } | Self::Duplicate { tick } => tick,
        }
    }
}

impl fmt::Display for Settlement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Applied { tick } => write!(f, "settled at tick {}", tick.0),
            Self::Duplicate { tick } => {
                write!(f, "already settled at tick {}; nothing applied", tick.0)
            }
        }
    }
}

/// The host's once-only record of departed peers.
///
/// This is the guard behind "the authoritative state resolves once"
/// (F58 acceptance AC03): a peer's departure is settled the first time it is
/// reported, and every later report of that peer is
/// [`Settlement::Duplicate`] and applies nothing — so a farewell that the
/// transport also reports as a dead link cannot settle the same state twice.
///
/// State is bounded by [`crate::bounds::MAX_SESSION_PEERS`], which is exactly
/// the number of peer ids [`crate::compat::PeerAllocator`] will ever issue in
/// one session (it never recycles), so the ledger cannot hold more records
/// than the session can have members, and a flood of departure notices for
/// unknown peers is refused by name instead of growing it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DepartureLedger {
    settled: BTreeMap<PeerId, DepartureRecord>,
}

/// Why a departure could not be recorded: the bounded-state refusal of the
/// once-only guard, separate from [`RecoveryError`] because it says nothing
/// about epochs or recovery decisions — only that this ledger will not grow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepartureError {
    /// The ledger already holds [`crate::bounds::MAX_SESSION_PEERS`] records
    /// and refuses to track another peer rather than grow.
    LedgerFull {
        /// [`crate::bounds::MAX_SESSION_PEERS`].
        max: usize,
    },
}

impl fmt::Display for DepartureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LedgerFull { max } => {
                write!(
                    f,
                    "the departure ledger already holds its maximum of {max} peers"
                )
            }
        }
    }
}

impl std::error::Error for DepartureError {}

impl DepartureLedger {
    /// An empty ledger: no departure has been settled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The recorded departure, when `peer` was already settled.
    #[must_use]
    pub fn get(&self, peer: PeerId) -> Option<DepartureRecord> {
        self.settled.get(&peer).copied()
    }

    /// Whether `peer` was already settled.
    #[must_use]
    pub fn is_settled(&self, peer: PeerId) -> bool {
        self.settled.contains_key(&peer)
    }

    /// Checks that `peer` *could* be recorded, without recording it.
    ///
    /// A caller that tears state down or settles something first runs this
    /// first, so a ledger that is full refuses while the session is still
    /// exactly as it was, instead of half-applied and unrecorded.
    ///
    /// # Errors
    ///
    /// [`DepartureError::LedgerFull`] when the ledger already holds
    /// [`crate::bounds::MAX_SESSION_PEERS`] records and `peer` is not one of
    /// them.
    pub fn check(&self, peer: PeerId) -> Result<(), DepartureError> {
        if self.settled.contains_key(&peer) {
            return Ok(());
        }
        if self.settled.len() >= MAX_SESSION_PEERS {
            return Err(DepartureError::LedgerFull {
                max: MAX_SESSION_PEERS,
            });
        }
        Ok(())
    }

    /// Records `peer`'s departure at `tick`.
    ///
    /// A peer that is already recorded returns
    /// [`Settlement::Duplicate`] with its original tick and changes nothing,
    /// so recording is idempotent for the caller as well.
    ///
    /// # Errors
    ///
    /// [`DepartureError::LedgerFull`] when the ledger already holds
    /// [`crate::bounds::MAX_SESSION_PEERS`] records and `peer` is not one of
    /// them. Nothing is inserted, so the map cannot grow past its bound
    /// either.
    pub fn record(
        &mut self,
        peer: PeerId,
        cause: DisconnectCause,
        tick: Tick,
    ) -> Result<Settlement, DepartureError> {
        self.check(peer)?;
        if let Some(record) = self.settled.get(&peer) {
            return Ok(Settlement::Duplicate { tick: record.tick });
        }
        self.settled.insert(peer, DepartureRecord { cause, tick });
        Ok(Settlement::Applied { tick })
    }

    /// How many departures have been settled. Never above
    /// [`crate::bounds::MAX_SESSION_PEERS`].
    #[must_use]
    pub fn len(&self) -> usize {
        self.settled.len()
    }

    /// Whether no departure has been settled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.settled.is_empty()
    }
}
