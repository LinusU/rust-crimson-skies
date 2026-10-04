//! Capital-ship mounts, anchors and section pools: the per-kind detail
//! records the ship definition carries beside the shared subsystem identity.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stages `### F35-A` and `### F35-B`. These are the runtime forms of the
//! declared turret, docking-anchor, section and cargo data — a turret's
//! weapon binding, traverse arc and boresight, an anchor's body-frame
//! offset, a section's damage pool, the ship's cargo capacity. Turret aim
//! and fire gating consume the mount in F35-B and section pools absorb hits;
//! docking/cargo wiring is F35-C. Every measured value is a [`Resolved`], so
//! an unmeasured binding stays unknown instead of defaulting.

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
    /// The body-frame direction the mount bears on uncommanded. It is
    /// normalized when the ship is constructed, so a consuming system can
    /// treat it as a unit vector. Original mounts measured only weapon and
    /// traverse; the boresight is authored synthetic data (F35-B designed
    /// aim model).
    pub boresight: [f64; 3],
}

/// A docking anchor and its body-frame offset.
#[derive(Clone, Debug, PartialEq)]
pub struct DockingAnchor {
    /// The docking-anchor subsystem this record is.
    pub key: SubsystemKey,
    /// The anchor offset in the ship body frame, or an explicit unknown.
    pub offset_m: Resolved<[f64; 3]>,
}

/// A gas cell or structural section's declared damage pool: how much section
/// damage the part absorbs before it is destroyed.
///
/// Only `SubsystemKind::GasCell` and `SubsystemKind::StructuralSection` parts
/// carry one, mirroring the schema's `DeclaredSection`. The integrity stays a
/// `Resolved` value: an unmeasured section never invents a pool, and a hit on
/// it is blocked by claim rather than absorbed silently.
#[derive(Clone, Debug, PartialEq)]
pub struct IntegrityPool {
    /// The section subsystem this pool is.
    pub key: SubsystemKey,
    /// The declared integrity the pool starts with, or an explicit unknown.
    pub integrity: Resolved<f64>,
}
