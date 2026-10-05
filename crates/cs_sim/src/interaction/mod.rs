//! Docking, passenger pickups, boarding and plane-swap contracts (F36-A).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Stage **F36-A** is the typed contract and a minimal synthetic fixture — not
//! the moving-frame eligibility runtime and atomic transfer (F36-B), the
//! consumer wiring (F36-C) or the original interaction validation (F36-D). The
//! module is split so each part is one owner:
//!
//! * [`state`] is the shared identity and state vocabulary:
//!   [`state::InteractionKind`], [`state::InteractionCompletion`], the
//!   explicit [`state::InteractionState`] chain, the stable
//!   [`state::InteractionId`] and the [`state::InteractionAuthorization`] the
//!   active objective supplies.
//! * [`eligibility`] is the swept eligibility test
//!   ([`eligibility::evaluate_eligibility`]) — closest approach over a sweep,
//!   relative speed and approach direction, never a single radius test
//!   (non-negotiable behavior 1).
//! * [`transaction`] is [`transaction::InteractionTransaction`] and the
//!   declared per-transition [`transaction::TransferPolicy`]: one
//!   [`transaction::ControlOwner`] at every stage, completion validates
//!   authorization and target liveness, and an abort applies no effects.
//! * [`transfer`] (F36-B) derives the initiator's velocity from f64 world
//!   positions so an origin rebase cannot create a false speed, and applies a
//!   completed outcome to pilot and inventory bindings atomically and once.
//! * [`session`] (F36-C) drives transactions and the ledger together and
//!   resolves control when a carrier is destroyed, a pause or retry aborts,
//!   or a transfer is refused.
//! * [`synthetic`] is the designed moving-hook fixture the acceptance tests
//!   drive.
//!
//! The declared, provenance-carrying schema that records where an interaction
//! and its envelope came from is `cs_content::interaction`; the lowering
//! boundary is `cs_app::interaction`. The original docking, pickup, boarding
//! and plane-swap rules — envelope values, transition set and transfer policy
//! — are unrecovered, so everything here is designed behavior and nothing is
//! an original-fidelity claim: see
//! `docs/findings/2026-10-01-f36-a-interaction-state-machines.md`.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`): no Bevy, no renderer, no file access.

pub mod eligibility;
pub mod session;
pub mod state;
pub mod synthetic;
pub mod transaction;
pub mod transfer;

pub use eligibility::{
    EligibilityEnvelope, EligibilityRefusal, EnvelopeError, evaluate_eligibility,
};
pub use session::{ControlHolder, InteractionSession, SessionRefusal};
pub use state::{
    InteractionAuthorization, InteractionCompletion, InteractionId, InteractionKind,
    InteractionState,
};
pub use synthetic::{
    SYNTHETIC_CAPTURE_RADIUS_M, SYNTHETIC_HOOK_TARGET, SYNTHETIC_INITIATOR,
    SYNTHETIC_MAX_APPROACH_ANGLE_DEG, SYNTHETIC_MAX_RELATIVE_SPEED_M_S, SYNTHETIC_SESSION,
    synthetic_docking_anchor, synthetic_docking_authorization, synthetic_docking_envelope,
    synthetic_docking_id, synthetic_docking_objective, synthetic_docking_transaction,
    synthetic_hook_trajectory,
};
pub use transaction::{
    AbortReason, CameraTransfer, ControlOwner, InteractionAbort, InteractionOutcome,
    InteractionRefusal, InteractionTransaction, InventoryTransfer, LatchRefusal, PilotTransfer,
    TransferEffects, TransferPolicy, VelocityTransfer,
};
pub use transfer::{
    InitiatorMotion, MotionError, PilotId, TransferLedger, TransferRefusal, TransferReport,
    evaluate_motion_eligibility,
};
