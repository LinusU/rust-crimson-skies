//! World instances, sectors and collision roles (F18-A, F18-B).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **typed input/output contract** of the world feature —
//! nothing here builds a collider, opens a file or touches Bevy (`cs_content`
//! must never depend on Bevy or Avian). Stage F18-A declares what a world
//! importer produces and what the runtime consumes; stage F18-B implements the
//! importer and the static-collision generation against these records (adding
//! the condition vocabulary the load owns), and F18-C adds mission overlays and
//! streaming policy.
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
//! [`WorldDefinition`] and [`WorldInstance`] are **newly authored engine
//! contract**. Which surface classes the 2000 PC original distinguishes,
//! whether it stores world geometry per sector at all, how it identifies an
//! object instance and what its boundary/ceiling rules are are **unknown**
//! until an evidence stage measures them; nothing in this module claims to
//! reproduce the original. The designed-vs-measured split and the unknowns
//! this stage met are recorded in
//! `docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`.

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
            provenance,
        })
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
    /// different world, or [`WorldError::UnknownInstanceObject`] when a
    /// member of the population or of the initial-damage set is not an
    /// object of `definition`. The name says which collection failed, so a
    /// typo is reported instead of silently loading a different set.
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
        Ok(())
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
