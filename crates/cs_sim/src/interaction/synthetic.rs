//! The minimal synthetic interaction fixture (F36-A).
//!
//! A moving docking hook, its designed envelope and a docking transaction.
//! Everything here is newly authored synthetic fixture data — it is never
//! original game data and it stands in for nothing retail.

use cs_script::ir::ActorId as ScriptActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

use super::eligibility::EligibilityEnvelope;
use super::state::{InteractionAuthorization, InteractionId, InteractionKind};
use super::transaction::{InteractionTransaction, TransferPolicy};
use crate::damage::ActorId;
use crate::world_actors::Quat;
use crate::world_actors::anchor::AnchorSocket;
use crate::world_actors::trajectory::{Keyframe, Trajectory};

/// The session generation the synthetic fixture belongs to.
pub const SYNTHETIC_SESSION: u64 = 7;
/// The initiator of the synthetic docking interaction.
pub const SYNTHETIC_INITIATOR: ActorId = ActorId {
    session: SYNTHETIC_SESSION,
    serial: 1,
};
/// The owner of the synthetic docking hook.
pub const SYNTHETIC_HOOK_TARGET: ActorId = ActorId {
    session: SYNTHETIC_SESSION,
    serial: 2,
};
/// The capture radius of the synthetic envelope, in metres.
pub const SYNTHETIC_CAPTURE_RADIUS_M: f64 = 5.0;
/// The largest relative speed of the synthetic envelope, in m/s.
pub const SYNTHETIC_MAX_RELATIVE_SPEED_M_S: f64 = 10.0;
/// The largest approach angle of the synthetic envelope, in degrees.
pub const SYNTHETIC_MAX_APPROACH_ANGLE_DEG: f64 = 35.0;

/// The target's authored path: 50 m along +X in 10 s at 10 ticks/s, no
/// rotation.
#[must_use]
pub fn synthetic_hook_trajectory() -> Trajectory {
    Trajectory::new(
        vec![
            Keyframe {
                tick: Tick(0),
                position_m: [0.0, 0.0, 0.0],
                orientation: Quat::IDENTITY,
            },
            Keyframe {
                tick: Tick(100),
                position_m: [50.0, 0.0, 0.0],
                orientation: Quat::IDENTITY,
            },
        ],
        10,
    )
    .expect("the synthetic hook trajectory is valid")
}

/// The synthetic hook anchor, at the target's origin facing +X.
#[must_use]
pub const fn synthetic_docking_anchor() -> AnchorSocket {
    AnchorSocket {
        actor: ScriptActorId(2),
        socket: 0,
        offset_m: [0.0, 0.0, 0.0],
    }
}

/// The designed docking envelope: a 5 m capture radius, at most 10 m/s
/// relative speed, at most 35 degrees off the +X docking axis.
#[must_use]
pub fn synthetic_docking_envelope() -> EligibilityEnvelope {
    EligibilityEnvelope::try_new(
        SYNTHETIC_CAPTURE_RADIUS_M,
        SYNTHETIC_MAX_RELATIVE_SPEED_M_S,
        0.0,
        SYNTHETIC_MAX_APPROACH_ANGLE_DEG,
        [1.0, 0.0, 0.0],
    )
    .expect("the synthetic envelope is valid")
}

/// The objective that authorizes the synthetic docking interaction.
#[must_use]
pub fn synthetic_docking_objective() -> ContentId {
    ContentId::from_source(ContentKind::Objective, "m01.dock")
        .expect("the synthetic objective id is valid")
}

/// The mission authorization for the synthetic docking interaction.
#[must_use]
pub fn synthetic_docking_authorization() -> InteractionAuthorization {
    InteractionAuthorization::new(
        SYNTHETIC_SESSION,
        InteractionKind::Docking,
        synthetic_docking_objective(),
    )
}

/// The stable identity of the synthetic docking interaction.
#[must_use]
pub const fn synthetic_docking_id() -> InteractionId {
    InteractionId {
        session: SYNTHETIC_SESSION,
        serial: 1,
        initiator: SYNTHETIC_INITIATOR,
        target: SYNTHETIC_HOOK_TARGET,
    }
}

/// A docking transaction at [`super::state::InteractionState::Available`].
#[must_use]
pub fn synthetic_docking_transaction() -> InteractionTransaction {
    InteractionTransaction::begin(
        synthetic_docking_id(),
        InteractionKind::Docking,
        synthetic_docking_authorization(),
        TransferPolicy::docking(),
    )
}
