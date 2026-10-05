//! Moving-frame eligibility input and the atomic transfer ledger (F36-B).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-B`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`, "Interaction transaction".
//!
//! Two production paths live here:
//!
//! * [`InitiatorMotion`] derives the initiator's velocity from its **f64 world
//!   positions** at the two ends of a swept segment, never from f32 local
//!   coordinates. A world-origin rebase changes every local coordinate but no
//!   world position, so the velocity — and therefore the relative speed that
//!   [`evaluate_motion_eligibility`] compares against the envelope — cannot
//!   pick up a false jump from the rebase.
//! * [`TransferLedger`] applies a completed [`InteractionOutcome`] to the
//!   pilot and inventory bindings in one atomic step: every precondition is
//!   checked before any binding changes, so a refused transfer changes
//!   nothing and an applied one cannot be applied twice (no duplicate pilot
//!   or cargo, non-negotiable behavior 4).
//!
//! All behavior is designed; the original transfer rules are unrecovered.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::Tick;

use super::eligibility::{EligibilityEnvelope, EligibilityRefusal, evaluate_eligibility};
use super::state::InteractionId;
use super::transaction::{CameraTransfer, ControlOwner, InteractionOutcome, VelocityTransfer};
use crate::damage::ActorId;
use crate::world_actors::anchor::{AnchorSample, AnchorSocket};
use crate::world_actors::trajectory::Trajectory;

/// Why an [`InitiatorMotion`] could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionError {
    /// A world position was NaN or infinite.
    NonFinite,
    /// The segment spans zero ticks, so no velocity exists.
    ZeroTicks,
    /// The tick rate was zero.
    ZeroTickRate,
}

impl fmt::Display for MotionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => write!(f, "a world position was non-finite"),
            Self::ZeroTicks => write!(f, "the motion segment spans zero ticks"),
            Self::ZeroTickRate => write!(f, "the tick rate must be greater than zero"),
        }
    }
}

impl std::error::Error for MotionError {}

/// The initiator's motion over one swept segment, in canonical f64 world
/// coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InitiatorMotion {
    from_world_m: [f64; 3],
    to_world_m: [f64; 3],
    ticks: u64,
    ticks_per_second: u32,
}

impl InitiatorMotion {
    /// Builds a motion from the world positions at both ends of the segment.
    ///
    /// # Errors
    ///
    /// [`MotionError`] for a non-finite position, zero elapsed ticks or a zero
    /// tick rate.
    pub fn try_new(
        from_world_m: [f64; 3],
        to_world_m: [f64; 3],
        ticks: u64,
        ticks_per_second: u32,
    ) -> Result<Self, MotionError> {
        if !from_world_m
            .iter()
            .chain(&to_world_m)
            .all(|v| v.is_finite())
        {
            return Err(MotionError::NonFinite);
        }
        if ticks == 0 {
            return Err(MotionError::ZeroTicks);
        }
        if ticks_per_second == 0 {
            return Err(MotionError::ZeroTickRate);
        }
        Ok(Self {
            from_world_m,
            to_world_m,
            ticks,
            ticks_per_second,
        })
    }

    /// The world position at the end of the segment.
    #[must_use]
    pub const fn position_m(&self) -> [f64; 3] {
        self.to_world_m
    }

    /// The segment's duration in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.ticks as f64 / f64::from(self.ticks_per_second)
    }

    /// The world-frame velocity over the segment, in m/s.
    #[must_use]
    pub fn velocity_m_s(&self) -> [f64; 3] {
        let seconds = self.seconds();
        [
            (self.to_world_m[0] - self.from_world_m[0]) / seconds,
            (self.to_world_m[1] - self.from_world_m[1]) / seconds,
            (self.to_world_m[2] - self.from_world_m[2]) / seconds,
        ]
    }
}

/// Evaluates an initiator's world-frame motion against a moving anchor.
///
/// The velocity is derived from [`InitiatorMotion`], and the sweep interval is
/// the segment's own duration, so the swept closest approach covers exactly
/// the path the initiator flew since the previous tick.
///
/// # Errors
///
/// [`EligibilityRefusal`] from [`evaluate_eligibility`].
pub fn evaluate_motion_eligibility(
    tick: Tick,
    target_trajectory: &Trajectory,
    anchor: &AnchorSocket,
    motion: &InitiatorMotion,
    envelope: &EligibilityEnvelope,
) -> Result<AnchorSample, EligibilityRefusal> {
    evaluate_eligibility(
        tick,
        target_trajectory,
        anchor,
        motion.position_m(),
        motion.velocity_m_s(),
        motion.seconds(),
        envelope,
    )
}

/// A pilot identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PilotId(pub u64);

/// Why a transfer was refused. A refused transfer changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferRefusal {
    /// The interaction belongs to a different session generation.
    StaleSession {
        /// The ledger's session.
        ledger: u64,
        /// The interaction's session.
        interaction: u64,
    },
    /// This interaction's transfer was already applied.
    AlreadyApplied(InteractionId),
    /// The pilot was to move but the initiator has none.
    NoPilot(ActorId),
    /// The pilot was to move onto a target that already has a pilot.
    TargetOccupied {
        /// The target actor.
        target: ActorId,
        /// Its current pilot.
        pilot: PilotId,
    },
    /// The inventory count would overflow.
    InventoryOverflow(ActorId),
}

impl fmt::Display for TransferRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleSession {
                ledger,
                interaction,
            } => write!(
                f,
                "interaction session {interaction} does not match ledger session {ledger}"
            ),
            Self::AlreadyApplied(id) => write!(f, "interaction {id} was already applied"),
            Self::NoPilot(actor) => write!(f, "actor {actor:?} has no pilot to move"),
            Self::TargetOccupied { target, pilot } => {
                write!(f, "target {target:?} already has pilot {pilot:?}")
            }
            Self::InventoryOverflow(actor) => {
                write!(f, "inventory of {actor:?} would overflow")
            }
        }
    }
}

impl std::error::Error for TransferRefusal {}

/// What one applied transfer changed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransferReport {
    /// The interaction that was applied.
    pub id: InteractionId,
    /// The initiator's velocity after the transfer, in m/s.
    pub velocity_m_s: [f64; 3],
    /// The pilot that moved to the target, if the policy moved one.
    pub pilot_moved: Option<PilotId>,
    /// How many inventory items moved to the target.
    pub inventory_moved: u32,
    /// The actor the camera binding follows afterwards.
    pub camera_actor: ActorId,
    /// The actor that owns pose/control afterwards.
    pub control_actor: ActorId,
}

/// The pilot and inventory bindings of one session, and the transfers already
/// applied to them.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferLedger {
    session: u64,
    pilots: BTreeMap<ActorId, PilotId>,
    inventory: BTreeMap<ActorId, u32>,
    applied: BTreeSet<InteractionId>,
    initial_pilots: BTreeMap<ActorId, PilotId>,
    initial_inventory: BTreeMap<ActorId, u32>,
}

impl TransferLedger {
    /// A ledger for `session` whose initial bindings are what a retry
    /// restores.
    #[must_use]
    pub fn new(
        session: u64,
        pilots: impl IntoIterator<Item = (ActorId, PilotId)>,
        inventory: impl IntoIterator<Item = (ActorId, u32)>,
    ) -> Self {
        let pilots: BTreeMap<_, _> = pilots.into_iter().collect();
        let inventory: BTreeMap<_, _> = inventory.into_iter().collect();
        Self {
            session,
            initial_pilots: pilots.clone(),
            initial_inventory: inventory.clone(),
            pilots,
            inventory,
            applied: BTreeSet::new(),
        }
    }

    /// The session generation this ledger belongs to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The pilot bound to `actor`.
    #[must_use]
    pub fn pilot_of(&self, actor: ActorId) -> Option<PilotId> {
        self.pilots.get(&actor).copied()
    }

    /// The inventory count bound to `actor`.
    #[must_use]
    pub fn inventory_of(&self, actor: ActorId) -> u32 {
        self.inventory.get(&actor).copied().unwrap_or(0)
    }

    /// Applies a completed interaction atomically.
    ///
    /// `initiator_velocity_m_s` and `target_velocity_m_s` are the world-frame
    /// velocities at completion; the policy decides which one the initiator
    /// keeps. Every precondition is checked before any binding changes.
    ///
    /// # Errors
    ///
    /// [`TransferRefusal`]; the ledger is unchanged on error.
    pub fn apply(
        &mut self,
        outcome: &InteractionOutcome,
        initiator_velocity_m_s: [f64; 3],
        target_velocity_m_s: [f64; 3],
    ) -> Result<TransferReport, TransferRefusal> {
        let id = outcome.id;
        if id.session != self.session {
            return Err(TransferRefusal::StaleSession {
                ledger: self.session,
                interaction: id.session,
            });
        }
        if self.applied.contains(&id) {
            return Err(TransferRefusal::AlreadyApplied(id));
        }

        let effects = outcome.effects;
        let pilot = self.pilots.get(&id.initiator).copied();
        if effects.pilot_moved {
            let Some(pilot) = pilot else {
                return Err(TransferRefusal::NoPilot(id.initiator));
            };
            if let Some(&occupant) = self.pilots.get(&id.target) {
                debug_assert_ne!(occupant, pilot);
                return Err(TransferRefusal::TargetOccupied {
                    target: id.target,
                    pilot: occupant,
                });
            }
        }
        let carried = if effects.inventory_moved {
            self.inventory_of(id.initiator)
        } else {
            0
        };
        let new_target_inventory = self
            .inventory_of(id.target)
            .checked_add(carried)
            .ok_or(TransferRefusal::InventoryOverflow(id.target))?;

        // Commit: nothing below can fail.
        let pilot_moved = if effects.pilot_moved {
            self.pilots.remove(&id.initiator);
            if let Some(pilot) = pilot {
                self.pilots.insert(id.target, pilot);
            }
            pilot
        } else {
            None
        };
        if carried > 0 {
            self.inventory.remove(&id.initiator);
            self.inventory.insert(id.target, new_target_inventory);
        }
        self.applied.insert(id);

        Ok(TransferReport {
            id,
            velocity_m_s: match effects.velocity {
                VelocityTransfer::MatchTarget => target_velocity_m_s,
                VelocityTransfer::PreserveInitiator => initiator_velocity_m_s,
            },
            pilot_moved,
            inventory_moved: carried,
            camera_actor: match effects.camera {
                CameraTransfer::FollowTarget => id.target,
                CameraTransfer::FollowInitiator => id.initiator,
            },
            control_actor: match effects.control_owner {
                ControlOwner::Target => id.target,
                ControlOwner::Initiator | ControlOwner::LatchController => id.initiator,
            },
        })
    }

    /// Restores the initial bindings under a new session generation, so a
    /// retried mission starts from its original actor with no applied
    /// transfers carried over.
    pub fn retry(&mut self, session: u64) {
        self.session = session;
        self.pilots = self.initial_pilots.clone();
        self.inventory = self.initial_inventory.clone();
        self.applied.clear();
    }
}
