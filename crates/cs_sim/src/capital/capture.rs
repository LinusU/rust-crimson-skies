//! Capital-ship capture: a staged ownership transaction (F35-A).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`, "Interaction transaction".
//!
//! Non-negotiable behavior 4: capture is a staged transaction, never a
//! single flag flip, and when it completes the ship's ownership changes at
//! once. The contract here is the [`CaptureStage`] vocabulary and the
//! [`CaptureTransaction`] state machine — approaching, eligible, latching,
//! transferring, completed, or aborted — plus the [`Ownership`] record it
//! produces. Which guns, targeting relations and docking checks switch is
//! the F35-C wiring; this stage fixes the identity and the ordering those
//! systems read.
//!
//! Every stage and the ownership switch are newly authored design; the
//! original capture rules are unrecovered.

use cs_script::ir::ActorId;
use cs_types::content::ContentId;

/// The stage of one capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CaptureStage {
    /// The claimant is closing on the ship.
    Approaching,
    /// The claimant satisfies the capture preconditions.
    Eligible,
    /// A dedicated control owner is active.
    Latching,
    /// Ownership is being transferred.
    Transferring,
    /// The transfer committed.
    Completed,
    /// The attempt was abandoned.
    Aborted,
}

impl CaptureStage {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Approaching => "approaching",
            Self::Eligible => "eligible",
            Self::Latching => "latching",
            Self::Transferring => "transferring",
            Self::Completed => "completed",
            Self::Aborted => "aborted",
        }
    }

    /// Whether the stage is terminal.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Aborted)
    }
}

/// Why a capture could not advance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureRefusal {
    /// The transaction already reached a terminal stage.
    AlreadyTerminal,
    /// The transaction was aborted and cannot resume.
    Aborted,
}

/// A ship's ownership state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ownership {
    /// The faction that owns the ship.
    pub owner: ContentId,
    /// Whether the current owner took it by capture rather than starting
    /// with it.
    pub captured: bool,
}

/// One staged ownership change. Ids are opaque; nothing about a ship or a
/// claimant says who may capture it — that authorization is the mission's
/// and is supplied by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureTransaction {
    /// The session this transaction belongs to.
    pub session: u64,
    /// The ship being captured.
    pub ship: ActorId,
    /// The actor attempting the capture.
    pub claimant: ActorId,
    /// The ship's owner before the capture.
    pub owner_before: ContentId,
    /// The owner that takes the ship on completion.
    pub owner_after: ContentId,
    stage: CaptureStage,
}

impl CaptureTransaction {
    /// Begins a capture at [`CaptureStage::Approaching`].
    #[must_use]
    pub fn begin(
        session: u64,
        ship: ActorId,
        claimant: ActorId,
        owner_before: ContentId,
        owner_after: ContentId,
    ) -> Self {
        Self {
            session,
            ship,
            claimant,
            owner_before,
            owner_after,
            stage: CaptureStage::Approaching,
        }
    }

    /// The current stage.
    #[must_use]
    pub fn stage(&self) -> CaptureStage {
        self.stage
    }

    /// Advances one stage. `Approaching -> Eligible -> Latching ->
    /// Transferring -> Completed`; a terminal transaction refuses.
    ///
    /// # Errors
    ///
    /// [`CaptureRefusal::AlreadyTerminal`] or [`CaptureRefusal::Aborted`].
    pub fn advance(&mut self) -> Result<CaptureStage, CaptureRefusal> {
        self.stage = match self.stage {
            CaptureStage::Approaching => CaptureStage::Eligible,
            CaptureStage::Eligible => CaptureStage::Latching,
            CaptureStage::Latching => CaptureStage::Transferring,
            CaptureStage::Transferring => CaptureStage::Completed,
            CaptureStage::Completed => return Err(CaptureRefusal::AlreadyTerminal),
            CaptureStage::Aborted => return Err(CaptureRefusal::Aborted),
        };
        Ok(self.stage)
    }

    /// Aborts the capture. A completed or already aborted transaction
    /// refuses.
    ///
    /// # Errors
    ///
    /// [`CaptureRefusal::AlreadyTerminal`] or [`CaptureRefusal::Aborted`].
    pub fn abort(&mut self) -> Result<CaptureStage, CaptureRefusal> {
        match self.stage {
            CaptureStage::Completed => Err(CaptureRefusal::AlreadyTerminal),
            CaptureStage::Aborted => Err(CaptureRefusal::Aborted),
            _ => {
                self.stage = CaptureStage::Aborted;
                Ok(self.stage)
            }
        }
    }

    /// The ownership the ship takes once the transaction completes, or
    /// `None` while it is unfinished.
    #[must_use]
    pub fn ownership(&self) -> Option<Ownership> {
        if self.stage == CaptureStage::Completed {
            Some(Ownership {
                owner: self.owner_after.clone(),
                captured: true,
            })
        } else {
            None
        }
    }
}
