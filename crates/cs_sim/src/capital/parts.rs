//! Capital-ship mounts and anchors: the per-kind detail records the ship
//! definition carries beside the shared subsystem identity (F35-A).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. These are the runtime forms of the declared turret,
//! docking-anchor and cargo data — a turret's weapon binding and traverse,
//! an anchor's body-frame offset, the ship's cargo capacity. They are
//! carried, not yet consumed: turret behaviour is F35-B and docking/cargo
//! wiring is F35-C. Every value is a [`Resolved`], so an unmeasured binding
//! stays unknown instead of defaulting.

use cs_types::content::{ContentId, Resolved};

use super::subsystem::SubsystemKey;

/// A turret mount and the weapon it carries.
#[derive(Clone, Debug, PartialEq)]
pub struct TurretMount {
    /// The turret subsystem this mount is.
    pub key: SubsystemKey,
    /// The weapon the turret mounts, or an explicit unknown.
    pub weapon: Resolved<ContentId>,
    /// The traverse arc in degrees, or an explicit unknown.
    pub traverse_deg: Resolved<f64>,
}

/// A docking anchor and its body-frame offset.
#[derive(Clone, Debug, PartialEq)]
pub struct DockingAnchor {
    /// The docking-anchor subsystem this record is.
    pub key: SubsystemKey,
    /// The anchor offset in the ship body frame, or an explicit unknown.
    pub offset_m: Resolved<[f64; 3]>,
}
