//! The interaction transaction and per-transition transfer policy (F36-A).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`, "Interaction transaction".
//!
//! [`InteractionTransaction`] is the explicit state machine a docking, a
//! passenger pickup, a boarding or an aircraft swap runs: it only reaches
//! [`InteractionState::Latching`] when the swept eligibility check passed, it
//! exposes exactly one [`ControlOwner`] at every stage, it validates
//! authorization and target liveness at completion, and it aborts cleanly
//! (producing **no** transfer effects) for a destroyed target, a cancelled
//! cinematic, a pause, a retry or a disconnect (non-negotiable behavior 4).
//!
//! The per-transition [`TransferPolicy`] declares what moves — velocity, the
//! pilot, inventory and the camera binding — so an aircraft swap is atomic
//! and never leaves two actors controlled or two pilots alive.
//!
//! All behavior here is designed; the original transition set is unrecovered.

use std::fmt;

use cs_types::content::ContentId;

use super::eligibility::EligibilityRefusal;
use super::state::{
    InteractionAuthorization, InteractionCompletion, InteractionId, InteractionKind,
    InteractionState,
};

/// How the arriving actor's velocity is resolved by a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VelocityTransfer {
    /// The arriving actor takes the target's velocity (a hard dock).
    MatchTarget,
    /// The arriving actor keeps its own velocity.
    PreserveInitiator,
}

impl VelocityTransfer {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::MatchTarget => "match_target",
            Self::PreserveInitiator => "preserve_initiator",
        }
    }
}

impl fmt::Display for VelocityTransfer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Where the pilot identity ends up after a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PilotTransfer {
    /// The initiating pilot moves to the target aircraft. The old actor no
    /// longer has a pilot, so a retry cannot leave a duplicate.
    MoveToTarget,
    /// No pilot identity changes.
    None,
}

impl PilotTransfer {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::MoveToTarget => "move_to_target",
            Self::None => "none",
        }
    }
}

impl fmt::Display for PilotTransfer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Where inventory (passengers, cargo) ends up after a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InventoryTransfer {
    /// The inventory moves to the target exactly once.
    MoveToTarget,
    /// No inventory moves.
    None,
}

impl InventoryTransfer {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::MoveToTarget => "move_to_target",
            Self::None => "none",
        }
    }
}

impl fmt::Display for InventoryTransfer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which actor the camera binding follows after a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CameraTransfer {
    /// The camera follows the target after the transfer.
    FollowTarget,
    /// The camera keeps following the initiator.
    FollowInitiator,
}

impl CameraTransfer {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::FollowTarget => "follow_target",
            Self::FollowInitiator => "follow_initiator",
        }
    }
}

impl fmt::Display for CameraTransfer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The one system that owns pose and control.
///
/// Non-negotiable behavior 2: during latch and release exactly one owner is
/// active. [`ControlOwner::LatchController`] is that dedicated owner; the
/// transaction never reports two owners at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlOwner {
    /// The initiating actor keeps control.
    Initiator,
    /// The target actor owns control.
    Target,
    /// A dedicated latch controller owns both actors until release.
    LatchController,
}

impl ControlOwner {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Initiator => "initiator",
            Self::Target => "target",
            Self::LatchController => "latch_controller",
        }
    }
}

impl fmt::Display for ControlOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The declared policy of one interaction's transfer.
///
/// It is per-transition, not global: a docking matches the target's velocity
/// and moves no pilot, an aircraft swap moves the pilot and camera to the new
/// actor. Nothing here is inferred from the interaction kind at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransferPolicy {
    /// How velocity is resolved.
    pub velocity: VelocityTransfer,
    /// Where the pilot identity goes.
    pub pilot: PilotTransfer,
    /// Where inventory goes.
    pub inventory: InventoryTransfer,
    /// Where the camera binding goes.
    pub camera: CameraTransfer,
    /// Who owns pose/control after release.
    pub control_after_release: ControlOwner,
}

impl TransferPolicy {
    /// The docking default: match velocity, move no pilot or inventory, the
    /// target owns control after release.
    #[must_use]
    pub const fn docking() -> Self {
        Self {
            velocity: VelocityTransfer::MatchTarget,
            pilot: PilotTransfer::None,
            inventory: InventoryTransfer::None,
            camera: CameraTransfer::FollowInitiator,
            control_after_release: ControlOwner::Target,
        }
    }

    /// The aircraft-swap policy: move the pilot and camera to the new actor,
    /// and the target owns control.
    #[must_use]
    pub const fn aircraft_swap() -> Self {
        Self {
            velocity: VelocityTransfer::PreserveInitiator,
            pilot: PilotTransfer::MoveToTarget,
            inventory: InventoryTransfer::MoveToTarget,
            camera: CameraTransfer::FollowTarget,
            control_after_release: ControlOwner::Target,
        }
    }
}

/// Why the transaction refused a requested transition.
#[derive(Clone, Debug, PartialEq)]
pub enum InteractionRefusal {
    /// The transaction was in a different stage than the transition requires.
    WrongState {
        /// The stage the transition requires.
        expected: InteractionState,
        /// The stage the transaction was in.
        found: InteractionState,
    },
    /// The transaction already completed or aborted.
    AlreadyTerminal {
        /// The terminal stage.
        state: InteractionState,
    },
    /// The transaction was aborted and cannot resume.
    Aborted,
    /// The objective that authorized the interaction is no longer active.
    NotAuthorized {
        /// The objective the authorization named.
        objective: ContentId,
    },
    /// The target was destroyed; the transaction aborted with no effects.
    TargetLost,
}

impl fmt::Display for InteractionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongState { expected, found } => write!(
                f,
                "transition requires stage {expected}, but the transaction is {found}"
            ),
            Self::AlreadyTerminal { state } => {
                write!(f, "the interaction is already terminal ({state})")
            }
            Self::Aborted => write!(f, "the interaction was aborted and cannot resume"),
            Self::NotAuthorized { objective } => write!(
                f,
                "objective {objective} no longer authorizes this interaction"
            ),
            Self::TargetLost => write!(f, "the target was destroyed before completion"),
        }
    }
}

impl std::error::Error for InteractionRefusal {}

/// Why a latch was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum LatchRefusal {
    /// The transaction was not yet eligible.
    NotReady {
        /// The stage it was in.
        state: InteractionState,
    },
    /// The swept eligibility check did not pass; the transaction stays
    /// eligible and does not latch.
    NotEligible(EligibilityRefusal),
}

impl fmt::Display for LatchRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotReady { state } => {
                write!(f, "the interaction is {state}, not eligible to latch")
            }
            Self::NotEligible(source) => write!(f, "the interaction cannot latch: {source}"),
        }
    }
}

impl std::error::Error for LatchRefusal {}

/// The effects one completed transfer applied, derived from its policy.
///
/// It exists only for a completed transaction: an aborted one produces no
/// effects, which is how "without duplicate pilots or cargo" is enforced in
/// code rather than by convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransferEffects {
    /// Who owns control once the transfer is done.
    pub control_owner: ControlOwner,
    /// How velocity was resolved.
    pub velocity: VelocityTransfer,
    /// Whether the pilot moved.
    pub pilot_moved: bool,
    /// Whether inventory moved.
    pub inventory_moved: bool,
    /// Where the camera binding went.
    pub camera: CameraTransfer,
}

/// The published result of one completed interaction.
#[derive(Clone, Debug, PartialEq)]
pub struct InteractionOutcome {
    /// The interaction that completed.
    pub id: InteractionId,
    /// Its kind.
    pub kind: InteractionKind,
    /// The semantic completion event the mission receives.
    pub completion: InteractionCompletion,
    /// The effects that were applied.
    pub effects: TransferEffects,
}

/// The record of one aborted interaction. It carries no effects.
#[derive(Clone, Debug, PartialEq)]
pub struct InteractionAbort {
    /// The interaction that aborted.
    pub id: InteractionId,
    /// The stage it aborted from.
    pub from: InteractionState,
    /// Why it aborted.
    pub reason: AbortReason,
}

/// Why an interaction aborted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbortReason {
    /// The target actor was destroyed.
    TargetDestroyed,
    /// A cinematic was cancelled.
    CinematicCancelled,
    /// The session was paused.
    Pause,
    /// The mission retried.
    Retry,
    /// The peer disconnected.
    Disconnect,
    /// The interaction was cancelled for another authored reason.
    Cancelled,
}

impl AbortReason {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::TargetDestroyed => "target_destroyed",
            Self::CinematicCancelled => "cinematic_cancelled",
            Self::Pause => "pause",
            Self::Retry => "retry",
            Self::Disconnect => "disconnect",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for AbortReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One explicit interaction transaction.
///
/// It opens at [`InteractionState::Available`] and only its own transitions
/// can move it; there is no public field a caller could write behind its
/// back.
#[derive(Clone, Debug, PartialEq)]
pub struct InteractionTransaction {
    id: InteractionId,
    kind: InteractionKind,
    authorization: InteractionAuthorization,
    policy: TransferPolicy,
    state: InteractionState,
}

impl InteractionTransaction {
    /// Opens a transaction at [`InteractionState::Available`].
    #[must_use]
    pub fn begin(
        id: InteractionId,
        kind: InteractionKind,
        authorization: InteractionAuthorization,
        policy: TransferPolicy,
    ) -> Self {
        Self {
            id,
            kind,
            authorization,
            policy,
            state: InteractionState::Available,
        }
    }

    /// The stable identity.
    #[must_use]
    pub const fn id(&self) -> InteractionId {
        self.id
    }

    /// The interaction kind.
    #[must_use]
    pub const fn kind(&self) -> InteractionKind {
        self.kind
    }

    /// The mission authorization.
    #[must_use]
    pub const fn authorization(&self) -> &InteractionAuthorization {
        &self.authorization
    }

    /// The declared transfer policy.
    #[must_use]
    pub const fn policy(&self) -> &TransferPolicy {
        &self.policy
    }

    /// The current stage.
    #[must_use]
    pub const fn state(&self) -> InteractionState {
        self.state
    }

    /// The one control owner active at the current stage.
    ///
    /// Before latch the initiating actor owns control, during latch and
    /// transfer the dedicated [`ControlOwner::LatchController`] owns it, and
    /// from release onward the policy's declared owner decides. Exactly one is
    /// returned at every stage.
    #[must_use]
    pub const fn control_owner(&self) -> ControlOwner {
        if self.state.control_is_dedicated() {
            ControlOwner::LatchController
        } else if matches!(
            self.state,
            InteractionState::Released | InteractionState::Completed
        ) {
            self.policy.control_after_release
        } else {
            ControlOwner::Initiator
        }
    }

    /// Advances one stage on the canonical chain.
    ///
    /// # Errors
    ///
    /// [`InteractionRefusal::AlreadyTerminal`] or
    /// [`InteractionRefusal::Aborted`] from a terminal stage.
    pub fn advance(&mut self) -> Result<InteractionState, InteractionRefusal> {
        let Some(next) = self.state.next() else {
            return Err(match self.state {
                InteractionState::Aborted => InteractionRefusal::Aborted,
                other => InteractionRefusal::AlreadyTerminal { state: other },
            });
        };
        self.state = next;
        Ok(self.state)
    }

    /// Attempts to latch, consuming a swept eligibility result.
    ///
    /// The transaction must already be [`InteractionState::Eligible`]. A
    /// refused eligibility check leaves the stage untouched — this is the
    /// F36-A minimum scenario: a too-fast or wrong-direction pass does not
    /// latch.
    ///
    /// # Errors
    ///
    /// [`LatchRefusal::NotReady`] when the stage is not eligible, or
    /// [`LatchRefusal::NotEligible`] carrying the eligibility refusal.
    pub fn latch(
        &mut self,
        eligibility: Result<crate::world_actors::anchor::AnchorSample, EligibilityRefusal>,
    ) -> Result<crate::world_actors::anchor::AnchorSample, LatchRefusal> {
        if self.state != InteractionState::Eligible {
            return Err(LatchRefusal::NotReady { state: self.state });
        }
        let sample = eligibility.map_err(LatchRefusal::NotEligible)?;
        self.state = InteractionState::Latching;
        Ok(sample)
    }

    /// Completes the transfer after release, validating that the mission
    /// phase still authorizes it and the target is still alive.
    ///
    /// A changed phase or a destroyed target aborts the transaction and
    /// returns the reason, so it never publishes a completion event, a
    /// duplicate pilot or duplicate cargo.
    ///
    /// # Errors
    ///
    /// [`InteractionRefusal`] naming the wrong stage, an aborted transaction,
    /// a lost authorization or a destroyed target.
    pub fn complete(
        &mut self,
        active_objective: &ContentId,
        target_alive: bool,
    ) -> Result<InteractionOutcome, InteractionRefusal> {
        if self.state == InteractionState::Aborted {
            return Err(InteractionRefusal::Aborted);
        }
        if self.state == InteractionState::Completed {
            return Err(InteractionRefusal::AlreadyTerminal {
                state: InteractionState::Completed,
            });
        }
        if self.state != InteractionState::Released {
            return Err(InteractionRefusal::WrongState {
                expected: InteractionState::Released,
                found: self.state,
            });
        }
        if !self.authorization.authorizes(self.kind, active_objective) {
            self.state = InteractionState::Aborted;
            return Err(InteractionRefusal::NotAuthorized {
                objective: self.authorization.objective.clone(),
            });
        }
        if !target_alive {
            self.state = InteractionState::Aborted;
            return Err(InteractionRefusal::TargetLost);
        }
        self.state = InteractionState::Completed;
        Ok(InteractionOutcome {
            id: self.id,
            kind: self.kind,
            completion: self.kind.completion(),
            effects: self.effects().expect("a completed transfer has effects"),
        })
    }

    /// Aborts the interaction, applying no effects.
    ///
    /// # Errors
    ///
    /// [`InteractionRefusal::AlreadyTerminal`] from a completed transaction or
    /// [`InteractionRefusal::Aborted`] from an already aborted one.
    pub fn abort(&mut self, reason: AbortReason) -> Result<InteractionAbort, InteractionRefusal> {
        match self.state {
            InteractionState::Completed => Err(InteractionRefusal::AlreadyTerminal {
                state: InteractionState::Completed,
            }),
            InteractionState::Aborted => Err(InteractionRefusal::Aborted),
            from => {
                self.state = InteractionState::Aborted;
                Ok(InteractionAbort {
                    id: self.id,
                    from,
                    reason,
                })
            }
        }
    }

    /// The effects a completed transfer applied, or `None` while the
    /// transaction has not completed.
    #[must_use]
    pub fn effects(&self) -> Option<TransferEffects> {
        if self.state != InteractionState::Completed {
            return None;
        }
        Some(TransferEffects {
            control_owner: self.policy.control_after_release,
            velocity: self.policy.velocity,
            pilot_moved: self.policy.pilot == PilotTransfer::MoveToTarget,
            inventory_moved: self.policy.inventory == InventoryTransfer::MoveToTarget,
            camera: self.policy.camera,
        })
    }
}
