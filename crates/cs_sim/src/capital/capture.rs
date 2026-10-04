//! Capital-ship capture: a staged ownership transaction (F35-A), plus the
//! control state its completion switches (F35-C).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stages `### F35-A` and `### F35-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`, "Interaction transaction".
//!
//! Non-negotiable behavior 4: capture is a staged transaction, never a
//! single flag flip, and when it completes the ship's ownership changes at
//! once. The contract here is the [`CaptureStage`] vocabulary and the
//! [`CaptureTransaction`] state machine — approaching, eligible, latching,
//! transferring, completed, or aborted — plus the [`Ownership`] record it
//! produces. F35-C adds the two halves that make it a *transaction* rather
//! than a vocabulary: [`ShipControl`], the one record the guns, targeting,
//! docking eligibility and AI relation all read, and [`CaptureTicket`], the
//! generation-qualified handle a caller must present so a delayed callback
//! from a superseded attempt cannot commit.
//!
//! Every stage, the control switch and the ownership switch are newly
//! authored design; the original capture rules are unrecovered.

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureRefusal {
    /// The transaction already reached a terminal stage.
    AlreadyTerminal,
    /// The transaction was aborted and cannot resume.
    Aborted,
    /// A lethal subsystem destroyed the ship: nothing aboard a wreck can be
    /// captured, and an attempt in flight is torn down without transferring
    /// ownership.
    ShipDestroyed {
        /// The wreck.
        ship: ActorId,
    },
    /// The ship despawned, so its record is closed.
    ShipDespawned {
        /// The despawned ship.
        ship: ActorId,
    },
    /// Another transaction already holds this ship. Only one capture may hold
    /// a ship at a time, so a second claimant cannot race the first to a
    /// commit.
    InProgress {
        /// The contested ship.
        ship: ActorId,
        /// The claimant that holds it.
        claimant: ActorId,
        /// The stage the holding transaction reached.
        stage: CaptureStage,
    },
    /// The ticket is not the ship's current transaction — it belongs to an
    /// aborted or superseded attempt. A delayed callback from a dead attempt
    /// can never commit ownership (`STATE-TRANSACTIONS`: results and delayed
    /// callbacks are always generation-qualified).
    StaleTicket {
        /// The ship the ticket names.
        ship: ActorId,
        /// The attempt the presented ticket carries.
        ticket: u64,
        /// The attempt the ship actually holds.
        current: u64,
    },
    /// The ticket's session generation is not the ship's: a callback from a
    /// finished session is refused.
    ForeignSession {
        /// The ship the ticket names.
        ship: ActorId,
        /// The session the presented ticket carries.
        ticket: u64,
        /// The session the ship's current transaction belongs to.
        current: u64,
    },
    /// The ship's owner is no longer the one the transaction began against —
    /// another owner change landed first. The attempt is stale, not merely
    /// out of date, and commits nothing.
    OwnershipChanged {
        /// The contested ship.
        ship: ActorId,
        /// The owner the transaction expected to find.
        expected: ContentId,
        /// The owner the ship actually has.
        actual: ContentId,
    },
}

impl std::fmt::Display for CaptureRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyTerminal => write!(f, "the capture already reached a terminal stage"),
            Self::Aborted => write!(f, "the capture was aborted and cannot resume"),
            Self::ShipDestroyed { ship } => {
                write!(f, "ship {ship:?} is destroyed and cannot be captured")
            }
            Self::ShipDespawned { ship } => {
                write!(f, "ship {ship:?} despawned and cannot be captured")
            }
            Self::InProgress {
                ship,
                claimant,
                stage,
            } => write!(
                f,
                "ship {ship:?} is already being captured by {claimant:?} ({})",
                stage.label()
            ),
            Self::StaleTicket {
                ship,
                ticket,
                current,
            } => write!(
                f,
                "capture ticket {ticket} for ship {ship:?} is stale; attempt {current} holds it"
            ),
            Self::ForeignSession {
                ship,
                ticket,
                current,
            } => write!(
                f,
                "capture ticket for ship {ship:?} belongs to session {ticket}, not {current}"
            ),
            Self::OwnershipChanged {
                ship,
                expected,
                actual,
            } => write!(
                f,
                "ship {ship:?} is owned by {actual}, the capture expected {expected}"
            ),
        }
    }
}

impl std::error::Error for CaptureRefusal {}

/// A ship's ownership state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ownership {
    /// The faction that owns the ship.
    pub owner: ContentId,
    /// Whether the current owner took it by capture rather than starting
    /// with it.
    pub captured: bool,
}

/// Who a ship's systems act for (F35-C).
///
/// Non-negotiable behavior 4 requires guns, targeting, docking eligibility
/// and the AI relation to switch *coherently*: they must never read different
/// owners. This record is that single answer. It carries two owners because
/// the contract's "during latch a dedicated control owner is active" needs
/// them to differ for the two middle stages, and it carries the docking gate
/// so no consumer re-derives it:
///
/// * before any capture both owners are the ship's own;
/// * at [`CaptureStage::Latching`] and [`CaptureStage::Transferring`] the
///   control owner is the claimant's while the guns still serve the previous
///   owner — the ship's weapons have not been turned yet;
/// * at [`CaptureStage::Completed`] every owner is the new one, switched in
///   the same step as the [`Ownership`] record.
///
/// Whether two owners are hostile is deliberately *not* here: that relation
/// table is the mission's and F33 roster's data. This record guarantees the
/// four consumers read the same owner, not what the relation between two
/// owners is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipControl {
    /// The owner the ship's AI, targeting and control act for.
    pub control_owner: ContentId,
    /// The owner whose side the ship's guns serve.
    pub guns_owner: ContentId,
    /// Whether a boarding may still latch: false once the ship's docking
    /// anchor is destroyed or the ship itself is.
    pub docking_open: bool,
}

impl ShipControl {
    /// The control a freshly registered ship runs under: its own owner, its
    /// own guns and whatever its docking anchor allows.
    #[must_use]
    pub fn initial(owner: &ContentId, docking_open: bool) -> Self {
        Self {
            control_owner: owner.clone(),
            guns_owner: owner.clone(),
            docking_open,
        }
    }

    /// Moves the dedicated control owner in for the latch, leaving the guns
    /// with the previous owner.
    #[must_use]
    pub fn latched(mut self, claimant_owner: &ContentId) -> Self {
        self.control_owner = claimant_owner.clone();
        self
    }

    /// Completes the switch: guns, AI and targeting all serve `owner_after`.
    #[must_use]
    pub fn captured(mut self, owner_after: &ContentId) -> Self {
        self.control_owner = owner_after.clone();
        self.guns_owner = owner_after.clone();
        self
    }

    /// Closes the docking gate (a destroyed anchor, or a dead ship).
    #[must_use]
    pub fn with_docking(mut self, docking_open: bool) -> Self {
        self.docking_open = docking_open;
        self
    }

    /// Whether every consumer already reads `owner` — the invariant a
    /// completed capture must satisfy and an in-flight one must not.
    #[must_use]
    pub fn is_coherent_for(&self, owner: &ContentId) -> bool {
        &self.control_owner == owner && &self.guns_owner == owner
    }
}

/// The generation-qualified handle to one capture attempt (F35-C).
///
/// [`docs/contracts/STATE-TRANSACTIONS.md`](crate) requires that results and
/// delayed callbacks are always generation-qualified: a callback that fires
/// after its attempt was aborted, or after a retry began, must not commit
/// ownership. The ticket carries the attempt ordinal the set assigned when
/// the attempt began; the set refuses any ticket that is not the ship's
/// current attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureTicket {
    /// The ship being captured.
    pub ship: ActorId,
    /// The actor attempting the capture.
    pub claimant: ActorId,
    /// The session generation the attempt belongs to.
    pub session: u64,
    /// The ship's attempt ordinal, starting at 1 and never reused.
    pub attempt: u64,
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

    /// The session generation the transaction belongs to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The ship being captured.
    #[must_use]
    pub const fn ship(&self) -> ActorId {
        self.ship
    }

    /// The actor attempting the capture.
    #[must_use]
    pub const fn claimant(&self) -> ActorId {
        self.claimant
    }

    /// The owner the ship had when the transaction began.
    #[must_use]
    pub fn owner_before(&self) -> &ContentId {
        &self.owner_before
    }

    /// The owner the ship takes on completion.
    #[must_use]
    pub fn owner_after(&self) -> &ContentId {
        &self.owner_after
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
