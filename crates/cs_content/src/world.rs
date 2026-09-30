//! World instances, sectors, collision roles and mission overlays
//! (F18-A, F18-B, F18-C).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-A`, `### F18-B` and `### F18-C`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **typed input/output contract** of the world feature —
//! nothing here builds a collider, opens a file or touches Bevy (`cs_content`
//! must never depend on Bevy or Avian). Stage F18-A declares what a world
//! importer produces and what the runtime consumes; stage F18-B implements the
//! importer and the static-collision generation against these records (adding
//! the condition vocabulary the load owns); stage F18-C adds the mission-local
//! overlay layer ([`MissionOverlay`], [`OverlayEffect`]) and the load's
//! declaration of which objects gameplay requires.
//!
//! # The records
//!
//! * [`WorldId`] wraps a [`ContentId`] of kind [`ContentKind::World`] — the
//!   namespace `IDENTITY-CONTENT` already reserves for "a world group or one
//!   variant of a world".
//! * [`SectorId`] and [`WorldObjectId`] are validated subordinate identities:
//!   they are addressed inside one [`WorldDefinition`] and are deliberately
//!   *not* catalog `ContentId`s, because the workspace's `ContentKind`
//!   vocabulary has no sector or instance kind and inventing one would claim a
//!   namespace that does not exist.
//! * [`Sector`] is one streaming unit: a stable id plus a validated
//!   [`Aabb`] in canonical meters. Membership is stored on the object, so an
//!   object may belong to several sectors and an object that belongs to none
//!   is *resident* (it is never streamed away) rather than an authoring error.
//! * [`WorldObjectInstance`] is one placed object: its stable id, its render
//!   mesh reference, its authored [`CanonicalTransform`], and three separate
//!   roles — [`Resolved<WorldCollisionRole>`] (does it block motion, or only
//!   report overlap), [`Resolved<WorldCollisionShape>`] (what geometry bounds
//!   it) and [`Resolved<SurfaceRole>`] (which gameplay surface rule its
//!   contacts follow).
//! * [`WorldDefinition`] is one authored world: its origin, its declared
//!   [`WorldBoundary`], its sectors and its object instances, validated once
//!   at construction.
//! * [`WorldInstance`] is **one concrete load** of a definition for a mission
//!   or session: the variant it applies, the object population it activates
//!   and the objects it starts damaged. It exists so F18 non-negotiable
//!   behavior 5 ("loading a reused world for another mission applies its
//!   authored variant, damage initial state and object population, not
//!   leftovers from the last run") is a *record*, not a convention: every load
//!   states its own population and damage, and
//!   [`WorldInstance::validate_against`] refuses an id the definition does not
//!   have.
//! * [`WorldObjectCondition`] is the two-value answer to "in what condition is
//!   this object, for the load that owns it". It is what makes F18
//!   non-negotiable behavior 3 ("object identity survives sector streaming")
//!   a statement about *state* as well as identity: a condition belongs to the
//!   load, never to a spawned entity, so unloading a sector cannot lose it and
//!   reloading it cannot invent a fresh one.
//! * [`MissionOverlay`] is one **mission-local** change a load carries: a
//!   trigger object, and the [`OverlayEffect`] crossing it applies to a target
//!   object. It is the record acceptance scenario AC03 ("open an authored door
//!   and verify both render and collision update once") is written against: the
//!   trigger is a [`WorldCollisionRole::Sensor`] volume, the effect is
//!   *designed* vocabulary, and the load — not the engine — decides that the
//!   effect has already been applied.
//! * [`WorldInstance::required_objects`] is the load's declaration of which
//!   objects gameplay cannot lose to streaming. It is the other half of F18
//!   non-negotiable behavior 3: an object outside render visibility is either
//!   still simulated (its sector is held) or correctly summarized (its state
//!   lives in the load, which is where the condition and the applied overlays
//!   are kept).
//!
//! # Known is known, unknown is unknown
//!
//! Every role and the boundary arrive as [`Resolved`]: a record whose evidence
//! never named a role carries [`Resolved::Unknown`] with its claim id and a
//! reason, never a guessed default. [`WorldDefinition`] exposes the
//! unresolved ones ([`WorldDefinition::unresolved_collision`],
//! [`WorldDefinition::unresolved_surface`], [`WorldDefinition::unresolved_shape`])
//! so a consumer can refuse them visibly instead of picking a side.
//!
//! # Designed vocabulary, not original data
//!
//! The id grammar, [`Aabb`], [`Sector`], [`WorldCollisionRole`],
//! [`WorldCollisionShape`], [`SurfaceRole`], [`WorldBoundary`],
//! [`WorldDefinition`], [`WorldInstance`], [`OverlayEffect`] and
//! [`MissionOverlay`] are **newly authored engine contract**. Which surface
//! classes the 2000 PC original distinguishes, whether it stores world
//! geometry per sector at all, how it identifies an object instance, what
//! changes a mission makes to a world object and what its boundary/ceiling
//! rules are are **unknown** until an evidence stage measures them; nothing in
//! this module claims to reproduce the original. The designed-vs-measured
//! split and the unknowns the F18 stages met are recorded in
//! `docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`,
//! `docs/findings/2026-09-30-f18-b-world-import-and-static-collision.md` and
//! `docs/findings/2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md`.

use std::collections::BTreeSet;
use std::fmt;

use cs_assets::install::sha256;
use cs_types::content::{ContentId, ContentIdError, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ContentHash;

use crate::scene::CanonicalTransform;

/// The longest subordinate world key this module accepts, in bytes.
pub const MAX_WORLD_KEY_LEN: usize = 128;

// ---------------------------------------------------------------- identity ---

/// Why a subordinate world key was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldKeyError {
    /// The key was empty or only separators.
    Empty,
    /// The key exceeded [`MAX_WORLD_KEY_LEN`] bytes.
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The key contained a character outside `[a-z0-9._-]` after ASCII
    /// lowercasing.
    BadCharacter {
        /// The offending character.
        ch: char,
    },
    /// The key had no ASCII alphanumeric character.
    NoAlphanumeric,
}

impl fmt::Display for WorldKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "world key must not be empty"),
            Self::TooLong { len } => {
                write!(f, "world key is {len} bytes, max is {MAX_WORLD_KEY_LEN}")
            }
            Self::BadCharacter { ch } => {
                write!(f, "world key contains disallowed character {ch:?}")
            }
            Self::NoAlphanumeric => write!(
                f,
                "world key must contain at least one ASCII alphanumeric character"
            ),
        }
    }
}

impl std::error::Error for WorldKeyError {}

/// Validates a subordinate world key against the shared `[a-z0-9._-]`
/// grammar (ASCII lowercased), so sector and object ids are comparable,
/// reportable and stable across parses.
fn validate_world_key(key: &str) -> Result<String, WorldKeyError> {
    let normalized = key.to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(WorldKeyError::Empty);
    }
    if normalized.len() > MAX_WORLD_KEY_LEN {
        return Err(WorldKeyError::TooLong {
            len: normalized.len(),
        });
    }
    if !normalized.chars().any(|ch| ch.is_ascii_alphanumeric()) {
        return Err(WorldKeyError::NoAlphanumeric);
    }
    for ch in normalized.chars() {
        if !ch.is_ascii_alphanumeric() && !matches!(ch, '.' | '_' | '-') {
            return Err(WorldKeyError::BadCharacter { ch });
        }
    }
    Ok(normalized)
}

/// The stable identity of one authored world: a world group or one variant of
/// it.
///
/// Wraps a [`ContentId`] of kind [`ContentKind::World`], the namespace
/// `IDENTITY-CONTENT` declares as "A world group or one variant of a world".
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldId(ContentId);

impl WorldId {
    /// Wraps a content id, refusing any id that is not a `world` id.
    ///
    /// # Errors
    ///
    /// [`WorldIdError::WrongKind`] when `id`'s namespace is not
    /// [`ContentKind::World`].
    pub fn new(id: ContentId) -> Result<Self, WorldIdError> {
        if id.kind() != ContentKind::World {
            return Err(WorldIdError::WrongKind { found: id.kind() });
        }
        Ok(Self(id))
    }

    /// Builds an id from a semantic source key, e.g. `c1` or `c1.mission_02`.
    ///
    /// # Errors
    ///
    /// [`WorldIdError::Key`] when the key breaks the `ContentId` grammar.
    pub fn from_key(key: &str) -> Result<Self, WorldIdError> {
        Ok(Self(ContentId::from_source(ContentKind::World, key)?))
    }

    /// The underlying content id.
    #[must_use]
    pub fn content_id(&self) -> &ContentId {
        &self.0
    }

    /// The normalized key, without the `world/` namespace.
    #[must_use]
    pub fn key(&self) -> &str {
        self.0.key()
    }

    /// The canonical `world/<key>` text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for WorldId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Why a [`WorldId`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldIdError {
    /// The content id named a namespace other than `world`.
    WrongKind {
        /// The namespace the rejected id actually carried.
        found: ContentKind,
    },
    /// The key broke the `ContentId` grammar.
    Key(ContentIdError),
}

impl fmt::Display for WorldIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongKind { found } => write!(
                f,
                "a world id must use the `world` namespace, found `{found}`"
            ),
            Self::Key(err) => write!(f, "invalid world key: {err}"),
        }
    }
}

impl std::error::Error for WorldIdError {}

impl From<ContentIdError> for WorldIdError {
    fn from(err: ContentIdError) -> Self {
        Self::Key(err)
    }
}

/// The stable identity of one sector inside a [`WorldDefinition`].
///
/// A sector is the unit world geometry streams by; its key is stable across
/// parses so a mission reference never depends on an enumeration index.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectorId(String);

impl SectorId {
    /// Validates and wraps a sector key.
    ///
    /// # Errors
    ///
    /// [`WorldKeyError`] naming what the grammar rejected.
    pub fn new(key: &str) -> Result<Self, WorldKeyError> {
        Ok(Self(validate_world_key(key)?))
    }

    /// The normalized key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The stable identity of one placed object inside a [`WorldDefinition`].
///
/// Object identity is what F18 non-negotiable behavior 3 requires to survive
/// sector streaming: an objective keeps this id whether or not its sector is
/// currently resident.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldObjectId(String);

impl WorldObjectId {
    /// Validates and wraps an object key.
    ///
    /// # Errors
    ///
    /// [`WorldKeyError`] naming what the grammar rejected.
    pub fn new(key: &str) -> Result<Self, WorldKeyError> {
        Ok(Self(validate_world_key(key)?))
    }

    /// The normalized key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorldObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ------------------------------------------------------------------ bounds ---

/// Why an [`Aabb`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum AabbError {
    /// A named component was NaN or infinite.
    NonFinite {
        /// The offending field, as the caller spelled it.
        field: &'static str,
    },
    /// A corner's `min` component exceeded its `max` component.
    Inverted {
        /// The axis whose extent came out backwards.
        axis: &'static str,
        /// The rejected minimum.
        min: f64,
        /// The rejected maximum.
        max: f64,
    },
}

impl fmt::Display for AabbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::Inverted { axis, min, max } => {
                write!(f, "{axis} extent is inverted: min {min} exceeds max {max}")
            }
        }
    }
}

impl std::error::Error for AabbError {}

/// An axis-aligned bounding box in canonical meters.
///
/// Validated once at construction: every component finite and `min <= max`
/// per axis, so a sector's extent can never silently become a NaN that
/// matches nothing (or everything).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    min: [f64; 3],
    max: [f64; 3],
}

impl Aabb {
    /// Validates a box from its two corners.
    ///
    /// # Errors
    ///
    /// [`AabbError::NonFinite`] naming the first non-finite component, or
    /// [`AabbError::Inverted`] when a minimum exceeds its maximum.
    pub fn try_new(min: [f64; 3], max: [f64; 3]) -> Result<Self, AabbError> {
        for (index, value) in min.into_iter().chain(max).enumerate() {
            if !value.is_finite() {
                return Err(AabbError::NonFinite {
                    field: BOX_FIELDS[index],
                });
            }
        }
        for (axis, (lo, hi)) in ["x", "y", "z"].into_iter().zip(min.into_iter().zip(max)) {
            if lo > hi {
                return Err(AabbError::Inverted {
                    axis,
                    min: lo,
                    max: hi,
                });
            }
        }
        Ok(Self { min, max })
    }

    /// The minimum corner.
    #[must_use]
    pub const fn min(&self) -> [f64; 3] {
        self.min
    }

    /// The maximum corner.
    #[must_use]
    pub const fn max(&self) -> [f64; 3] {
        self.max
    }

    /// Whether `point` lies inside or on the surface of the box.
    #[must_use]
    pub fn contains(&self, point: [f64; 3]) -> bool {
        (0..3).all(|axis| point[axis] >= self.min[axis] && point[axis] <= self.max[axis])
    }
}

const BOX_FIELDS: [&str; 6] = ["min[0]", "min[1]", "min[2]", "max[0]", "max[1]", "max[2]"];

impl fmt::Display for Aabb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({:.3}, {:.3}, {:.3})..({:.3}, {:.3}, {:.3})",
            self.min[0], self.min[1], self.min[2], self.max[0], self.max[1], self.max[2]
        )
    }
}

// ------------------------------------------------------------------ sector ---

/// One sector of a [`WorldDefinition`]: a stable identity plus the extent its
/// geometry streams by.
///
/// Sector membership is recorded on [`WorldObjectInstance::sectors`], not
/// here, so the same object can live in overlapping sectors and an object
/// with no sector stays resident. Which visibility/streaming metadata the
/// original stores per sector is unmeasured; the field is deliberately absent
/// rather than invented.
#[derive(Clone, Debug, PartialEq)]
pub struct Sector {
    id: SectorId,
    bounds: Aabb,
}

impl Sector {
    /// Builds a sector from a validated id and extent.
    #[must_use]
    pub const fn new(id: SectorId, bounds: Aabb) -> Self {
        Self { id, bounds }
    }

    /// The sector's stable id.
    #[must_use]
    pub fn id(&self) -> &SectorId {
        &self.id
    }

    /// The extent the sector covers, in canonical meters.
    #[must_use]
    pub const fn bounds(&self) -> Aabb {
        self.bounds
    }
}

// ------------------------------------------------------------------- roles ---

/// Which gameplay surface rule a world contact follows (F18 non-negotiable
/// behavior 2).
///
/// **Designed vocabulary.** The spec names water and ground explicitly, so
/// those two exist; the surface classes the original distinguishes, and the
/// contact rules they carry, are unmeasured and arrive as
/// [`Resolved::Unknown`] on the instance instead of a guessed default. Water
/// is a *role*, not a plane: nothing in this contract lets a water surface
/// invent collision geometry over legitimate low-flight areas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SurfaceRole {
    /// Solid ground a landing gear or a crash contacts.
    Ground,
    /// Water: a contact follows the water rule, not the ground rule.
    Water,
}

impl SurfaceRole {
    /// Every declared surface role, in a stable order.
    pub const ALL: [Self; 2] = [Self::Ground, Self::Water];

    /// The stable label used in ids and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ground => "ground",
            Self::Water => "water",
        }
    }

    /// Looks a surface role up by its label; `None` for an unknown spelling.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|role| role.label() == label)
    }
}

impl fmt::Display for SurfaceRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What a world object's geometry does to a body that reaches it.
///
/// **Designed vocabulary.** `None` and `Solid` mirror the participation split
/// `cs_content::scene::CollisionRole` already declares for scene nodes; the
/// `Sensor` case is the world-level "report an overlap, never block motion"
/// role that F23's `ShapeClass` distinguishes. Which roles the original
/// assigns to which authored geometry is unmeasured: a record that the
/// evidence never classified carries [`Resolved::Unknown`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorldCollisionRole {
    /// The instance is presented but never blocks or reports a contact.
    None,
    /// The instance bounds solid static geometry: bodies stop against it.
    Solid,
    /// The instance reports an overlap and never blocks motion.
    Sensor,
}

impl WorldCollisionRole {
    /// Every declared collision role, in a stable order.
    pub const ALL: [Self; 3] = [Self::None, Self::Solid, Self::Sensor];

    /// The stable label used in ids and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Solid => "solid",
            Self::Sensor => "sensor",
        }
    }

    /// Looks a collision role up by its label; `None` for an unknown spelling.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|role| role.label() == label)
    }

    /// Whether this role creates a collider at all.
    #[must_use]
    pub const fn creates_collider(self) -> bool {
        matches!(self, Self::Solid | Self::Sensor)
    }
}

impl fmt::Display for WorldCollisionRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What geometry bounds a world object's collider.
///
/// **Designed vocabulary.** [`WorldCollisionShape::Cuboid`] is what the
/// synthetic fixtures author; [`WorldCollisionShape::FromMesh`] is the
/// declared input for real geometry, and it is built from the *same* mesh
/// reference the instance's visual uses (F18 non-negotiable behavior 1:
/// shared provenance, possibly different verified simplifications, and never
/// a convex hull that closes a traversable opening). The shape names **no
/// mesh of its own** on purpose: the instance's `mesh` field is the single
/// reference both consumers resolve, which is what makes "one upload, one
/// asset, one set of triangles" checkable rather than merely intended.
/// F18-B builds this variant through `cs_app::world::spawn`; an instance whose
/// evidence never resolved a shape arrives as [`Resolved::Unknown`] and is
/// reported, never guessed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorldCollisionShape {
    /// An axis-aligned box in the instance's local frame, half extents in
    /// canonical meters.
    Cuboid {
        /// Half of each local dimension; every axis strictly positive.
        half_extents_m: [f64; 3],
    },
    /// A collider derived from the instance's own render mesh reference.
    /// Built by F18-B, not by this stage.
    FromMesh,
}

/// Why a [`WorldCollisionShape`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum CollisionShapeError {
    /// A named field was NaN or infinite.
    NonFinite {
        /// The offending field, as the caller spelled it.
        field: &'static str,
    },
    /// A box half extent was not strictly positive.
    NonPositiveHalfExtent {
        /// The axis whose half extent was zero or negative.
        axis: &'static str,
    },
}

impl fmt::Display for CollisionShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositiveHalfExtent { axis } => {
                write!(f, "half_extents_m[{axis}] must be greater than zero")
            }
        }
    }
}

impl std::error::Error for CollisionShapeError {}

impl WorldCollisionShape {
    /// A validated box in the instance's local frame.
    ///
    /// # Errors
    ///
    /// [`CollisionShapeError::NonFinite`] or
    /// [`CollisionShapeError::NonPositiveHalfExtent`].
    pub fn cuboid(half_extents_m: [f64; 3]) -> Result<Self, CollisionShapeError> {
        for (index, value) in half_extents_m.into_iter().enumerate() {
            if !value.is_finite() {
                return Err(CollisionShapeError::NonFinite {
                    field: HALF_EXTENT_FIELDS[index],
                });
            }
            if value <= 0.0 {
                return Err(CollisionShapeError::NonPositiveHalfExtent {
                    axis: ["x", "y", "z"][index],
                });
            }
        }
        Ok(Self::Cuboid { half_extents_m })
    }

    /// The validated box's half extents, when this is a box.
    #[must_use]
    pub const fn cuboid_half_extents(&self) -> Option<[f64; 3]> {
        match self {
            Self::Cuboid { half_extents_m } => Some(*half_extents_m),
            Self::FromMesh => None,
        }
    }
}

const HALF_EXTENT_FIELDS: [&str; 3] = [
    "half_extents_m[0]",
    "half_extents_m[1]",
    "half_extents_m[2]",
];

// ---------------------------------------------------------- world boundary ---

/// Why a [`WorldBoundary`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum BoundaryError {
    /// A named field was NaN or infinite.
    NonFinite {
        /// The offending field, as the caller spelled it.
        field: &'static str,
    },
    /// The floor sat above the ceiling.
    FloorAboveCeiling {
        /// The rejected floor height, in meters.
        floor_m: f64,
        /// The rejected ceiling height, in meters.
        ceiling_m: f64,
    },
}

impl fmt::Display for BoundaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::FloorAboveCeiling { floor_m, ceiling_m } => write!(
                f,
                "the floor ({floor_m} m) is above the ceiling ({ceiling_m} m)"
            ),
        }
    }
}

impl std::error::Error for BoundaryError {}

/// The world's floor, ceiling and lateral limits, in canonical meters.
///
/// Every limit is optional: an absent limit means *no rule*, never an
/// implicit wall (F18 non-negotiable behavior 4 — boundaries are data-driven
/// or a documented design fallback, and fidelity mode gets no arbitrary
/// invisible wall). A consumer that needs a limit the record does not carry
/// must say so, not invent one.
///
/// The original's own boundary and ceiling rules are unmeasured; the struct
/// is the typed home they will be read into.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldBoundary {
    floor_m: Option<f64>,
    ceiling_m: Option<f64>,
    lateral_m: Option<Aabb>,
}

impl WorldBoundary {
    /// Validates a boundary: finite limits, and a floor at or below its
    /// ceiling when both exist.
    ///
    /// # Errors
    ///
    /// [`BoundaryError::NonFinite`] or [`BoundaryError::FloorAboveCeiling`].
    pub fn try_new(
        floor_m: Option<f64>,
        ceiling_m: Option<f64>,
        lateral_m: Option<Aabb>,
    ) -> Result<Self, BoundaryError> {
        for (value, field) in [(floor_m, "floor_m"), (ceiling_m, "ceiling_m")] {
            if let Some(value) = value
                && !value.is_finite()
            {
                return Err(BoundaryError::NonFinite { field });
            }
        }
        if let (Some(floor), Some(ceiling)) = (floor_m, ceiling_m)
            && floor > ceiling
        {
            return Err(BoundaryError::FloorAboveCeiling {
                floor_m: floor,
                ceiling_m: ceiling,
            });
        }
        Ok(Self {
            floor_m,
            ceiling_m,
            lateral_m,
        })
    }

    /// The floor height, or `None` for no floor rule.
    #[must_use]
    pub const fn floor_m(&self) -> Option<f64> {
        self.floor_m
    }

    /// The ceiling height, or `None` for no ceiling rule.
    #[must_use]
    pub const fn ceiling_m(&self) -> Option<f64> {
        self.ceiling_m
    }

    /// The lateral extent, or `None` for no lateral rule.
    #[must_use]
    pub const fn lateral_m(&self) -> Option<Aabb> {
        self.lateral_m
    }

    /// Whether this boundary declares no rule at all.
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        self.floor_m.is_none() && self.ceiling_m.is_none() && self.lateral_m.is_none()
    }
}

impl Default for WorldBoundary {
    /// The empty boundary: no floor, no ceiling, no lateral rule — never an
    /// implicit wall.
    fn default() -> Self {
        Self {
            floor_m: None,
            ceiling_m: None,
            lateral_m: None,
        }
    }
}

// -------------------------------------------------------- object instances ---

/// One placed object of a [`WorldDefinition`].
///
/// The visual, the collision geometry and the surface rule are **three
/// separate roles over one record**: they share this record's id, transform
/// and provenance by construction, which is what "visual and collision meshes
/// share provenance and coordinate conversion" means at the record level for
/// F18 non-negotiable behavior 1: a consumer that builds both from the same
/// instance cannot make them disagree about *where* an opening is.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldObjectInstance {
    id: WorldObjectId,
    mesh: Resolved<ContentId>,
    transform: CanonicalTransform,
    collision: Resolved<WorldCollisionRole>,
    shape: Resolved<WorldCollisionShape>,
    surface: Resolved<SurfaceRole>,
    sectors: Vec<SectorId>,
    provenance: Provenance,
}

/// Why an object instance was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectInstanceError {
    /// The same sector was listed twice for one object.
    DuplicateSectorRef {
        /// The duplicated sector.
        sector: SectorId,
    },
}

impl fmt::Display for ObjectInstanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSectorRef { sector } => {
                write!(f, "sector {sector} is referenced twice by one object")
            }
        }
    }
}

impl std::error::Error for ObjectInstanceError {}

impl WorldObjectInstance {
    /// Builds an instance from its parts, validating the sector list.
    ///
    /// # Errors
    ///
    /// [`ObjectInstanceError::DuplicateSectorRef`] when the same sector is
    /// listed twice. Roles are *not* validated here: an unresolved role is a
    /// legal, reportable state of the record, not a construction failure.
    // Eight explicit parts rather than a builder: every one of them is a
    // field of the record, so an importer cannot forget to pass one and get a
    // defaulted role back.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        id: WorldObjectId,
        mesh: Resolved<ContentId>,
        transform: CanonicalTransform,
        collision: Resolved<WorldCollisionRole>,
        shape: Resolved<WorldCollisionShape>,
        surface: Resolved<SurfaceRole>,
        sectors: Vec<SectorId>,
        provenance: Provenance,
    ) -> Result<Self, ObjectInstanceError> {
        let mut seen = BTreeSet::new();
        for sector in &sectors {
            if !seen.insert(sector.clone()) {
                return Err(ObjectInstanceError::DuplicateSectorRef {
                    sector: sector.clone(),
                });
            }
        }
        Ok(Self {
            id,
            mesh,
            transform,
            collision,
            shape,
            surface,
            sectors,
            provenance,
        })
    }

    /// Builds an object that belongs to no sector: it stays resident while
    /// any part of the world is loaded, so "always visible" is stated
    /// explicitly instead of being the absence of a membership record.
    ///
    /// # Errors
    ///
    /// Never for the sector list — it is empty by construction — but the
    /// `Result` keeps both constructors' failure surface identical for
    /// callers that build instances in a loop.
    pub fn resident(
        id: WorldObjectId,
        mesh: Resolved<ContentId>,
        transform: CanonicalTransform,
        collision: Resolved<WorldCollisionRole>,
        shape: Resolved<WorldCollisionShape>,
        surface: Resolved<SurfaceRole>,
        provenance: Provenance,
    ) -> Result<Self, ObjectInstanceError> {
        Self::try_new(
            id,
            mesh,
            transform,
            collision,
            shape,
            surface,
            Vec::new(),
            provenance,
        )
    }

    /// The instance's stable id.
    #[must_use]
    pub fn id(&self) -> &WorldObjectId {
        &self.id
    }

    /// The render mesh this instance draws, or the explicit unknown its
    /// evidence left.
    #[must_use]
    pub fn mesh(&self) -> &Resolved<ContentId> {
        &self.mesh
    }

    /// The authored transform, in canonical meters: the one value the visual
    /// and the collision build both convert from.
    #[must_use]
    pub const fn transform(&self) -> &CanonicalTransform {
        &self.transform
    }

    /// The collision role, or the explicit unknown its evidence left.
    #[must_use]
    pub fn collision(&self) -> &Resolved<WorldCollisionRole> {
        &self.collision
    }

    /// The known collision role, or `None` when it is unknown.
    #[must_use]
    pub fn known_collision(&self) -> Option<WorldCollisionRole> {
        match &self.collision {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The collision shape, or the explicit unknown its evidence left.
    #[must_use]
    pub fn shape(&self) -> &Resolved<WorldCollisionShape> {
        &self.shape
    }

    /// The known collision shape, or `None` when it is unknown.
    #[must_use]
    pub fn known_shape(&self) -> Option<WorldCollisionShape> {
        match &self.shape {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The gameplay surface role, or the explicit unknown its evidence left.
    #[must_use]
    pub fn surface(&self) -> &Resolved<SurfaceRole> {
        &self.surface
    }

    /// The known surface role, or `None` when it is unknown.
    #[must_use]
    pub fn known_surface(&self) -> Option<SurfaceRole> {
        match &self.surface {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The sectors this instance belongs to, in supplied order; empty for a
    /// resident object.
    #[must_use]
    pub fn sectors(&self) -> &[SectorId] {
        &self.sectors
    }

    /// Whether this instance belongs to no sector and is therefore always
    /// resident.
    #[must_use]
    pub fn is_resident(&self) -> bool {
        self.sectors.is_empty()
    }

    /// The provenance of the record itself.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// ------------------------------------------------------------ world record ---

/// Why a [`WorldDefinition`] or a [`WorldInstance`] validation was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldError {
    /// Two sectors carried the same id.
    DuplicateSector {
        /// The duplicated sector id.
        id: SectorId,
    },
    /// Two objects carried the same id.
    DuplicateObject {
        /// The duplicated object id.
        id: WorldObjectId,
    },
    /// An object referenced a sector the definition does not declare.
    DanglingSectorRef {
        /// The object that referenced it.
        object: WorldObjectId,
        /// The sector that does not exist in this definition.
        sector: SectorId,
    },
    /// A load record was checked against a definition it does not read from.
    DefinitionMismatch {
        /// The definition the load record names.
        expected: WorldId,
        /// The definition it was actually checked against.
        found: WorldId,
    },
    /// An instance named an object the definition does not declare.
    UnknownInstanceObject {
        /// The unknown object id.
        object: WorldObjectId,
        /// Which collection of the instance named it.
        context: &'static str,
    },
    /// A load authored damage for an object its own population never activates.
    ///
    /// The object can never be spawned by that load, so the condition names a
    /// state nothing could ever show: exactly the silent gap F18 non-negotiable
    /// behavior 5 forbids.
    DamagedObjectNotActivated {
        /// The object the damage names.
        object: WorldObjectId,
    },
    /// A load instance declared an empty explicit population, which would
    /// load nothing while looking configured.
    EmptyPopulation,
    /// Two mission overlays share one trigger object.
    ///
    /// The trigger is the overlay's only identity — a load that declared two
    /// effects for the same volume would have to pick one silently, and the
    /// order it picked in would be the whole behaviour.
    DuplicateOverlayTrigger {
        /// The object two overlays were declared for.
        object: WorldObjectId,
    },
    /// A mission overlay's displacement was NaN or infinite.
    ///
    /// A non-finite offset is not a displacement: it is a value no runtime
    /// transform can hold, and applying it would move the target nowhere or
    /// everywhere.
    NonFiniteOverlayOffset {
        /// The overlay's target object.
        object: WorldObjectId,
        /// The axis that was not finite.
        axis: usize,
    },
    /// A mission overlay's trigger object is not a trigger.
    ///
    /// An overlay fires on an *overlap with a sensor volume*. A trigger whose
    /// record is solid is a wall the body is stopped by and never enters; a
    /// trigger whose role is an explicit unknown is a content gap. Either way
    /// the overlay could not fire as declared, and a load that declared one
    /// anyway would look configured while doing nothing.
    OverlayTriggerNotASensor {
        /// The object the overlay was declared for.
        object: WorldObjectId,
    },
    /// A mission overlay's trigger role is an explicit unknown, so it cannot
    /// be established that the trigger can be entered at all.
    OverlayTriggerRoleUnknown {
        /// The object whose role was never resolved.
        object: WorldObjectId,
    },
    /// A mission overlay names a trigger or target the load's population never
    /// activates, or a required object the population never activates.
    ///
    /// The object is never spawned by that load, so the overlay could never
    /// fire, the effect could never be seen, and the "gameplay-required"
    /// declaration would name a state nothing could show.
    InactiveLoadObject {
        /// The object that is never activated.
        object: WorldObjectId,
        /// What the object was named as.
        context: &'static str,
    },
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSector { id } => write!(f, "duplicate sector id `{id}`"),
            Self::DuplicateObject { id } => write!(f, "duplicate object id `{id}`"),
            Self::DanglingSectorRef { object, sector } => write!(
                f,
                "object `{object}` references undeclared sector `{sector}`"
            ),
            Self::DefinitionMismatch { expected, found } => write!(
                f,
                "the load record reads `{expected}` but was checked against `{found}`"
            ),
            Self::UnknownInstanceObject { object, context } => write!(
                f,
                "load instance names `{object}` in {context}, which the definition does not declare"
            ),
            Self::DamagedObjectNotActivated { object } => write!(
                f,
                "load instance starts `{object}` damaged, but its population never activates it"
            ),
            Self::EmptyPopulation => {
                write!(f, "an explicit world population must not be empty")
            }
            Self::DuplicateOverlayTrigger { object } => {
                write!(f, "two mission overlays share the trigger `{object}`")
            }
            Self::NonFiniteOverlayOffset { object, axis } => write!(
                f,
                "the mission overlay that displaces `{object}` has a non-finite offset on axis {axis}"
            ),
            Self::OverlayTriggerNotASensor { object } => write!(
                f,
                "mission overlay trigger `{object}` is not a sensor volume, so an \
                 overlap with it can never fire the overlay"
            ),
            Self::OverlayTriggerRoleUnknown { object } => write!(
                f,
                "mission overlay trigger `{object}` has an unresolved collision role, so it \
                 cannot be established that a body can enter it"
            ),
            Self::InactiveLoadObject { object, context } => write!(
                f,
                "the load names `{object}` in {context}, but its population never activates it"
            ),
        }
    }
}

impl std::error::Error for WorldError {}

/// One authored world: its sectors, its object instances, its boundary and
/// where the record came from.
///
/// Construction validates the whole record once — duplicate ids, dangling
/// sector references and a malformed boundary are refused at the door, so a
/// runtime consumer walks a record whose references already hold.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldDefinition {
    id: WorldId,
    origin: Origin,
    boundary: Resolved<WorldBoundary>,
    sectors: Vec<Sector>,
    objects: Vec<WorldObjectInstance>,
    provenance: Provenance,
}

impl WorldDefinition {
    /// Builds and validates a world definition.
    ///
    /// The boundary arrives as an already-validated [`WorldBoundary`] inside
    /// its [`Resolved`]; this constructor checks the *structure*: ids are
    /// unique and every sector reference resolves.
    ///
    /// # Errors
    ///
    /// [`WorldError::DuplicateSector`], [`WorldError::DuplicateObject`] or
    /// [`WorldError::DanglingSectorRef`], each naming the record that failed.
    pub fn try_new(
        id: WorldId,
        origin: Origin,
        boundary: Resolved<WorldBoundary>,
        sectors: Vec<Sector>,
        objects: Vec<WorldObjectInstance>,
        provenance: Provenance,
    ) -> Result<Self, WorldError> {
        let mut sector_ids = BTreeSet::new();
        for sector in &sectors {
            if !sector_ids.insert(sector.id().clone()) {
                return Err(WorldError::DuplicateSector {
                    id: sector.id().clone(),
                });
            }
        }
        let mut object_ids = BTreeSet::new();
        for object in &objects {
            if !object_ids.insert(object.id().clone()) {
                return Err(WorldError::DuplicateObject {
                    id: object.id().clone(),
                });
            }
            for sector in object.sectors() {
                if !sector_ids.contains(sector) {
                    return Err(WorldError::DanglingSectorRef {
                        object: object.id().clone(),
                        sector: sector.clone(),
                    });
                }
            }
        }
        Ok(Self {
            id,
            origin,
            boundary,
            sectors,
            objects,
            provenance,
        })
    }

    /// The world's identity.
    #[must_use]
    pub fn id(&self) -> &WorldId {
        &self.id
    }

    /// Where this definition came from: installation bytes, a synthetic
    /// fixture or designed content.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The declared boundary, or the explicit unknown its evidence left.
    #[must_use]
    pub fn boundary(&self) -> &Resolved<WorldBoundary> {
        &self.boundary
    }

    /// The known boundary, or `None` when the record left it unknown.
    #[must_use]
    pub fn known_boundary(&self) -> Option<&WorldBoundary> {
        match &self.boundary {
            Resolved::Known(known) => Some(&known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The declared sectors, in supplied order.
    #[must_use]
    pub fn sectors(&self) -> &[Sector] {
        &self.sectors
    }

    /// The object instances, in supplied order.
    #[must_use]
    pub fn objects(&self) -> &[WorldObjectInstance] {
        &self.objects
    }

    /// Looks a sector up by its id.
    #[must_use]
    pub fn sector(&self, id: &SectorId) -> Option<&Sector> {
        self.sectors.iter().find(|sector| sector.id() == id)
    }

    /// Looks an object up by its stable id.
    #[must_use]
    pub fn object(&self, id: &WorldObjectId) -> Option<&WorldObjectInstance> {
        self.objects.iter().find(|object| object.id() == id)
    }

    /// Every object that belongs to `sector`, in definition order.
    #[must_use]
    pub fn objects_in_sector(&self, sector: &SectorId) -> Vec<&WorldObjectInstance> {
        self.objects
            .iter()
            .filter(|object| object.sectors().contains(sector))
            .collect()
    }

    /// The objects that belong to no sector and therefore stay resident.
    #[must_use]
    pub fn resident_objects(&self) -> Vec<&WorldObjectInstance> {
        self.objects
            .iter()
            .filter(|object| object.is_resident())
            .collect()
    }

    /// The objects whose collision role the evidence never resolved, in
    /// definition order. A consumer must refuse these visibly, never pick a
    /// default.
    #[must_use]
    pub fn unresolved_collision(&self) -> Vec<&WorldObjectInstance> {
        self.objects
            .iter()
            .filter(|object| !object.collision().is_known())
            .collect()
    }

    /// The objects whose collision shape the evidence never resolved.
    #[must_use]
    pub fn unresolved_shape(&self) -> Vec<&WorldObjectInstance> {
        self.objects
            .iter()
            .filter(|object| !object.shape().is_known())
            .collect()
    }

    /// The objects whose gameplay surface role the evidence never resolved.
    #[must_use]
    pub fn unresolved_surface(&self) -> Vec<&WorldObjectInstance> {
        self.objects
            .iter()
            .filter(|object| !object.surface().is_known())
            .collect()
    }

    /// A canonical SHA-256 fingerprint of the definition's identity and
    /// structure: the world id, its declared boundary, every sector's id and
    /// bounds, and every object's id, mesh reference, transform, three roles
    /// and sector membership.
    ///
    /// Sectors, objects and each object's sector list are hashed **in id
    /// order**, so two definitions that describe the same world compare
    /// equal regardless of the order their records were supplied in (sector
    /// and object ids are unique in a definition, so id order is total).
    ///
    /// It fingerprints the *record*, not the source bytes, and it
    /// deliberately leaves `origin` and `provenance` out: those say where the
    /// record came from, not what it says. Like every other fingerprint in
    /// the workspace it is a [`ContentHash`] (64 lowercase hex characters),
    /// stable across Rust releases — not an implementation-defined hash.
    #[must_use]
    pub fn record_fingerprint(&self) -> ContentHash {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"cs/content/world/record/v1\0");
        push_text(&mut bytes, self.id.as_str());
        push_resolved(&mut bytes, &self.boundary, |bytes, boundary| {
            push_optional_f64(bytes, boundary.floor_m());
            push_optional_f64(bytes, boundary.ceiling_m());
            match boundary.lateral_m() {
                None => bytes.push(0),
                Some(bounds) => {
                    bytes.push(1);
                    push_aabb(bytes, &bounds);
                }
            }
        });

        let mut sectors: Vec<&Sector> = self.sectors.iter().collect();
        sectors.sort_by(|left, right| left.id().cmp(right.id()));
        bytes.extend_from_slice(&(sectors.len() as u32).to_le_bytes());
        for sector in sectors {
            push_text(&mut bytes, sector.id().as_str());
            push_aabb(&mut bytes, &sector.bounds());
        }

        let mut objects: Vec<&WorldObjectInstance> = self.objects.iter().collect();
        objects.sort_by(|left, right| left.id().cmp(right.id()));
        bytes.extend_from_slice(&(objects.len() as u32).to_le_bytes());
        for object in objects {
            push_text(&mut bytes, object.id().as_str());
            push_resolved(&mut bytes, object.mesh(), |bytes, id| {
                push_text(bytes, id.as_str())
            });
            push_resolved(&mut bytes, object.collision(), |bytes, role| {
                push_text(bytes, role.label())
            });
            push_resolved(&mut bytes, object.shape(), |bytes, shape| match shape {
                WorldCollisionShape::Cuboid { half_extents_m } => {
                    bytes.push(0);
                    for extent in half_extents_m {
                        push_f64(bytes, *extent);
                    }
                }
                WorldCollisionShape::FromMesh => bytes.push(1),
            });
            push_resolved(&mut bytes, object.surface(), |bytes, role| {
                push_text(bytes, role.label())
            });
            for row in object.transform().linear() {
                for cell in row {
                    push_f64(&mut bytes, cell);
                }
            }
            for cell in object.transform().translation() {
                push_f64(&mut bytes, cell);
            }
            let mut members: Vec<&SectorId> = object.sectors().iter().collect();
            members.sort();
            bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
            for member in members {
                push_text(&mut bytes, member.as_str());
            }
        }
        sha256(&bytes)
    }
}

/// Appends a length-free, NUL-terminated string: no byte sequence can be
/// mistaken for a terminator inside the next field.
fn push_text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(0);
}

fn push_f64(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn push_optional_f64(bytes: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => bytes.push(0),
        Some(value) => {
            bytes.push(1);
            push_f64(bytes, value);
        }
    }
}

fn push_aabb(bytes: &mut Vec<u8>, bounds: &Aabb) {
    for value in bounds.min().into_iter().chain(bounds.max()) {
        push_f64(bytes, value);
    }
}

/// Encodes a [`Resolved`] record: a `1` and the value, or a `0`, the claim
/// id and the reason an explicit unknown carries.
fn push_resolved<T>(
    bytes: &mut Vec<u8>,
    value: &Resolved<T>,
    push_value: impl FnOnce(&mut Vec<u8>, &T),
) {
    match value {
        Resolved::Known(known) => {
            bytes.push(1);
            push_value(bytes, &known.value);
        }
        Resolved::Unknown { claim_id, reason } => {
            bytes.push(0);
            push_text(bytes, claim_id.as_str());
            push_text(bytes, reason);
        }
    }
}

// ------------------------------------------------------------- load record ---

/// The condition one world object is in, for the load that owns it.
///
/// **Designed vocabulary, not original data.** F18 non-negotiable behavior 3
/// requires object *identity* to survive sector streaming, and behavior 5
/// requires a load to apply its authored "damage initial state". Both are
/// statements about a world object that outlives the entities that currently
/// present it, so the condition is a value the **load** owns — never a
/// component that despawns with a sector. This two-value vocabulary is the
/// smallest answer that lets a consumer tell "the condition the record
/// authored" from "a condition that accumulated since", which is exactly what
/// reloading a sector has to preserve (AC02).
///
/// Whether the original tracks per-object damage on world geometry at all, and
/// what a damaged world object *is* (destroyed, burning, merely dented) are
/// **unmeasured**; nothing here claims the original has this distinction. The
/// damage *rules* — integrity, hit attribution, what a hit removes — belong to
/// `cs_content::damage` and `cs_sim::damage`; this value only says which
/// condition an object is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WorldObjectCondition {
    /// Nothing has damaged the object since its load began: it is in the
    /// condition the load authored.
    Authored,
    /// The object is damaged — either because the load authored it damaged, or
    /// because it was damaged after the load began. The two are deliberately
    /// not distinguished: the source of the damage is the damage system's
    /// record, not the world record's.
    Damaged,
}

impl WorldObjectCondition {
    /// Every declared condition, in a stable order.
    pub const ALL: [Self; 2] = [Self::Authored, Self::Damaged];

    /// The stable label used in ids and reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Authored => "authored",
            Self::Damaged => "damaged",
        }
    }

    /// Looks a condition up by its label; `None` for an unknown spelling.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|role| role.label() == label)
    }

    /// Whether this condition means the object was damaged.
    #[must_use]
    pub const fn is_damaged(self) -> bool {
        matches!(self, Self::Damaged)
    }
}

impl fmt::Display for WorldObjectCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which objects a [`WorldInstance`] activates (F18 non-negotiable behavior
/// 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldPopulation {
    /// Every object the definition declares.
    AllAuthored,
    /// Exactly the named objects; anything else stays unloaded.
    Only(BTreeSet<WorldObjectId>),
}

/// One concrete load of a [`WorldDefinition`] for a mission or session.
///
/// This is what "the authored variant, damage initial state and object
/// population" of F18 non-negotiable behavior 5 *is*: a record. The previous
/// run of the same world cannot contribute anything to it, because nothing
/// lives outside this struct, and [`WorldInstance::validate_against`] refuses
/// an id the definition does not declare instead of dropping it quietly.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldInstance {
    definition: WorldId,
    variant: Resolved<WorldId>,
    population: WorldPopulation,
    initially_damaged: BTreeSet<WorldObjectId>,
    overlays: Vec<MissionOverlay>,
    required_objects: BTreeSet<WorldObjectId>,
    provenance: Provenance,
}

impl WorldInstance {
    /// Builds a load record.
    ///
    /// # Errors
    ///
    /// [`WorldError::EmptyPopulation`] when an explicit population names
    /// nothing.
    pub fn try_new(
        definition: WorldId,
        variant: Resolved<WorldId>,
        population: WorldPopulation,
        initially_damaged: BTreeSet<WorldObjectId>,
        provenance: Provenance,
    ) -> Result<Self, WorldError> {
        if let WorldPopulation::Only(ids) = &population
            && ids.is_empty()
        {
            return Err(WorldError::EmptyPopulation);
        }
        Ok(Self {
            definition,
            variant,
            population,
            initially_damaged,
            overlays: Vec::new(),
            required_objects: BTreeSet::new(),
            provenance,
        })
    }

    /// Adds this mission's **overlay layer**: the triggers that change the
    /// world, and the objects gameplay cannot lose to streaming.
    ///
    /// The two belong together because they are the two halves of one rule (F18
    /// non-negotiable behavior 3): an object gameplay needs must either stay
    /// simulated while it is out of render visibility — which is what naming it
    /// required holds for — or be correctly summarized when its sector is
    /// streamed away, which is the load keeping the effect it already applied.
    ///
    /// A load with no overlays and no required objects is not a degraded load:
    /// it is a world that never changes and never streams, which is exactly
    /// what F18-A's and F18-B's fixtures declare.
    ///
    /// # Errors
    ///
    /// [`WorldError::DuplicateOverlayTrigger`] when two overlays share a
    /// trigger, and [`WorldError::NonFiniteOverlayOffset`] when a displacement
    /// is not finite. Whether the objects exist, are activated and — for a
    /// trigger — are sensors is checked by
    /// [`WorldInstance::validate_against`], which is the only place the
    /// definition is available.
    pub fn with_mission_layer(
        mut self,
        overlays: Vec<MissionOverlay>,
        required_objects: BTreeSet<WorldObjectId>,
    ) -> Result<Self, WorldError> {
        let mut triggers = BTreeSet::new();
        for overlay in &overlays {
            if !triggers.insert(overlay.trigger().clone()) {
                return Err(WorldError::DuplicateOverlayTrigger {
                    object: overlay.trigger().clone(),
                });
            }
        }
        self.overlays = overlays;
        self.required_objects = required_objects;
        Ok(self)
    }

    /// The mission-local overlays this load declares, in declaration order.
    #[must_use]
    pub fn overlays(&self) -> &[MissionOverlay] {
        &self.overlays
    }

    /// The overlay whose trigger is `trigger`, when this load declares one.
    #[must_use]
    pub fn overlay_for(&self, trigger: &WorldObjectId) -> Option<&MissionOverlay> {
        self.overlays
            .iter()
            .find(|overlay| overlay.trigger() == trigger)
    }

    /// The objects gameplay requires, in stable order: the load declares these
    /// as ones streaming must not take away. The whole declaration, and the only
    /// way to read it — a per-object `is_required` here would be a second
    /// spelling of `required_objects().contains(..)`, not a second question.
    #[must_use]
    pub const fn required_objects(&self) -> &BTreeSet<WorldObjectId> {
        &self.required_objects
    }

    /// The definition this load reads from.
    #[must_use]
    pub fn definition(&self) -> &WorldId {
        &self.definition
    }

    /// The authored variant this load applies, or the explicit unknown the
    /// record left.
    #[must_use]
    pub fn variant(&self) -> &Resolved<WorldId> {
        &self.variant
    }

    /// The objects this load activates.
    #[must_use]
    pub fn population(&self) -> &WorldPopulation {
        &self.population
    }

    /// The objects this load starts damaged, in stable order.
    pub fn initially_damaged(&self) -> impl Iterator<Item = &WorldObjectId> {
        self.initially_damaged.iter()
    }

    /// The provenance of the load record itself.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Whether `object` is active in this load.
    #[must_use]
    pub fn activates(&self, object: &WorldObjectId) -> bool {
        match &self.population {
            WorldPopulation::AllAuthored => true,
            WorldPopulation::Only(ids) => ids.contains(object),
        }
    }

    /// The condition this load *authors* `object` in.
    ///
    /// Only the load's own initial damage set decides it, so a condition
    /// reached in a previous run of the same world cannot reach this one —
    /// there is nowhere for it to live. An object the load does not activate
    /// is [`WorldObjectCondition::Authored`] here, and
    /// [`WorldInstance::validate_against`] refuses a load that names an
    /// unactivated object as damaged, so the two can never disagree.
    #[must_use]
    pub fn initial_condition(&self, object: &WorldObjectId) -> WorldObjectCondition {
        if self.initially_damaged.contains(object) {
            WorldObjectCondition::Damaged
        } else {
            WorldObjectCondition::Authored
        }
    }

    /// Checks every id this load names against the definition it reads from.
    ///
    /// # Errors
    ///
    /// [`WorldError::DefinitionMismatch`] when the load record names a
    /// different world, [`WorldError::UnknownInstanceObject`] when a member of
    /// the population, of the initial-damage set, of the overlay layer or of
    /// the required set is not an object of `definition`,
    /// [`WorldError::DamagedObjectNotActivated`] when the load starts an object
    /// damaged that its population never activates,
    /// [`WorldError::InactiveLoadObject`] when an overlay's trigger, an
    /// overlay's target or a required object is outside the population,
    /// [`WorldError::OverlayTriggerRoleUnknown`] when a trigger's collision
    /// role is an explicit unknown, and
    /// [`WorldError::OverlayTriggerNotASensor`] when a trigger's role is
    /// resolved to anything but a sensor. The name says which collection
    /// failed, so a typo is reported instead of silently loading a different
    /// set, and an overlay that could never fire is refused at the door
    /// instead of loading as a configuration that does nothing.
    pub fn validate_against(&self, definition: &WorldDefinition) -> Result<(), WorldError> {
        if definition.id() != &self.definition {
            return Err(WorldError::DefinitionMismatch {
                expected: self.definition.clone(),
                found: definition.id().clone(),
            });
        }
        let ids: BTreeSet<&WorldObjectId> = definition
            .objects()
            .iter()
            .map(|object| object.id())
            .collect();
        let check = |object: &WorldObjectId, context: &'static str| {
            if ids.contains(object) {
                Ok(())
            } else {
                Err(WorldError::UnknownInstanceObject {
                    object: object.clone(),
                    context,
                })
            }
        };
        if let WorldPopulation::Only(members) = &self.population {
            for object in members {
                check(object, "the population")?;
            }
        }
        for object in &self.initially_damaged {
            check(object, "the initial damage set")?;
            if !self.activates(object) {
                return Err(WorldError::DamagedObjectNotActivated {
                    object: object.clone(),
                });
            }
        }
        for object in &self.required_objects {
            check(object, "the required object set")?;
            if !self.activates(object) {
                return Err(WorldError::InactiveLoadObject {
                    object: object.clone(),
                    context: "the required object set",
                });
            }
        }
        for overlay in &self.overlays {
            let trigger = overlay.trigger();
            check(trigger, "a mission overlay's trigger")?;
            let effect_target = overlay.effect().target();
            check(effect_target, "a mission overlay's effect target")?;
            for (object, context) in [
                (trigger, "a mission overlay's trigger"),
                (effect_target, "a mission overlay's effect target"),
            ] {
                if !self.activates(object) {
                    return Err(WorldError::InactiveLoadObject {
                        object: object.clone(),
                        context,
                    });
                }
            }
            match definition.object(trigger) {
                Some(record) => match record.collision() {
                    Resolved::Known(known) if known.value == WorldCollisionRole::Sensor => {}
                    Resolved::Known(_) => {
                        return Err(WorldError::OverlayTriggerNotASensor {
                            object: trigger.clone(),
                        });
                    }
                    Resolved::Unknown { .. } => {
                        return Err(WorldError::OverlayTriggerRoleUnknown {
                            object: trigger.clone(),
                        });
                    }
                },
                // Unreachable: `check` above refused an undeclared object.
                None => {
                    return Err(WorldError::UnknownInstanceObject {
                        object: trigger.clone(),
                        context: "a mission overlay's trigger",
                    });
                }
            }
        }
        Ok(())
    }
}

// ----------------------------------------------------------- mission overlay ---

/// What one mission-local overlay does to an authored object.
///
/// **Designed vocabulary.** The 2000 PC original's mission scripts name
/// objectives and triggers, and this repository's `cs_content::script` and
/// `missions/` sheets are where their *semantics* are measured (F06/F07
/// opcode classes, and the mission sheets themselves). Which changes a world
/// object undergoes, and how they are authored, is **unmeasured**; this
/// vocabulary is the smallest one the spec's own acceptance scenario needs —
/// acceptance scenario AC03 is *open an authored door*, and a door that moves
/// is the one change that lets both consumers be checked against each other:
/// the drawn geometry and the collision geometry are the same object, so
/// moving one without the other is visible.
#[derive(Clone, Debug, PartialEq)]
pub enum OverlayEffect {
    /// The target object's geometry is displaced by this translation, in
    /// canonical meters, **once**.
    ///
    /// "Once" is a property of the load, not of this value: the load records
    /// that it applied the effect, so a second crossing of the same trigger is
    /// not a second displacement. Which object the displacement belongs to —
    /// a door panel, a drawbridge, a cargo hatch — is the record's business
    /// and is not claimed to be the original's.
    Displace {
        /// The object whose geometry moves.
        target: WorldObjectId,
        /// The displacement, in canonical meters.
        offset_m: [f64; 3],
    },
}

impl OverlayEffect {
    /// The object this effect moves.
    #[must_use]
    pub const fn target(&self) -> &WorldObjectId {
        match self {
            Self::Displace { target, .. } => target,
        }
    }
}

/// One mission-local overlay: a trigger object, and the effect an actor
/// reaching it applies.
///
/// The trigger is an ordinary authored object — an instance of the same
/// [`WorldDefinition`] the rest of the world is built from — whose collision
/// role is [`WorldCollisionRole::Sensor`]. It is *not* a special "trigger"
/// kind, because the spec's own non-negotiable behavior 3 requires gameplay
/// state to survive streaming for ordinary world objects, and an overlay whose
/// trigger lived outside that machinery would be exactly the unstreamable
/// exception it forbids. [`WorldInstance::validate_against`] refuses an overlay
/// whose trigger is not a sensor, so the trigger's role is part of what the
/// load guarantees rather than a convention.
///
/// The *applier* is the runtime's business ([`cs_app::world`]), not this
/// module's: a record states what crossing a volume does, and the engine
/// decides how a body reaching it is noticed.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionOverlay {
    trigger: WorldObjectId,
    effect: OverlayEffect,
    provenance: Provenance,
}

impl MissionOverlay {
    /// Builds an overlay from a trigger and an effect.
    ///
    /// # Errors
    ///
    /// [`WorldError::NonFiniteOverlayOffset`] when a displacement is NaN or
    /// infinite — a value no runtime transform can hold, and a displacement
    /// that is not one.
    pub fn try_new(
        trigger: WorldObjectId,
        effect: OverlayEffect,
        provenance: Provenance,
    ) -> Result<Self, WorldError> {
        let OverlayEffect::Displace { target, offset_m } = &effect;
        for (axis, offset) in offset_m.iter().enumerate() {
            if !offset.is_finite() {
                return Err(WorldError::NonFiniteOverlayOffset {
                    object: target.clone(),
                    axis,
                });
            }
        }
        Ok(Self {
            trigger,
            effect,
            provenance,
        })
    }

    /// The object whose overlap fires this overlay.
    #[must_use]
    pub const fn trigger(&self) -> &WorldObjectId {
        &self.trigger
    }

    /// What this overlay does when it fires.
    #[must_use]
    pub const fn effect(&self) -> &OverlayEffect {
        &self.effect
    }

    /// The provenance of the overlay record itself.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// ------------------------------------------------- the world-group audit ---

/// The five opening classes the sheet's deliverable names, as an explicit
/// vocabulary.
///
/// `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md` says
/// "Preserve tunnels, arches, building openings, hangars and stunt passages".
/// This enum is those five nouns and nothing else. It is a **declared list to
/// look for**, never a classifier: no code in this workspace may decide that a
/// mesh is a tunnel, because a tunnel is a property of *placed* geometry
/// between two spaces, and placement is not decoded (see
/// [`TraversalBlocker::PlacementUndecoded`]). A class is either located by an
/// audit that had the facts, or it is [`StuntOpeningVerdict::Unlocated`] with
/// the blocker that stopped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OpeningClass {
    /// A passage through solid ground.
    Tunnel,
    /// A spanned opening.
    Arch,
    /// A doorway or window in a building shell.
    BuildingOpening,
    /// An aircraft hangar: an opening large enough to enter.
    Hangar,
    /// A stunt passage: an opening a flown route is meant to thread.
    StuntPassage,
}

impl OpeningClass {
    /// Every class, in the order the sheet names them. A report that iterates
    /// this list visits every class the deliverable names, so a class added
    /// here cannot be silently skipped by a report that forgot it.
    pub const ALL: [Self; 5] = [
        Self::Tunnel,
        Self::Arch,
        Self::BuildingOpening,
        Self::Hangar,
        Self::StuntPassage,
    ];

    /// Stable lowercase identifier.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Tunnel => "tunnel",
            Self::Arch => "arch",
            Self::BuildingOpening => "building_opening",
            Self::Hangar => "hangar",
            Self::StuntPassage => "stunt_passage",
        }
    }
}

impl fmt::Display for OpeningClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One world group the installation declares: the identity the audit uses, the
/// directory it was discovered in, and the two containers the group holds.
///
/// The two container spellings are **measured inputs**, never conventions: a
/// caller states the keys its session actually resolved, so a group whose
/// geometry archive has another name in the original is describable without
/// this module guessing a layout.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorldGroupRef {
    world: WorldId,
    directory: String,
    geometry_container: String,
    texture_archive: String,
    missions: Vec<String>,
}

impl WorldGroupRef {
    /// Builds one group row, refusing a blank mission label and a repeated one.
    ///
    /// An **empty** mission list is accepted and is not an error: production
    /// discovery reports a world group as a *directory* under the installation's
    /// `zbd` root, so a group the campaign walk does not mention is still a
    /// discovered group whose geometry exists, and dropping it — or inventing a
    /// mission to satisfy a rule — would be exactly the silent hole this record
    /// exists to prevent. [`Self::has_missions`] reports the empty case.
    ///
    /// # Errors
    ///
    /// [`WorldAuditError::BlankMissionLabel`] when a mission label is empty or
    /// only whitespace, and [`WorldAuditError::DuplicateMission`] when one label
    /// is repeated.
    pub fn new(
        world: WorldId,
        directory: impl Into<String>,
        geometry_container: impl Into<String>,
        texture_archive: impl Into<String>,
        mut missions: Vec<String>,
    ) -> Result<Self, WorldAuditError> {
        for (index, mission) in missions.iter().enumerate() {
            if mission.trim().is_empty() {
                return Err(WorldAuditError::BlankMissionLabel {
                    world: world.key().to_owned(),
                    index,
                });
            }
        }
        missions.sort();
        for pair in missions.windows(2) {
            if pair[0] == pair[1] {
                return Err(WorldAuditError::DuplicateMission {
                    world: world.key().to_owned(),
                    mission: pair[0].clone(),
                });
            }
        }
        Ok(Self {
            world,
            directory: directory.into(),
            geometry_container: geometry_container.into(),
            texture_archive: texture_archive.into(),
            missions,
        })
    }

    /// The group's stable identity.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        &self.world
    }

    /// The directory the installation spells the group with, e.g. `ZBD/c1c`.
    #[must_use]
    pub fn directory(&self) -> &str {
        &self.directory
    }

    /// The logical key of the container holding the group's stored geometry.
    #[must_use]
    pub fn geometry_container(&self) -> &str {
        &self.geometry_container
    }

    /// The logical key of the texture archive the group's materials resolve
    /// against.
    #[must_use]
    pub fn texture_archive(&self) -> &str {
        &self.texture_archive
    }

    /// The mission directories that live in this group, sorted. Empty when the
    /// campaign walk declares none in it, which is a fact about the
    /// installation and not a reason to skip the group.
    #[must_use]
    pub fn missions(&self) -> &[String] {
        &self.missions
    }

    /// Whether the campaign declares a mission in this group.
    #[must_use]
    pub fn has_missions(&self) -> bool {
        !self.missions.is_empty()
    }
}

/// What the survey established about where the group's stored meshes sit.
///
/// This is the single fact the traversal and opening audits turn on, so it is
/// a value with two honest variants rather than a boolean: "nobody decoded the
/// placement" and "the placement is decoded, and here is how many placed
/// objects there are" are different worlds, and a report must not be able to
/// confuse them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementSource {
    /// No production path decoded the container's node array, so no stored mesh
    /// has a position, orientation or scale in world space.
    ///
    /// The numbers are the container header's own: how many stored node records
    /// it declares and where the array starts. They are quoted so a reader can
    /// see how much is waiting behind the missing step.
    Undecoded {
        /// The header's `node_array_size`: stored node records in the container.
        stored_node_records: u32,
        /// The header's `nodes_offset`: where the node array starts.
        nodes_offset: u32,
    },
    /// The placement was decoded: `placed_objects` meshes carry a transform.
    Decoded {
        /// How many stored meshes a node placed.
        placed_objects: usize,
    },
}

impl PlacementSource {
    /// How many stored node records the container's placement section declares,
    /// or `None` once it has been decoded (the array is no longer waiting).
    #[must_use]
    pub const fn stored_node_records(&self) -> Option<u32> {
        match self {
            Self::Undecoded {
                stored_node_records,
                ..
            } => Some(*stored_node_records),
            Self::Decoded { .. } => None,
        }
    }

    /// Whether a placement transform is available for the group's meshes.
    #[must_use]
    pub const fn is_decoded(&self) -> bool {
        matches!(self, Self::Decoded { .. })
    }
}

/// What the production upload adapter did with one representative mesh.
///
/// The verdict is **measured by the adapter itself**, not predicted: the audit
/// hands the render mesh to `cs_app::render::bevy_mesh::upload_groups` and
/// records the answer. That matters because the answer is not always yes — the
/// adapter refuses a buffer it cannot fill from stored values alone (a normal
/// stored on some of a group's vertices and not others), and a world group whose
/// largest meshes are in that state cannot be presented at all. A census that
/// reported only triangles and vertices would read as "presentable" for a mesh
/// the world load would refuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadVerdict {
    /// The adapter uploaded every material group.
    Uploaded {
        /// How many material groups it drew, one draw each.
        groups: usize,
        /// Vertices it uploaded, after per-group compaction.
        vertices: usize,
        /// Triangles it uploaded, degenerate ones included.
        triangles: usize,
    },
    /// The adapter refused a material group, with its own reason verbatim.
    Refused {
        /// The material group that would not upload.
        material_group: usize,
        /// The adapter's message, verbatim.
        reason: String,
    },
}

impl UploadVerdict {
    /// Whether the mesh went through the upload adapter.
    #[must_use]
    pub const fn is_uploaded(&self) -> bool {
        matches!(self, Self::Uploaded { .. })
    }
}

impl fmt::Display for UploadVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uploaded {
                groups,
                vertices,
                triangles,
            } => write!(
                f,
                "uploaded as {groups} group(s), {vertices} vertices and {triangles} triangles"
            ),
            Self::Refused {
                material_group,
                reason,
            } => write!(f, "material group {material_group} refused: {reason}"),
        }
    }
}

/// One measured mesh of a world group, chosen by the audit's declared rule.
///
/// The bounds are in the container's **stored units**, which are not metres
/// and whose scale is unmeasured: `cs_content::mesh` applies no scale to
/// stored positions and nothing in the workspace has established the original's
/// world-vertex unit. A consumer must not read a number here as a length.
#[derive(Clone, Debug, PartialEq)]
pub struct RepresentativeGeometry {
    /// The mesh's array index in the container.
    pub mesh_index: u32,
    /// Triangles the mesh draws, degenerate ones included.
    pub triangles: usize,
    /// Render vertices the upload holds.
    pub vertices: usize,
    /// Distinct stored material groups the mesh's polygons carry.
    pub material_groups: usize,
    /// Lowest stored corner over every axis, in stored units.
    pub stored_min: [f64; 3],
    /// Highest stored corner over every axis, in stored units.
    pub stored_max: [f64; 3],
    /// SHA-256 that identifies this mesh: its container key and its array index,
    /// hashed together. A per-mesh digest over the stored span would have to
    /// keep the whole container alive, and the pair already names the geometry
    /// exactly — index 12 of `c1` is not index 12 of `c5`.
    pub fingerprint: ContentHash,
    /// What the production upload adapter did with it.
    pub upload: UploadVerdict,
}

impl RepresentativeGeometry {
    /// The stored extent of this mesh along one axis, in stored units.
    #[must_use]
    pub fn stored_extent(&self, axis: usize) -> f64 {
        self.stored_max[axis] - self.stored_min[axis]
    }

    /// The largest stored extent over the three axes, in stored units. The
    /// capture camera's framing distance is derived from this, so the value is
    /// a documented part of the render contract rather than a private choice.
    #[must_use]
    pub fn stored_radius(&self) -> f64 {
        (0..3)
            .map(|axis| self.stored_extent(axis))
            .fold(0.0_f64, f64::max)
    }
}

/// The counted numbers one survey read out of a world group's container.
///
/// Split from [`WorldGroupCensus`] so the assembly reads as the two things it
/// is: counts, and the facts that decide whether a route can be stated at all.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupFacts {
    /// The container the counts came from.
    pub container_key: String,
    /// SHA-256 of the whole container file, from production discovery.
    pub container_sha256: String,
    /// Array slots the container has, absent stubs included.
    pub mesh_slots: usize,
    /// Slots that stored a mesh.
    pub present_meshes: usize,
    /// `polygon_count` summed over the present stored mesh records.
    pub declared_faces: u64,
    /// Triangles the group's stored faces draw, degenerate ones excluded.
    pub drawn_triangles: u64,
    /// Stored faces that reach no drawable triangle.
    pub missing_faces: u64,
    /// Distinct stored texture names the group's material records name.
    pub texture_names: usize,
    /// Of those, the ones bound to exactly one stored origin.
    pub bound_texture_names: usize,
    /// Stored polygons that carry more than one material group.
    pub multi_material_group_polygons: usize,
}

impl GroupFacts {
    /// Whether the container stored at least one mesh. A group whose geometry
    /// container stores none has no geometry to visit, and the survey reports
    /// that as [`WorldGroupBlocker::NoGeometry`] rather than as a census of
    /// zeroes.
    #[must_use]
    pub const fn has_geometry(&self) -> bool {
        self.present_meshes > 0
    }
}

/// What one survey established about one world group.
///
/// A census is **measured**: every number in it came from reading the group's
/// own container, and the two fields that decide the traversal verdict
/// ([`Self::placement`] and [`Self::routes`]) say what the survey did and did
/// not establish rather than asserting a fact about the original.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldGroupCensus {
    world: WorldId,
    container_key: String,
    container_sha256: String,
    mesh_slots: usize,
    present_meshes: usize,
    declared_faces: u64,
    drawn_triangles: u64,
    missing_faces: u64,
    texture_names: usize,
    bound_texture_names: usize,
    multi_material_group_polygons: usize,
    placement: PlacementSource,
    vertex_scale_to_m: Option<f64>,
    representative: Vec<RepresentativeGeometry>,
    routes: Vec<TraversalRoute>,
    openings: Vec<StuntOpening>,
}

impl WorldGroupCensus {
    /// Assembles one measured census from the counts a survey read and the facts
    /// it established.
    ///
    /// `routes` and `openings` are **not** checked here: a census that claims a
    /// route while the facts it needs are missing is a real state, and the
    /// audit's job is to name it ([`WorldAuditGap::RouteWithoutFacts`]) rather
    /// than to make it unconstructible.
    ///
    /// # Errors
    ///
    /// [`WorldAuditError::NonFiniteVertexScale`] when a scale is NaN or
    /// infinite, and [`WorldAuditError::NonFiniteStoredCorner`] when a
    /// representative mesh carries a stored corner no render vertex can hold.
    /// Both are values a reader would otherwise compare and find silently wrong.
    pub fn new(
        world: WorldId,
        facts: GroupFacts,
        placement: PlacementSource,
        vertex_scale_to_m: Option<f64>,
        representative: Vec<RepresentativeGeometry>,
        routes: Vec<TraversalRoute>,
        openings: Vec<StuntOpening>,
    ) -> Result<Self, WorldAuditError> {
        if let Some(scale) = vertex_scale_to_m
            && !scale.is_finite()
        {
            return Err(WorldAuditError::NonFiniteVertexScale {
                world: world.key().to_owned(),
                scale,
            });
        }
        for mesh in &representative {
            for (axis, corner) in mesh
                .stored_min
                .iter()
                .chain(mesh.stored_max.iter())
                .enumerate()
            {
                if !corner.is_finite() {
                    return Err(WorldAuditError::NonFiniteStoredCorner {
                        world: world.key().to_owned(),
                        mesh_index: mesh.mesh_index,
                        axis,
                    });
                }
            }
        }
        Ok(Self {
            world,
            container_key: facts.container_key,
            container_sha256: facts.container_sha256,
            mesh_slots: facts.mesh_slots,
            present_meshes: facts.present_meshes,
            declared_faces: facts.declared_faces,
            drawn_triangles: facts.drawn_triangles,
            missing_faces: facts.missing_faces,
            texture_names: facts.texture_names,
            bound_texture_names: facts.bound_texture_names,
            multi_material_group_polygons: facts.multi_material_group_polygons,
            placement,
            vertex_scale_to_m,
            representative,
            routes,
            openings,
        })
    }

    /// The group's identity this census is about.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        &self.world
    }

    /// The container the geometry was read from.
    #[must_use]
    pub fn container_key(&self) -> &str {
        &self.container_key
    }

    /// SHA-256 of the whole container file, from production discovery.
    #[must_use]
    pub fn container_sha256(&self) -> &str {
        &self.container_sha256
    }

    /// Array slots the container has, absent stubs included.
    #[must_use]
    pub const fn mesh_slots(&self) -> usize {
        self.mesh_slots
    }

    /// Slots that stored a mesh.
    #[must_use]
    pub const fn present_meshes(&self) -> usize {
        self.present_meshes
    }

    /// `polygon_count` summed over the present stored mesh records.
    #[must_use]
    pub const fn declared_faces(&self) -> u64 {
        self.declared_faces
    }

    /// Triangles the group's stored faces draw, degenerate ones excluded.
    #[must_use]
    pub const fn drawn_triangles(&self) -> u64 {
        self.drawn_triangles
    }

    /// Stored faces that reach no drawable triangle.
    #[must_use]
    pub const fn missing_faces(&self) -> u64 {
        self.missing_faces
    }

    /// Distinct stored texture names the group's material records name.
    #[must_use]
    pub const fn texture_names(&self) -> usize {
        self.texture_names
    }

    /// Of those, the ones the audit bound to exactly one stored origin.
    #[must_use]
    pub const fn bound_texture_names(&self) -> usize {
        self.bound_texture_names
    }

    /// Stored polygons that carry more than one material group.
    #[must_use]
    pub const fn multi_material_group_polygons(&self) -> usize {
        self.multi_material_group_polygons
    }

    /// What the survey established about the group's placement.
    #[must_use]
    pub const fn placement(&self) -> PlacementSource {
        self.placement
    }

    /// The factor from the container's stored vertex units to canonical metres,
    /// or `None` while it is unmeasured.
    #[must_use]
    pub const fn vertex_scale_to_m(&self) -> Option<f64> {
        self.vertex_scale_to_m
    }

    /// The meshes the audit chose as this group's representative geometry, in
    /// the order it chose them.
    #[must_use]
    pub fn representative(&self) -> &[RepresentativeGeometry] {
        &self.representative
    }

    /// Of the representative meshes, the ones the upload adapter accepted.
    pub fn uploadable(&self) -> impl Iterator<Item = &RepresentativeGeometry> + '_ {
        self.representative
            .iter()
            .filter(|mesh| mesh.upload.is_uploaded())
    }

    /// Of the representative meshes, the ones the upload adapter refused.
    pub fn refused(&self) -> impl Iterator<Item = &RepresentativeGeometry> + '_ {
        self.representative
            .iter()
            .filter(|mesh| !mesh.upload.is_uploaded())
    }

    /// Of the representative meshes, how many the upload adapter refused.
    #[must_use]
    pub fn refused_representatives(&self) -> usize {
        self.refused().count()
    }

    /// The traversal routes the survey established. Empty whenever
    /// [`Self::placement`] is undecoded or [`Self::vertex_scale_to_m`] is
    /// `None`; the audit refuses that combination rather than reading it.
    #[must_use]
    pub fn routes(&self) -> &[TraversalRoute] {
        &self.routes
    }

    /// The stunt-critical openings the survey located. Same rule as
    /// [`Self::routes`].
    #[must_use]
    pub fn openings(&self) -> &[StuntOpening] {
        &self.openings
    }
}

/// One route through a world group, between two authored points.
///
/// The route is a **value the survey produced**, not a graph this module
/// searches for: how a route is found, and whether the original game routes an
/// aircraft at all, is unmeasured. What the audit can honestly check is the
/// bookkeeping — a route needs a placement, a unit scale and at least one
/// opening, and none of those is invented here.
#[derive(Clone, Debug, PartialEq)]
pub struct TraversalRoute {
    /// The route's stable key inside the group.
    pub route: String,
    /// Where the route starts, in canonical metres.
    pub from_m: [f64; 3],
    /// Where the route ends, in canonical metres.
    pub to_m: [f64; 3],
    /// The narrowest clearance the survey measured along it, in canonical
    /// metres, or `None` when nothing measured one.
    pub clearance_m: Option<f64>,
    /// The openings the route threads, as `(class, mesh index)` pairs.
    pub openings: Vec<(OpeningClass, u32)>,
}

/// One located stunt-critical opening of a world group.
#[derive(Clone, Debug, PartialEq)]
pub struct StuntOpening {
    /// Which class of opening this is.
    pub class: OpeningClass,
    /// The stored mesh that carries the opening's geometry.
    pub mesh_index: u32,
    /// The opening's narrowest measured extent, in canonical metres, or `None`
    /// when nothing measured one.
    pub clearance_m: Option<f64>,
}

/// Why a traversal route or a stunt-critical opening could not be stated.
#[derive(Clone, Debug, PartialEq)]
pub enum TraversalBlocker {
    /// The group's stored meshes have no position, orientation or scale in
    /// world space, so no opening can be located and no route can be measured.
    ///
    /// The numbers are the container header's own.
    PlacementUndecoded {
        /// The group awaiting a placement.
        world: WorldId,
        /// The header's `node_array_size`.
        stored_node_records: u32,
        /// The header's `nodes_offset`.
        nodes_offset: u32,
    },
    /// The container's stored vertex unit is unmeasured, so even a decoded
    /// placement yields no length: a clearance in metres cannot be computed
    /// from a stored extent.
    VertexScaleUnmeasured {
        /// The group awaiting a measured scale.
        world: WorldId,
        /// The largest stored extent the survey did measure, in stored units,
        /// so the missing factor has a magnitude to be missing from.
        largest_stored_extent: f64,
    },
}

impl TraversalBlocker {
    /// The group this blocker is about.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        match self {
            Self::PlacementUndecoded { world, .. } | Self::VertexScaleUnmeasured { world, .. } => {
                world
            }
        }
    }
}

impl fmt::Display for TraversalBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlacementUndecoded {
                world,
                stored_node_records,
                nodes_offset,
            } => write!(
                f,
                "{world} declares {stored_node_records} stored node records at offset \
                 {nodes_offset}, and none of them is decoded: no stored mesh has a position, \
                 so no opening can be located and no traversal route can be measured"
            ),
            Self::VertexScaleUnmeasured {
                world,
                largest_stored_extent,
            } => write!(
                f,
                "{world} stores a largest measured extent of {largest_stored_extent} in \
                 unmeasured vertex units, and no stored-unit-to-meter scale has been \
                 established, so no clearance can be stated in metres"
            ),
        }
    }
}

/// What the audit found for one world's opening classes.
#[derive(Clone, Debug, PartialEq)]
pub struct StuntOpeningAudit {
    world: WorldId,
    located: Vec<StuntOpening>,
    unlocated: Vec<OpeningClass>,
}

impl StuntOpeningAudit {
    /// The group this audit is about.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        &self.world
    }

    /// The openings that were located, in the order the census listed them.
    #[must_use]
    pub fn located(&self) -> &[StuntOpening] {
        &self.located
    }

    /// The classes nothing located, in [`OpeningClass::ALL`] order.
    #[must_use]
    pub fn unlocated(&self) -> &[OpeningClass] {
        &self.unlocated
    }

    /// Whether every class the sheet names was located.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unlocated.is_empty() && !self.located.is_empty()
    }
}

/// One group's verdict inside a [`WorldGroupAuditReport`].
#[derive(Clone, Debug, PartialEq)]
pub struct WorldGroupVerdict {
    group: WorldGroupRef,
    census: Option<WorldGroupCensus>,
    blocker: Option<WorldGroupBlocker>,
    openings: Vec<StuntOpeningAudit>,
    traversal_blockers: Vec<TraversalBlocker>,
    gaps: Vec<WorldAuditGap>,
}

impl WorldGroupVerdict {
    /// The group this verdict is about.
    #[must_use]
    pub const fn group(&self) -> &WorldGroupRef {
        &self.group
    }

    /// The measured census, or `None` when the survey could not produce one.
    #[must_use]
    pub const fn census(&self) -> Option<&WorldGroupCensus> {
        self.census.as_ref()
    }

    /// Why no census exists.
    #[must_use]
    pub const fn blocker(&self) -> Option<&WorldGroupBlocker> {
        self.blocker.as_ref()
    }

    /// The traversal verdict for each of the sheet's opening classes.
    #[must_use]
    pub fn openings(&self) -> &[StuntOpeningAudit] {
        &self.openings
    }

    /// What stopped the traversal routes, one entry per missing fact.
    #[must_use]
    pub fn traversal_blockers(&self) -> &[TraversalBlocker] {
        &self.traversal_blockers
    }

    /// The shortfalls found inside a census the audit *could* read.
    #[must_use]
    pub fn gaps(&self) -> &[WorldAuditGap] {
        &self.gaps
    }

    /// Whether the traversal routes were measured.
    #[must_use]
    pub fn routes_measured(&self) -> bool {
        self.traversal_blockers.is_empty()
    }

    /// The routes, empty while [`Self::routes_measured`] is false.
    #[must_use]
    pub fn routes(&self) -> &[TraversalRoute] {
        self.census
            .as_ref()
            .map_or(&[][..], |census| census.routes())
    }

    /// Whether the geometry side of this group was read at all.
    #[must_use]
    pub fn is_visited(&self) -> bool {
        self.census.is_some()
    }
}

/// Why one world group could not be surveyed at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldGroupBlocker {
    /// The group's geometry container could not be read.
    GeometryUnreadable {
        /// The group that could not be surveyed.
        world: WorldId,
        /// The container key that failed.
        container: String,
        /// The reader's own message, verbatim.
        reason: String,
    },
    /// The group's geometry container holds no stored mesh at all.
    NoGeometry {
        /// The empty group.
        world: WorldId,
        /// Array slots the container has, absent stubs included.
        slots: usize,
    },
}

impl WorldGroupBlocker {
    /// The group this blocker is about.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        match self {
            Self::GeometryUnreadable { world, .. } | Self::NoGeometry { world, .. } => world,
        }
    }
}

impl fmt::Display for WorldGroupBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GeometryUnreadable {
                world,
                container,
                reason,
            } => write!(f, "{world}: {container} could not be read: {reason}"),
            Self::NoGeometry { world, slots } => write!(
                f,
                "{world}: its geometry container has {slots} mesh slots and stores no mesh in \
                 any of them"
            ),
        }
    }
}

/// A shortfall the audit found inside a census it *could* read.
///
/// Each variant names the affected content and the missing fact, so a gap can
/// be filed as a follow-up rather than discovered later as a hole in a claim.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldAuditGap {
    /// The census claims routes or openings while the placement or the unit
    /// scale it needs is missing. A route is not a smaller claim than a
    /// placement: without one it is a number with no referent.
    RouteWithoutFacts {
        /// The group whose census contradicts itself.
        world: WorldId,
        /// How many routes the census listed.
        routes: usize,
        /// How many openings the census located.
        openings: usize,
    },
    /// The placement and the unit scale are both established, and the group
    /// still states no route. A group that measured cleanly and reported
    /// nothing has been measured for the wrong thing.
    NoRouteMeasured {
        /// The group that produced no route.
        world: WorldId,
        /// How many placed objects the placement source reported.
        placed_objects: usize,
    },
    /// The census names a group its row does not declare, so the measured
    /// numbers belong to some other installation's world.
    CensusGroupMismatch {
        /// The group the row declared.
        declared: WorldId,
        /// The group the census is about.
        measured: WorldId,
    },
}

impl fmt::Display for WorldAuditGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RouteWithoutFacts {
                world,
                routes,
                openings,
            } => write!(
                f,
                "{world} states {routes} traversal routes and {openings} located openings while \
                 the facts they need are missing: a route without a placement has no referent"
            ),
            Self::NoRouteMeasured {
                world,
                placed_objects,
            } => write!(
                f,
                "{world} placed {placed_objects} objects and measured the unit scale, and still \
                 states no traversal route"
            ),
            Self::CensusGroupMismatch { declared, measured } => write!(
                f,
                "the audit row declares {declared} but the census is about {measured}"
            ),
        }
    }
}

/// Why a [`WorldGroupAudit`] was refused at construction.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldAuditError {
    /// Two rows declare the same world group.
    DuplicateGroup {
        /// The repeated group.
        world: String,
    },
    /// A mission label is empty or only whitespace.
    BlankMissionLabel {
        /// The group the label was given for.
        world: String,
        /// Its position in the supplied list.
        index: usize,
    },
    /// One mission is named twice in the same group row.
    DuplicateMission {
        /// The group that names it twice.
        world: String,
        /// The repeated mission.
        mission: String,
    },
    /// A declared stored-unit-to-meter scale is NaN or infinite.
    NonFiniteVertexScale {
        /// The group the scale was declared for.
        world: String,
        /// The offending value.
        scale: f64,
    },
    /// A representative mesh carries a stored corner that is NaN or infinite.
    NonFiniteStoredCorner {
        /// The group the mesh came from.
        world: String,
        /// The mesh's array index.
        mesh_index: u32,
        /// Which axis of the bound the offending corner is on.
        axis: usize,
    },
}

impl fmt::Display for WorldAuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateGroup { world } => {
                write!(f, "two audit rows declare the world group {world:?}")
            }
            Self::BlankMissionLabel { world, index } => {
                write!(
                    f,
                    "world group {world:?} has a blank mission label at index {index}"
                )
            }
            Self::DuplicateMission { world, mission } => {
                write!(
                    f,
                    "world group {world:?} declares mission {mission:?} twice"
                )
            }
            Self::NonFiniteVertexScale { world, scale } => write!(
                f,
                "world group {world:?} declares a stored-unit-to-meter scale of {scale}, which is \
                 not a length"
            ),
            Self::NonFiniteStoredCorner {
                world,
                mesh_index,
                axis,
            } => write!(
                f,
                "world group {world:?} mesh {mesh_index} has a non-finite stored corner on axis \
                 {axis}"
            ),
        }
    }
}

impl std::error::Error for WorldAuditError {}

/// The declared world-group audit: which groups exist, and what a survey of
/// each one established.
///
/// The **declared** half and the **measured** half are separate on purpose.
/// [`Self::audit`] takes the declared rows and a `survey_of` seam, so a caller
/// that can measure a group hands over a
/// [`WorldGroupCensus`] and a caller that cannot hands over a
/// [`WorldGroupBlocker`] carrying the measured facts. "Blocked" and "measured"
/// are then the same verdict with a different input rather than two different
/// reports, so the stage that supplies the missing half changes no audit code.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldGroupAudit {
    groups: Vec<WorldGroupRef>,
}

impl WorldGroupAudit {
    /// Collects the declared group rows, refusing contradictions.
    ///
    /// # Errors
    ///
    /// [`WorldAuditError::DuplicateGroup`] when two rows declare the same world
    /// group, which would make one group's census count twice.
    pub fn new(groups: Vec<WorldGroupRef>) -> Result<Self, WorldAuditError> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for group in &groups {
            if !seen.insert(group.world().key().to_owned()) {
                return Err(WorldAuditError::DuplicateGroup {
                    world: group.world().key().to_owned(),
                });
            }
        }
        Ok(Self { groups })
    }

    /// The declared rows, in supplied order.
    #[must_use]
    pub fn groups(&self) -> &[WorldGroupRef] {
        &self.groups
    }

    /// Visits every declared group, taking each one's measurement from
    /// `survey_of`.
    ///
    /// `survey_of` is asked **once per group**, for the row the group names, and
    /// the report it produces is used for that group only: a census about
    /// another group is [`WorldAuditGap::CensusGroupMismatch`], never quietly
    /// filed under the row that was asked. A survey that names a group no row
    /// declares is asked for nothing and appears nowhere.
    pub fn audit<F>(&self, mut survey_of: F) -> WorldGroupAuditReport
    where
        F: FnMut(&WorldGroupRef) -> Result<WorldGroupCensus, WorldGroupBlocker>,
    {
        let groups = self
            .groups
            .iter()
            .map(|group| match survey_of(group) {
                Ok(census) => census_verdict(group, census),
                Err(blocker) => WorldGroupVerdict {
                    group: group.clone(),
                    census: None,
                    blocker: Some(blocker),
                    openings: Vec::new(),
                    traversal_blockers: Vec::new(),
                    gaps: Vec::new(),
                },
            })
            .collect();
        WorldGroupAuditReport { groups }
    }
}

/// Turns one measured census into one group's verdict.
fn census_verdict(group: &WorldGroupRef, census: WorldGroupCensus) -> WorldGroupVerdict {
    let mut gaps = Vec::new();
    if census.world() != group.world() {
        gaps.push(WorldAuditGap::CensusGroupMismatch {
            declared: group.world().clone(),
            measured: census.world().clone(),
        });
    }

    // The two facts a route needs, in the order they are checked. Both are
    // recorded even when both are missing, because a report that names one cause
    // and hides the other is a report a reader has to re-run to complete.
    let mut traversal_blockers = Vec::new();
    if let PlacementSource::Undecoded {
        stored_node_records,
        nodes_offset,
    } = census.placement()
    {
        traversal_blockers.push(TraversalBlocker::PlacementUndecoded {
            world: group.world().clone(),
            stored_node_records,
            nodes_offset,
        });
    }
    if census.vertex_scale_to_m().is_none() {
        let largest = census
            .representative()
            .iter()
            .map(RepresentativeGeometry::stored_radius)
            .fold(0.0_f64, f64::max);
        traversal_blockers.push(TraversalBlocker::VertexScaleUnmeasured {
            world: group.world().clone(),
            largest_stored_extent: largest,
        });
    }

    let facts_established = traversal_blockers.is_empty();
    if !facts_established && !(census.routes().is_empty() && census.openings().is_empty()) {
        gaps.push(WorldAuditGap::RouteWithoutFacts {
            world: group.world().clone(),
            routes: census.routes().len(),
            openings: census.openings().len(),
        });
    }
    if facts_established && census.routes().is_empty() {
        gaps.push(WorldAuditGap::NoRouteMeasured {
            world: group.world().clone(),
            placed_objects: match census.placement() {
                PlacementSource::Decoded { placed_objects } => placed_objects,
                PlacementSource::Undecoded { .. } => 0,
            },
        });
    }

    // The opening audit visits every class the sheet names, whether or not
    // anything located it, so a report can always be asked "was a hangar
    // looked for?" and get an answer.
    //
    // There is deliberately **no** "class outside the vocabulary" check here.
    // It was written once and could never fire: `located` is filtered by
    // `opening.class == *class` over `OpeningClass::ALL`, and
    // `StuntOpening::class` is a closed enum, so an unknown class is not
    // representable in the first place. The gap variant is gone rather than
    // left as unreachable production code with no test that can reach it.
    let openings = OpeningClass::ALL
        .iter()
        .map(|class| {
            let located: Vec<StuntOpening> = census
                .openings()
                .iter()
                .filter(|opening| opening.class == *class)
                .cloned()
                .collect();
            let unlocated = if located.is_empty() {
                vec![*class]
            } else {
                Vec::new()
            };
            StuntOpeningAudit {
                world: group.world().clone(),
                located,
                unlocated,
            }
        })
        .collect();

    WorldGroupVerdict {
        group: group.clone(),
        census: Some(census),
        blocker: None,
        openings,
        traversal_blockers,
        gaps,
    }
}

/// Every group's verdict: the F18-D acceptance scenario's report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldGroupAuditReport {
    groups: Vec<WorldGroupVerdict>,
}

impl WorldGroupAuditReport {
    /// Every declared group, in declared order.
    #[must_use]
    pub fn groups(&self) -> &[WorldGroupVerdict] {
        &self.groups
    }

    /// The groups whose geometry was read.
    pub fn visited(&self) -> impl Iterator<Item = &WorldGroupVerdict> + '_ {
        self.groups.iter().filter(|audit| audit.is_visited())
    }

    /// The groups whose geometry could not be read.
    pub fn blocked(&self) -> impl Iterator<Item = &WorldGroupVerdict> + '_ {
        self.groups.iter().filter(|audit| !audit.is_visited())
    }

    /// The groups whose traversal routes were measured.
    pub fn routed(&self) -> impl Iterator<Item = &WorldGroupVerdict> + '_ {
        self.groups
            .iter()
            .filter(|audit| audit.routes_measured() && !audit.routes().is_empty())
    }

    /// How many blockers the report holds: one per unreadable group plus every
    /// missing traversal fact and every gap.
    #[must_use]
    pub fn blocker_count(&self) -> usize {
        self.groups
            .iter()
            .filter(|audit| audit.blocker.is_some())
            .count()
            + self
                .groups
                .iter()
                .map(|audit| audit.traversal_blockers().len())
                .sum::<usize>()
            + self.gap_count()
    }

    /// How many gaps the report holds.
    #[must_use]
    pub fn gap_count(&self) -> usize {
        self.groups.iter().map(|audit| audit.gaps().len()).sum()
    }

    /// The total number of stored meshes read across every visited group.
    #[must_use]
    pub fn present_mesh_count(&self) -> u64 {
        self.visited()
            .filter_map(|audit| audit.census())
            .map(|census| census.present_meshes() as u64)
            .sum()
    }

    /// The total number of drawable triangles read across every visited group.
    #[must_use]
    pub fn drawn_triangle_count(&self) -> u64 {
        self.visited()
            .filter_map(|audit| audit.census())
            .map(|census| census.drawn_triangles())
            .sum()
    }

    /// Whether the audit is complete: every group visited, every route
    /// measured, every opening class located and nothing missing anywhere.
    ///
    /// Deliberately strict. An audit of no group read nothing and is **not** a
    /// pass, so an empty report is incomplete; so is a report whose traversal
    /// verdicts are all blocked, which is the honest result over an
    /// installation whose world placement is not decoded.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.groups.is_empty()
            && self.visited().count() == self.groups.len()
            && self.routed().count() == self.groups.len()
            && self
                .groups
                .iter()
                .all(|audit| audit.openings().iter().all(StuntOpeningAudit::is_complete))
            && self.blocker_count() == 0
    }

    /// Whether the audit looked at no group at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

// ------------------------------------------------------------------- tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_grammar_rejects_bad_keys_and_normalizes_good_ones() {
        assert_eq!(
            SectorId::new(""),
            Err(WorldKeyError::Empty),
            "an empty key must be refused"
        );
        assert_eq!(
            WorldObjectId::new("has/slash"),
            Err(WorldKeyError::BadCharacter { ch: '/' }),
            "a path separator must be refused: ids are not paths"
        );
        assert_eq!(
            WorldObjectId::new("..."),
            Err(WorldKeyError::NoAlphanumeric),
            "a key with no alphanumeric character has no identity"
        );
        assert_eq!(
            SectorId::new("Approach").expect("uppercase is normalized"),
            SectorId::new("approach").expect("lowercase is valid"),
            "keys must compare equal after ASCII lowercasing"
        );
    }
}
