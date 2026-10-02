//! Interaction identity and state vocabulary (F36-A).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`, "Interaction transaction".
//!
//! This module is the **typed vocabulary only**: the four interaction kinds
//! that share infrastructure but keep distinct eligibility and effects, the
//! semantic completion event each kind owes the mission, the explicit
//! [`InteractionState`] state machine, the stable [`InteractionId`] that
//! binds initiating actor, target, mission authorization and session, and the
//! [`InteractionAuthorization`] the active objective supplies.
//!
//! Every label, transition and completion event is newly authored project
//! design: the original game's docking, pickup, boarding and plane-swap rules
//! are unrecovered. See
//! `docs/findings/2026-10-01-f36-a-interaction-state-machines.md`.

use std::fmt;

use cs_types::content::ContentId;

use crate::damage::ActorId;

/// Which of the four interactions an [`InteractionId`] names.
///
/// Docking, passenger collection, boarding and changing the player's
/// aircraft share the state machine and the transfer policy but retain
/// distinct eligibility and effects (spec F36, "Deliverable and
/// interfaces"). Nothing here collapses them into one generic "enter".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InteractionKind {
    /// A flying actor attaches to a moving docking hook.
    Docking,
    /// A passenger or cargo item is collected from a pickup point.
    PassengerPickup,
    /// An actor boards a carrier through a hatch or bay.
    Boarding,
    /// The player changes aircraft mid-mission.
    AircraftSwap,
}

impl InteractionKind {
    /// Every kind, in a stable order.
    pub const ALL: &'static [InteractionKind] = &[
        Self::Docking,
        Self::PassengerPickup,
        Self::Boarding,
        Self::AircraftSwap,
    ];

    /// The stable label used in reports and catalog keys.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Docking => "docking",
            Self::PassengerPickup => "passenger_pickup",
            Self::Boarding => "boarding",
            Self::AircraftSwap => "aircraft_swap",
        }
    }

    /// The semantic completion event this kind owes the active objective.
    ///
    /// Non-negotiable behavior 3: "Do not complete a mission merely because
    /// any zeppelin was approached. The active objective specifies which
    /// interaction and semantic completion event is required." A completed
    /// transaction therefore publishes exactly one typed event, and the
    /// mission completes only when its objective names *this* event.
    #[must_use]
    pub const fn completion(self) -> InteractionCompletion {
        match self {
            Self::Docking => InteractionCompletion::Docked,
            Self::PassengerPickup => InteractionCompletion::PassengersDelivered,
            Self::Boarding => InteractionCompletion::Boarded,
            Self::AircraftSwap => InteractionCompletion::AircraftSwapped,
        }
    }

    /// Looks a kind up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.label() == label)
    }
}

impl fmt::Display for InteractionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The mission-facing completion event one interaction publishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InteractionCompletion {
    /// A docking completed.
    Docked,
    /// The required passengers or cargo were delivered.
    PassengersDelivered,
    /// A boarding completed.
    Boarded,
    /// The player is now in the destination aircraft.
    AircraftSwapped,
}

impl InteractionCompletion {
    /// Every completion event, in a stable order.
    pub const ALL: &'static [InteractionCompletion] = &[
        Self::Docked,
        Self::PassengersDelivered,
        Self::Boarded,
        Self::AircraftSwapped,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Docked => "docked",
            Self::PassengersDelivered => "passengers_delivered",
            Self::Boarded => "boarded",
            Self::AircraftSwapped => "aircraft_swapped",
        }
    }
}

impl fmt::Display for InteractionCompletion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The explicit stage of one interaction.
///
/// The canonical chain is
/// `Available -> Approaching -> Eligible -> Latching -> Transferring ->
/// Released -> Completed`; [`InteractionState::Aborted`] is reachable from any
/// non-terminal stage. A stage is never skipped (STATE-TRANSACTIONS: "It
/// progresses through approaching/eligible/latching/transferring/completed or
/// aborted").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InteractionState {
    /// The interaction exists but nothing has begun.
    Available,
    /// The initiator is closing on the target.
    Approaching,
    /// The initiator satisfies the swept eligibility envelope.
    Eligible,
    /// A dedicated control owner has taken pose/control.
    Latching,
    /// Pose, pilot, inventory and camera bindings are transferring.
    Transferring,
    /// The transfer is done and the actors are being released.
    Released,
    /// The transfer committed and the completion event was published.
    Completed,
    /// The attempt was abandoned; no effects were applied.
    Aborted,
}

impl InteractionState {
    /// Every stage, in canonical order.
    pub const ALL: &'static [InteractionState] = &[
        Self::Available,
        Self::Approaching,
        Self::Eligible,
        Self::Latching,
        Self::Transferring,
        Self::Released,
        Self::Completed,
        Self::Aborted,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Approaching => "approaching",
            Self::Eligible => "eligible",
            Self::Latching => "latching",
            Self::Transferring => "transferring",
            Self::Released => "released",
            Self::Completed => "completed",
            Self::Aborted => "aborted",
        }
    }

    /// Whether the stage is terminal: no further transition is possible.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Aborted)
    }

    /// Whether a dedicated control owner is active (non-negotiable behavior
    /// 2: "Exactly one system owns pose/control during latch and release").
    #[must_use]
    pub const fn control_is_dedicated(self) -> bool {
        matches!(self, Self::Latching | Self::Transferring)
    }

    /// The next stage on the canonical chain, or `None` at a terminal stage.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Available => Some(Self::Approaching),
            Self::Approaching => Some(Self::Eligible),
            Self::Eligible => Some(Self::Latching),
            Self::Latching => Some(Self::Transferring),
            Self::Transferring => Some(Self::Released),
            Self::Released => Some(Self::Completed),
            Self::Completed | Self::Aborted => None,
        }
    }
}

impl fmt::Display for InteractionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The stable identity of one interaction attempt.
///
/// It binds the initiating actor, the target, the session generation and a
/// per-session serial, so a result, a delayed callback or a duplicated event
/// can never be attributed to the wrong attempt. No serial is reused inside a
/// session (STATE-TRANSACTIONS: "No actor identifier is reused within a
/// session").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InteractionId {
    /// The session generation the attempt belongs to.
    pub session: u64,
    /// The attempt's serial within that session.
    pub serial: u64,
    /// The actor that initiated the interaction.
    pub initiator: ActorId,
    /// The actor (or anchor's owner) the interaction targets.
    pub target: ActorId,
}

impl InteractionId {
    /// Binds an actor, a target, a session and a serial into one identity.
    #[must_use]
    pub const fn new(session: u64, serial: u64, initiator: ActorId, target: ActorId) -> Self {
        Self {
            session,
            serial,
            initiator,
            target,
        }
    }

    /// Whether both actors belong to this interaction's session generation.
    #[must_use]
    pub const fn is_same_generation(&self) -> bool {
        self.initiator.session.get() == self.session && self.target.session.get() == self.session
    }
}

impl fmt::Display for InteractionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "interaction {}:{} ({} -> {})",
            self.session, self.serial, self.initiator, self.target
        )
    }
}

/// The mission authorization one interaction runs under.
///
/// The active objective names the interaction kind and the semantic
/// completion event it requires (non-negotiable behavior 3), so an
/// interaction without a matching authorization can never progress to latch
/// or complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionAuthorization {
    /// The session generation the authorization was issued in.
    pub session: u64,
    /// The kind of interaction the objective authorizes.
    pub kind: InteractionKind,
    /// The objective that authorizes it.
    pub objective: ContentId,
}

impl InteractionAuthorization {
    /// Issues an authorization for one objective and kind.
    #[must_use]
    pub const fn new(session: u64, kind: InteractionKind, objective: ContentId) -> Self {
        Self {
            session,
            kind,
            objective,
        }
    }

    /// The completion event this authorization requires, from its kind.
    #[must_use]
    pub const fn completion(&self) -> InteractionCompletion {
        self.kind.completion()
    }

    /// Whether this authorization still authorizes `kind` against the
    /// currently active `objective`.
    ///
    /// Completion re-runs this check (STATE-TRANSACTIONS: "Completion
    /// validates that both actors and the mission phase still authorize the
    /// operation"), so a mission phase change mid-transfer aborts instead of
    /// completing.
    #[must_use]
    pub fn authorizes(&self, kind: InteractionKind, objective: &ContentId) -> bool {
        self.kind == kind && &self.objective == objective
    }
}

impl fmt::Display for InteractionAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} authorization for {} in session {}",
            self.kind, self.objective, self.session
        )
    }
}
