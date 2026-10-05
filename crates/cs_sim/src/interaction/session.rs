//! The interaction session driver (F36-C).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`, "Interaction transaction".
//!
//! [`InteractionSession`] is the one owner that connects the per-attempt
//! [`InteractionTransaction`] (F36-A) to the [`TransferLedger`] (F36-B) and to
//! the world events that end an attempt early. It adds what neither piece can
//! do alone:
//!
//! * the transaction and the ledger commit together: a completion the ledger
//!   refuses aborts the transaction instead of publishing a completion event
//!   whose effects never happened;
//! * a destroyed carrier (or initiator) aborts every active attempt that
//!   involves it, and [`InteractionSession::control_of`] then names the
//!   initiator as the pose/control owner again, so control never stays with a
//!   latch controller whose target is gone;
//! * pause, cinematic cancel, disconnect and retry abort cleanly, and a retry
//!   restores the ledger's initial bindings under the new session generation.
//!
//! All behavior is designed; the original lifecycle rules are unrecovered.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::content::ContentId;

use super::eligibility::EligibilityRefusal;
use super::state::{InteractionId, InteractionState};
use super::transaction::{
    AbortReason, ControlOwner, InteractionAbort, InteractionOutcome, InteractionRefusal,
    InteractionTransaction,
};
use super::transfer::{TransferLedger, TransferRefusal, TransferReport};
use crate::damage::ActorId;
use crate::world_actors::anchor::AnchorSample;

/// Who holds pose/control of an initiator's aircraft right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlHolder {
    /// The dedicated latch controller of this interaction.
    Latch(InteractionId),
    /// The actor itself.
    Actor(ActorId),
}

/// Why the session refused a request. A refused request changes nothing,
/// except where its own documentation says the attempt aborted.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionRefusal {
    /// The interaction belongs to a different session generation.
    StaleSession {
        /// The session's generation.
        session: u64,
        /// The interaction's generation.
        interaction: u64,
    },
    /// An interaction with this identity was already opened.
    Duplicate(InteractionId),
    /// The initiator already has an active interaction.
    InitiatorBusy {
        /// The initiator.
        initiator: ActorId,
        /// The interaction that holds it.
        active: InteractionId,
    },
    /// No such interaction.
    Unknown(InteractionId),
    /// The transaction refused the transition (it may have aborted).
    Transaction(InteractionRefusal),
    /// The ledger refused the transfer; the interaction aborted.
    Transfer(TransferRefusal),
}

impl fmt::Display for SessionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleSession {
                session,
                interaction,
            } => write!(
                f,
                "interaction session {interaction} does not match session {session}"
            ),
            Self::Duplicate(id) => write!(f, "{id} was already opened"),
            Self::InitiatorBusy { initiator, active } => {
                write!(f, "actor {initiator} is already in {active}")
            }
            Self::Unknown(id) => write!(f, "{id} is not known to this session"),
            Self::Transaction(source) => write!(f, "{source}"),
            Self::Transfer(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for SessionRefusal {}

/// The session's interaction attempts and the bindings they transfer.
#[derive(Clone, Debug, PartialEq)]
pub struct InteractionSession {
    ledger: TransferLedger,
    transactions: BTreeMap<InteractionId, InteractionTransaction>,
}

impl InteractionSession {
    /// A session over `ledger`, with no interaction opened.
    #[must_use]
    pub fn new(ledger: TransferLedger) -> Self {
        Self {
            ledger,
            transactions: BTreeMap::new(),
        }
    }

    /// The pilot and inventory bindings.
    #[must_use]
    pub const fn ledger(&self) -> &TransferLedger {
        &self.ledger
    }

    /// The session generation.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.ledger.session()
    }

    /// The stage of an interaction, if it is known.
    #[must_use]
    pub fn state(&self, id: InteractionId) -> Option<InteractionState> {
        self.transactions
            .get(&id)
            .map(InteractionTransaction::state)
    }

    /// The interaction that currently involves `actor` and is not terminal.
    #[must_use]
    pub fn active_for(&self, actor: ActorId) -> Option<InteractionId> {
        self.transactions
            .iter()
            .find(|(id, tx)| {
                !tx.state().is_terminal() && (id.initiator == actor || id.target == actor)
            })
            .map(|(id, _)| *id)
    }

    /// Opens a transaction.
    ///
    /// # Errors
    ///
    /// [`SessionRefusal`] for a stale generation, a duplicate identity or an
    /// initiator that already has an active interaction.
    pub fn open(&mut self, transaction: InteractionTransaction) -> Result<(), SessionRefusal> {
        let id = transaction.id();
        if id.session != self.session() {
            return Err(SessionRefusal::StaleSession {
                session: self.session(),
                interaction: id.session,
            });
        }
        if self.transactions.contains_key(&id) {
            return Err(SessionRefusal::Duplicate(id));
        }
        if let Some(active) = self.active_for(id.initiator) {
            return Err(SessionRefusal::InitiatorBusy {
                initiator: id.initiator,
                active,
            });
        }
        self.transactions.insert(id, transaction);
        Ok(())
    }

    /// Feeds one tick's swept eligibility result to an interaction.
    ///
    /// An unstarted interaction starts approaching; an approaching one becomes
    /// eligible and latches only on an eligible result. A refused result
    /// leaves the interaction approaching and is returned, so a too-fast pass
    /// never latches.
    ///
    /// # Errors
    ///
    /// [`SessionRefusal`] for an unknown or terminal interaction.
    pub fn observe(
        &mut self,
        id: InteractionId,
        eligibility: Result<AnchorSample, EligibilityRefusal>,
    ) -> Result<Result<InteractionState, EligibilityRefusal>, SessionRefusal> {
        let tx = self
            .transactions
            .get_mut(&id)
            .ok_or(SessionRefusal::Unknown(id))?;
        if tx.state() == InteractionState::Available {
            tx.advance().map_err(SessionRefusal::Transaction)?;
        }
        if tx.state() == InteractionState::Approaching {
            if let Err(refusal) = eligibility {
                return Ok(Err(refusal));
            }
            tx.advance().map_err(SessionRefusal::Transaction)?;
        }
        match tx.latch(eligibility) {
            Ok(_) => Ok(Ok(tx.state())),
            Err(super::transaction::LatchRefusal::NotEligible(refusal)) => Ok(Err(refusal)),
            Err(super::transaction::LatchRefusal::NotReady { state }) => Err(
                SessionRefusal::Transaction(InteractionRefusal::WrongState {
                    expected: InteractionState::Eligible,
                    found: state,
                }),
            ),
        }
    }

    /// Moves a latched interaction through transfer to release.
    ///
    /// # Errors
    ///
    /// [`SessionRefusal`] for an unknown interaction or one that is not
    /// latching.
    pub fn release(&mut self, id: InteractionId) -> Result<InteractionState, SessionRefusal> {
        let tx = self
            .transactions
            .get_mut(&id)
            .ok_or(SessionRefusal::Unknown(id))?;
        if tx.state() != InteractionState::Latching {
            return Err(SessionRefusal::Transaction(
                InteractionRefusal::WrongState {
                    expected: InteractionState::Latching,
                    found: tx.state(),
                },
            ));
        }
        tx.advance().map_err(SessionRefusal::Transaction)?;
        tx.advance().map_err(SessionRefusal::Transaction)
    }

    /// Completes a released interaction and applies its transfer in one step.
    ///
    /// The transaction and the ledger commit together: if the transaction
    /// refuses (lost authorization, destroyed target) it has aborted; if the
    /// ledger refuses, the interaction is aborted here. Either way no
    /// completion event is returned and no binding changed.
    ///
    /// # Errors
    ///
    /// [`SessionRefusal`].
    pub fn complete(
        &mut self,
        id: InteractionId,
        active_objective: &ContentId,
        target_alive: bool,
        initiator_velocity_m_s: [f64; 3],
        target_velocity_m_s: [f64; 3],
    ) -> Result<(InteractionOutcome, TransferReport), SessionRefusal> {
        let tx = self
            .transactions
            .get_mut(&id)
            .ok_or(SessionRefusal::Unknown(id))?;
        let mut trial = tx.clone();
        let outcome = match trial.complete(active_objective, target_alive) {
            Ok(outcome) => outcome,
            Err(refusal) => {
                *tx = trial;
                return Err(SessionRefusal::Transaction(refusal));
            }
        };
        match self
            .ledger
            .apply(&outcome, initiator_velocity_m_s, target_velocity_m_s)
        {
            Ok(report) => {
                *tx = trial;
                Ok((outcome, report))
            }
            Err(refusal) => {
                // The trial reached Completed; the real transaction is still
                // Released and aborts so it cannot be completed again.
                tx.abort(AbortReason::Cancelled)
                    .map_err(SessionRefusal::Transaction)?;
                Err(SessionRefusal::Transfer(refusal))
            }
        }
    }

    /// Aborts every active interaction that involves `actor`, because it was
    /// destroyed. Control of each surviving initiator returns to the
    /// initiator and no binding changes.
    pub fn actor_destroyed(&mut self, actor: ActorId) -> Vec<InteractionAbort> {
        self.abort_where(AbortReason::TargetDestroyed, |id| {
            id.initiator == actor || id.target == actor
        })
    }

    /// Aborts every active interaction (pause, cinematic cancel, disconnect).
    pub fn abort_all(&mut self, reason: AbortReason) -> Vec<InteractionAbort> {
        self.abort_where(reason, |_| true)
    }

    /// Retries the mission: aborts every active interaction, drops all
    /// attempts and restores the ledger's initial bindings under `session`.
    pub fn retry(&mut self, session: u64) -> Vec<InteractionAbort> {
        let aborts = self.abort_all(AbortReason::Retry);
        self.transactions.clear();
        self.ledger.retry(session);
        aborts
    }

    /// Who holds pose/control of `initiator`'s aircraft: the latch controller
    /// of its active interaction, otherwise its transaction policy's owner
    /// after a completed transfer, otherwise the actor itself.
    #[must_use]
    pub fn control_of(&self, initiator: ActorId) -> ControlHolder {
        for (id, tx) in &self.transactions {
            if id.initiator != initiator {
                continue;
            }
            match (tx.state(), tx.control_owner()) {
                (InteractionState::Latching | InteractionState::Transferring, _) => {
                    return ControlHolder::Latch(*id);
                }
                (InteractionState::Completed, ControlOwner::Target) => {
                    return ControlHolder::Actor(id.target);
                }
                _ => {}
            }
        }
        ControlHolder::Actor(initiator)
    }

    fn abort_where(
        &mut self,
        reason: AbortReason,
        matches: impl Fn(&InteractionId) -> bool,
    ) -> Vec<InteractionAbort> {
        self.transactions
            .iter_mut()
            .filter(|(id, tx)| !tx.state().is_terminal() && matches(id))
            .filter_map(|(_, tx)| tx.abort(reason).ok())
            .collect()
    }
}
