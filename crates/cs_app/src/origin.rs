//! World origin, local frames and the rebase/teleport distinction (F16-A).
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! stages `### F16-A` and `### F16-B`. Non-negotiable behavior 2 (an f64
//! [`WorldPosition`] with a world origin and f32 local positions where
//! precision requires it) and 5 (teleport and rebase are distinct) are typed
//! here; [`OriginShift`] is the F16-B transaction that applies one origin
//! shift to every [`SpatialAnchor`] — body, projectile, trigger, AI path,
//! audio or camera history — atomically. Wiring it into those subsystems is
//! F16-C.
//!
//! A [`WorldOrigin`] is an epoch plus a canonical position. Local positions
//! are only meaningful against the epoch that produced them, so a rebase
//! opens a **new epoch** instead of mutating the old one in place: world
//! identity (the f64 coordinates) survives, and stale local coordinates are
//! detectable because they belong to an earlier epoch.
//!
//! # Conversion and origin-shift transactions
//!
//! A rebase is a *conversion*: every spatial record's f32 local cache is the
//! result of converting its invariant f64 world position through the old
//! frame and back through the new one. [`OriginShift::apply`] performs that
//! conversion for a whole set of records in two phases — it plans every
//! record first and commits only if all of them convert — so a refused
//! conversion leaves every record and the origin itself untouched. A sweep
//! keeps both endpoints (the previous world position and its converted local
//! position), which is what makes a rebase preserve swept continuity while a
//! teleport clears it (non-negotiable behavior 5).
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
    /// A spatial record belonged to a different origin epoch than the
    /// operation is for, so applying it would move the record with the wrong
    /// frame. Refused instead of silently rebasing from the wrong origin.
    EpochMismatch {
        /// The epoch stored on the record.
        anchor: OriginEpoch,
        /// The epoch the operation is anchored to.
        frame: OriginEpoch,
    },
    /// A spatial value crossed the boundary and was rejected there
    /// (non-finite component, or an overflow to infinity in f32/f64).
    Space(SpaceError),
}

impl fmt::Display for OriginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EpochExhausted => write!(f, "origin epoch counter is exhausted"),
            Self::EpochMismatch { anchor, frame } => write!(
                f,
                "spatial record is in origin epoch {}, but the operation is anchored to epoch {}",
                anchor.0, frame.0
            ),
            Self::Space(error) => write!(f, "origin conversion rejected its input: {error}"),
        }
    }
}

impl std::error::Error for OriginError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EpochExhausted | Self::EpochMismatch { .. } => None,
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

/// The previous endpoint of one spatial record's movement.
///
/// A trigger or swept query tests the path a record travelled last tick, so
/// it needs both the previous and the current position. The segment stores
/// both endpoints in world (f64) and local (f32) form because the two answer
/// different questions: the world endpoints are the invariant world identity
/// a rebase must keep (`OriginShift`), while the local endpoint is what the
/// f32 physics/render side actually queries. A `SweptSegment`'s
/// [`from_local`](Self::from_local) is only meaningful in the epoch of the
/// [`SpatialAnchor`] that holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweptSegment {
    from_world: WorldPosition,
    from_local: LocalPosition,
}

impl SweptSegment {
    /// The previous world position, unchanged by a rebase.
    #[must_use]
    pub const fn from_world(&self) -> WorldPosition {
        self.from_world
    }

    /// The previous position in its anchor's current local frame.
    #[must_use]
    pub const fn from_local(&self) -> LocalPosition {
        self.from_local
    }
}

/// One spatial record: an invariant f64 world identity, its f32 local pose in
/// the frame named by [`epoch`](Self::epoch), and the previous endpoint of
/// its movement.
///
/// This is the per-record pose owner F16-C will bind to a body, projectile,
/// trigger, AI path, audio emitter or camera history. Movement happens
/// through [`advance_local`](Self::advance_local) /
/// [`move_to`](Self::move_to) (which keep the swept segment continuous) or
/// [`teleport`](Self::teleport) (which discards it); a frame change happens
/// through [`OriginShift`], which moves the local cache into a new epoch
/// without touching [`world`](Self::world).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialAnchor {
    world: WorldPosition,
    local: LocalPosition,
    epoch: OriginEpoch,
    sweep: Option<SweptSegment>,
}

impl SpatialAnchor {
    /// A record at a world position, with its local pose derived from
    /// `origin`.
    ///
    /// # Errors
    ///
    /// [`OriginError::Space`] when the world position cannot be expressed in
    /// the origin's frame (an f32 overflow).
    pub fn new(origin: &WorldOrigin, world: WorldPosition) -> Result<Self, OriginError> {
        let local = origin.local_of(world)?;
        Ok(Self {
            world,
            local,
            epoch: origin.epoch(),
            sweep: None,
        })
    }

    /// A record at an explicit local position in `origin`'s frame.
    ///
    /// # Errors
    ///
    /// [`OriginError::Space`] when the local position's world sum overflows
    /// f64.
    pub fn from_local(origin: &WorldOrigin, local: LocalPosition) -> Result<Self, OriginError> {
        let world = origin.world_of(local)?;
        Ok(Self {
            world,
            local,
            epoch: origin.epoch(),
            sweep: None,
        })
    }

    /// The f64 world identity, unchanged by a rebase.
    #[must_use]
    pub const fn world(&self) -> WorldPosition {
        self.world
    }

    /// The f32 pose in the frame named by [`epoch`](Self::epoch).
    #[must_use]
    pub const fn local(&self) -> LocalPosition {
        self.local
    }

    /// The origin frame this record's local pose belongs to.
    #[must_use]
    pub const fn epoch(&self) -> OriginEpoch {
        self.epoch
    }

    /// The previous endpoint of the last movement, if one was recorded.
    #[must_use]
    pub const fn sweep(&self) -> Option<SweptSegment> {
        self.sweep
    }

    /// Advances the local pose by `delta` in the current frame, recording the
    /// swept segment from the previous pose.
    ///
    /// This is the local (f32) movement a physics integration produces; the
    /// world identity is recomputed from the new local pose so it can never
    /// drift from what the frame says.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochMismatch`] when `origin` is not the frame this
    /// record belongs to, or [`OriginError::Space`] when the new local pose
    /// is non-finite. Nothing is mutated on error.
    pub fn advance_local(
        &mut self,
        origin: &WorldOrigin,
        delta: [f32; 3],
    ) -> Result<(), OriginError> {
        self.require_epoch(origin.epoch())?;
        let [x, y, z] = self.local.to_array();
        let local = LocalPosition::try_new([x + delta[0], y + delta[1], z + delta[2]])?;
        let world = origin.world_of(local)?;
        self.sweep = Some(SweptSegment {
            from_world: self.world,
            from_local: self.local,
        });
        self.local = local;
        self.world = world;
        Ok(())
    }

    /// Moves the record to an explicit world position in the current frame,
    /// recording the swept segment from the previous pose.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochMismatch`] when `origin` is not the frame this
    /// record belongs to, or [`OriginError::Space`] when the new position
    /// cannot be expressed in the frame. Nothing is mutated on error.
    pub fn move_to(
        &mut self,
        origin: &WorldOrigin,
        world: WorldPosition,
    ) -> Result<(), OriginError> {
        self.require_epoch(origin.epoch())?;
        let local = origin.local_of(world)?;
        self.sweep = Some(SweptSegment {
            from_world: self.world,
            from_local: self.local,
        });
        self.world = world;
        self.local = local;
        Ok(())
    }

    /// Jumps the record to a new world position with no continuous path.
    ///
    /// Per non-negotiable behavior 5 a teleport is *not* a rebase: prior
    /// sweep segments no longer describe this record and are discarded, so a
    /// later trigger cannot fire on the segment that never happened.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochMismatch`] when `origin` is not the frame this
    /// record belongs to, or [`OriginError::Space`] when the new position
    /// cannot be expressed in the frame. Nothing is mutated on error.
    pub fn teleport(
        &mut self,
        origin: &WorldOrigin,
        world: WorldPosition,
    ) -> Result<(), OriginError> {
        self.require_epoch(origin.epoch())?;
        let local = origin.local_of(world)?;
        self.world = world;
        self.local = local;
        self.sweep = None;
        Ok(())
    }

    /// Recomputes this record's local pose (and its swept segment) in a
    /// strictly newer origin frame, keeping its world identity.
    ///
    /// The single-record form of [`OriginShift::apply`]; prefer the
    /// transaction when a whole set of records must move together.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochMismatch`] when `to` is not a newer epoch than
    /// this record's, or [`OriginError::Space`] when the record cannot be
    /// expressed in `to`. Nothing is mutated on error.
    pub fn rebase(&mut self, to: &WorldOrigin) -> Result<(), OriginError> {
        *self = self.rebased(to)?;
        Ok(())
    }

    /// This record as it would be in the newer frame `to`, without mutating
    /// it. See [`rebase`](Self::rebase).
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochMismatch`] when `to` is not a newer epoch, or
    /// [`OriginError::Space`] when the record cannot be expressed in `to`.
    pub fn rebased(self, to: &WorldOrigin) -> Result<Self, OriginError> {
        if to.epoch() <= self.epoch {
            return Err(OriginError::EpochMismatch {
                anchor: self.epoch,
                frame: to.epoch(),
            });
        }
        let local = to.local_of(self.world)?;
        let sweep = match self.sweep {
            Some(segment) => Some(SweptSegment {
                from_world: segment.from_world,
                from_local: to.local_of(segment.from_world)?,
            }),
            None => None,
        };
        Ok(Self {
            world: self.world,
            local,
            epoch: to.epoch(),
            sweep,
        })
    }

    /// Rejects an origin that is not this record's frame.
    fn require_epoch(&self, current: OriginEpoch) -> Result<(), OriginError> {
        if self.epoch == current {
            Ok(())
        } else {
            Err(OriginError::EpochMismatch {
                anchor: self.epoch,
                frame: current,
            })
        }
    }
}

/// One origin rebase applied to a whole set of [`SpatialAnchor`]s at once.
///
/// A rebase changes the origin frame but not world identity: every record
/// keeps its f64 [`WorldPosition`], both endpoints of its swept segment keep
/// their world positions, and only the local (f32) cache is converted into
/// the new frame. [`apply`](Self::apply) is atomic — it plans every record
/// before it commits any — so a conversion that is refused (an f32 overflow)
/// leaves every record and the origin untouched, and no consumer can ever see
/// a half-rebased set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OriginShift {
    from: WorldOrigin,
    to: WorldOrigin,
}

impl OriginShift {
    /// A rebase of `from` to a frame anchored at `new_position`.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochExhausted`] when the epoch counter would wrap.
    pub fn rebase(from: WorldOrigin, new_position: WorldPosition) -> Result<Self, OriginError> {
        let to = from.rebased(new_position)?;
        Ok(Self { from, to })
    }

    /// A rebase from one fully built origin to a strictly newer one.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochMismatch`] when `to` is not a newer epoch than
    /// `from`, so the shift can never move a record backwards in time.
    pub fn new(from: WorldOrigin, to: WorldOrigin) -> Result<Self, OriginError> {
        if to.epoch() <= from.epoch() {
            return Err(OriginError::EpochMismatch {
                anchor: from.epoch(),
                frame: to.epoch(),
            });
        }
        Ok(Self { from, to })
    }

    /// The frame being shifted away from.
    #[must_use]
    pub const fn from(&self) -> WorldOrigin {
        self.from
    }

    /// The new frame. The caller adopts this once [`apply`](Self::apply) has
    /// succeeded.
    #[must_use]
    pub const fn to(&self) -> WorldOrigin {
        self.to
    }

    /// A rebase is always a [`OriginChange::Rebase`].
    #[must_use]
    pub const fn change(&self) -> OriginChange {
        OriginChange::Rebase
    }

    /// Converts every record's local cache into the new frame, atomically.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochMismatch`] when a record is not in the
    /// [`from`](Self::from) frame, or [`OriginError::Space`] when a record
    /// (or a swept endpoint) cannot be expressed in the
    /// [`to`](Self::to) frame. On any error nothing is mutated.
    pub fn apply(&self, anchors: &mut [SpatialAnchor]) -> Result<(), OriginError> {
        let mut planned = Vec::with_capacity(anchors.len());
        for anchor in anchors.iter() {
            if anchor.epoch() != self.from.epoch() {
                return Err(OriginError::EpochMismatch {
                    anchor: anchor.epoch(),
                    frame: self.from.epoch(),
                });
            }
            planned.push(anchor.rebased(&self.to)?);
        }
        for (anchor, rebased) in anchors.iter_mut().zip(planned) {
            *anchor = rebased;
        }
        Ok(())
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

    /// **F16-B minimum mechanism:** an [`OriginShift`] converts every
    /// record's f32 local cache into the new frame while the f64 world
    /// identity and both endpoints of a swept segment keep their world
    /// positions, and movement continues continuously afterwards. If the
    /// transaction forgot to re-anchor the local cache (or converted it with
    /// the old origin) the world positions read back from the local cache
    /// would be off by the origin offset.
    #[test]
    fn accept_f16_b_rebase_converts_every_local_cache_without_moving_world_identity() {
        let before = origin(0.0, 0.0, 0.0, 0);
        let mut anchors = [
            SpatialAnchor::new(&before, world(1200.0, 40.0, -800.0)).expect("finite"),
            SpatialAnchor::new(&before, world(-3000.0, 12.5, 250.0)).expect("finite"),
        ];
        anchors[0]
            .advance_local(&before, [8.0, -4.0, 6.0])
            .expect("move inside the frame");
        let moved_worlds = [anchors[0].world(), anchors[1].world()];
        let sweep_before = anchors[0].sweep().expect("a move leaves a swept segment");

        let shift =
            OriginShift::rebase(before, world(1000.0, 40.0, -800.0)).expect("epoch 0 can rebase");
        assert_eq!(shift.from(), before);
        assert_eq!(shift.to().epoch(), OriginEpoch(1));
        assert_eq!(shift.change(), OriginChange::Rebase);
        assert!(shift.change().preserves_swept_continuity());
        shift.apply(&mut anchors).expect("every record converts");

        for (index, anchor) in anchors.iter().enumerate() {
            assert_eq!(
                anchor.world(),
                moved_worlds[index],
                "world identity must survive the rebase"
            );
            assert_eq!(anchor.epoch(), OriginEpoch(1), "the epoch advances once");
            let local = anchor.local();
            let back = shift.to().world_of(local).expect("finite sum");
            let tolerance = local_round_trip_tolerance_m(anchor.world(), local);
            for (axis, (actual, wanted)) in back
                .to_array()
                .into_iter()
                .zip(anchor.world().to_array())
                .enumerate()
            {
                assert!(
                    (actual - wanted).abs() <= tolerance,
                    "record {index}: the local cache must convert into the new frame \
                     (axis {axis}: {actual} != {wanted})"
                );
            }
        }

        let sweep_after = anchors[0].sweep().expect("a rebase keeps the segment");
        assert_eq!(
            sweep_after.from_world(),
            sweep_before.from_world(),
            "both endpoints of a sweep keep their world positions"
        );
        let back = shift
            .to()
            .world_of(sweep_after.from_local())
            .expect("finite sum");
        let tolerance =
            local_round_trip_tolerance_m(sweep_after.from_world(), sweep_after.from_local());
        for (actual, wanted) in back
            .to_array()
            .into_iter()
            .zip(sweep_after.from_world().to_array())
        {
            assert!(
                (actual - wanted).abs() <= tolerance,
                "the swept endpoint's local address must convert consistently: {actual} != {wanted}"
            );
        }

        anchors[0]
            .advance_local(&shift.to(), [8.0, -4.0, 6.0])
            .expect("move in the new frame");
        assert_eq!(
            anchors[0]
                .sweep()
                .expect("movement stays continuous")
                .from_world(),
            moved_worlds[0],
            "the segment still starts where the record was after the rebase"
        );
    }

    /// The transaction is atomic: when one record cannot be expressed in the
    /// new frame, no record (and no origin) moves — a consumer can never see a
    /// half-rebased set.
    #[test]
    fn accept_f16_b_refused_conversion_leaves_every_record_unchanged() {
        let before = origin(0.0, 0.0, 0.0, 0);
        // 3e38 is representable in the old frame's f32 locals; the far corner
        // is not, once the origin moves a long way away.
        let mut anchors = [
            SpatialAnchor::new(&before, world(0.0, 0.0, 0.0)).expect("finite"),
            SpatialAnchor::new(&before, world(3.0e38, 0.0, 0.0)).expect("finite in the old frame"),
        ];
        let snapshot = anchors;

        let shift = OriginShift::rebase(before, world(-1.0e38, 0.0, 0.0)).expect("valid rebase");
        assert_eq!(
            shift.apply(&mut anchors),
            Err(OriginError::Space(SpaceError::NonFinite {
                field: "local.x"
            })),
            "a record that overflows f32 in the new frame must be refused, not produced as inf"
        );
        assert_eq!(
            anchors, snapshot,
            "a refused transaction must not mutate any record, not even one that converted"
        );
        assert_eq!(anchors[0].epoch(), OriginEpoch(0));
        assert_eq!(shift.to().epoch(), OriginEpoch(1));
    }

    /// A record is only shifted from the frame it actually belongs to, and a
    /// shift only ever moves forwards in epochs.
    #[test]
    fn accept_f16_b_stale_and_foreign_epochs_are_refused() {
        let before = origin(0.0, 0.0, 0.0, 0);
        let started = SpatialAnchor::new(&before, world(10.0, 20.0, 30.0)).expect("finite");
        let mut anchor = started;
        let newer = before.rebased(world(1.0, 1.0, 1.0)).expect("epoch 1");

        let other = origin(5.0, 5.0, 5.0, 7);
        let foreign_to = other.rebased(world(0.0, 0.0, 0.0)).expect("epoch 8");
        let shift = OriginShift::new(other, foreign_to).expect("a newer target frame");
        assert_eq!(
            shift.apply(std::slice::from_mut(&mut anchor)),
            Err(OriginError::EpochMismatch {
                anchor: OriginEpoch(0),
                frame: OriginEpoch(7),
            }),
            "an anchor of another epoch must not be dragged into the shift"
        );
        assert_eq!(anchor, started, "a refused shift mutates nothing");

        for result in [
            anchor.advance_local(&newer, [1.0, 0.0, 0.0]),
            anchor.move_to(&newer, world(0.0, 0.0, 0.0)),
            anchor.teleport(&newer, world(0.0, 0.0, 0.0)),
        ] {
            assert_eq!(
                result,
                Err(OriginError::EpochMismatch {
                    anchor: OriginEpoch(0),
                    frame: OriginEpoch(1),
                }),
                "movement must use the record's own frame"
            );
        }
        assert_eq!(anchor, started, "a refused movement mutates nothing");

        assert_eq!(
            OriginShift::new(before, before),
            Err(OriginError::EpochMismatch {
                anchor: OriginEpoch(0),
                frame: OriginEpoch(0),
            }),
            "a shift must move forwards in time"
        );
        assert_eq!(
            anchor.rebased(&before),
            Err(OriginError::EpochMismatch {
                anchor: OriginEpoch(0),
                frame: OriginEpoch(0),
            }),
            "a record cannot rebase into its own or an older frame"
        );
    }

    /// Non-negotiable 5 for a real record: a rebase keeps the swept segment
    /// (both endpoints shift together), while a teleport discards it so a
    /// trigger cannot fire on a path the record never travelled.
    #[test]
    fn accept_f16_b_teleport_invalidates_sweep_while_rebase_preserves_it() {
        let before = origin(0.0, 0.0, 0.0, 0);
        let mut anchor = SpatialAnchor::new(&before, world(0.0, 0.0, 0.0)).expect("finite");
        anchor
            .advance_local(&before, [4.0, 0.0, 0.0])
            .expect("move");
        assert!(anchor.sweep().is_some(), "movement records a segment");

        let shift = OriginShift::rebase(before, world(-500.0, 0.0, 0.0)).expect("valid rebase");
        shift
            .apply(std::slice::from_mut(&mut anchor))
            .expect("converts");
        let kept = anchor.sweep().expect("a rebase preserves swept continuity");
        assert_eq!(kept.from_world(), world(0.0, 0.0, 0.0));
        assert_eq!(anchor.epoch(), OriginEpoch(1));

        anchor
            .teleport(&shift.to(), world(40.0, 0.0, 0.0))
            .expect("teleport inside the new frame");
        assert_eq!(anchor.sweep(), None, "a teleport invalidates the segment");
        assert_eq!(anchor.world(), world(40.0, 0.0, 0.0));
        assert_eq!(
            anchor.local(),
            LocalPosition::try_new([540.0, 0.0, 0.0]).expect("finite")
        );
    }
}
