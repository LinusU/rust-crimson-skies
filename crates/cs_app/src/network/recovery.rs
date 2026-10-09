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
//! intent validation and rate/resource caps, and F58-C wires it to the pinned
//! transport.
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

use cs_net::message::{ClientMessage, ClientPayload};
use cs_net::validation::{
    Admission, ClientIntent, FireRequest, IntentRefusal, IntentValidator, IntentVerdict,
    MatchStage, RateBudget, SessionGate, SessionViolation, fire_requests,
};
use cs_sim::weapons::{FireIntent, FireIntentId};
use cs_types::net::{ActorId, PeerId, SessionId};

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
