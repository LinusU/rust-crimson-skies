//! World origin, local frames and the rebase/teleport distinction (F16-A).
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! stage `### F16-A`. Non-negotiable behavior 2 (an f64 [`WorldPosition`]
//! with a world origin and f32 local positions where precision requires it)
//! and 5 (teleport and rebase are distinct) are typed here; the transaction
//! that applies an origin shift to every body, projectile, trigger, AI path,
//! audio and camera history atomically is F16-B, and wiring it into those
//! subsystems is F16-C.
//!
//! A [`WorldOrigin`] is an epoch plus a canonical position. Local positions
//! are only meaningful against the epoch that produced them, so a rebase
//! opens a **new epoch** instead of mutating the old one in place: world
//! identity (the f64 coordinates) survives, and stale local coordinates are
//! detectable because they belong to an earlier epoch.
//!
//! This module stays Bevy-free; `cs_app` composes it with the engine.

use std::fmt;

use cs_types::space::{LocalPosition, SpaceError, WorldPosition};

/// Generation of the world origin.
///
/// Every rebase allocates the next epoch, so an epoch identifies the frame a
/// local coordinate was produced in. Epochs are never recycled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OriginEpoch(pub u64);

/// Why an origin operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum OriginError {
    /// The epoch counter would have wrapped; an epoch must never be reused
    /// for a different origin frame.
    EpochExhausted,
    /// A spatial value crossed the boundary and was rejected there
    /// (non-finite component, or an overflow to infinity in f32/f64).
    Space(SpaceError),
}

impl fmt::Display for OriginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EpochExhausted => write!(f, "origin epoch counter is exhausted"),
            Self::Space(error) => write!(f, "origin conversion rejected its input: {error}"),
        }
    }
}

impl std::error::Error for OriginError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EpochExhausted => None,
            Self::Space(error) => Some(error),
        }
    }
}

impl From<SpaceError> for OriginError {
    fn from(value: SpaceError) -> Self {
        Self::Space(value)
    }
}

/// The world origin: one epoch plus the canonical f64 position everything
/// local is measured from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldOrigin {
    epoch: OriginEpoch,
    position: WorldPosition,
}

impl WorldOrigin {
    /// Builds an origin at `position`, first epoch by convention when the
    /// caller passes [`OriginEpoch::default()`].
    #[must_use]
    pub const fn new(epoch: OriginEpoch, position: WorldPosition) -> Self {
        Self { epoch, position }
    }

    /// The epoch this frame belongs to.
    #[must_use]
    pub const fn epoch(&self) -> OriginEpoch {
        self.epoch
    }

    /// The canonical position of the origin, in meters.
    #[must_use]
    pub const fn position(&self) -> WorldPosition {
        self.position
    }

    /// Canonical world position → this frame's local position (f32).
    ///
    /// # Errors
    ///
    /// [`OriginError::Space`] when the difference overflows f32, so a
    /// runaway coordinate is reported instead of silently becoming an
    /// infinity.
    pub fn local_of(&self, world: WorldPosition) -> Result<LocalPosition, OriginError> {
        let [wx, wy, wz] = world.to_array();
        let [ox, oy, oz] = self.position.to_array();
        Ok(LocalPosition::try_new([
            (wx - ox) as f32,
            (wy - oy) as f32,
            (wz - oz) as f32,
        ])?)
    }

    /// This frame's local position → canonical world position (f64).
    ///
    /// # Errors
    ///
    /// [`OriginError::Space`] when the sum overflows f64.
    pub fn world_of(&self, local: LocalPosition) -> Result<WorldPosition, OriginError> {
        let [lx, ly, lz] = local.to_array();
        let [ox, oy, oz] = self.position.to_array();
        Ok(WorldPosition::try_new([
            ox + f64::from(lx),
            oy + f64::from(ly),
            oz + f64::from(lz),
        ])?)
    }

    /// The same world frame re-anchored at `new_position`, under the next
    /// epoch.
    ///
    /// This is the *typed input* of a rebase: it says where the origin moves
    /// and that the epoch changes. Applying it to bodies, projectiles,
    /// triggers, AI paths, audio and camera histories atomically — and
    /// keeping swept segments continuous across it — is F16-B's transaction.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochExhausted`] if the epoch counter would wrap.
    pub fn rebased(&self, new_position: WorldPosition) -> Result<Self, OriginError> {
        let epoch = self
            .epoch
            .0
            .checked_add(1)
            .ok_or(OriginError::EpochExhausted)?;
        Ok(Self {
            epoch: OriginEpoch(epoch),
            position: new_position,
        })
    }
}

/// Absolute floor of [`local_round_trip_tolerance_m`], in meters.
///
/// It covers the f64 rounding of the origin addition for ordinary world
/// coordinates; the value-dependent terms cover everything larger.
pub const LOCAL_ABSOLUTE_TOLERANCE_M: f64 = 1e-6;

/// The declared tolerance of a world → local → world round trip, in meters.
///
/// The bound is stated as a function of the values involved rather than a
/// number chosen to make a test pass:
///
/// * `4 · f64::EPSILON · |world|` for rounding the origin addition back into
///   an f64 of that magnitude,
/// * `4 · f32::EPSILON · |local|` for storing the local vector as f32,
/// * [`LOCAL_ABSOLUTE_TOLERANCE_M`] as the floor for small coordinates.
///
/// The f32 term is 8× the worst-case half-ulp of the cast, so the bound is
/// tight enough to catch a real regression (a lost component, a wrong
/// origin) and loose enough never to be flaky.
#[must_use]
pub fn local_round_trip_tolerance_m(world: WorldPosition, local: LocalPosition) -> f64 {
    let world_max = world
        .to_array()
        .iter()
        .fold(0.0_f64, |peak, value| peak.max(value.abs()));
    let local_max = local
        .to_array()
        .iter()
        .fold(0.0_f64, |peak, value| peak.max(f64::from(*value).abs()));
    LOCAL_ABSOLUTE_TOLERANCE_M
        + 4.0 * f64::EPSILON * world_max
        + 4.0 * f64::from(f32::EPSILON) * local_max
}

/// How an origin or world-state change treats swept continuity and world
/// identity (`F16` non-negotiable behavior 5).
///
/// The two are different operations with different consequences, so they are
/// different values instead of a boolean someone can invert by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OriginChange {
    /// The origin moves and every consumer's local coordinates are
    /// recomputed. World positions (f64) are unchanged, so world identity,
    /// swept trigger/projectile segments and histories survive: both
    /// endpoints of a sweep shift together.
    Rebase,
    /// A body is moved to a new world position with no continuous path.
    /// Prior sweep segments no longer describe this body and are discarded.
    Teleport,
}

impl OriginChange {
    /// Whether swept segments and world identity survive this change.
    #[must_use]
    pub const fn preserves_swept_continuity(self) -> bool {
        match self {
            Self::Rebase => true,
            Self::Teleport => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(x: f64, y: f64, z: f64, epoch: u64) -> WorldOrigin {
        WorldOrigin::new(
            OriginEpoch(epoch),
            WorldPosition::try_new([x, y, z]).expect("finite origin"),
        )
    }

    fn world(x: f64, y: f64, z: f64) -> WorldPosition {
        WorldPosition::try_new([x, y, z]).expect("finite position")
    }

    /// World → local (f32) → world stays inside the declared tolerance for
    /// nearby, far and large coordinates, and the local frame's own origin
    /// maps to exactly zero.
    #[test]
    fn accept_f16_a_local_world_round_trip_within_declared_tolerance() {
        let origin = origin(1_000_000.0, -500_000.0, 250_000.0, 3);
        for candidate in [
            world(1_000_000.0, -500_000.0, 250_000.0),
            world(1_000_010.5, -499_990.25, 250_001.75),
            world(0.0, 0.0, 0.0),
            world(-2_500_000.0, 12.5, 8_000_000.0),
        ] {
            let local = origin.local_of(candidate).expect("finite difference");
            let back = origin.world_of(local).expect("finite sum");
            let tolerance = local_round_trip_tolerance_m(candidate, local);
            assert!(
                tolerance > 0.0 && tolerance.is_finite(),
                "declared tolerance must be a usable bound"
            );
            for (axis, (actual, wanted)) in back
                .to_array()
                .into_iter()
                .zip(candidate.to_array())
                .enumerate()
            {
                assert!(
                    (actual - wanted).abs() <= tolerance,
                    "round trip must stay within the declared tolerance {tolerance} m on axis {axis}: {actual} != {wanted}"
                );
            }
        }

        let at_origin = origin
            .local_of(world(1_000_000.0, -500_000.0, 250_000.0))
            .expect("finite difference");
        assert_eq!(
            at_origin,
            LocalPosition::ZERO,
            "the origin maps to its own local zero"
        );
    }

    /// A rebase opens exactly one new epoch, and the same world position is
    /// still reachable through the new frame — while the *old* local
    /// coordinate is no longer the right one, which is what makes a stale
    /// epoch detectable.
    #[test]
    fn accept_f16_a_rebase_opens_a_new_epoch_and_keeps_world_positions() {
        let before = origin(0.0, 0.0, 0.0, 0);
        let point = world(120.0, 40.0, -80.0);
        let local_before = before.local_of(point).expect("finite difference");

        let after = before
            .rebased(world(1_000.0, 0.0, -250.0))
            .expect("epoch 0 can rebase");
        assert_eq!(
            after.epoch(),
            OriginEpoch(1),
            "a rebase advances the epoch exactly once"
        );
        assert_ne!(
            after.position(),
            before.position(),
            "the origin actually moved"
        );

        let local_after = after.local_of(point).expect("finite difference");
        let back = after.world_of(local_after).expect("finite sum");
        let tolerance = local_round_trip_tolerance_m(point, local_after);
        for (actual, wanted) in back.to_array().into_iter().zip(point.to_array()) {
            assert!(
                (actual - wanted).abs() <= tolerance,
                "world identity must survive a rebase: {actual} != {wanted}"
            );
        }

        let stale = after.world_of(local_before).expect("finite sum").to_array();
        let wanted = point.to_array();
        assert!(
            (stale[0] - wanted[0]).abs() > 100.0,
            "the pre-rebase local coordinate must not still describe the world point"
        );

        let rebased_again = after
            .rebased(world(0.0, 0.0, 0.0))
            .expect("epoch 1 can rebase");
        assert_eq!(
            rebased_again.epoch(),
            OriginEpoch(2),
            "epochs keep increasing"
        );
    }

    /// Non-negotiable 5 at the type level: rebase and teleport are distinct
    /// values, and only a rebase keeps swept continuity and world identity.
    #[test]
    fn accept_f16_a_rebase_and_teleport_are_distinct_and_declare_swept_continuity() {
        assert_ne!(OriginChange::Rebase, OriginChange::Teleport);
        assert!(
            OriginChange::Rebase.preserves_swept_continuity(),
            "a rebase must preserve swept segments and world identity"
        );
        assert!(
            !OriginChange::Teleport.preserves_swept_continuity(),
            "a teleport must invalidate prior sweep segments"
        );
    }

    /// Overflowing the frame conversion is reported, never produced as an
    /// infinity that would later look like a valid position.
    #[test]
    fn accept_f16_a_origin_boundary_overflow_is_refused() {
        let tiny = origin(-f64::MAX, 0.0, 0.0, 0);
        let far = world(f64::MAX, 0.0, 0.0);
        assert!(
            matches!(
                tiny.local_of(far),
                Err(OriginError::Space(SpaceError::NonFinite { .. }))
            ),
            "an overflowing world difference must be refused"
        );

        let frame = origin(0.0, 0.0, 0.0, 0);
        let huge = LocalPosition::try_new([f32::MAX, 0.0, 0.0]).expect("finite f32");
        assert!(
            frame.world_of(huge).is_ok(),
            "an f32 local position is always a finite f64 sum"
        );

        let edge = origin(0.0, 0.0, 0.0, u64::MAX);
        assert_eq!(
            edge.rebased(world(1.0, 2.0, 3.0)),
            Err(OriginError::EpochExhausted),
            "the epoch counter must never wrap"
        );
    }
}
