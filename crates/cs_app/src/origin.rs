//! World origin, local frames and the rebase/teleport distinction (F16-A).
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! stages `### F16-A` and `### F16-B`. Non-negotiable behavior 2 (an f64
//! [`WorldPosition`] with a world origin and f32 local positions where
//! precision requires it) and 5 (teleport and rebase are distinct) are typed
//! here; [`OriginShift`] is the F16-B transaction that applies one origin
//! shift to every [`SpatialAnchor`] — body, projectile, trigger, AI path,
//! audio or camera history — atomically. F16-C binds those anchors into a
//! [`SpatialWorld`] and drives them from the frame clock of `cs_sim::time`
//! through [`FixedTickDriver`], so a render frame's wall time only ever adds
//! whole fixed ticks and the same input at 30, 60 or 144 FPS produces the
//! same ticks and the same spatial state.
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
use std::time::Duration;

use cs_sim::time::{ClockPolicy, SimClock, TickRate, TimeError};
use cs_types::Tick;
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

/// Which spatial subsystem one [`SpatialRecord`] belongs to.
///
/// `F16` non-negotiable behavior 2 requires that a rebase moves "all bodies,
/// projectiles, triggers, AI paths, audio and camera histories atomically".
/// Naming that set as a type keeps a rebase from silently skipping one member
/// and lets a test prove that every subsystem converted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpatialSubsystem {
    /// A dynamic or kinematic body (aircraft, debris).
    Body,
    /// A projectile or other ballistic particle.
    Projectile,
    /// A trigger volume or swept interaction region.
    Trigger,
    /// A scripted AI route or path node.
    AiPath,
    /// A positional audio emitter or listener history.
    Audio,
    /// A camera or spyglass pose history.
    CameraHistory,
}

impl SpatialSubsystem {
    /// Every subsystem a rebase must move, in a stable order.
    pub const ALL: [Self; 6] = [
        Self::Body,
        Self::Projectile,
        Self::Trigger,
        Self::AiPath,
        Self::Audio,
        Self::CameraHistory,
    ];

    /// Stable label for diagnostics, traces and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::Projectile => "projectile",
            Self::Trigger => "trigger",
            Self::AiPath => "ai-path",
            Self::Audio => "audio",
            Self::CameraHistory => "camera-history",
        }
    }
}

/// A record's stable id inside one [`SpatialWorld`]. Ids are never reused, so
/// a removed record's id cannot silently address a later one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpatialId(pub u64);

/// Why a [`SpatialWorld`] operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum SpatialError {
    /// The origin frame rejected a world or local position.
    Origin(OriginError),
    /// A per-tick local displacement contained NaN or infinity; refused by
    /// name instead of being fed into the integrator.
    NonFiniteDisplacement {
        /// The offending component, `"x"`, `"y"` or `"z"`.
        axis: &'static str,
    },
    /// A rebase limit was zero, negative or non-finite.
    InvalidRebaseLimit {
        /// The rejected limit, in metres.
        limit_m: f32,
    },
    /// The record-id counter would have wrapped. Ids are never reused, so a
    /// session that has used every id must not recycle one.
    RecordIdExhausted,
    /// No record with this id exists in the world.
    UnknownRecord {
        /// The id that was looked up.
        id: SpatialId,
    },
}

impl fmt::Display for SpatialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Origin(error) => write!(f, "spatial world rejected its input: {error}"),
            Self::NonFiniteDisplacement { axis } => {
                write!(f, "per-tick displacement.{axis} must be finite")
            }
            Self::InvalidRebaseLimit { limit_m } => {
                write!(f, "rebase limit must be finite and positive, got {limit_m}")
            }
            Self::RecordIdExhausted => write!(f, "spatial record id counter is exhausted"),
            Self::UnknownRecord { id } => write!(f, "no spatial record with id {}", id.0),
        }
    }
}

impl std::error::Error for SpatialError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Origin(error) => Some(error),
            Self::NonFiniteDisplacement { .. }
            | Self::InvalidRebaseLimit { .. }
            | Self::RecordIdExhausted
            | Self::UnknownRecord { .. } => None,
        }
    }
}

impl From<OriginError> for SpatialError {
    fn from(value: OriginError) -> Self {
        Self::Origin(value)
    }
}

/// One spatial record: the subsystem that owns it, its [`SpatialAnchor`] pose
/// and the local displacement a fixed tick applies.
///
/// The displacement is a per-tick local (`f32`) delta — what one fixed
/// integration step contributes — so it never depends on the render frame
/// rate. A real subsystem will replace it with an integrated velocity; F16-D
/// calibrates that behaviour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialRecord {
    id: SpatialId,
    subsystem: SpatialSubsystem,
    anchor: SpatialAnchor,
    per_tick_local: [f32; 3],
}

impl SpatialRecord {
    /// The record's stable id.
    #[must_use]
    pub const fn id(&self) -> SpatialId {
        self.id
    }

    /// The subsystem that owns this record.
    #[must_use]
    pub const fn subsystem(&self) -> SpatialSubsystem {
        self.subsystem
    }

    /// The invariant f64 world identity, unchanged by a rebase.
    #[must_use]
    pub const fn world(&self) -> WorldPosition {
        self.anchor.world()
    }

    /// The f32 pose in the frame named by [`epoch`](Self::epoch).
    #[must_use]
    pub const fn local(&self) -> LocalPosition {
        self.anchor.local()
    }

    /// The origin frame this record's local pose belongs to.
    #[must_use]
    pub const fn epoch(&self) -> OriginEpoch {
        self.anchor.epoch()
    }

    /// The previous endpoint of the last movement, if one was recorded.
    #[must_use]
    pub const fn sweep(&self) -> Option<SweptSegment> {
        self.anchor.sweep()
    }

    /// The local displacement one fixed tick applies.
    #[must_use]
    pub const fn per_tick_local(&self) -> [f32; 3] {
        self.per_tick_local
    }
}

/// Every spatial record of one session in a single origin frame.
///
/// This is the consumer of an origin rebase: [`rebase`](Self::rebase) moves
/// every subsystem's record into the new frame through one atomic
/// [`OriginShift`], while [`advance`](Self::advance) steps the whole set one
/// fixed tick at a time. Teardown is explicit ([`despawn`](Self::despawn) for
/// one record, [`teardown`](Self::teardown) for all) so a retry cannot inherit
/// a stale trigger or camera history.
#[derive(Clone, Debug, PartialEq)]
pub struct SpatialWorld {
    origin: WorldOrigin,
    next_id: u64,
    records: Vec<SpatialRecord>,
}

impl SpatialWorld {
    /// An empty world in `origin`'s frame.
    #[must_use]
    pub const fn new(origin: WorldOrigin) -> Self {
        Self {
            origin,
            next_id: 0,
            records: Vec::new(),
        }
    }

    /// The frame every record's local pose belongs to.
    #[must_use]
    pub const fn origin(&self) -> WorldOrigin {
        self.origin
    }

    /// The current origin epoch.
    #[must_use]
    pub const fn epoch(&self) -> OriginEpoch {
        self.origin.epoch()
    }

    /// Every record, in spawn order.
    #[must_use]
    pub fn records(&self) -> &[SpatialRecord] {
        &self.records
    }

    /// Number of records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the world holds no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The record with this id, if it still exists.
    #[must_use]
    pub fn record(&self, id: SpatialId) -> Option<&SpatialRecord> {
        self.records.iter().find(|record| record.id == id)
    }

    /// The world position of a record, if it still exists.
    #[must_use]
    pub fn world_position(&self, id: SpatialId) -> Option<WorldPosition> {
        self.record(id).map(SpatialRecord::world)
    }

    /// Adds a record for `subsystem` at `world`, moving by `per_tick_local`
    /// each fixed tick.
    ///
    /// # Errors
    ///
    /// [`SpatialError::NonFiniteDisplacement`] for a NaN/infinite delta,
    /// [`SpatialError::Origin`] when the position cannot be expressed in the
    /// current frame, or [`SpatialError::RecordIdExhausted`] when no fresh id
    /// remains. Nothing is added on error.
    pub fn spawn(
        &mut self,
        subsystem: SpatialSubsystem,
        world: WorldPosition,
        per_tick_local: [f32; 3],
    ) -> Result<SpatialId, SpatialError> {
        const AXES: [&str; 3] = ["x", "y", "z"];
        for (axis, value) in AXES.into_iter().zip(per_tick_local) {
            if !value.is_finite() {
                return Err(SpatialError::NonFiniteDisplacement { axis });
            }
        }
        let anchor = SpatialAnchor::new(&self.origin, world)?;
        let id = SpatialId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(SpatialError::RecordIdExhausted)?;
        self.records.push(SpatialRecord {
            id,
            subsystem,
            anchor,
            per_tick_local,
        });
        Ok(id)
    }

    /// Advances every record by `ticks` fixed ticks in the current frame.
    ///
    /// # Errors
    ///
    /// [`SpatialError::Origin`] if a step cannot be expressed in the frame; the
    /// error is propagated rather than swallowed, and a refused step aborts the
    /// caller's tick loop.
    pub fn advance(&mut self, ticks: u64) -> Result<(), SpatialError> {
        for _ in 0..ticks {
            for record in &mut self.records {
                record
                    .anchor
                    .advance_local(&self.origin, record.per_tick_local)?;
            }
        }
        Ok(())
    }

    /// Jumps one record to a new world position, discarding its swept segment
    /// (non-negotiable behavior 5). See [`SpatialAnchor::teleport`].
    ///
    /// # Errors
    ///
    /// [`SpatialError::UnknownRecord`] for an id that is not present, or
    /// [`SpatialError::Origin`] when the position cannot be expressed in the
    /// frame. Nothing is mutated on error.
    pub fn teleport(&mut self, id: SpatialId, world: WorldPosition) -> Result<(), SpatialError> {
        let origin = self.origin;
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(SpatialError::UnknownRecord { id })?;
        record.anchor.teleport(&origin, world)?;
        Ok(())
    }

    /// Removes one record; returns whether it existed. The id is never reused.
    pub fn despawn(&mut self, id: SpatialId) -> bool {
        let before = self.records.len();
        self.records.retain(|record| record.id != id);
        self.records.len() != before
    }

    /// Removes every record and returns how many were removed.
    pub fn teardown(&mut self) -> usize {
        let removed = self.records.len();
        self.records.clear();
        removed
    }

    /// Applies one origin rebase to every record atomically and adopts the new
    /// frame, returning its epoch.
    ///
    /// # Errors
    ///
    /// [`SpatialError::Origin`] when a record (or a swept endpoint) cannot be
    /// expressed in the new frame; on error nothing is mutated, not even the
    /// origin.
    pub fn rebase(&mut self, new_position: WorldPosition) -> Result<OriginEpoch, SpatialError> {
        let shift = OriginShift::rebase(self.origin, new_position)?;
        let mut anchors: Vec<SpatialAnchor> =
            self.records.iter().map(|record| record.anchor).collect();
        shift.apply(&mut anchors)?;
        for (record, anchor) in self.records.iter_mut().zip(anchors) {
            record.anchor = anchor;
        }
        self.origin = shift.to();
        Ok(self.origin.epoch())
    }

    /// Rebases when `policy` says a record has drifted past its limit,
    /// returning the new epoch when it did.
    ///
    /// The check runs once per fixed tick (the driver's loop calls it before
    /// each step), never once per render frame, so a rebase lands on the same
    /// tick at any frame rate (AC03).
    ///
    /// # Errors
    ///
    /// [`SpatialError::Origin`] from `rebase`; the policy's trigger is
    /// deterministic, so a refused rebase aborts the run instead of being
    /// retried at a different tick.
    pub fn rebase_if_needed(
        &mut self,
        policy: &RebasePolicy,
    ) -> Result<Option<OriginEpoch>, SpatialError> {
        let Some(id) = policy.trigger(self) else {
            return Ok(None);
        };
        let target = self
            .world_position(id)
            .ok_or(SpatialError::UnknownRecord { id })?;
        self.rebase(target).map(Some)
    }
}

/// The designed origin-rebase policy of [`FixedTickDriver`].
///
/// Which threshold the original game uses, and even whether it rebases at all,
/// is F16-D's measurement; the default limit here is a designed value, not
/// original behaviour, chosen well below the f32 spacing where local
/// coordinates would start to quantize.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RebasePolicy {
    limit_m: Option<f32>,
}

impl RebasePolicy {
    /// The default local-coordinate limit before the driver moves the origin,
    /// in metres.
    pub const DEFAULT_LIMIT_M: f32 = 512.0;

    /// A policy that rebases when any record's local component exceeds
    /// `limit_m` in magnitude.
    ///
    /// # Errors
    ///
    /// [`SpatialError::InvalidRebaseLimit`] when `limit_m` is not finite and
    /// strictly positive.
    pub fn at_limit(limit_m: f32) -> Result<Self, SpatialError> {
        if !limit_m.is_finite() || limit_m <= 0.0 {
            return Err(SpatialError::InvalidRebaseLimit { limit_m });
        }
        Ok(Self {
            limit_m: Some(limit_m),
        })
    }

    /// A policy that never rebases.
    #[must_use]
    pub const fn disabled() -> Self {
        Self { limit_m: None }
    }

    /// The configured limit, or `None` when rebasing is disabled.
    #[must_use]
    pub const fn limit_m(self) -> Option<f32> {
        self.limit_m
    }

    /// The id of the first record whose local pose lies outside the limit, or
    /// `None`. Records are checked in spawn order, so the choice is stable
    /// across runs.
    #[must_use]
    pub fn trigger(&self, world: &SpatialWorld) -> Option<SpatialId> {
        let limit = self.limit_m?;
        world.records().iter().find_map(|record| {
            if record
                .local()
                .to_array()
                .iter()
                .any(|component| component.abs() > limit)
            {
                Some(record.id())
            } else {
                None
            }
        })
    }
}

impl Default for RebasePolicy {
    fn default() -> Self {
        Self {
            limit_m: Some(Self::DEFAULT_LIMIT_M),
        }
    }
}

/// Why a [`FixedTickDriver`] frame was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum DriverError {
    /// The clock refused to advance (a wrapped counter).
    Time(TimeError),
    /// The spatial world refused a step or a rebase.
    Spatial(SpatialError),
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Time(error) => write!(f, "frame clock refused the frame: {error}"),
            Self::Spatial(error) => write!(f, "spatial world refused the frame: {error}"),
        }
    }
}

impl std::error::Error for DriverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Time(error) => Some(error),
            Self::Spatial(error) => Some(error),
        }
    }
}

impl From<TimeError> for DriverError {
    fn from(value: TimeError) -> Self {
        Self::Time(value)
    }
}

impl From<SpatialError> for DriverError {
    fn from(value: SpatialError) -> Self {
        Self::Spatial(value)
    }
}

/// The F16-C integration: one render frame's wall time becomes whole fixed
/// ticks (the `cs_sim::time` [`SimClock`], the producer) and every spatial
/// subsystem advances in the origin frame (the [`SpatialWorld`], the
/// consumer), with the rebase policy applied once per fixed tick.
///
/// A frame's wall delta only ever adds *whole fixed ticks* — it never becomes
/// a variable dt — so the same input over the same wall time yields the same
/// tick count and the same spatial state at 30, 60 or 144 render FPS (AC03).
/// An error is returned, never logged; the failed session is not reused, and
/// [`retry`](Self::retry) starts a fresh one.
#[derive(Clone, Debug, PartialEq)]
pub struct FixedTickDriver {
    policy: ClockPolicy,
    rate: TickRate,
    clock: SimClock,
    world: SpatialWorld,
    rebase: RebasePolicy,
    rebase_count: u64,
    first_rebase_tick: Option<Tick>,
    last_rebase_tick: Option<Tick>,
}

impl FixedTickDriver {
    /// A driver at tick 0 with an empty world in `origin`'s frame.
    #[must_use]
    pub fn new(
        policy: ClockPolicy,
        rate: TickRate,
        origin: WorldOrigin,
        rebase: RebasePolicy,
    ) -> Self {
        Self {
            policy,
            rate,
            clock: SimClock::new(policy, rate),
            world: SpatialWorld::new(origin),
            rebase,
            rebase_count: 0,
            first_rebase_tick: None,
            last_rebase_tick: None,
        }
    }

    /// The frame clock.
    #[must_use]
    pub const fn clock(&self) -> &SimClock {
        &self.clock
    }

    /// The spatial world.
    #[must_use]
    pub const fn world(&self) -> &SpatialWorld {
        &self.world
    }

    /// Mutable access, for spawning records between frames.
    pub fn world_mut(&mut self) -> &mut SpatialWorld {
        &mut self.world
    }

    /// The last committed tick.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.clock.tick()
    }

    /// The fixed rate this driver runs at.
    #[must_use]
    pub const fn rate(&self) -> TickRate {
        self.rate
    }

    /// The rebase policy.
    #[must_use]
    pub const fn rebase_policy(&self) -> RebasePolicy {
        self.rebase
    }

    /// How many rebases have happened.
    #[must_use]
    pub const fn rebase_count(&self) -> u64 {
        self.rebase_count
    }

    /// The tick of the first rebase, if any.
    #[must_use]
    pub const fn first_rebase_tick(&self) -> Option<Tick> {
        self.first_rebase_tick
    }

    /// The tick of the most recent rebase, if any.
    #[must_use]
    pub const fn last_rebase_tick(&self) -> Option<Tick> {
        self.last_rebase_tick
    }

    /// Pauses or resumes the session clock. A freezing clock's paused wall
    /// time produces no ticks (and no spatial movement) at all.
    pub fn set_paused(&mut self, paused: bool) {
        self.clock.set_paused(paused);
    }

    /// Advances the session by one render frame of `elapsed` wall time,
    /// returning the whole fixed ticks the frame produced.
    ///
    /// # Errors
    ///
    /// [`DriverError::Time`] when the clock refuses the frame, or
    /// [`DriverError::Spatial`] when a step or a rebase is refused. An error
    /// is terminal for the session; call [`retry`](Self::retry) to start a
    /// clean one.
    pub fn advance_frame(&mut self, elapsed: Duration) -> Result<u64, DriverError> {
        let ticks = self.clock.advance(elapsed)?;
        let first_tick = self.clock.tick().0 - ticks;
        for offset in 0..ticks {
            if self.world.rebase_if_needed(&self.rebase)?.is_some() {
                let tick = Tick(first_tick + offset);
                self.rebase_count += 1;
                self.first_rebase_tick.get_or_insert(tick);
                self.last_rebase_tick = Some(tick);
            }
            self.world.advance(1)?;
        }
        Ok(ticks)
    }

    /// Rebases the session to `new_position` immediately.
    ///
    /// # Errors
    ///
    /// [`DriverError::Spatial`] when a record cannot be expressed in the new
    /// frame; on error nothing is mutated.
    pub fn rebase(&mut self, new_position: WorldPosition) -> Result<OriginEpoch, DriverError> {
        Ok(self.world.rebase(new_position)?)
    }

    /// Teleports one record, discarding its swept segment.
    ///
    /// # Errors
    ///
    /// [`DriverError::Spatial`] for an unknown id or an unrepresentable
    /// position.
    pub fn teleport(&mut self, id: SpatialId, world: WorldPosition) -> Result<(), DriverError> {
        Ok(self.world.teleport(id, world)?)
    }

    /// Removes one record; returns whether it existed.
    pub fn despawn(&mut self, id: SpatialId) -> bool {
        self.world.despawn(id)
    }

    /// Starts a fresh session generation in `origin`'s frame: the clock is
    /// back at tick 0 and every record is gone, so no stale tick, origin epoch
    /// or trigger survives the retry. The caller respawns its authored initial
    /// state afterwards.
    pub fn retry(&mut self, origin: WorldOrigin) {
        self.clock = SimClock::new(self.policy, self.rate);
        self.world = SpatialWorld::new(origin);
        self.rebase_count = 0;
        self.first_rebase_tick = None;
        self.last_rebase_tick = None;
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

    /// F16-C: every subsystem a rebase must move lives in one [`SpatialWorld`],
    /// and one `rebase` converts all of them: world identity survives, each
    /// local cache is re-anchored in the new epoch, a swept segment is kept,
    /// and the origin only moves after every record converted.
    #[test]
    fn accept_f16_c_spatial_world_rebases_every_subsystem_atomically() {
        let before = origin(0.0, 0.0, 0.0, 0);
        let mut spatial = SpatialWorld::new(before);
        let mut ids = Vec::new();
        for (index, subsystem) in SpatialSubsystem::ALL.into_iter().enumerate() {
            let position = world(4096.0 + index as f64 * 16.0, 512.0, -2048.0);
            ids.push(
                spatial
                    .spawn(subsystem, position, [1.5, 0.0, 0.0])
                    .expect("finite spawn"),
            );
        }
        spatial.advance(3).expect("finite steps");
        let worlds: Vec<WorldPosition> = ids
            .iter()
            .map(|id| spatial.world_position(*id).expect("spawned id exists"))
            .collect();
        let sweep_before = spatial.record(ids[0]).expect("spawned id exists").sweep();
        assert!(sweep_before.is_some(), "a step records a swept segment");

        let epoch = spatial
            .rebase(world(512.0, 0.0, 0.25))
            .expect("every record converts");
        assert_eq!(epoch, OriginEpoch(1));
        assert_eq!(spatial.epoch(), OriginEpoch(1));
        assert_eq!(spatial.origin().position(), world(512.0, 0.0, 0.25));

        for (index, id) in ids.iter().enumerate() {
            let record = spatial.record(*id).expect("the record survives");
            assert_eq!(
                record.subsystem(),
                SpatialSubsystem::ALL[index],
                "the subsystem identity is kept"
            );
            assert_eq!(
                record.epoch(),
                OriginEpoch(1),
                "every subsystem converts into the new epoch"
            );
            assert_eq!(
                record.world(),
                worlds[index],
                "world identity survives the rebase"
            );
            let back = spatial
                .origin()
                .world_of(record.local())
                .expect("finite local sum");
            let tolerance = local_round_trip_tolerance_m(record.world(), record.local());
            for (actual, wanted) in back.to_array().into_iter().zip(record.world().to_array()) {
                assert!(
                    (actual - wanted).abs() <= tolerance,
                    "record {index}: the local cache must address the new frame \
                     (axis: {actual} != {wanted})"
                );
            }
        }

        let sweep_after = spatial.record(ids[0]).expect("record exists").sweep();
        assert_eq!(
            sweep_after.map(|segment| segment.from_world()),
            sweep_before.map(|segment| segment.from_world()),
            "a rebase keeps the swept segment's world endpoint"
        );
    }

    /// F16-C: a rebase that cannot express one record in the new frame is
    /// refused and leaves *every* record and the origin untouched, so no
    /// consumer can observe a half-rebased world.
    #[test]
    fn accept_f16_c_refused_spatial_rebase_changes_nothing() {
        let before = origin(0.0, 0.0, 0.0, 0);
        let mut spatial = SpatialWorld::new(before);
        spatial
            .spawn(
                SpatialSubsystem::Projectile,
                world(0.0, 0.0, 0.0),
                [0.0, 0.0, 0.0],
            )
            .expect("finite spawn");
        let snapshot = spatial.clone();

        assert_eq!(
            spatial.rebase(world(-1.0e40, 0.0, 0.0)),
            Err(SpatialError::Origin(OriginError::Space(
                SpaceError::NonFinite { field: "local.x" }
            ))),
            "an f32 overflow in the new frame must be refused, not produced as inf"
        );
        assert_eq!(
            spatial, snapshot,
            "a refused rebase must leave every record and the origin unchanged"
        );
        assert_eq!(spatial.epoch(), OriginEpoch(0));
    }

    /// F16-C teardown/retry: spawn validates its displacement, ids are unique
    /// and never reused, `despawn` removes one record and `teardown` clears the
    /// rest.
    #[test]
    fn accept_f16_c_spawn_validation_despawn_and_teardown() {
        assert_eq!(
            RebasePolicy::at_limit(0.0),
            Err(SpatialError::InvalidRebaseLimit { limit_m: 0.0 })
        );
        assert!(matches!(
            RebasePolicy::at_limit(f32::NAN),
            Err(SpatialError::InvalidRebaseLimit { .. })
        ));
        assert_eq!(RebasePolicy::disabled().limit_m(), None);
        assert_eq!(
            RebasePolicy::default().limit_m(),
            Some(RebasePolicy::DEFAULT_LIMIT_M)
        );

        let mut spatial = SpatialWorld::new(origin(0.0, 0.0, 0.0, 0));
        assert_eq!(
            spatial.spawn(
                SpatialSubsystem::Audio,
                world(0.0, 0.0, 0.0),
                [f32::NAN, 0.0, 0.0]
            ),
            Err(SpatialError::NonFiniteDisplacement { axis: "x" }),
            "a non-finite displacement is refused before a record exists"
        );
        assert!(spatial.is_empty());

        let first = spatial
            .spawn(
                SpatialSubsystem::Body,
                world(0.0, 0.0, 0.0),
                [1.0, 0.0, 0.0],
            )
            .expect("finite spawn");
        let second = spatial
            .spawn(
                SpatialSubsystem::Trigger,
                world(1.0, 0.0, 0.0),
                [0.0, 0.0, 0.0],
            )
            .expect("finite spawn");
        assert_ne!(first, second, "ids are unique");
        assert_eq!(spatial.len(), 2);

        assert!(spatial.despawn(first), "despawning an existing record");
        assert!(!spatial.despawn(first), "a removed id is never reused");
        assert_eq!(
            spatial.teleport(first, world(2.0, 0.0, 0.0)),
            Err(SpatialError::UnknownRecord { id: first })
        );
        assert_eq!(spatial.teardown(), 1);
        assert!(spatial.is_empty());
    }
}
