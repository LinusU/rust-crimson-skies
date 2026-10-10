//! Host-side session admission and recovery boundary (F58-A).
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-A`. Contract: `docs/contracts/UI-NETWORK.md`.
//!
//! [`cs_net::validation`] owns the threat model and the pure session identity
//! rules; [`cs_net::recovery`] owns the reconnect, binding and reward-replay
//! rules. This module is the *app boundary* between them and the running
//! server: it is where a decoded [`ClientMessage`] becomes either a refusal or
//! the typed [`FireRequest`]s that weapon acceptance (F27) consumes.
//!
//! # The receive path
//!
//! [`SessionReceiver::receive`] is the production path. It runs
//! [`cs_net::validation::SessionGate::admit`] first, so a packet from an
//! unauthenticated peer, a dead session epoch, an oversized payload or a
//! replayed sequence produces no fire request at all. Only an admitted input
//! packet is scanned for fire edges, and only the aircraft the server bound to
//! that peer is named ([`FireRequest::actor`]).
//!
//! That is what makes F58 AC01's minimum scenario — "replay a prior-session
//! fire packet and prove no projectile spawns" — hold at this boundary: the
//! stale packet yields a [`cs_net::validation::Admission::Refused`] with
//! [`cs_net::validation::ThreatDisposition::Absorb`] and an empty fire list, so
//! the resolver is never asked to fire. `cs_sim::weapons::FireResolver`
//! independently refuses an intent whose session is not its own, so the
//! property holds at the resolver too.
//!
//! F58-A defines the typed boundary and nothing more; F58-B extends it with
//! intent validation and rate/resource caps.
//!
//! # F58-C: the disconnect, reconnect and clean host-loss flow
//!
//! [`RecoveryFlow`] is that flow at this boundary. Its producer is the
//! receive path: [`RecoveryFlow::departure_cause`] turns one decoded packet
//! into the [`cs_net::recovery::DisconnectCause`] it asks for — the client's
//! own farewell, or a refusal whose declared disposition is
//! [`cs_net::validation::ThreatDisposition::Disconnect`]. Its consumers are
//! the boundary itself and the authoritative state a departing peer held:
//! [`RecoveryFlow::depart`] tears the peer down, drops what it still carried
//! in the authoritative match and records the departure, so a second report
//! of the same peer applies nothing ([`cs_net::recovery::Settlement`]).
//! [`RecoveryFlow::host_lost`] ends the match cleanly once — clients return
//! to the menu, no migration is claimed — and [`RecoveryFlow::recover`] runs
//! the F58-A decision and reopens the boundary on the fresh epoch.
//!
//! **Not wired here:** `cs_net::lifecycle`'s `ServerSession::pump` still
//! admits packets straight through the transport's own gate and never builds
//! a [`MatchStage`], so the rate caps and the tick window are not yet charged
//! on the wire path (F58-B finding 2). That pump is not an F58-C owner path;
//! the gap is recorded in `docs/findings/2026-10-10-f58-c-disconnect-reconnect-and-host-loss.md`
//! and filed as a follow-up task rather than reached into from here.
//!
//! # The intent path (F58-B)
//!
//! [`SessionReceiver::receive_validated`] is the same packet path plus the
//! intent layer: an admitted input packet becomes a
//! [`cs_net::validation::ClientIntent`] and is judged by
//! [`cs_net::validation::IntentValidator`] against the host's
//! [`cs_net::validation::MatchStage`] — server-owned truth, server-owned
//! actors, loadouts, the rate budget, the tick window and the match phase.
//! A refused intent authorizes nothing: the fire list comes back empty and
//! the bounded [`cs_net::validation::IntentRefusal`] says whether to absorb
//! the peer or disconnect it.
//!
//! [`SessionReceiver::validate_intent`] is that judge for one ask at a time,
//! which is where "the client requests damage/score directly and the server
//! rejects it" is answered: both asks name a server-owned domain and are
//! refused before any state moves.
//!
//! No original networked behavior is asserted; every cap and window here is
//! newly authored engine design.

use std::fmt;

use cs_net::compat::PeerAllocator;
use cs_net::message::{ClientMessage, ClientPayload};
use cs_net::recovery::{
    DepartureError, DepartureLedger, DepartureRecord, DisconnectCause, RecoveryDecision,
    RecoveryPolicy, RecoveryRequest, SessionGenerations, Settlement,
};
use cs_net::validation::{
    Admission, ClientIntent, FireRequest, IntentRefusal, IntentValidator, IntentVerdict,
    MatchStage, RateBudget, SessionGate, SessionViolation, ThreatDisposition, fire_requests,
};
use cs_sim::interaction::{AbortReason, InteractionAbort, InteractionSession};
use cs_sim::multiplayer::objective::{
    ObjectiveAction, ObjectiveEvent, ObjectiveId, ObjectiveState,
};
use cs_sim::multiplayer::result::Submitted;
use cs_sim::multiplayer::session::{MatchSession, SessionError};
use cs_sim::weapons::{FireIntent, FireIntentId};
use cs_types::Tick;
use cs_types::net::{ActorId, EventId, PeerId, SessionId};

/// The host app's live receive boundary for one session epoch.
///
/// It owns the [`SessionGate`] (epoch, peer membership, replay windows and the
/// peer-to-actor ownership table) and exposes the app-facing result of one
/// decoded packet. It holds no game state itself: the server's authoritative
/// match state lives in `cs_sim` and the match-level reconnect tables
/// ([`cs_net::recovery::PilotBindings`], [`cs_net::recovery::RewardLedger`])
/// live beside it, because they survive an epoch change while the gate does
/// not.
#[derive(Clone, Debug)]
pub struct SessionReceiver {
    gate: SessionGate,
    validator: IntentValidator,
}

impl SessionReceiver {
    /// A receiver for the live session `session`, at the designed rate limits.
    #[must_use]
    pub fn new(session: SessionId) -> Self {
        Self {
            gate: SessionGate::new(session),
            validator: IntentValidator::designed(),
        }
    }

    /// The live session epoch.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.gate.identity().session()
    }

    /// The underlying session gate.
    #[must_use]
    pub fn gate(&self) -> &SessionGate {
        &self.gate
    }

    /// The underlying session gate, mutably (for the ownership table).
    pub fn gate_mut(&mut self) -> &mut SessionGate {
        &mut self.gate
    }

    /// Admits `peer` to the live session; idempotent.
    pub fn admit_peer(&mut self, peer: PeerId) {
        self.gate.admit_peer(peer);
    }

    /// Forgets a departed peer and its replay window.
    pub fn forget_peer(&mut self, peer: PeerId) {
        self.gate.forget_peer(peer);
    }

    /// Binds an aircraft to the peer that flies it.
    ///
    /// # Errors
    ///
    /// [`SessionViolation::ActorNotOwned`] when another peer owns it.
    pub fn bind_actor(&mut self, peer: PeerId, actor: ActorId) -> Result<(), SessionViolation> {
        self.gate.ownership_mut().bind(peer, actor)
    }

    /// The aircraft `peer` flies, if any.
    #[must_use]
    pub fn actor_of(&self, peer: PeerId) -> Option<ActorId> {
        self.gate.ownership().actor_of(peer)
    }

    /// Releases an aircraft binding, returning its former owner.
    ///
    /// This is the teardown half of [`Self::bind_actor`]: a departed peer's
    /// aircraft goes back to the server's table so a respawn or a reconnect
    /// reclaims it through the match's own `PilotBindings`, never through the
    /// client's word.
    pub fn release_actor(&mut self, actor: ActorId) -> Option<PeerId> {
        self.gate.ownership_mut().release(actor)
    }

    /// Receives one decoded client packet and returns what the server may do
    /// with it.
    pub fn receive(&mut self, peer: PeerId, message: &ClientMessage) -> Inbound {
        let admission = self.gate.admit(peer, message);
        let fires = match (&admission, &message.payload) {
            (Admission::Accepted { sequence }, ClientPayload::Input(batch)) => {
                match self.gate.ownership().actor_of(peer) {
                    Some(actor) => fire_requests(
                        batch,
                        self.gate.identity().session(),
                        peer,
                        actor,
                        *sequence,
                    ),
                    // The peer has no aircraft yet (join before spawn, or a
                    // late join awaiting its binding): nothing to fire for.
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        Inbound { admission, fires }
    }

    /// Reopens the receiver on a fresh epoch, discarding the old gate.
    ///
    /// This is the session-identity half of a reconnect: the old epoch's
    /// replay windows and peer membership die with it, and every packet from
    /// the prior connection becomes stale. Match-level state that must survive
    /// (aircraft bindings, awarded rewards) is *not* here; it is
    /// [`cs_net::recovery::PilotBindings`] and
    /// [`cs_net::recovery::RewardLedger`], and the caller rebinds the returning
    /// pilot through them.
    pub fn reopen(&mut self, session: SessionId) {
        self.gate = SessionGate::new(session);
        // A fresh epoch starts every rate window over as well: the new
        // connection's peers are new identities with no charges behind them.
        self.validator.reopen();
    }

    /// The intent validator (its rate budget is what [`Self::budget`] reads).
    #[must_use]
    pub fn intent_validator(&self) -> &IntentValidator {
        &self.validator
    }

    /// The live rate budget.
    #[must_use]
    pub fn budget(&self) -> &RateBudget {
        self.validator.budget()
    }

    /// Judges one ask from `peer` against the server-owned facts of `stage`.
    ///
    /// This is the F58 minimum acceptance scenario: a client asking for
    /// damage or score directly is refused with
    /// [`IntentRefusal::ClientAuthoredTruth`] before anything else runs, and
    /// no state moves either way.
    ///
    /// # Errors
    ///
    /// The [`IntentRefusal`] naming the check that refused it. The function
    /// applies nothing; it only says what the server may do.
    pub fn validate_intent(
        &mut self,
        peer: PeerId,
        intent: &ClientIntent,
        stage: MatchStage<'_>,
    ) -> Result<(), IntentRefusal> {
        self.validator
            .validate(peer, intent, self.gate.ownership(), stage)
    }

    /// The packet path plus the intent layer: admit the packet, then judge the
    /// ask it carries.
    ///
    /// A packet the identity gate refuses, or one that is not input, yields no
    /// verdict ([`ValidatedInbound::verdict`] is `None`) and is handled exactly
    /// as [`Self::receive`] handles it. An admitted input packet yields a
    /// [`ClientIntent::Input`] naming the aircraft this peer owns and the
    /// packet's newest frame tick; when the validator refuses that intent the
    /// fire list is emptied, so a refused ask authorizes no request at all.
    #[must_use]
    pub fn receive_validated(
        &mut self,
        peer: PeerId,
        message: &ClientMessage,
        stage: MatchStage<'_>,
    ) -> ValidatedInbound {
        let inbound = self.receive(peer, message);
        if !inbound.accepted() || !matches!(message.payload, ClientPayload::Input(_)) {
            return ValidatedInbound {
                inbound,
                verdict: None,
            };
        }
        let tick = match &message.payload {
            ClientPayload::Input(batch) => batch.frames.last().map(|frame| frame.frame_tick()),
            ClientPayload::Leave => None,
        };
        let Some(tick) = tick else {
            return ValidatedInbound {
                inbound,
                verdict: None,
            };
        };
        let intent = ClientIntent::Input {
            actor: self.gate.ownership().actor_of(peer),
            tick,
        };
        match self
            .validator
            .validate(peer, &intent, self.gate.ownership(), stage)
        {
            Ok(()) => ValidatedInbound {
                inbound,
                verdict: Some(IntentVerdict::Accepted(intent)),
            },
            Err(reason) => ValidatedInbound {
                inbound: Inbound {
                    admission: inbound.admission,
                    fires: Vec::new(),
                },
                verdict: Some(IntentVerdict::Refused { intent, reason }),
            },
        }
    }
}

/// What one received packet produced: the admission verdict and the fire
/// requests it authorizes (empty for every refusal).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inbound {
    /// The gate's verdict.
    pub admission: Admission,
    /// The peer's admitted fire requests, oldest frame first. Empty unless the
    /// packet was admitted input for a bound aircraft.
    pub fires: Vec<FireRequest>,
}

impl Inbound {
    /// Whether the packet was admitted.
    #[must_use]
    pub const fn accepted(&self) -> bool {
        self.admission.accepted()
    }

    /// The refusal, when there was one.
    #[must_use]
    pub const fn violation(&self) -> Option<&SessionViolation> {
        self.admission.violation()
    }

    /// The fire requests this packet authorized.
    #[must_use]
    pub fn fires(&self) -> &[FireRequest] {
        &self.fires
    }
}

/// One packet through the identity gate **and** the intent layer (F58-B).
///
/// `verdict` is `None` when there was no intent to judge — the packet was
/// refused by the gate, or it was not input. When it is
/// [`Some(IntentVerdict::Refused { .. })]`, `inbound.fires` is empty: a
/// refused ask authorizes no request, so nothing downstream can act on it.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedInbound {
    /// The identity-gate result.
    pub inbound: Inbound,
    /// The intent verdict, when the packet carried one.
    pub verdict: Option<IntentVerdict>,
}

impl ValidatedInbound {
    /// Whether the identity gate admitted the packet.
    #[must_use]
    pub const fn admitted(&self) -> bool {
        self.inbound.accepted()
    }

    /// The intent verdict, when the packet carried one.
    #[must_use]
    pub const fn verdict(&self) -> Option<&IntentVerdict> {
        match &self.verdict {
            Some(verdict) => Some(verdict),
            None => None,
        }
    }

    /// The intent refusal, when the validator refused the ask.
    #[must_use]
    pub const fn refusal(&self) -> Option<&IntentRefusal> {
        match &self.verdict {
            Some(IntentVerdict::Refused { reason, .. }) => Some(reason),
            _ => None,
        }
    }

    /// The fire requests this packet authorized; always empty when the
    /// validator refused the ask.
    #[must_use]
    pub fn fires(&self) -> &[FireRequest] {
        self.inbound.fires()
    }
}

/// The F27 fire intent an authorized [`FireRequest`] asks the authoritative
/// resolver to resolve.
///
/// The intent's `producer` is the peer number (a `u16`, so it never truncates
/// and is unique per session); its `sequence` is the admitting packet's
/// sequence, and the tick comes from the input frame. A `FireRequest` from a
/// dead epoch never reaches here, and if one did the resolver would refuse it
/// as a foreign session before touching any projectile.
#[must_use]
pub fn fire_intent(request: &FireRequest) -> FireIntent {
    FireIntent {
        id: FireIntentId {
            session: request.session.get(),
            tick: request.tick,
            producer: u32::from(request.peer.get()),
            sequence: request.sequence,
        },
        shooter: cs_sim::damage::ActorId {
            session: request.session,
            serial: request.actor.serial,
        },
    }
}

// ---------------------------------------------------------------------------
// The disconnect, reconnect and clean host-loss flow (F58-C)
// ---------------------------------------------------------------------------

/// The `EventId::producer` serial a departure settlement is stamped with.
///
/// The producer orders events inside one tick, so the host's own settlement
/// takes the top of that space: it can never collide with a peer-produced
/// event of the same tick, and it is judged after that tick's own events — a
/// claim that arrives on the closing tick still wins or loses on its own
/// merits before the departing holder's drop is applied.
const DEPARTURE_PRODUCER: u32 = u32::MAX;

/// Why the flow could not settle what it was asked to settle.
///
/// Nothing here is swallowed: each variant names the layer that refused, and
/// which layer it was decides whether anything moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowError {
    /// The departure ledger will not track another peer
    /// ([`DepartureError::LedgerFull`]). The bound is checked before
    /// anything is torn down or settled, so this refusal leaves the session
    /// exactly as it was.
    Ledger(DepartureError),
    /// The authoritative match refused the settlement: the match is over, the
    /// peer is not in the roster, or the tick the caller named is already
    /// closed. A refused event changes nothing in the match and the departure
    /// is **not** recorded, so a corrected report (a later tick, say) can
    /// still settle it.
    Settlement(SessionError),
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ledger(error) => write!(f, "the departure could not be recorded: {error}"),
            Self::Settlement(error) => write!(f, "the departure could not be settled: {error}"),
        }
    }
}

impl std::error::Error for FlowError {}

impl From<DepartureError> for FlowError {
    fn from(error: DepartureError) -> Self {
        Self::Ledger(error)
    }
}

impl From<SessionError> for FlowError {
    fn from(error: SessionError) -> Self {
        Self::Settlement(error)
    }
}

/// What the host must do after a clean host loss (spec F58 non-negotiable 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostLossAction {
    /// The match ended without a migration: every client leaves the match and
    /// returns to the menu (`UI-NETWORK`'s `match -> results -> lobby` path).
    /// No seamless recovery is claimed, because none is implemented.
    ReturnToMenu,
}

/// The result of settling one peer's departure: the teardown, what this
/// report settled, and what it refused to settle a second time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Departure {
    /// The peer this report is about.
    pub peer: PeerId,
    /// The cause the settlement used. On a duplicate report this is the cause
    /// the **first** report named, which is the one that settled.
    pub cause: DisconnectCause,
    /// The aircraft this peer flew at the time of this report; `None` once the
    /// binding was released (which every first report does).
    pub actor: Option<ActorId>,
    /// Whether this report settled the departure, or a previous one had.
    pub settlement: Settlement,
    /// The objectives the peer was still carrying, queued to drop on this
    /// tick. Always empty on a duplicate report.
    pub dropped: Vec<ObjectiveId>,
}

/// The result of the host-loss flow.
#[derive(Clone, Debug, PartialEq)]
pub struct HostLossOutcome {
    /// Whether this report ended the match, or a previous one had.
    pub settlement: Settlement,
    /// The mission's interactions that aborted on this report. Always empty
    /// on a duplicate report: an aborted interaction is terminal, so the
    /// authoritative state resolves once.
    pub aborted: Vec<InteractionAbort>,
    /// What the UI owes every client now.
    pub action: HostLossAction,
}

/// One packet through the boundary and the departure it asked for, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct SettledInbound {
    /// The packet's identity-gate and intent result.
    pub inbound: ValidatedInbound,
    /// The settled departure, when the packet asked for one.
    pub departure: Option<Departure>,
}

/// The host's disconnect, reconnect and clean host-loss flow (F58-C).
///
/// It wires the pieces F58-A and F58-B defined onto their producers and
/// consumers:
///
/// * **producer** — the receive path. [`RecoveryFlow::departure_cause`] turns
///   one decoded packet into the [`DisconnectCause`] it asks for: the client's
///   own farewell, or a refusal whose declared disposition is
///   [`ThreatDisposition::Disconnect`]. [`RecoveryFlow::receive_and_depart`]
///   runs the packet and the settlement in one step.
/// * **consumer** — the boundary and the authoritative state.
///   [`RecoveryFlow::depart`] tears the peer down (aircraft binding,
///   membership, replay window), settles what it still held (the objectives
///   it carried are queued to drop) and records the departure, so a second
///   report of the same peer is [`Settlement::Duplicate`] and applies
///   nothing.
/// * **teardown / retry** — [`RecoveryFlow::host_lost`] ends the match
///   cleanly once and aborts the mission's in-flight interactions;
///   [`RecoveryFlow::recover`] runs the F58-A decision and, on a resume,
///   reopens the boundary on the fresh epoch and admits the new peer id.
///
/// The flow holds no roster: membership belongs to the lobby, so a host
/// reports **each** member's departure through [`RecoveryFlow::depart`], and
/// the match-level tables that survive an epoch (`PilotBindings`,
/// `RewardLedger`) stay with the match, exactly as `SessionReceiver` documents.
#[derive(Clone, Debug)]
pub struct RecoveryFlow {
    receiver: SessionReceiver,
    departures: DepartureLedger,
    ended: Option<DepartureRecord>,
}

impl RecoveryFlow {
    /// A flow for the live session `session`.
    #[must_use]
    pub fn new(session: SessionId) -> Self {
        Self {
            receiver: SessionReceiver::new(session),
            departures: DepartureLedger::new(),
            ended: None,
        }
    }

    /// The live session epoch.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.receiver.session()
    }

    /// The receive boundary this flow drives.
    #[must_use]
    pub fn receiver(&self) -> &SessionReceiver {
        &self.receiver
    }

    /// The receive boundary, mutably (admitting members, binding aircraft).
    pub fn receiver_mut(&mut self) -> &mut SessionReceiver {
        &mut self.receiver
    }

    /// The once-only record of departed peers.
    #[must_use]
    pub fn departures(&self) -> &DepartureLedger {
        &self.departures
    }

    /// Whether the host-loss flow has ended the match.
    #[must_use]
    pub fn is_ended(&self) -> bool {
        self.ended.is_some()
    }

    /// The departure one received packet asks for, if any — the producer side
    /// of [`Self::depart`].
    ///
    /// Three producers, in order:
    ///
    /// 1. an **admitted** [`ClientPayload::Leave`] is the client's own
    ///    farewell ([`DisconnectCause::Voluntary`]); a farewell the gate
    ///    refused (a stale epoch) is not a departure, it is a stale packet;
    /// 2. an intent refusal whose declared disposition is
    ///    [`ThreatDisposition::Disconnect`] — forged truth, foreign
    ///    ownership, a blown rate or resource budget — cuts the peer off and
    ///    carries that refusal's own bounded label;
    /// 3. a gate refusal the declared dispositions cut off (an unauthenticated
    ///    sender, an oversized packet) does the same.
    ///
    /// A transport-reported dead link has no packet and therefore no producer
    /// here: the caller reports it directly as [`DisconnectCause::Timeout`].
    #[must_use]
    pub fn departure_cause(
        message: &ClientMessage,
        inbound: &ValidatedInbound,
    ) -> Option<DisconnectCause> {
        if matches!(message.payload, ClientPayload::Leave) && inbound.admitted() {
            return Some(DisconnectCause::Voluntary);
        }
        if let Some(reason) = inbound.refusal()
            && reason.disposition() == ThreatDisposition::Disconnect
        {
            return Some(DisconnectCause::Abusive {
                reason: reason.label(),
            });
        }
        if let Some(violation) = inbound.inbound.violation()
            && violation.disposition() == ThreatDisposition::Disconnect
        {
            return Some(DisconnectCause::Abusive {
                reason: violation.label(),
            });
        }
        None
    }

    /// One packet through the boundary: it is judged against `stage` and,
    /// when the packet asks for this peer to leave, the departure is settled
    /// in the same step.
    ///
    /// The settlement's stamp is [`MatchStage::server_tick`] — the server's
    /// own clock, never anything the client sent. `match_state` is the
    /// authoritative match whose carried objectives settle with the peer;
    /// pass `None` for a mode that holds none.
    ///
    /// # Errors
    ///
    /// [`FlowError`] from the settlement, which carries no packet result: on
    /// the error path [`SettledInbound`] is not returned. Nothing is lost
    /// with it — only a packet that asks for a departure can fail here, and
    /// such a packet authorizes no work of its own (a farewell is not input,
    /// a refused intent has already emptied its fire list, and a packet the
    /// gate refused never had one), so only the admission verdict is dropped.
    pub fn receive_and_depart(
        &mut self,
        peer: PeerId,
        message: &ClientMessage,
        stage: MatchStage<'_>,
        match_state: Option<&mut MatchSession>,
    ) -> Result<SettledInbound, FlowError> {
        let inbound = self.receiver.receive_validated(peer, message, stage);
        let departure = match Self::departure_cause(message, &inbound) {
            Some(cause) => Some(self.depart(peer, cause, stage.server_tick, match_state)?),
            None => None,
        };
        Ok(SettledInbound { inbound, departure })
    }

    /// Settles one peer's departure: the teardown at the boundary plus the
    /// authoritative state the peer still held — **once**.
    ///
    /// The order is deliberate:
    ///
    /// 1. a peer already recorded returns [`Settlement::Duplicate`] and moves
    ///    nothing at all, not even the teardown, so a farewell that the
    ///    transport also reports as a dead link cannot settle twice;
    /// 2. the ledger's bound is paid before any state moves, so a full ledger
    ///    refuses ([`FlowError::Ledger`]) while the session is unchanged;
    /// 3. the teardown releases the aircraft binding, then forgets the peer's
    ///    membership and replay window (peer ids are never recycled, so the
    ///    window must not outlive the peer);
    /// 4. every objective the peer still carries is queued to drop on `tick`,
    ///    for the board to apply when the tick closes; a refusal there is
    ///    propagated as [`FlowError::Settlement`] and **nothing is recorded**,
    ///    so a corrected report can still settle it;
    /// 5. only then is the departure recorded, which is what makes the next
    ///    report of this peer a duplicate.
    ///
    /// `tick` is the server's own tick — the one currently being simulated. A
    /// tick the board already closed is refused rather than silently
    /// retimed.
    ///
    /// # Errors
    ///
    /// [`FlowError`]; see the order above for which state, if any, moved.
    pub fn depart(
        &mut self,
        peer: PeerId,
        cause: DisconnectCause,
        tick: Tick,
        match_state: Option<&mut MatchSession>,
    ) -> Result<Departure, FlowError> {
        if let Some(record) = self.departures.get(peer) {
            return Ok(Departure {
                peer,
                cause: record.cause,
                actor: self.receiver.actor_of(peer),
                settlement: Settlement::Duplicate { tick: record.tick },
                dropped: Vec::new(),
            });
        }
        // The ledger's bound is paid before anything is torn down or
        // settled, so a ledger that is full refuses while the session is
        // unchanged.
        self.departures.check(peer).map_err(FlowError::Ledger)?;

        let actor = self.receiver.actor_of(peer);
        if let Some(actor) = actor {
            self.receiver.release_actor(actor);
        }
        self.receiver.forget_peer(peer);

        let mut dropped = Vec::new();
        if let Some(state) = match_state {
            let session = state.session();
            for objective in state.objective_ids().collect::<Vec<_>>() {
                if state.objective_state(objective) != Some(ObjectiveState::Held(peer)) {
                    continue;
                }
                let event = ObjectiveEvent {
                    id: EventId {
                        session,
                        tick,
                        producer: DEPARTURE_PRODUCER,
                        sequence: objective.get(),
                    },
                    objective,
                    action: ObjectiveAction::Drop { holder: peer },
                };
                match state.submit_objective(event) {
                    // Queued for this tick's adjudication: the board applies
                    // it once, when the tick closes.
                    Ok(Submitted::Accepted) => dropped.push(objective),
                    // The board has already seen this exact settlement.
                    Ok(Submitted::Duplicate) => {}
                    Err(error) => return Err(FlowError::Settlement(error)),
                }
            }
        }

        let settlement = self
            .departures
            .record(peer, cause, tick)
            .map_err(FlowError::Ledger)?;
        Ok(Departure {
            peer,
            cause,
            actor,
            settlement,
            dropped,
        })
    }

    /// The host is gone: the match ends cleanly and every client returns to
    /// the menu — **once** (spec F58 non-negotiable 3: host migration is out
    /// of scope, so the match ends; no seamless recovery is claimed).
    ///
    /// The mission's in-flight interactions (docking, pickup, boarding, plane
    /// swap) abort with [`AbortReason::Disconnect`] on the first report. A
    /// later report is [`Settlement::Duplicate`] and aborts nothing — an
    /// aborted interaction is terminal, so the authoritative state resolves
    /// exactly once. Pass `None` for a mode that holds no mission
    /// interactions (a multiplayer match).
    ///
    /// Each member's own teardown is still its own [`Self::depart`] report:
    /// the flow holds no roster.
    #[must_use]
    pub fn host_lost(
        &mut self,
        tick: Tick,
        interactions: Option<&mut InteractionSession>,
    ) -> HostLossOutcome {
        if let Some(record) = self.ended {
            return HostLossOutcome {
                settlement: Settlement::Duplicate { tick: record.tick },
                aborted: Vec::new(),
                action: HostLossAction::ReturnToMenu,
            };
        }
        let aborted = match interactions {
            Some(session) => session.abort_all(AbortReason::Disconnect),
            None => Vec::new(),
        };
        self.ended = Some(DepartureRecord {
            cause: DisconnectCause::HostLoss,
            tick,
        });
        HostLossOutcome {
            settlement: Settlement::Applied { tick },
            aborted,
            action: HostLossAction::ReturnToMenu,
        }
    }

    /// A returning client asks to resume: the F58-A decision, wired to its
    /// retry.
    ///
    /// `peers` is the match's one [`PeerAllocator`]: peer ids are never
    /// recycled inside a session, so a returning pilot is admitted under a
    /// freshly allocated identity and its old id stays settled in the
    /// departure ledger for good.
    ///
    /// [`RecoveryPolicy::match_running`] is forced false once
    /// [`Self::host_lost`] has ended the match, so a resume after a host loss
    /// is refused with `RecoveryRefusal::MatchEnded` instead of resumed into
    /// a match that is over. On a [`RecoveryDecision::Resume`] the boundary
    /// reopens on the fresh epoch — the old gate's membership and replay
    /// windows die with it, so every packet from the prior connection is
    /// stale — and the freshly allocated peer id is admitted. The client's
    /// claims are refused exactly as `decide_recovery` refuses them, and
    /// reclaiming the aircraft stays with the match's `PilotBindings`, never
    /// with the client's word.
    pub fn recover(
        &mut self,
        request: RecoveryRequest,
        policy: &RecoveryPolicy,
        generations: &mut SessionGenerations,
        peers: &mut PeerAllocator,
    ) -> RecoveryDecision {
        let effective = RecoveryPolicy {
            match_running: policy.match_running && !self.is_ended(),
            ..*policy
        };
        let decision = cs_net::recovery::decide_recovery(&effective, generations, peers, &request);
        if let RecoveryDecision::Resume {
            session: fresh,
            peer: allocated,
            ..
        } = &decision
        {
            self.receiver.reopen(*fresh);
            self.receiver.admit_peer(*allocated);
        }
        decision
    }
}
