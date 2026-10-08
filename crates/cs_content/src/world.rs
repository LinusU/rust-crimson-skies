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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_assets::install::sha256;
use cs_formats::gamez::{
    GameZNodes, NODE_TYPE_OBJECT3D, NODE_TYPE_WORLD, NodeKind as StoredNodeKind, RawNode,
    RawObject3dData, WORLD_DATA_BYTES, WORLD_PARTITION_BYTES, WORLD_PARTITION_VALUE_BYTES,
};
use cs_types::content::{
    ContentId, ContentIdError, ContentKind, Known, Origin, Provenance, Resolved,
};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::space::SpaceError;

use crate::coordinates::{AngleUnit, CalibratedQuantity, RotationSense, SourceAdapter};
use crate::scene::{
    BindingMap, CanonicalTransform, GameZSceneError, MeshSlot, ParsedNode, SceneError, SceneGraph,
    parsed_nodes_from_gamez,
};

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
/// between two spaces, and although placement is decoded and the unit measured
/// (F18-E), no measured rule yet classifies one (task #732). A class is either
/// located by an
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
    /// The survey produced no decoded placement for the container's node array,
    /// so no stored mesh has a position, orientation or scale in world space.
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
/// The bounds are in the container's **stored units**: `cs_content::mesh`
/// applies no scale to stored positions. The stored unit is measured — one
/// unit is the metre (task #677, `observed_tool`; consumed by F18-E) — but
/// this type deliberately carries the stored numbers, so a consumer must not
/// read a number here as a converted length.
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
    unlocated_openings: Vec<UnlocatedOpening>,
    route_search: RouteSearch,
}

impl WorldGroupCensus {
    /// Assembles one measured census from the counts a survey read and the facts
    /// it established.
    ///
    /// `routes`, `openings` and `unlocated_openings` are **not** checked here:
    /// a census that claims a route while the facts it needs are missing is a
    /// real state, and the audit's job is to name it
    /// ([`WorldAuditGap::RouteWithoutFacts`]) rather than to make it
    /// unconstructible. What *is* refused is an unlocated class with no
    /// measurement behind it ([`WorldAuditError::BlankOpeningReason`]), because
    /// a reason-less unlocated class is the silent zero the audit exists to
    /// prevent.
    ///
    /// # Errors
    ///
    /// [`WorldAuditError::NonFiniteVertexScale`] when a scale is NaN or
    /// infinite, [`WorldAuditError::NonFiniteStoredCorner`] when a
    /// representative mesh carries a stored corner no render vertex can hold,
    /// and [`WorldAuditError::BlankOpeningReason`] when one entry of
    /// `unlocated_openings` carries no measurement.
    /// The first two are values a reader would otherwise compare and find
    /// silently wrong.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        world: WorldId,
        facts: GroupFacts,
        placement: PlacementSource,
        vertex_scale_to_m: Option<f64>,
        representative: Vec<RepresentativeGeometry>,
        routes: Vec<TraversalRoute>,
        openings: Vec<StuntOpening>,
        unlocated_openings: Vec<UnlocatedOpening>,
        route_search: RouteSearch,
    ) -> Result<Self, WorldAuditError> {
        for row in &unlocated_openings {
            if row.measured().trim().is_empty() {
                return Err(WorldAuditError::BlankOpeningReason {
                    class: row.class().code().to_owned(),
                });
            }
        }
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
            unlocated_openings,
            route_search,
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
    /// or `None` when the survey carries none. The retail GameZ unit is
    /// measured — one unit is the metre (task #677) — so an audit over the
    /// owner's installation reports `Some(1.0)`.
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

    /// The measured reason each class the survey searched and did **not**
    /// locate carries, one row per class it searched.
    ///
    /// A class that is not in this list and not in [`Self::openings`] was never
    /// searched, and the audit reports [`OPENING_SEARCH_UNSUPPLIED`] for it
    /// rather than nothing — an unlocated class always reaches a reader with a
    /// reason.
    #[must_use]
    pub fn unlocated_openings(&self) -> &[UnlocatedOpening] {
        &self.unlocated_openings
    }

    /// The measured verdict about traversal routes, consulted by the audit when
    /// [`Self::routes`] is empty.
    #[must_use]
    pub const fn route_search(&self) -> &RouteSearch {
        &self.route_search
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

/// Why one opening class was **not** located in one world group, with the
/// measurement that says so.
///
/// This is the audit's answer to "why is there no hangar here?", and it exists
/// because the alternative — a class reported unlocated with no statement at
/// all — is the silent zero the F18 sheet's acceptance criteria forbid. Two
/// shapes of answer are both legitimate and both are carried verbatim in
/// [`Self::measured`]:
///
/// * *the corpus holds none of this class*: the survey applied its measured
///   rule and nothing matched, with the rule and what it searched named;
/// * *the search itself could not be taken*: the measurement failed or was
///   never applied here, with what stopped it named.
///
/// Either way the text is never empty, so a reader who sees the class
/// unlocated sees a reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnlocatedOpening {
    /// The class nothing located.
    class: OpeningClass,
    /// What was searched and what the corpus holds, as measured; never empty.
    measured: String,
}

impl UnlocatedOpening {
    /// Records one unlocated class with the measurement behind it.
    ///
    /// # Errors
    ///
    /// [`WorldAuditError::BlankOpeningReason`] when `measured` is empty or only
    /// whitespace: an unlocated class with no reason is exactly the silent
    /// zero this record exists to prevent, so it is refused at the boundary
    /// rather than printed later as an empty string.
    pub fn new(class: OpeningClass, measured: impl Into<String>) -> Result<Self, WorldAuditError> {
        let measured = measured.into();
        if measured.trim().is_empty() {
            return Err(WorldAuditError::BlankOpeningReason {
                class: class.code().to_owned(),
            });
        }
        Ok(Self { class, measured })
    }

    /// The class nothing located.
    #[must_use]
    pub const fn class(&self) -> OpeningClass {
        self.class
    }

    /// The measurement that says why, verbatim and never empty.
    pub fn measured(&self) -> &str {
        &self.measured
    }
}

/// The reason a class carries when the census that produced it carries no
/// measured opening search at all.
///
/// Stated once so a test can assert against it rather than against a message
/// it re-spelled: a caller that hands the audit a census without searching for
/// openings must still get a reason per unlocated class, and this is the one
/// that says the search never happened.
pub const OPENING_SEARCH_UNSUPPLIED: &str = "this census carries no measured opening search, so no rule searched for this class and \
     nothing measured says whether the corpus holds one";

/// What the survey measured about traversal routes in one world group.
///
/// Consulted by the audit only when the census states **no** route: a census
/// that stated one has answered the question whatever this says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteSearch {
    /// The measured route rule was applied to this group and the corpus states
    /// no route for it. `measured` cites what was searched and what the corpus
    /// holds; the audit reports it as a gap that names the affected content
    /// rather than as the old "measured for the wrong thing" shortfall.
    Unstated {
        /// What was searched and what the corpus holds, as measured.
        measured: String,
    },
    /// No measured route rule reached this census — the caller measured no
    /// rule, or the one it had could not be applied — so the audit keeps the
    /// shortfall gap ([`WorldAuditGap::NoRouteMeasured`], whose text names the
    /// two facts it is missing) instead of reporting a measured absence.
    /// `measured` stays readable through [`WorldGroupCensus::route_search`] for
    /// a caller that wants what stopped the search; the gap itself does not
    /// quote it, because nothing here measured a reason worth quoting.
    Unsought {
        /// What stopped the route search, as measured; never empty.
        measured: String,
    },
}

impl Default for RouteSearch {
    fn default() -> Self {
        Self::Unsought {
            measured: "this census carries no measured route search".to_owned(),
        }
    }
}

impl RouteSearch {
    /// The measurement this verdict carries, whatever the verdict is.
    pub fn measured(&self) -> &str {
        match self {
            Self::Unstated { measured } | Self::Unsought { measured } => measured,
        }
    }

    /// Whether the measured rule was applied and the corpus itself states no
    /// route — the state in which the shortfall gap is replaced by
    /// [`WorldAuditGap::NoRouteInMeasuredCorpus`].
    #[must_use]
    pub const fn corpus_states_none(&self) -> bool {
        matches!(self, Self::Unstated { .. })
    }
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
    unlocated: Vec<UnlocatedOpening>,
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

    /// The classes nothing located, in [`OpeningClass::ALL`] order, each one
    /// carrying the measurement that says why.
    ///
    /// Never a bare class: a report that shows *which* classes are missing
    /// without *why* is the silent zero this audit exists to prevent, so every
    /// row here answers both.
    #[must_use]
    pub fn unlocated(&self) -> &[UnlocatedOpening] {
        &self.unlocated
    }

    /// The classes nothing located, as the classes alone, in
    /// [`OpeningClass::ALL`] order.
    #[must_use]
    pub fn unlocated_classes(&self) -> Vec<OpeningClass> {
        self.unlocated.iter().map(UnlocatedOpening::class).collect()
    }

    /// The measurement that says why one class is not located here, or `None`
    /// when this class *was* located.
    pub fn unlocated_reason(&self, class: OpeningClass) -> Option<&str> {
        self.unlocated
            .iter()
            .find(|row| row.class() == class)
            .map(UnlocatedOpening::measured)
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
    ///
    /// This is the **shortfall** form, and the audit only reaches it when no
    /// measured route rule reached the census ([`RouteSearch::Unsought`]): a
    /// group the rule *did* reach reports [`Self::NoRouteInMeasuredCorpus`]
    /// instead, which cites what was searched and which content the missing
    /// route affects.
    NoRouteMeasured {
        /// The group that produced no route.
        world: WorldId,
        /// How many placed objects the placement source reported.
        placed_objects: usize,
    },
    /// The measured route rule was applied to this group and the corpus itself
    /// states no traversal route for it.
    ///
    /// The successor to [`Self::NoRouteMeasured`] for every group a measured
    /// rule covered. Where the shortfall gap said only "you measured and found
    /// nothing", this one names **what** was searched, **why** the corpus holds
    /// no route and **which content** the absence affects, so it can be filed
    /// as a follow-up against the task that resolves it instead of being
    /// rediscovered as a hole in a claim.
    NoRouteInMeasuredCorpus {
        /// The group the rule was applied to.
        world: WorldId,
        /// How many openings the same rule located in this group, so a reader
        /// sees what the search did find beside what it did not.
        located_openings: usize,
        /// The content the absent route affects: the group's missions, or the
        /// group itself when the campaign declares no mission in it.
        affected: Vec<String>,
        /// What was searched and what the corpus holds, as measured.
        measured: String,
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
            Self::NoRouteInMeasuredCorpus {
                world,
                located_openings,
                affected,
                measured,
            } => write!(
                f,
                "{world}: the measured route search states no traversal route affecting {} \
                 ({located_openings} opening(s) located by the same rule): {measured}",
                if affected.is_empty() {
                    "no named content".to_owned()
                } else {
                    affected.join(", ")
                }
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
    /// An opening class was recorded as unlocated with no measurement behind
    /// it.
    ///
    /// Refused rather than reported, because a class that is unlocated and
    /// silent is exactly the hole the opening audit exists to close.
    BlankOpeningReason {
        /// The class that carried no measurement, by its stable code.
        class: String,
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
            Self::BlankOpeningReason { class } => write!(
                f,
                "the opening class {class:?} was recorded as unlocated with no measurement \
                 behind it"
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
        if let RouteSearch::Unstated { measured } = census.route_search() {
            // The rule was applied and the corpus itself says there is no
            // route: a measured finding, not a shortfall. It names the content
            // the absence affects — the missions this group carries, or the
            // group itself when the campaign declares none in it — so a reader
            // can file it against a task instead of rediscovering it.
            let affected: Vec<String> = if group.missions().is_empty() {
                vec![format!("world group {}", group.world().key())]
            } else {
                group.missions().to_vec()
            };
            gaps.push(WorldAuditGap::NoRouteInMeasuredCorpus {
                world: group.world().clone(),
                located_openings: census.openings().len(),
                affected,
                measured: measured.clone(),
            });
        } else {
            gaps.push(WorldAuditGap::NoRouteMeasured {
                world: group.world().clone(),
                placed_objects: match census.placement() {
                    PlacementSource::Decoded { placed_objects } => placed_objects,
                    PlacementSource::Undecoded { .. } => 0,
                },
            });
        }
    }

    // The opening audit visits every class the sheet names, whether or not
    // anything located it, so a report can always be asked "was a hangar
    // looked for?" and get an answer — and, when the answer is no, *why* not.
    // Every unlocated class therefore carries a measurement: the one the
    // survey recorded for it, or the stated fact that no survey recorded one.
    // Neither can be an empty string ([`WorldAuditError::BlankOpeningReason`]
    // refuses that at the census boundary).
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
                vec![
                    UnlocatedOpening::new(
                        *class,
                        census
                            .unlocated_openings()
                            .iter()
                            .find(|row| row.class() == *class)
                            .map_or(OPENING_SEARCH_UNSUPPLIED, UnlocatedOpening::measured),
                    )
                    .expect("a non-empty measurement, refused at the census boundary"),
                ]
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

// --------------------------------------------- the retail trigger-volume survey ---

/// The member of a campaign mission's reader archive that names the mission's
/// detection zones.
///
/// A **measured** name, over the owner's installation: 23 of the 53 campaign
/// mission readers list a member of exactly this name, and no other member in
/// any reader carries the [`DETECTION_ZONE_PREFIX`] vocabulary. What the member
/// declares, and whether it declares any geometry at all, is **undecoded** — see
/// [`RetailTriggerVolumeSurvey::zone_declarations_are_decoded`].
pub const DETECTION_ZONE_MEMBER: &str = "dzones.zrd";

/// The name prefix the original's own world nodes give a detection zone.
///
/// A measured name: the world containers carry nodes called `dzpath1`,
/// `dzpath2`, … under a parent node called `dzpaths`, and the campaign's own
/// mission members name the same strings. The suffix is a **decimal index**, not
/// a guess: every measured node name after the prefix is digits, and
/// [`DETECTION_ZONE_PARENT`] is the one measured name that is not.
pub const DETECTION_ZONE_PREFIX: &str = "dzpath";

/// The measured parent node name of a world container's detection zones.
///
/// It is **not** a zone: [`is_detection_zone_name`] refuses it, and the survey
/// never counts it. It is stated as a constant because the survey and a reader
/// of this record must agree about which node is the container of the others.
pub const DETECTION_ZONE_PARENT: &str = "dzpaths";

/// Whether a world node's stored name is one **numbered** detection zone.
///
/// The rule is the measured one and nothing else: the [`DETECTION_ZONE_PREFIX`]
/// followed by at least one decimal digit and by nothing else. The parent node
/// ([`DETECTION_ZONE_PARENT`]) is refused, because it carries no zone of its own,
/// and a name with a suffix that is not digits is refused rather than truncated
/// — a rule that accepted `dzpath1_backup` would be reading a name the store
/// never used.
#[must_use]
pub fn is_detection_zone_name(name: &str) -> bool {
    let Some(index) = name.strip_prefix(DETECTION_ZONE_PREFIX) else {
        return false;
    };
    !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
}

/// Why a measured stored volume was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum TriggerVolumeError {
    /// A stored corner was NaN or infinite, so the extent it would produce is
    /// not a number a comparison could trust.
    NonFiniteCorner {
        /// Which corner: `0` the minimum, `1` the maximum.
        corner: usize,
        /// The axis the non-finite value was on.
        axis: usize,
    },
    /// The stored minimum was above the stored maximum on some axis, so the
    /// record is not a box.
    Inverted {
        /// The axis the record is inverted on.
        axis: usize,
    },
    /// A stored-unit-to-metre factor was NaN or infinite.
    NonFiniteScale {
        /// The factor that was refused.
        scale: f64,
    },
    /// A stored-unit-to-metre factor was zero or negative, which would make a
    /// length either vanish or flip.
    NonPositiveScale {
        /// The factor that was refused.
        scale: f64,
    },
    /// A speed was NaN or infinite, so one tick of travel is not a number.
    NonFiniteSpeed {
        /// The speed that was refused.
        speed_m_s: f64,
    },
    /// A speed was zero or negative. A body that is not moving forward has no
    /// **one tick of travel** to be compared against, and a negative speed
    /// would invert the comparison rather than answer it.
    NonPositiveSpeed {
        /// The speed that was refused.
        speed_m_s: f64,
    },
    /// A tick rate was zero or negative, which has no tick in it. A negative
    /// rate would make one tick of travel a distance in the wrong direction.
    NonPositiveTickRate {
        /// The rate that was refused.
        tick_hz: f64,
    },
    /// Two zones of the same world claimed the same name.
    DuplicateZone {
        /// The world the duplicate is in.
        world: String,
        /// The zone name both records claim.
        zone: String,
    },
    /// Two mission declarations claimed the same mission.
    DuplicateMission {
        /// The mission both declarations claim.
        mission: String,
    },
}

impl fmt::Display for TriggerVolumeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteCorner { corner, axis } => {
                let which = if *corner == 0 { "minimum" } else { "maximum" };
                write!(
                    f,
                    "the stored {which} corner on axis {axis} is not a number"
                )
            }
            Self::Inverted { axis } => write!(
                f,
                "the stored minimum is above the stored maximum on axis {axis}"
            ),
            Self::NonFiniteScale { scale } => {
                write!(f, "the stored-unit-to-metre factor {scale} is not a number")
            }
            Self::NonPositiveScale { scale } => write!(
                f,
                "the stored-unit-to-metre factor {scale} is not greater than zero"
            ),
            Self::NonFiniteSpeed { speed_m_s } => {
                write!(f, "the speed {speed_m_s} m/s is not a number")
            }
            Self::NonPositiveSpeed { speed_m_s } => {
                write!(f, "the speed {speed_m_s} m/s is not greater than zero")
            }
            Self::NonPositiveTickRate { tick_hz } => {
                write!(f, "the tick rate {tick_hz} Hz is not greater than zero")
            }
            Self::DuplicateZone { world, zone } => {
                write!(f, "two zones of world {world} claim the name {zone}")
            }
            Self::DuplicateMission { mission } => {
                write!(f, "two declarations claim the mission {mission}")
            }
        }
    }
}

impl std::error::Error for TriggerVolumeError {}

/// One axis-aligned box a world node stores, in the container's **stored units**.
///
/// The stored unit is measured — one unit is the metre (tasks #677 and #436) —
/// and this type still carries the **stored** numbers, the same statement
/// [`RepresentativeGeometry`] makes about a stored mesh extent. A
/// consumer that needs a length in canonical metres must go through
/// [`RetailTriggerVolumeSurvey::tick_verdict`], which refuses to compare while
/// the survey supplies no scale — this type exists so a reader cannot reach a
/// number and mistake it for one.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredVolume {
    min: [f64; 3],
    max: [f64; 3],
}

impl StoredVolume {
    /// Validates and wraps the two stored corners, in stored units.
    ///
    /// # Errors
    ///
    /// [`TriggerVolumeError::NonFiniteCorner`] for a corner no arithmetic can
    /// use, and [`TriggerVolumeError::Inverted`] for a minimum above its
    /// maximum. A degenerate box — a minimum equal to its maximum on one axis —
    /// is **accepted**: it is a flat plane, and a plane is a real authored
    /// volume. What the survey counts is the thickness, not the volume.
    pub fn new(min: [f64; 3], max: [f64; 3]) -> Result<Self, TriggerVolumeError> {
        for (corner, values) in [(0usize, min), (1usize, max)] {
            for (axis, value) in values.iter().enumerate() {
                if !value.is_finite() {
                    return Err(TriggerVolumeError::NonFiniteCorner { corner, axis });
                }
            }
        }
        for axis in 0..3 {
            if min[axis] > max[axis] {
                return Err(TriggerVolumeError::Inverted { axis });
            }
        }
        Ok(Self { min, max })
    }

    /// The stored minimum corner.
    #[must_use]
    pub const fn min(&self) -> [f64; 3] {
        self.min
    }

    /// The stored maximum corner.
    #[must_use]
    pub const fn max(&self) -> [f64; 3] {
        self.max
    }

    /// The stored extent along one axis, in stored units.
    ///
    /// # Panics
    ///
    /// If `axis` is not `0`, `1` or `2`. The three axes are the whole of what a
    /// stored box has, so an out-of-range index is a programming error rather
    /// than untrusted input.
    #[must_use]
    pub fn extent(&self, axis: usize) -> f64 {
        self.max[axis] - self.min[axis]
    }

    /// The **smallest** stored extent over the three axes, in stored units.
    ///
    /// This is the number the one-tick question turns on: a body crossing the
    /// volume has to get through its thinnest direction to be outrun by a
    /// sample, so the thinnest axis is the axis that decides it.
    #[must_use]
    pub fn thinnest_extent(&self) -> f64 {
        (0..3)
            .map(|axis| self.extent(axis))
            .fold(f64::INFINITY, f64::min)
    }

    /// Which axis [`Self::thinnest_extent`] is on, the first of a tie.
    #[must_use]
    pub fn thinnest_axis(&self) -> usize {
        let mut axis = 0;
        let mut thinnest = self.extent(0);
        for candidate in 1..3 {
            let extent = self.extent(candidate);
            if extent < thinnest {
                thinnest = extent;
                axis = candidate;
            }
        }
        axis
    }

    /// Whether every axis is zero, which is a record that stores no box at all.
    ///
    /// Deliberately **not** the same test as `thinnest_extent() == 0.0`: a box
    /// whose minimum equals its maximum on one axis is a plane, and a plane is a
    /// real authored volume. A reader that conflated the two would refuse every
    /// flat trigger the original authored and call it an absent one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        (0..3).all(|axis| self.extent(axis) == 0.0)
    }
}

/// Where one measured zone's bytes are, so a reader can go back to them.
///
/// This is the **source span** a trigger-volume measurement is required to
/// carry. It is a value rather than four fields on the zone for one reason: a
/// measurement whose numbers cannot be traced to a byte range is a number in a
/// document, and the difference is exactly what task #427's first acceptance
/// criterion asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerVolumeSpan {
    world: WorldId,
    container: String,
    container_sha256: String,
    node_slot: u32,
    node_offset: u64,
    node_bytes: u64,
}

impl TriggerVolumeSpan {
    /// Assembles the span of one node's own record inside one container.
    ///
    /// `container_sha256` is the SHA-256 of the **whole** container file as
    /// production discovery hashed it, so a rerun over a different installation
    /// reports different digests instead of the same numbers.
    #[must_use]
    pub fn new(
        world: WorldId,
        container: impl Into<String>,
        container_sha256: impl Into<String>,
        node_slot: u32,
        node_offset: u64,
        node_bytes: u64,
    ) -> Self {
        Self {
            world,
            container: container.into(),
            container_sha256: container_sha256.into(),
            node_slot,
            node_offset,
            node_bytes,
        }
    }

    /// The world container the node lives in.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        &self.world
    }

    /// The logical key of the container the bytes came from.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// SHA-256 of that whole container file, from production discovery.
    #[must_use]
    pub fn container_sha256(&self) -> &str {
        &self.container_sha256
    }

    /// The node's slot in the container's node array, in stored order.
    #[must_use]
    pub const fn node_slot(&self) -> u32 {
        self.node_slot
    }

    /// Absolute container offset of the node's own record.
    #[must_use]
    pub const fn node_offset(&self) -> u64 {
        self.node_offset
    }

    /// How many bytes the node's own record occupies.
    #[must_use]
    pub const fn node_bytes(&self) -> u64 {
        self.node_bytes
    }
}

/// One retail detection zone, measured out of a world container's node array.
///
/// The volume is the box the node's own info record stores — not a box this
/// workspace inferred, sized, or assumed. Its **unit is the container's stored
/// vertex unit** — measured, one unit the metre (tasks #677 and #436) — but
/// this type still carries the stored numbers, and the survey supplies no
/// factor of its own, so nothing read off it here is a stated length in
/// metres.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailTriggerVolume {
    span: TriggerVolumeSpan,
    zone: String,
    mesh_index: Option<i32>,
    volume: StoredVolume,
}

impl RetailTriggerVolume {
    /// Assembles one measured zone from its span, its name, its mesh binding and
    /// its stored box.
    #[must_use]
    pub const fn new(
        span: TriggerVolumeSpan,
        zone: String,
        mesh_index: Option<i32>,
        volume: StoredVolume,
    ) -> Self {
        Self {
            span,
            zone,
            mesh_index,
            volume,
        }
    }

    /// Where this zone's bytes are.
    #[must_use]
    pub const fn span(&self) -> &TriggerVolumeSpan {
        &self.span
    }

    /// The world container the node lives in.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        self.span.world()
    }

    /// The logical key of the container the bytes came from.
    #[must_use]
    pub fn container(&self) -> &str {
        self.span.container()
    }

    /// SHA-256 of that whole container file, from production discovery.
    #[must_use]
    pub fn container_sha256(&self) -> &str {
        self.span.container_sha256()
    }

    /// The node's slot in the container's node array, in stored order.
    #[must_use]
    pub const fn node_slot(&self) -> u32 {
        self.span.node_slot()
    }

    /// Absolute container offset of the node's own record.
    #[must_use]
    pub const fn node_offset(&self) -> u64 {
        self.span.node_offset()
    }

    /// How many bytes the node's own record occupies.
    #[must_use]
    pub const fn node_bytes(&self) -> u64 {
        self.span.node_bytes()
    }

    /// The node's stored name, which is the zone's own identity.
    #[must_use]
    pub fn zone(&self) -> &str {
        &self.zone
    }

    /// The mesh slot the node binds, or `None` when it stores `-1`.
    ///
    /// This matters for what the zone **is**: a measured corpus binds a mesh to
    /// every zone, which is the difference between a region of the world that
    /// carries geometry and a bare marker. What the original does *with* that
    /// geometry is unmeasured; see the findings record.
    #[must_use]
    pub const fn mesh_index(&self) -> Option<i32> {
        self.mesh_index
    }

    /// The stored box this node carries.
    #[must_use]
    pub const fn volume(&self) -> &StoredVolume {
        &self.volume
    }

    /// The smallest stored extent of this zone, in stored units.
    #[must_use]
    pub fn thinnest_stored_extent(&self) -> f64 {
        self.volume.thinnest_extent()
    }
}

/// What the survey can say about "is a fast aircraft's tick longer than the
/// original's thinnest trigger volume".
///
/// The three variants are the three honest states, and the survey never picks
/// one it has not earned: while the survey carries no stored-unit-to-metre
/// factor, the answer is [`Self::UnitUnmeasured`] carrying the **break-even
/// factor** — the factor at which the verdict would flip — so the missing
/// input is a number a later stage can supply rather than a shrug. The factor
/// is measured (task #677, one unit the metre); this survey deliberately does
/// not carry it — see `RetailTriggerVolumeSurvey::vertex_scale_to_m`.
#[derive(Clone, Debug, PartialEq)]
pub enum TriggerTickVerdict {
    /// The survey measured no zone, so there is nothing to compare.
    NoZones,
    /// The survey carries no stored-unit-to-metre factor, so a stored extent
    /// cannot be turned into a length and the comparison cannot be made.
    ///
    /// `break_even_meters_per_unit` is what one stored unit would have to be
    /// worth, in metres, for the thinnest measured zone to be **exactly** one
    /// tick of travel thick: at or above that factor every zone is thicker than
    /// a tick, and below it the thinnest zone can be outrun.
    UnitUnmeasured {
        /// The tick the comparison is about.
        speed_m_s: f64,
        /// The tick rate the travel is divided by.
        tick_hz: f64,
        /// How far the body travels in one tick, in canonical metres.
        travel_m_per_tick: f64,
        /// The thinnest measured zone, in stored units.
        thinnest_stored_extent: f64,
        /// The zone that measurement came from.
        thinnest_zone: String,
        /// One stored unit's worth in metres at which the thinnest zone is
        /// exactly one tick thick.
        break_even_meters_per_unit: f64,
    },
    /// Every measured zone is at least as thick as one tick of travel, so a
    /// discrete per-tick sample cannot step over the thinnest of them.
    EveryZoneSpansATick {
        /// The tick the comparison is about.
        speed_m_s: f64,
        /// The tick rate the travel was divided by.
        tick_hz: f64,
        /// How far the body travels in one tick, in canonical metres.
        travel_m_per_tick: f64,
        /// The thinnest measured zone, in canonical metres.
        thinnest_m: f64,
        /// The zone that measurement came from.
        thinnest_zone: String,
    },
    /// At least one measured zone is thinner than one tick of travel, so a
    /// discrete per-tick sample can step over it.
    ThinnestZoneOutrun {
        /// The tick the comparison is about.
        speed_m_s: f64,
        /// The tick rate the travel was divided by.
        tick_hz: f64,
        /// How far the body travels in one tick, in canonical metres.
        travel_m_per_tick: f64,
        /// The thinnest measured zone, in canonical metres.
        thinnest_m: f64,
        /// The zone that measurement came from.
        thinnest_zone: String,
    },
}

impl TriggerTickVerdict {
    /// The factor at which the verdict would flip, whatever the verdict is.
    ///
    /// `None` when there is no thinnest zone to flip on, which is the
    /// [`Self::NoZones`] state and nothing else.
    ///
    /// `Some(f64::INFINITY)` when the thinnest measured zone has **zero**
    /// thickness — a plane, which [`StoredVolume::new`] accepts — because a zone
    /// with no thickness is thinner than one tick under every factor, so no
    /// finite factor flips the verdict. It is `Some`, not `None`, because the
    /// zone was measured and the comparison was made; and it is an infinity
    /// rather than a number, because the caller has to be able to see that there
    /// is no finite one.
    #[must_use]
    pub fn break_even_meters_per_unit(&self) -> Option<f64> {
        match self {
            Self::NoZones => None,
            Self::UnitUnmeasured {
                break_even_meters_per_unit,
                ..
            } => Some(*break_even_meters_per_unit),
            Self::EveryZoneSpansATick {
                travel_m_per_tick,
                thinnest_m,
                ..
            } => Some(*travel_m_per_tick / *thinnest_m),
            Self::ThinnestZoneOutrun {
                travel_m_per_tick,
                thinnest_m,
                ..
            } => Some(*travel_m_per_tick / *thinnest_m),
        }
    }

    /// Whether the survey could answer the question at all.
    #[must_use]
    pub const fn is_decided(&self) -> bool {
        matches!(
            self,
            Self::EveryZoneSpansATick { .. } | Self::ThinnestZoneOutrun { .. }
        )
    }

    /// How far the body travels in one tick, in canonical metres, when the
    /// comparison got that far.
    #[must_use]
    pub fn travel_m_per_tick(&self) -> Option<f64> {
        match self {
            Self::NoZones => None,
            Self::UnitUnmeasured {
                travel_m_per_tick, ..
            }
            | Self::EveryZoneSpansATick {
                travel_m_per_tick, ..
            }
            | Self::ThinnestZoneOutrun {
                travel_m_per_tick, ..
            } => Some(*travel_m_per_tick),
        }
    }
}

/// Which key of a `dzones.zrd` record a zone name was stated under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneDeclarationKey {
    /// `disable`: meaning unmeasured.
    Disable,
    /// `nosnapshot`: meaning unmeasured.
    NoSnapshot,
    /// `objective_numbers`: a zone bound to an integer; meaning unmeasured.
    ObjectiveNumbers,
}

impl ZoneDeclarationKey {
    /// The key's stored spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::NoSnapshot => "nosnapshot",
            Self::ObjectiveNumbers => "objective_numbers",
        }
    }
}

/// What one campaign mission's `dzones.zrd` declares, with the member's span.
///
/// The **structure** is measured over all 23 retail members. The **meaning** is
/// not: nothing here says what disabling a zone, excluding it from a snapshot or
/// binding it to an objective number does in the original, or which objective an
/// integer indexes (`objectives.zrd`, F13-D / F39).
#[derive(Clone, Debug, PartialEq)]
pub struct MissionZoneDeclaration {
    mission: String,
    world: WorldId,
    member_container: String,
    member_container_sha256: String,
    member_offset: u64,
    member_bytes: u64,
    keys: Vec<ZoneDeclarationKey>,
    disable: Vec<String>,
    no_snapshot: Vec<String>,
    objective_numbers: Vec<(String, u32)>,
}

impl MissionZoneDeclaration {
    /// Assembles one mission's declaration. `keys` is the stored key order;
    /// a key absent from it is a key the member does not state.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mission: impl Into<String>,
        world: WorldId,
        member_container: impl Into<String>,
        member_container_sha256: impl Into<String>,
        member_span: (u64, u64),
        keys: Vec<ZoneDeclarationKey>,
        disable: Vec<String>,
        no_snapshot: Vec<String>,
        objective_numbers: Vec<(String, u32)>,
    ) -> Self {
        Self {
            mission: mission.into(),
            world,
            member_container: member_container.into(),
            member_container_sha256: member_container_sha256.into(),
            member_offset: member_span.0,
            member_bytes: member_span.1,
            keys,
            disable,
            no_snapshot,
            objective_numbers,
        }
    }

    /// The mission, as its logical key (`zbd/c3/m01`).
    #[must_use]
    pub fn mission(&self) -> &str {
        &self.mission
    }

    /// The world container whose zone nodes the names refer to.
    #[must_use]
    pub const fn world(&self) -> &WorldId {
        &self.world
    }

    /// The reader archive the member is in.
    #[must_use]
    pub fn member_container(&self) -> &str {
        &self.member_container
    }

    /// SHA-256 of that whole archive from production discovery.
    #[must_use]
    pub fn member_container_sha256(&self) -> &str {
        &self.member_container_sha256
    }

    /// The member's absolute offset and byte length in the archive.
    #[must_use]
    pub const fn member_span(&self) -> (u64, u64) {
        (self.member_offset, self.member_bytes)
    }

    /// The keys the member states, in stored order.
    #[must_use]
    pub fn keys(&self) -> &[ZoneDeclarationKey] {
        &self.keys
    }

    /// The `disable` names.
    #[must_use]
    pub fn disable(&self) -> &[String] {
        &self.disable
    }

    /// The `nosnapshot` names.
    #[must_use]
    pub fn no_snapshot(&self) -> &[String] {
        &self.no_snapshot
    }

    /// The `objective_numbers` pairs, in stored order.
    #[must_use]
    pub fn objective_numbers(&self) -> &[(String, u32)] {
        &self.objective_numbers
    }

    /// Every zone name the member states, with its key, in stored key order.
    #[must_use]
    pub fn named_zones(&self) -> Vec<(ZoneDeclarationKey, &str)> {
        let mut named = Vec::new();
        for key in &self.keys {
            match key {
                ZoneDeclarationKey::Disable => {
                    named.extend(self.disable.iter().map(|n| (*key, n.as_str())));
                }
                ZoneDeclarationKey::NoSnapshot => {
                    named.extend(self.no_snapshot.iter().map(|n| (*key, n.as_str())));
                }
                ZoneDeclarationKey::ObjectiveNumbers => {
                    named.extend(
                        self.objective_numbers
                            .iter()
                            .map(|(n, _)| (*key, n.as_str())),
                    );
                }
            }
        }
        named
    }
}

/// A zone a mission names that its world container has no node for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZoneDeclarationGap {
    /// The mission that names the zone.
    pub mission: String,
    /// The world container that lacks it.
    pub world: String,
    /// The zone name.
    pub zone: String,
    /// The key the mission stated it under.
    pub key: ZoneDeclarationKey,
}

/// Every retail detection zone the survey measured, with the fingerprints that
/// make the measurement checkable.
///
/// The mission side is attached with [`Self::with_declarations`]: one
/// [`MissionZoneDeclaration`] per campaign mission whose `dzones.zrd` was decoded
/// (task #513), cross-checked against the zones the world containers carry by
/// [`Self::declaration_gaps`].
#[derive(Clone, Debug, PartialEq)]
pub struct RetailTriggerVolumeSurvey {
    install_sha256: String,
    vertex_scale_to_m: Option<f64>,
    volumes: Vec<RetailTriggerVolume>,
    declarations: Option<Vec<MissionZoneDeclaration>>,
}

impl RetailTriggerVolumeSurvey {
    /// Assembles the survey from the zones a measurement produced.
    ///
    /// `vertex_scale_to_m` is the factor from the containers' stored vertex
    /// units to canonical metres. The retail GameZ unit is measured — one
    /// unit is the metre (tasks #677 and #436) — but whether this survey
    /// should carry that factor is a separate decision (task #733), and
    /// passing `None` is what makes [`Self::tick_verdict`] refuse rather than
    /// guess.
    ///
    /// # Errors
    ///
    /// [`TriggerVolumeError::NonFiniteScale`] / [`TriggerVolumeError::NonPositiveScale`]
    /// for a factor no comparison could use, and
    /// [`TriggerVolumeError::DuplicateZone`] for two zones of one world
    /// claiming one name — a real state a duplicate-only store would produce,
    /// and not something a consumer should have to break a tie on.
    pub fn new(
        install_sha256: impl Into<String>,
        vertex_scale_to_m: Option<f64>,
        volumes: Vec<RetailTriggerVolume>,
    ) -> Result<Self, TriggerVolumeError> {
        if let Some(scale) = vertex_scale_to_m {
            if !scale.is_finite() {
                return Err(TriggerVolumeError::NonFiniteScale { scale });
            }
            if scale <= 0.0 {
                return Err(TriggerVolumeError::NonPositiveScale { scale });
            }
        }
        for (index, volume) in volumes.iter().enumerate() {
            for other in &volumes[index + 1..] {
                if other.world().key() == volume.world().key() && other.zone() == volume.zone() {
                    return Err(TriggerVolumeError::DuplicateZone {
                        world: volume.world().key().to_owned(),
                        zone: volume.zone().to_owned(),
                    });
                }
            }
        }
        Ok(Self {
            install_sha256: install_sha256.into(),
            vertex_scale_to_m,
            volumes,
            declarations: None,
        })
    }

    /// Attaches the mission-side declarations a decode of every campaign
    /// mission's `dzones.zrd` produced.
    ///
    /// # Errors
    ///
    /// [`TriggerVolumeError::DuplicateMission`] for two declarations of one
    /// mission.
    pub fn with_declarations(
        mut self,
        declarations: Vec<MissionZoneDeclaration>,
    ) -> Result<Self, TriggerVolumeError> {
        for (index, declaration) in declarations.iter().enumerate() {
            if declarations[index + 1..]
                .iter()
                .any(|other| other.mission() == declaration.mission())
            {
                return Err(TriggerVolumeError::DuplicateMission {
                    mission: declaration.mission().to_owned(),
                });
            }
        }
        self.declarations = Some(declarations);
        Ok(self)
    }

    /// Every mission's decoded declaration, empty while none is attached.
    #[must_use]
    pub fn declarations(&self) -> &[MissionZoneDeclaration] {
        self.declarations.as_deref().unwrap_or_default()
    }

    /// The names a mission declares that **no** zone node in its world container
    /// carries, one row per name per key it was stated under.
    ///
    /// A **reported gap**, never a silent drop: a mission naming a zone its
    /// container lacks is a fact the survey must show.
    #[must_use]
    pub fn declaration_gaps(&self) -> Vec<ZoneDeclarationGap> {
        let mut gaps = Vec::new();
        for declaration in self.declarations() {
            for (key, zone) in declaration.named_zones() {
                let present = self.volumes.iter().any(|volume| {
                    volume.world().key() == declaration.world().key() && volume.zone() == zone
                });
                if !present {
                    gaps.push(ZoneDeclarationGap {
                        mission: declaration.mission().to_owned(),
                        world: declaration.world().key().to_owned(),
                        zone: zone.to_owned(),
                        key,
                    });
                }
            }
        }
        gaps
    }

    /// SHA-256 of the installation fingerprint the measurement was taken over.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// The factor from the containers' stored vertex units to canonical metres,
    /// or `None` while the survey carries none — the unit itself is measured
    /// (task #677); whether this survey consumes it is task #733.
    #[must_use]
    pub const fn vertex_scale_to_m(&self) -> Option<f64> {
        self.vertex_scale_to_m
    }

    /// Every zone, in the order the survey measured them.
    #[must_use]
    pub fn volumes(&self) -> &[RetailTriggerVolume] {
        &self.volumes
    }

    /// The zones of one world container, in the order they were measured.
    #[must_use]
    pub fn volumes_in(&self, world: &WorldId) -> Vec<&RetailTriggerVolume> {
        self.volumes
            .iter()
            .filter(|volume| volume.world().key() == world.key())
            .collect()
    }

    /// The zones that bind no mesh, which is the shape a bare marker would have.
    #[must_use]
    pub fn meshless_zones(&self) -> Vec<&RetailTriggerVolume> {
        self.volumes
            .iter()
            .filter(|volume| volume.mesh_index().is_none())
            .collect()
    }

    /// The thinnest measured zone, in stored units, and which zone it was.
    #[must_use]
    pub fn thinnest(&self) -> Option<(&RetailTriggerVolume, f64)> {
        self.volumes
            .iter()
            .map(|volume| (volume, volume.thinnest_stored_extent()))
            .min_by(|left, right| {
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.0.zone().cmp(right.0.zone()))
            })
    }

    /// Whether the campaign's own detection-zone members have been decoded and
    /// attached: `true` once [`Self::with_declarations`] has been given the
    /// mission side.
    ///
    /// What is decoded is the **grammar** (task #513: a list's second word is
    /// its child count plus one). What a mission *means* by `disable`,
    /// `nosnapshot` or an objective number is still unknown; see
    /// [`MissionZoneDeclaration`].
    #[must_use]
    pub const fn zone_declarations_are_decoded(&self) -> bool {
        self.declarations.is_some()
    }

    /// How far a body travelling at `speed_m_s` moves in one tick at `tick_hz`,
    /// in canonical metres.
    ///
    /// # Errors
    ///
    /// [`TriggerVolumeError::NonFiniteSpeed`] for a speed no arithmetic can use,
    /// [`TriggerVolumeError::NonPositiveSpeed`] for a body that is not moving
    /// forward, and [`TriggerVolumeError::NonPositiveTickRate`] for a rate with
    /// no tick in it — zero, negative, or not a number at all, since every one of
    /// those makes `speed / tick` something other than a distance.
    pub fn travel_m_per_tick(speed_m_s: f64, tick_hz: f64) -> Result<f64, TriggerVolumeError> {
        if !speed_m_s.is_finite() {
            return Err(TriggerVolumeError::NonFiniteSpeed { speed_m_s });
        }
        if speed_m_s <= 0.0 {
            return Err(TriggerVolumeError::NonPositiveSpeed { speed_m_s });
        }
        if !tick_hz.is_finite() || tick_hz <= 0.0 {
            return Err(TriggerVolumeError::NonPositiveTickRate { tick_hz });
        }
        Ok(speed_m_s / tick_hz)
    }

    /// Whether one tick of travel at `speed_m_s` and `tick_hz` can step over the
    /// thinnest zone this survey measured.
    ///
    /// The answer is a [`TriggerTickVerdict`], not a `bool`, because there are
    /// three states and the interesting one is currently
    /// [`TriggerTickVerdict::UnitUnmeasured`]. That variant carries the factor
    /// at which the answer would change, so the factor the survey does not
    /// carry is a number a later stage can supply rather than an open question.
    ///
    /// # Errors
    ///
    /// Whatever [`Self::travel_m_per_tick`] refuses.
    pub fn tick_verdict(
        &self,
        speed_m_s: f64,
        tick_hz: f64,
    ) -> Result<TriggerTickVerdict, TriggerVolumeError> {
        let travel_m_per_tick = Self::travel_m_per_tick(speed_m_s, tick_hz)?;
        let Some((thinnest, thinnest_stored_extent)) = self.thinnest() else {
            return Ok(TriggerTickVerdict::NoZones);
        };
        let thinnest_zone = thinnest.zone().to_owned();
        let Some(scale) = self.vertex_scale_to_m else {
            // The thinnest zone may be a **plane**: `StoredVolume::new` accepts a
            // degenerate box because a plane is a real authored volume, and the
            // survey keeps it (an all-zero box is refused by the survey itself,
            // which is a different state). For a plane this factor is `+inf`,
            // which is the honest answer rather than a number that pretends to
            // be one: a zero extent is thinner than one tick under **every**
            // factor, so no finite factor flips the verdict. A caller that needs
            // a finite number must check the thinnest extent first, and
            // `travel_m_per_tick` is already refused as non-positive so the
            // `0 / 0` that would make this a NaN cannot be reached.
            return Ok(TriggerTickVerdict::UnitUnmeasured {
                speed_m_s,
                tick_hz,
                travel_m_per_tick,
                thinnest_stored_extent,
                thinnest_zone,
                break_even_meters_per_unit: travel_m_per_tick / thinnest_stored_extent,
            });
        };
        let thinnest_m = thinnest_stored_extent * scale;
        Ok(if thinnest_m >= travel_m_per_tick {
            TriggerTickVerdict::EveryZoneSpansATick {
                speed_m_s,
                tick_hz,
                travel_m_per_tick,
                thinnest_m,
                thinnest_zone,
            }
        } else {
            TriggerTickVerdict::ThinnestZoneOutrun {
                speed_m_s,
                tick_hz,
                travel_m_per_tick,
                thinnest_m,
                thinnest_zone,
            }
        })
    }
}

// ------------------------------------------- the stored world hierarchy ---

// The measured facts this section rests on are in
// `docs/findings/2026-10-03-f18-world-hierarchy-authority.md`; the code
// comments below say what each refusal is for rather than repeating them.

/// The claim a world container's hierarchy conversion is recorded under when
/// the stored parent slot and the stored child list disagree.
///
/// The claim is deliberately narrow: it says **which side this conversion
/// trusts**, not that the original engine agrees. No original run has measured
/// the engine's behaviour, so the claim names the rule and the reason the
/// corpus supports it, and the unresolved question stays open in the finding.
pub const HIERARCHY_PARENT_SLOT_AUTHORITATIVE: &str =
    "f18-world.hierarchy-parent-slot-authoritative";

/// What a container's two stored hierarchy statements say about each other.
///
/// A GameZ node record carries its hierarchy twice: a `parent` slot in its own
/// record and a `children_count`-length list that follows the record. This
/// verdict is the **measurement** of how those two agree, over every stored
/// record of one container, and it is what the conversion below is allowed to
/// act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HierarchyVerdict {
    /// Both sides agree on every link: each child names the node that lists it,
    /// each listed child names the node that lists it, and no node is listed
    /// twice.
    Consistent {
        /// How many child slots the container's lists hold in total.
        child_slots: usize,
    },
    /// The child lists are a **partial index**: `omitted` records name a parent
    /// that does not list them, spread over `parents` distinct parents, and no
    /// link disagrees the other way. The parent slot is the only complete
    /// statement of ownership in the container, so it is the one this
    /// conversion follows.
    PartialChildIndex {
        /// Records that name a parent which does not list them.
        omitted: usize,
        /// How many distinct parents those records name.
        parents: usize,
        /// How many child slots the container's lists hold in total.
        child_slots: usize,
    },
    /// The two sides contradict each other in the direction the partial-index
    /// reading does not cover: `listed_but_not_naming` records are listed by a
    /// node they do not name as their parent, or `listed_twice` records are
    /// listed by two parents at once. The corpus has never shown either, and a
    /// container that does is evidence the adopted rule does not hold there, so
    /// it is blocked rather than converted under a rule it contradicts.
    Contradictory {
        /// Records listed by a node they do not name as their parent.
        listed_but_not_naming: usize,
        /// Records listed by two different parents.
        listed_twice: usize,
        /// How many child slots the container's lists hold in total.
        child_slots: usize,
    },
}

impl HierarchyVerdict {
    /// How many child slots the container's stored child lists hold in total.
    #[must_use]
    pub const fn child_slots(&self) -> usize {
        match self {
            Self::Consistent { child_slots }
            | Self::PartialChildIndex { child_slots, .. }
            | Self::Contradictory { child_slots, .. } => *child_slots,
        }
    }
}

/// One container's stored hierarchy, measured.
///
/// Every field is a **count** or a **slot** read out of the decoded records;
/// nothing here interprets a name, a transform or a kind. The audit is the
/// instrument the conversion below is driven by and the evidence a caller
/// reports when a world container does not convert, which is why it is a record
/// of its own rather than a side effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredHierarchyAudit {
    nodes: usize,
    child_slots: usize,
    named_but_unlisted: usize,
    listed_but_not_naming: usize,
    listed_twice: usize,
    partial_parents: usize,
    roots: usize,
    unreachable: usize,
    verdict: HierarchyVerdict,
}

impl StoredHierarchyAudit {
    /// How many stored node records the container holds.
    #[must_use]
    pub const fn nodes(&self) -> usize {
        self.nodes
    }

    /// How many child slots the container's stored child lists hold in total.
    #[must_use]
    pub const fn child_slots(&self) -> usize {
        self.child_slots
    }

    /// How many records name a parent that does not list them.
    ///
    /// This is the count the finding is about: it is 0 in the aircraft
    /// container and between 155 and 471 in each world container, and every one
    /// of those records names the same parent — the world node.
    ///
    /// A record naming a parent slot that is out of range is **not** counted
    /// here: it disagrees with nothing, because no such record exists to list
    /// it. [`SceneGraph::build`] refuses that as
    /// [`SceneError::DanglingParent`] instead, so the two statements stay
    /// separate — this count is about agreement, not about link validity.
    #[must_use]
    pub const fn named_but_unlisted(&self) -> usize {
        self.named_but_unlisted
    }

    /// How many records are listed by a node they do not name as their parent.
    #[must_use]
    pub const fn listed_but_not_naming(&self) -> usize {
        self.listed_but_not_naming
    }

    /// How many records two different parents list at once.
    #[must_use]
    pub const fn listed_twice(&self) -> usize {
        self.listed_twice
    }

    /// How many distinct parents hold a child list that omits records naming
    /// it.
    #[must_use]
    pub const fn partial_parents(&self) -> usize {
        self.partial_parents
    }

    /// How many records have an empty parent slot under the adopted rule.
    #[must_use]
    pub const fn roots(&self) -> usize {
        self.roots
    }

    /// How many records no root reaches when children are derived from the
    /// parent slots.
    ///
    /// A record nothing reaches has a parent chain that can only loop, so this
    /// is the count [`SceneGraph::build`] would refuse with
    /// [`SceneError::Cycle`] on. It is measured here so a caller learns it
    /// before the build, not after.
    #[must_use]
    pub const fn unreachable(&self) -> usize {
        self.unreachable
    }

    /// How the container's two stored hierarchy statements compare.
    #[must_use]
    pub const fn verdict(&self) -> &HierarchyVerdict {
        &self.verdict
    }

    /// Whether the adopted rule applies to this container unchanged.
    #[must_use]
    pub const fn is_convertible(&self) -> bool {
        !matches!(self.verdict, HierarchyVerdict::Contradictory { .. })
    }
}

/// Measures how one decoded container's stored parent slots and child lists
/// compare, in both directions.
///
/// The measurement is total and never refuses: a container whose two sides
/// contradict each other gets a [`HierarchyVerdict::Contradictory`] verdict
/// with the exact counts, because "these containers do not agree" is a result,
/// not an absence of one. What the counts mean is in the finding; what the
/// conversion does with them is [`world_hierarchy_from_gamez`].
#[must_use]
pub fn audit_stored_hierarchy(records: &GameZNodes) -> StoredHierarchyAudit {
    let mut child_slots = 0usize;
    let mut named_but_unlisted = 0usize;
    let mut listed_but_not_naming = 0usize;
    let mut partial_parents: BTreeSet<u32> = BTreeSet::new();
    // Every `(parent, child)` slot the container stores. A child that appears
    // under two parents is counted once, not once per extra listing.
    let mut slots: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    for node in &records.nodes {
        child_slots += node.children.len();
        for &child in &node.children {
            *slots.entry((node.index, child)).or_insert(0) += 1;
        }
        if let Some(parent) = node.parent
            && let Some(parent_node) = records.get(parent)
            && !parent_node.children.contains(&node.index)
        {
            named_but_unlisted += 1;
            partial_parents.insert(parent);
        }
        for &child in &node.children {
            if let Some(child_node) = records.get(child)
                && child_node.parent != Some(node.index)
            {
                listed_but_not_naming += 1;
            }
        }
    }
    let mut listed_by: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for &(parent, child) in slots.keys() {
        listed_by.entry(child).or_default().insert(parent);
    }
    let listed_twice = listed_by
        .values()
        .filter(|parents| parents.len() > 1)
        .count();

    // Reachability when children are derived from the parent slots, which is
    // the only hierarchy this conversion accepts.
    let roots: Vec<u32> = records.roots().map(|node| node.index).collect();
    let mut derived: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for node in &records.nodes {
        if let Some(parent) = node.parent {
            derived.entry(parent).or_default().push(node.index);
        }
    }
    let mut reached: BTreeSet<u32> = BTreeSet::new();
    let mut stack: Vec<u32> = roots.clone();
    while let Some(index) = stack.pop() {
        if !reached.insert(index) {
            continue;
        }
        if let Some(children) = derived.get(&index) {
            stack.extend(children.iter().copied());
        }
    }
    let unreachable = records.nodes.len() - reached.len();

    let verdict = if listed_but_not_naming > 0 || listed_twice > 0 {
        HierarchyVerdict::Contradictory {
            listed_but_not_naming,
            listed_twice,
            child_slots,
        }
    } else if named_but_unlisted > 0 {
        HierarchyVerdict::PartialChildIndex {
            omitted: named_but_unlisted,
            parents: partial_parents.len(),
            child_slots,
        }
    } else {
        HierarchyVerdict::Consistent { child_slots }
    };
    StoredHierarchyAudit {
        nodes: records.nodes.len(),
        child_slots,
        named_but_unlisted,
        listed_but_not_naming,
        listed_twice,
        partial_parents: partial_parents.len(),
        roots: roots.len(),
        unreachable,
        verdict,
    }
}

// ------------------------------------------------------------------ the rule ---

/// Why a world container's stored hierarchy could not be converted.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldHierarchyError {
    /// The container's two hierarchy statements contradict each other, so the
    /// adopted rule does not apply to it. The counts are the measurement, and
    /// the container is blocked rather than converted under a rule its own
    /// bytes contradict.
    Contradictory {
        /// The container's verdict.
        verdict: Box<HierarchyVerdict>,
    },
    /// The container's node records did not convert into [`ParsedNode`]s.
    Records(GameZSceneError),
}

impl fmt::Display for WorldHierarchyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contradictory { verdict } => write!(
                f,
                "the stored parent slots and child lists contradict each other ({verdict:?}), so \
                 neither side can be followed"
            ),
            Self::Records(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for WorldHierarchyError {}

/// One world container's hierarchy under the adopted rule: the reconciled
/// [`ParsedNode`] records plus the measurement they were reconciled under.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldHierarchy {
    nodes: Vec<ParsedNode>,
    audit: StoredHierarchyAudit,
}

impl WorldHierarchy {
    /// The reconciled records, in stored order.
    ///
    /// Their `children` lists are each record's own stored list **extended** with
    /// the records that name it and that list omits, so `SceneGraph::build` sees
    /// a hierarchy that agrees with itself and can judge it on its own terms
    /// while the stored order survives. The stored lists themselves stay in the
    /// caller's [`GameZNodes`], which is the only place they survive this
    /// conversion.
    #[must_use]
    pub fn nodes(&self) -> &[ParsedNode] {
        &self.nodes
    }

    /// The measurement the reconciliation was made under.
    #[must_use]
    pub const fn audit(&self) -> &StoredHierarchyAudit {
        &self.audit
    }
}

/// Converts a decoded world container's node array into reconciled
/// [`ParsedNode`] records under the adopted rule.
///
/// **The rule: the parent slot is the authoritative statement of ownership and
/// the child list is an index that cannot veto it.** Every record that names
/// this one is therefore one of its children, whatever its own list holds.
///
/// The reconciliation is **add-only in content and in order**: a record's stored
/// child list is kept exactly as stored and the links only a parent slot states
/// are appended to it, in the order the child records are stored. Three
/// properties make this the conservative reading rather than a convenient one.
/// Every added link comes from a parent slot the record itself stores, so the
/// conversion can add links the store states but never removes one: a record's
/// own ownership survives untouched. The stored order is never rewritten, so a
/// container the two sides already agree on converts **bit for bit** as it did
/// before this rule — measured over `planes.zbd`, whose stored order is not the
/// child-record order, replacing the lists instead of extending them would have
/// reordered 56 records for no reason the data supports. And the strict check in
/// [`SceneGraph::build`] is not relaxed to let this pass — the reconciled
/// records satisfy it, and the build still refuses a container whose records
/// contradict each other for any other reason.
///
/// What the rule cannot decide is also refused, not resolved: a container whose
/// child lists contradict the parent slots (a record listed by a node it does
/// not name, or by two parents at once) gets
/// [`WorldHierarchyError::Contradictory`] with the exact counts, because such a
/// container is evidence the rule does not hold there. The measured corpus has
/// none.
///
/// # Errors
///
/// [`WorldHierarchyError::Contradictory`] for a container the rule does not
/// apply to, and [`WorldHierarchyError::Records`] carrying
/// [`GameZSceneError`] verbatim when the records themselves do not convert.
pub fn world_hierarchy_from_gamez(
    records: &GameZNodes,
    meshes: &[MeshSlot],
) -> Result<WorldHierarchy, WorldHierarchyError> {
    let audit = audit_stored_hierarchy(records);
    if !audit.is_convertible() {
        return Err(WorldHierarchyError::Contradictory {
            verdict: Box::new(audit.verdict().clone()),
        });
    }
    let mut nodes =
        parsed_nodes_from_gamez(records, meshes).map_err(WorldHierarchyError::Records)?;
    // The derivation is the whole rule: a record's children are the records
    // that name it, whatever its own list holds.
    let mut derived: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for node in &nodes {
        if let Some(parent) = node.parent {
            derived.entry(parent).or_default().push(node.index);
        }
    }
    for node in &mut nodes {
        let Some(naming) = derived.remove(&node.index) else {
            // No record names this one, so it has no derived child. Its own
            // list is empty for the same reason: a listed child always names
            // the node that lists it, or the container was refused above.
            continue;
        };
        // Appended, never substituted: the stored links keep the store's own
        // order and only the links a parent slot states on its own are added.
        // Reordering a stored list would discard information the store does
        // provide — in the measured corpus the stored order of a child list is
        // not the order of the child records, so it is the store's statement
        // and not an artifact to tidy.
        for child in naming {
            if !node.children.contains(&child) {
                node.children.push(child);
            }
        }
    }
    Ok(WorldHierarchy { nodes, audit })
}

/// One world container converted into a [`SceneGraph`], with the measurement the
/// conversion was made under.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSceneGraph {
    graph: SceneGraph,
    audit: StoredHierarchyAudit,
}

impl WorldSceneGraph {
    /// The canonical graph.
    #[must_use]
    pub const fn graph(&self) -> &SceneGraph {
        &self.graph
    }

    /// The measurement of the container's two stored hierarchy statements.
    #[must_use]
    pub const fn audit(&self) -> &StoredHierarchyAudit {
        &self.audit
    }
}

/// Why one world container did not become a [`SceneGraph`].
#[derive(Clone, Debug, PartialEq)]
pub enum WorldSceneError {
    /// The stored hierarchy could not be reconciled under the adopted rule.
    Hierarchy(WorldHierarchyError),
    /// The hierarchy reconciled and the canonical build refused it.
    ///
    /// The build's own typed refusal is carried **verbatim** — nothing here
    /// translates, retries or weakens it — and the measurement travels with it,
    /// so a caller reporting "this world container is blocked" can name both the
    /// disagreement the adopted rule resolved and the refusal that remains.
    Build {
        /// [`SceneGraph::build`]'s own refusal.
        source: SceneError,
        /// How the container's two stored hierarchy statements compare.
        audit: Box<StoredHierarchyAudit>,
    },
}

impl fmt::Display for WorldSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hierarchy(error) => write!(f, "{error}"),
            Self::Build { source, audit } => write!(
                f,
                "{source} (the hierarchy itself reconciles: {} of {} records name a parent that \
                 does not list them, over {} parent(s); verdict {:?})",
                audit.named_but_unlisted(),
                audit.nodes(),
                audit.partial_parents(),
                audit.verdict()
            ),
        }
    }
}

impl std::error::Error for WorldSceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Hierarchy(error) => Some(error),
            Self::Build { source, .. } => Some(source),
        }
    }
}

/// Decodes a world container's node array and converts it into a canonical
/// [`SceneGraph`] under the adopted hierarchy rule.
///
/// This is the whole world-container path: [`world_hierarchy_from_gamez`]
/// reconciles the two stored statements, [`SceneGraph::build`] judges the
/// reconciled hierarchy, and the two verdicts stay separate so a container whose
/// records read and reconcile but whose canonical build refuses is reported as
/// exactly that.
pub fn world_scene_graph_from_gamez(
    container: &ContentId,
    records: &GameZNodes,
    meshes: &[MeshSlot],
    adapter: &SourceAdapter,
    bindings: &BindingMap,
) -> Result<WorldSceneGraph, WorldSceneError> {
    let hierarchy =
        world_hierarchy_from_gamez(records, meshes).map_err(WorldSceneError::Hierarchy)?;
    let audit = hierarchy.audit().clone();
    let graph =
        SceneGraph::build(container, hierarchy.nodes(), adapter, bindings).map_err(|source| {
            WorldSceneError::Build {
                source,
                audit: Box::new(audit.clone()),
            }
        })?;
    Ok(WorldSceneGraph { graph, audit })
}

/// The stored slot of the container's world node, if it has one.
///
/// The measured disagreement is entirely about this one record: in all eight
/// world containers the child list that omits records belongs to the world node
/// and to no other. A caller that needs to report *which* record a partial list
/// belongs to reads it here rather than re-deriving it.
#[must_use]
pub fn world_node_slot(records: &GameZNodes) -> Option<u32> {
    records
        .nodes
        .iter()
        .find(|node| node.kind.tag() == NODE_TYPE_WORLD)
        .map(|node| node.index)
}

// -------------------------------------------- the world-container import ---

// The measured facts this section rests on are in
// `docs/findings/2026-10-04-m01-lc-world-import.md`; the code comments below say
// what each refusal is for rather than repeating them.
//
// The conversion is two steps that are deliberately separate: `partition_grid`
// reads the world record's own spatial index out of the container bytes, and
// `import_world_container` turns that index plus the records it names into a
// `WorldDefinition`. Nothing in between invents a sector, an object or a role:
// every value the original did not state arrives as `Resolved::Unknown` with the
// claim id that says which measurement is missing.

/// Wraps one of this module's own claim id literals.
///
/// The literals are compile-time constants of this module, so a rejection here
/// would be a typo in the source rather than untrusted input; `expect` says
/// exactly that instead of turning a typo into a panic at run time.
fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("a claim id this module declares")
}

/// The world record's partition grid **is** the world's sector index.
///
/// Measured over all eight world containers of the original installation: the
/// grid is an `x × y` array of cells, every cell names records by their stored
/// node slot, and the set of distinct slots it names is **exactly** the set of
/// records the world node's own child list omits — the same set the hierarchy
/// rule already showed the list omits, reached by a second route. The claim is
/// about what the container's bytes say; no original run measured how the 2000
/// engine streamed on them.
pub const PARTITION_GRID_IS_THE_SECTOR_INDEX: &str = "f18-world.partition-grid-is-the-sector-index";

/// A record the world's partition grid names **is** the world's static spatial
/// geometry.
///
/// **Designed rule over a measured fact.** The container stores no per-record
/// collision field at all: what it stores is membership in the world's own
/// spatial index together with a stored world-space bounding box
/// (`unk140`). Measured: every indexed record in every container is an object
/// record, every one is distinct, and the ones that carry geometry are exactly
/// the ones whose stored box is non-zero. This conversion therefore treats an
/// indexed record as the world's static geometry (`Solid`, `FromMesh`) and
/// names every record the index does not name as role-unknown. It is a claim
/// about **this** conversion, not about how the 2000 engine collided.
///
/// **One measured exception (task #727).** The 2000 engine's own handling of
/// the partition grid, read out of the owner-supplied decrypted image
/// (`docs/findings/2026-10-07-f18-grid-collision-origin.md`), is a *broad-phase
/// candidate index*, never a solidity statement: the loader reads every cell
/// (`0x4e3081`–`0x4e3141`) and `cls_di.c`'s intersection-database builder walks
/// it (`0x4cb579`) to collect **candidates**, which are then filtered by node
/// flags, a zone whitelist and an optional name before any box is tested. So a
/// grid-named record is a record the engine could consider, not a record the
/// engine declared solid — and a record whose own measured role contradicts
/// static geometry must not inherit one from the index. That is the six
/// grid-named `fvol*` records of `c1c` and `c5`: with the candidate's own
/// narrow-phase bit measured clear (task #771) they resolve
/// [`WorldCollisionRole::None`] under [`FOG_VOLUME_RECORD_NEVER_BLOCKS`], and
/// a grid-named `fvol*` record storing that bit would resolve under
/// [`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`] instead. A grid-named record that
/// stores no geometry at all resolves [`GRID_RECORD_STORES_NO_GEOMETRY`].
pub const INDEXED_RECORD_IS_STATIC: &str = "f18-world.indexed-record-is-static";

/// A record the partition grid names whose name keys the original's fog-volume
/// consumer.
///
/// **Unmeasured collision role, measured contradiction.** Two statements about
/// the same six records disagree and neither of them is a solidity claim:
///
/// * the record is in the world's partition grid, which this conversion
///   otherwise reads as static geometry ([`INDEXED_RECORD_IS_STATIC`]);
/// * the record's name starts with the four bytes `fvol`, which is the prefix
///   the decrypted image's **only** name-keyed consumer of that prefix matches
///   — `strncmp` against `"fvol"` at VA `0x44e087`, inside the routine at VA
///   `0x44d9d0` that also reads `fogvol.zrd`'s fog keys, so the engine takes
///   such a record as a **fog volume**.
///
/// The measurement that settles it is how the image handles the grid itself
/// (task #727, `docs/findings/2026-10-07-f18-grid-collision-origin.md`): the
/// grid is read at load and walked only as a candidate set — `cls_di.c`'s
/// builder at `0x4cb420` gathers the cells overlapping a query box and then
/// filters each candidate by node flags, a zone whitelist (`0x56c430`) and an
/// optional name before its own bounding box is copied (`0x4cd960`). The
/// container states no collision field for these records at all, so their role
/// is an explicit **unknown** with this claim id rather than the index's
/// `Solid`: a consumer sees "the container did not say" instead of a fog bank
/// that stops a plane.
///
/// **Narrowed by task #771 — the six records it was written for no longer
/// carry it.** What this claim was missing was the candidate's own filter:
/// `cls_di.c`'s walk reads the record's flags word at `0x4cb635` and reaches
/// the narrow phase (the only branch that copies `[node+0x70]`) only with
/// [`INTERSECTION_NARROW_PHASE_FLAG`] set, otherwise recursing into children
/// or dropping the candidate. Measured over the original installation, all
/// six grid-named `fvol*` records store that bit clear with no children, so
/// they are dropped before any box test and resolve
/// [`WorldCollisionRole::None`] under [`FOG_VOLUME_RECORD_NEVER_BLOCKS`]
/// instead. This claim stays for what it still truthfully describes: a
/// grid-named `fvol*` record that **does** store the narrow-phase bit, whose
/// box the walk would copy — the container still states no collision role for
/// such a record, so it keeps the explicit unknown rather than the index's
/// `Solid`. Evidence class: `observed_tool` (static analysis of one
/// executable), never `verified_original`.
pub const GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED: &str =
    "f18-world.grid-named-fog-volume-role-unmeasured";

/// The record flag bit `cls_di.c`'s intersection walk requires before it runs
/// the narrow phase — the branch that copies a record's box through
/// `[node+0x70]`.
///
/// **Measured** (task #771, static analysis of the owner-supplied decrypted
/// image, `observed_tool`): the walk reads the candidate's flags word at
/// `0x4cb5eb` (bit `0x04`, the first filter), then at `0x4cb635`–`0x4cb668`
/// tests this bit (`test al, 0x40`) and, when it is clear, recurses into the
/// record's children instead — a record with no children is dropped there
/// (`cmp word [esi+0x56], bx` / `jle`, `0x4cb63e`) **before any box is read**.
/// With the bit set the walk tests bit `0x100` and only then copies the 24
/// bytes held at `[node+0x70]` (`0x4cd960`). `cls_util.c`'s own error string
/// names this bit the *proximity* flag (`0x62d784`), and the INTERP commands
/// `SetIntersectSurface` / `SetIntersectBBOX` / `SetAltitudeSurface` can write
/// the related bits at run time (`0x5bbccc`, `0x5bbc90`), so this is a stored
/// default and never a statement about what a script set later.
pub const INTERSECTION_NARROW_PHASE_FLAG: u32 = 0x40;

/// A record the partition grid names that stores **no geometry at all**: no
/// mesh index and no non-empty stored box.
///
/// **Measured store state, designed resolution** (task #771). The store's own
/// `mesh_index` is signed, with `-1` meaning "no mesh" (the loader skips the
/// mesh-base adjustment for it at `0x4e2aa9`), and the record's three stored
/// boxes (offsets `0x74`, `0x8c`, `0xa4`) are all empty — zero extent on every
/// axis — so nothing a collider or a drawing could come from exists in the
/// container. This is the same store state task #677 resolved for *unindexed*
/// records ([`UNINDEXED_RECORD_STORES_NO_GEOMETRY`]), reached here on a record
/// the grid names: the index only makes such a record a **candidate**
/// ([`INDEXED_RECORD_IS_STATIC`]), and a candidate that stores nothing has
/// nothing to test, so it resolves [`WorldCollisionRole::None`] and its absent
/// mesh stays an explicit unknown under [`OBJECT_STORES_NO_MESH`].
///
/// Measured over the original installation: one such record per `c1c` and
/// `c2b`, eight in `c1b`, thirteen in `c1`, twenty-three in `c2`, eighteen in
/// `c3`, four in `c4` and eighty in `c5` — a **subset** of
/// [`WorldImportReport::partition_records_with_mesh`]'s complement, because a
/// grid record with no mesh that still stores a box in one of the other two
/// slots keeps whatever its container says and is not resolved by this claim.
///
/// Evidence class: `observed_tool` (the store's bytes through the production
/// readers), never `verified_original`.
pub const GRID_RECORD_STORES_NO_GEOMETRY: &str = "f18-world.grid-record-stores-no-geometry";

/// Which gameplay query consumes `cls_di.c`'s intersection walk — unmeasured.
///
/// **Open, and open only in a way an original run can lift** (task #771). The
/// walk has exactly one caller chain in the image: game code at `0x4ab284` →
/// `fcn.005ac150` (a distance computation that then walks the intersections
/// database) → `fcn.004cb420`, whose returned distance the calling routine
/// compares against a threshold (`0x4ab28c`) before calling `0x4b7e70`. What
/// that routine *is* was not established: it is a `thiscall` over an object
/// holding node pointers and positions (`0x4aabb0`), and the RTTI comparisons
/// inside it name `Target`, `TargetVehicle` and `TargetTurret` (`0x620768`,
/// `0x620780`), which places it in the target/weapon code without naming the
/// query.
///
/// Affected content: any claim about *what the original used the walk for* —
/// target acquisition, proximity, line of sight or something else. Resolving
/// it needs a behavior observation, i.e. an original run (#358); nothing in
/// the repository's own artifacts answers it.
pub const INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED: &str =
    "f18-world.intersection-query-gameplay-consumer-unmeasured";

/// The original's coordinate handedness, axis order and angle unit — and, for
/// a conversion that never measured it, the world-vertex unit.
///
/// The **unit** is measured: task #677 pinned one stored GameZ unit to the
/// metre over five independent landmark censuses
/// ([`crate::coordinates::GAMEZ_VERTEX_UNIT_IS_THE_METRE`], at
/// `observed_tool`). The **axis convention** is measured too, by static
/// analysis of the owner-supplied decrypted image (task #436's owner note of
/// 2026-10-05): identity axis map, `+Y` up, right-handed, radians in GameZ
/// binaries — see [`WORLD_AXIS_CONVENTION_MEASURED`], which is what the import
/// reports through [`WorldImportReport::axis_class`] for the quantities this
/// conversion applies. The container family itself still stores no handedness,
/// axis-order or angle-unit declaration anyone has tied to an original
/// behavior: both measurements are code-derived, never observed in a run. The
/// import takes its conversion from the caller's [`SourceAdapter`] and reports
/// the factor it used, the axis map it applied and each quantity's own
/// evidence class, so a consumer always knows which numbers turned stored
/// units into canonical ones and how strong their evidence is.
pub const WORLD_UNIT_UNMEASURED: &str = "f18-world.unit-unmeasured";

/// The original's world axis convention, measured by static analysis of the
/// owner-supplied decrypted image (task #436's owner note, 2026-10-05).
///
/// **Code-derived, never `VerifiedOriginal`.** The landmarks the owner recorded
/// on `$CS_GAME_DIR/crimson.decrypted.exe` (sha256
/// `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`) say the
/// stored GameZ frame and the project's canonical frame are the **same** frame:
///
/// * the atmosphere is evaluated on `position.y` and gravity acts on `−y`
///   (VA `0x48fc40`, `0x48ff88`), so `+Y` is up and X/Z are horizontal;
/// * the matrix→Euler decomposition (VA `0x53df30`) takes yaw from the x/z
///   components and pitch from the y component of the local z axis, and the
///   view matrix (VA `0x53afc0`) is a proper rotation whose camera looks along
///   `−Z`, so the frame is right-handed with no mirror anywhere between
///   content and screen;
/// * GameZ node euler triples are radians (corpus maximum exactly π, composed
///   raw), `.zrd` text angles are degrees (π/180 at VA `0x6040e8`).
///
/// So the axis map this conversion applies to GameZ content **is** the
/// original's, at `observed_tool` — the strongest class a static analysis of
/// the bytes can reach. It never becomes `VerifiedOriginal`: no original
/// executable ran, and no behavior landmark exists until #358 supplies a run.
/// The evidence class for the applied map is reported per import through
/// [`WorldImportReport::axis_class`]; an installation-backed source that
/// applies something other than that identity map reports `Contradicted`,
/// because the measurement and the applied map disagree.
pub const WORLD_AXIS_CONVENTION_MEASURED: &str = "f18-world.world-axis-convention-measured";

/// An `fvol*` record is a fog volume: presented, never blocking, never
/// reporting a contact.
///
/// **Measured consumer, designed resolution.** Two facts, each measured:
///
/// * the decrypted image references the four-byte prefix `fvol` **exactly
///   once** (VA `0x6249f4` holds the bytes; the single instruction that
///   references them is `push 0x6249f4` at VA `0x44e087`, the middle argument
///   of a `strncmp(name, "fvol", 4)` over a name read from each record of a
///   global list — the name is the record's first field, as it is in the
///   store's own info slot), inside the routine at VA `0x44d9d0` that also
///   opens `fogvol.zrd` and reads its fog keys (`fog_zone`, `distance`,
///   `fog_fade_dist`, `interior_fog_fade_dist`, `fog_color`) and its `clutter`
///   groups. The caller of that routine compares the value it returns against
///   the fog distance globals the routine itself initializes — a fade
///   computation, not a contact test. So the only name-keyed consumer of an
///   `fvol*` record in the image is the fog system;
/// * `fogvol.zrd` exists in **every** group's `zrdr.zbd`, and no `.zrd` member
///   anywhere in the installation spells `fvol` (1 293 members scanned), so
///   nothing else — mission, objective or script document — names these
///   records either.
///
/// The records the partition grid omits therefore resolve to
/// [`WorldCollisionRole::None`]: the store draws them (their mesh stays a
/// known mesh reference) and nothing measured reports a contact for them. This
/// is a **designed resolution over a measured fact**, the same shape as
/// [`UNINDEXED_RECORD_STORES_NO_GEOMETRY`], and it is a claim about this
/// conversion — not a claim that the 2000 engine never intersected a fog box.
///
/// **Task #771 extended it to the `fvol*` records the partition grid *does*
/// name.** For those, task #727 had measured that the grid is a broad-phase
/// candidate index, never a solidity statement, and left their role an
/// explicit unknown because the store states no collision field either way.
/// What settles the candidate is `cls_di.c`'s own filter, measured from the
/// same decrypted image: at `0x4cb635` the walk reads the record's flags word
/// and reaches the narrow phase — the only branch that copies a box through
/// `[node+0x70]` — only with [`INTERSECTION_NARROW_PHASE_FLAG`] set, so a
/// grid-named `fvol*` record that stores the bit clear is dropped before any
/// box test and resolves the same `None` here, with the overlap still counted
/// by [`WorldImportReport::partition_records_fog_volume`]. The six such
/// records this installation stores (`c1c` node slots 944–947, `c5` node
/// slots 2304 and 2306) all store it clear with no children, so they resolve
/// this claim; one storing the bit would stay an explicit unknown under
/// [`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`]. Whether a script sets that bit
/// at run time, and which gameplay query the walk serves
/// ([`INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED`]) are the named
/// residuals; both need an original run (#358).
pub const FOG_VOLUME_RECORD_NEVER_BLOCKS: &str = "f18-world.fog-volume-record-never-blocks";

/// The original's world floor, ceiling and lateral rules.
///
/// Unmeasured. The world record stores no rule this stage can read, so the
/// imported definition carries an explicit unknown rather than an invisible
/// wall.
pub const WORLD_BOUNDARY_UNMEASURED: &str = "f18-world.boundary-unmeasured";

/// The original's gameplay surface classes.
///
/// Unmeasured: the container states no per-record surface, so **every** imported
/// object's surface is an explicit unknown with this claim id. A contact whose
/// rule was never named is a question for the damage stage, not a guess here.
pub const WORLD_SURFACE_UNMEASURED: &str = "f18-world.surface-unmeasured";

/// A record outside the world's partition grid has no measured collision role.
///
/// Unmeasured: the container indexes only the world's spatial geometry and
/// states nothing about the rest of its mesh-binding records — the effect
/// hierarchies, the vegetation groups, the aircraft. Those records become
/// world objects (they are the world node's own children) with this claim id
/// on their collision role, so a consumer sees "the container did not say"
/// instead of a world in which every effect blocks a plane.
///
/// The claim is kept for the records that still need it — an unindexed record
/// that **does** store collision geometry (a mesh, an extent or both) could be
/// a wall, a sensor or a decoration and the container does not say which. Two
/// classes leave it: an unindexed record that stores *neither* field resolves
/// to `None` under [`UNINDEXED_RECORD_STORES_NO_GEOMETRY`], and a record whose
/// name carries the measured `fvol` prefix resolves to `None` under
/// [`FOG_VOLUME_RECORD_NEVER_BLOCKS`], because the image's only name-keyed
/// consumer of that prefix is its fog system (task #716).
pub const UNINDEXED_ROLE_UNMEASURED: &str = "f18-world.unindexed-collision-role-unmeasured";

/// An unindexed record that binds no mesh and stores no extent carries no
/// collision geometry of its own, so its role resolves to `None`.
///
/// **Designed resolution over a measured fact.** Measured over all eight world
/// containers of the original installation (task #677): every record the
/// partition grid omits stores *either* a mesh index and a non-zero `unk140`
/// bounding box *or* neither of them — the corpus contains no unindexed record
/// carrying only one of the two. For the no-mesh, no-extent half — the
/// `horizon`, the `g*` transform groups, the zeppelin and vehicle anchors —
/// the store gives this record nothing a collider could be built from:
/// `FromMesh` has no mesh to derive from and a `Cuboid` has no extent to fill.
/// [`WorldCollisionRole::None`] is therefore what the record itself states:
/// presented, never blocking, never reporting a contact.
///
/// The claim is about **this record's own geometry only**. The content parked
/// under such an anchor — the airship the `*zep` record parents, the instances
/// a `g*` group scatters — is other records' business (the actor and animation
/// surfaces measure those), and does not change what *this* record bounds.
pub const UNINDEXED_RECORD_STORES_NO_GEOMETRY: &str =
    "f18-world.unindexed-record-stores-no-geometry";

/// The identity of an imported world object is its container's own node slot.
///
/// **Designed identity.** The container's records carry display names, but those
/// names are not unique (one container's world node lists thirty-four records
/// with the same name) and F11-A's `scene_node` id grammar refuses many of them
/// outright. The stored node slot is the one address the store gives every
/// record, it is stable across reads of the same container, and it is what the
/// partition grid itself names. It is **not** a claim that the original
/// identified a world object by slot.
pub const OBJECT_ID_IS_THE_NODE_SLOT: &str = "f18-world.object-id-is-the-node-slot";

/// A record that stores no mesh index binds no mesh.
///
/// **Measured fact.** `mesh_index` is a signed word; a negative one is the store
/// saying the record draws no stored mesh, and this conversion resolves no mesh
/// reference for it rather than substituting geometry. It has its own claim id
/// because it is a statement about a record's own bytes, not about the
/// identity of the record and not about a role the container never states.
pub const OBJECT_STORES_NO_MESH: &str = "f18-world.object-stores-no-mesh-index";

/// Why a world container could not be imported.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldImportError {
    /// The container holds no world record, so there is no world to import.
    NoWorldNode,
    /// The world record's own grid ran past the end of the container bytes.
    ///
    /// The node reader already walked the grid successfully to size it, so this
    /// is the importer refusing bytes that no longer match what the reader saw.
    GridTruncated {
        /// Absolute container offset the walk reached.
        offset: u64,
        /// Bytes the container holds.
        len: u64,
    },
    /// The grid walk did not end exactly where the node reader said it ends.
    GridEndMismatch {
        /// Where the importer's walk ended.
        walked: u64,
        /// Where the reader said the block ends.
        expected: u64,
    },
    /// A grid value named a node slot the container does not hold.
    PartitionSlotOutOfRange {
        /// The cell the value was in.
        cell: u32,
        /// The stored slot it named.
        slot: u32,
        /// How many records the container holds.
        nodes: usize,
    },
    /// Two grid values named the same record, in one cell or across cells.
    ///
    /// A spatial index that lists a record twice is evidence the index is not
    /// the one-address-per-record index this conversion reads, so the container
    /// is blocked rather than imported under a rule its own bytes contradict.
    PartitionSlotRepeated {
        /// The stored slot two values named.
        slot: u32,
    },
    /// A grid value named a record that is not an object record.
    PartitionSlotNotAnObject {
        /// The cell the value was in.
        cell: u32,
        /// The stored slot it named.
        slot: u32,
        /// The kind that record actually declares.
        kind: &'static str,
    },
    /// A record the world node owns is not an object record.
    ///
    /// Measured: every world-owned record in all eight containers is an object
    /// record. One that is not would be content this conversion does not know
    /// how to place, so it is refused by name instead of dropped.
    WorldOwnedNotAnObject {
        /// The stored slot.
        slot: u32,
        /// The kind that record actually declares.
        kind: &'static str,
    },
    /// The world node's ownership statements disagreed with each other.
    ///
    /// The hierarchy rule (`world_hierarchy_from_gamez`) makes the parent slot
    /// authoritative, and the partition grid is measured to name exactly the
    /// records the world node's child list omits. Those three statements have to
    /// agree for "the records the world owns" to be one set; the counts are
    /// carried so a report can name which side disagreed.
    OwnershipDisagreement {
        /// Distinct records the grid named.
        grid: usize,
        /// Records the world node's stored child list named.
        child_list: usize,
        /// Records that named the world node as their parent.
        naming: usize,
    },
    /// A stored coordinate was NaN or infinite.
    Space(SpaceError),
    /// A stored box was not a box.
    Volume(TriggerVolumeError),
    /// The converted sector extent was not a box in canonical metres.
    Bounds(AabbError),
    /// An object record was rejected by its own constructor.
    Object(ObjectInstanceError),
    /// The finished definition was rejected by its own validation.
    Definition(WorldError),
    /// An object identity or a sector identity broke the world key grammar.
    Key(WorldKeyError),
    /// The caller's mesh slot table named no element for a mesh index.
    MeshSlotMissing {
        /// The record's stored `mesh_index`.
        index: u32,
        /// How many slots the table has.
        slots: usize,
    },
}

impl fmt::Display for WorldImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoWorldNode => write!(f, "the container holds no world record"),
            Self::GridTruncated { offset, len } => write!(
                f,
                "the world record's partition grid ran past the container at offset {offset} \
                 of {len} bytes"
            ),
            Self::GridEndMismatch { walked, expected } => write!(
                f,
                "the partition grid walk ended at {walked}, the node reader said {expected}"
            ),
            Self::PartitionSlotOutOfRange { cell, slot, nodes } => write!(
                f,
                "partition cell {cell} names node slot {slot}, but the container holds {nodes} \
                 records"
            ),
            Self::PartitionSlotRepeated { slot } => {
                write!(f, "two partition values name node slot {slot}")
            }
            Self::PartitionSlotNotAnObject { cell, slot, kind } => write!(
                f,
                "partition cell {cell} names node slot {slot}, which is a {kind} record"
            ),
            Self::WorldOwnedNotAnObject { slot, kind } => write!(
                f,
                "the world node owns node slot {slot}, which is a {kind} record"
            ),
            Self::OwnershipDisagreement {
                grid,
                child_list,
                naming,
            } => write!(
                f,
                "the world node's ownership statements disagree: {grid} grid record(s), \
                 {child_list} stored child record(s), {naming} record(s) naming it"
            ),
            Self::Space(error) => write!(f, "{error}"),
            Self::Volume(error) => write!(f, "{error}"),
            Self::Bounds(error) => write!(f, "{error}"),
            Self::Object(error) => write!(f, "{error}"),
            Self::Definition(error) => write!(f, "{error}"),
            Self::Key(error) => write!(f, "{error}"),
            Self::MeshSlotMissing { index, slots } => write!(
                f,
                "a record names mesh index {index}, but the caller's table holds {slots} slot(s)"
            ),
        }
    }
}

impl From<SpaceError> for WorldImportError {
    fn from(error: SpaceError) -> Self {
        Self::Space(error)
    }
}

impl From<TriggerVolumeError> for WorldImportError {
    fn from(error: TriggerVolumeError) -> Self {
        Self::Volume(error)
    }
}

impl From<AabbError> for WorldImportError {
    fn from(error: AabbError) -> Self {
        Self::Bounds(error)
    }
}

impl From<ObjectInstanceError> for WorldImportError {
    fn from(error: ObjectInstanceError) -> Self {
        Self::Object(error)
    }
}

impl From<WorldError> for WorldImportError {
    fn from(error: WorldError) -> Self {
        Self::Definition(error)
    }
}

impl From<WorldKeyError> for WorldImportError {
    fn from(error: WorldKeyError) -> Self {
        Self::Key(error)
    }
}

impl std::error::Error for WorldImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Space(error) => Some(error),
            Self::Volume(error) => Some(error),
            Self::Bounds(error) => Some(error),
            Self::Object(error) => Some(error),
            Self::Definition(error) => Some(error),
            Self::Key(error) => Some(error),
            _ => None,
        }
    }
}

/// One cell of the world record's partition grid, decoded.
///
/// A cell is a **stored** record: its two grid coordinates, the node slots its
/// values name, and the six `f32` its own 24 bytes hold. The six floats are
/// carried because they are bytes the container stores and this stage does not
/// drop, but they are **not** interpreted — see
/// [`Self::header_floats_are_interpreted`].
#[derive(Clone, Debug, PartialEq)]
pub struct WorldPartitionCell {
    index: u32,
    grid_x: u32,
    grid_y: u32,
    slots: Vec<u32>,
    header_floats: [f32; 6],
}

impl WorldPartitionCell {
    /// The cell's position in the grid, in stored order.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// The cell's coordinate along the grid's first stored axis.
    #[must_use]
    pub const fn grid_x(&self) -> u32 {
        self.grid_x
    }

    /// The cell's coordinate along the grid's second stored axis.
    #[must_use]
    pub const fn grid_y(&self) -> u32 {
        self.grid_y
    }

    /// The node slots the cell's values name, in stored value order.
    #[must_use]
    pub fn slots(&self) -> &[u32] {
        &self.slots
    }

    /// The six `f32` the cell's own header stores, unchanged.
    ///
    /// Measured to be **not** a reliable cell extent: across the eight world
    /// containers the reading "these are the cell's low and high bounds on the
    /// two horizontal axes" holds for every cell of two containers and for
    /// between 73% and 91% of the cells of the other six, and the misses are
    /// cells whose members' stored boxes disagree with it. The sector extents
    /// this conversion publishes therefore come from the members' stored
    /// bounding boxes, which the store states per record, and these six floats
    /// stay unread.
    #[must_use]
    pub const fn header_floats(&self) -> [f32; 6] {
        self.header_floats
    }

    /// Whether this stage interprets a cell's own header floats.
    ///
    /// It does not, and the record says so rather than a consumer having to
    /// remember. The grid's *coordinates* and *membership* are measured; the
    /// header's field meanings are not.
    #[must_use]
    pub const fn header_floats_are_interpreted() -> bool {
        false
    }
}

/// The world record's partition grid: the container's own spatial index.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldPartitionGrid {
    world_node: u32,
    x_count: u32,
    y_count: u32,
    cells: Vec<WorldPartitionCell>,
}

impl WorldPartitionGrid {
    /// Reads the grid out of the container bytes the node reader already walked.
    ///
    /// The walk is the one `read_gamez_nodes` performed to size the block, run
    /// again here for its **content**: the node reader keeps the two grid counts
    /// and the block's length because they are what make the block knowable, and
    /// the bytes themselves stay addressable through
    /// [`RawNode::data_offset`](cs_formats::gamez::RawNode::data_offset). This
    /// is that re-derivation, and it ends exactly where the reader said the
    /// block ends — checked against both the recomputed block length and the
    /// reader's own recorded end (`data_bytes`, which also covers the world's
    /// child slots), not against one of them.
    ///
    /// # Errors
    ///
    /// [`WorldImportError::NoWorldNode`] when the container holds no world
    /// record, [`WorldImportError::GridTruncated`] /
    /// [`WorldImportError::GridEndMismatch`] when the bytes no longer match what
    /// the reader saw, and the four slot refusals
    /// ([`WorldImportError::PartitionSlotOutOfRange`],
    /// [`WorldImportError::PartitionSlotRepeated`],
    /// [`WorldImportError::PartitionSlotNotAnObject`]) when a value names
    /// nothing usable.
    pub fn read(records: &GameZNodes, container: &[u8]) -> Result<Self, WorldImportError> {
        let world = records
            .nodes
            .iter()
            .find(|node| node.kind.tag() == NODE_TYPE_WORLD)
            .ok_or(WorldImportError::NoWorldNode)?;
        let StoredNodeKind::World(data) = world.kind else {
            return Err(WorldImportError::NoWorldNode);
        };
        let base = u64::from(world.data_offset);
        let block = base + WORLD_DATA_BYTES + 4 + data.partition_bytes;
        let grid_start = base + WORLD_DATA_BYTES + 4;
        let len = container.len() as u64;

        let mut cells: Vec<WorldPartitionCell> = Vec::new();
        let mut named: BTreeSet<u32> = BTreeSet::new();
        let mut offset = grid_start;
        let count = u64::from(data.partition_x_count) * u64::from(data.partition_y_count);
        for cell in 0..count {
            // Every read below is inside the cell or inside its own values, and
            // both are bounds-checked against the container before they are
            // taken, so the offset conversion is the only place a value that
            // cannot be addressed on this host can surface. It is reported as
            // the truncation it would be, never as an index.
            let base_at = usize::try_from(offset)
                .map_err(|_| WorldImportError::GridTruncated { offset, len })?;
            if offset + WORLD_PARTITION_BYTES > len {
                return Err(WorldImportError::GridTruncated { offset, len });
            }
            let mut header_floats = [0.0f32; 6];
            for (word, float) in header_floats.iter_mut().enumerate() {
                let start = base_at + 8 + word * 4;
                *float = f32::from_le_bytes([
                    container[start],
                    container[start + 1],
                    container[start + 2],
                    container[start + 3],
                ]);
            }
            let count_at = base_at + 58;
            let values = u64::from(u16::from_le_bytes([
                container[count_at],
                container[count_at + 1],
            ]));
            let value_bytes = values * WORLD_PARTITION_VALUE_BYTES;
            if offset + WORLD_PARTITION_BYTES + value_bytes > len {
                return Err(WorldImportError::GridTruncated { offset, len });
            }
            let mut slots = Vec::new();
            for value in 0..values {
                let start = base_at
                    + WORLD_PARTITION_BYTES as usize
                    + (value * WORLD_PARTITION_VALUE_BYTES) as usize;
                let slot = u32::from_le_bytes([
                    container[start],
                    container[start + 1],
                    container[start + 2],
                    container[start + 3],
                ]);
                let record = records.get(slot).ok_or({
                    WorldImportError::PartitionSlotOutOfRange {
                        cell: u32::try_from(cell).unwrap_or(u32::MAX),
                        slot,
                        nodes: records.nodes.len(),
                    }
                })?;
                if record.kind.tag() != NODE_TYPE_OBJECT3D {
                    return Err(WorldImportError::PartitionSlotNotAnObject {
                        cell: u32::try_from(cell).unwrap_or(u32::MAX),
                        slot,
                        kind: record.kind.label(),
                    });
                }
                if !named.insert(slot) {
                    return Err(WorldImportError::PartitionSlotRepeated { slot });
                }
                slots.push(slot);
            }
            offset += WORLD_PARTITION_BYTES + value_bytes;
            cells.push(WorldPartitionCell {
                index: u32::try_from(cell).unwrap_or(u32::MAX),
                grid_x: u32::try_from(cell % u64::from(data.partition_x_count)).unwrap_or(0),
                grid_y: u32::try_from(cell / u64::from(data.partition_x_count)).unwrap_or(0),
                slots,
                header_floats,
            });
        }
        if offset != block {
            return Err(WorldImportError::GridEndMismatch {
                walked: offset,
                expected: block,
            });
        }
        // The reader walked the grid and **then** the world's own child slots, so
        // the grid's end plus those slots is where the reader recorded the
        // record ending (`data_bytes`). Checking only the block length recomputed
        // from the same two counts would accept a walk that agreed with itself
        // and with the counts while the reader had walked another distance.
        let child_bytes = 4 * u64::try_from(world.children.len()).unwrap_or(u64::MAX);
        let reader_end = base + world.data_bytes;
        if block + child_bytes != reader_end {
            return Err(WorldImportError::GridEndMismatch {
                walked: block + child_bytes,
                expected: reader_end,
            });
        }
        Ok(Self {
            world_node: world.index,
            x_count: data.partition_x_count,
            y_count: data.partition_y_count,
            cells,
        })
    }

    /// The world record the grid was read from.
    #[must_use]
    pub const fn world_node(&self) -> u32 {
        self.world_node
    }

    /// Cells along the grid's first stored axis.
    #[must_use]
    pub const fn x_count(&self) -> u32 {
        self.x_count
    }

    /// Cells along the grid's second stored axis.
    #[must_use]
    pub const fn y_count(&self) -> u32 {
        self.y_count
    }

    /// Every cell, in stored order.
    #[must_use]
    pub fn cells(&self) -> &[WorldPartitionCell] {
        &self.cells
    }

    /// One cell by its stored index.
    #[must_use]
    pub fn cell(&self, index: u32) -> Option<&WorldPartitionCell> {
        self.cells.iter().find(|cell| cell.index == index)
    }

    /// Every node slot the grid names, distinct, in stored order.
    #[must_use]
    pub fn indexed_slots(&self) -> Vec<u32> {
        let mut seen = BTreeSet::new();
        self.cells
            .iter()
            .flat_map(|cell| cell.slots.iter().copied())
            .filter(|slot| seen.insert(*slot))
            .collect()
    }

    /// How many values the grid holds in total.
    #[must_use]
    pub fn value_count(&self) -> usize {
        self.cells.iter().map(|cell| cell.slots.len()).sum()
    }

    /// The cells that name no record.
    #[must_use]
    pub fn empty_cells(&self) -> Vec<u32> {
        self.cells
            .iter()
            .filter(|cell| cell.slots.is_empty())
            .map(|cell| cell.index)
            .collect()
    }
}

/// What one world container's import measured and what it could not resolve.
///
/// Every field is a **count** or a **factor**. The counts are read out of the
/// container through the production readers, so a rerun over a different
/// installation reports different numbers instead of the same ones; the claim
/// status is the caller's coordinate conversion, carried so a reader of an
/// imported definition always knows which factor turned stored units into
/// numbers and how strong that factor's own evidence is.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldImportReport {
    world_node: u32,
    stored_child_list: usize,
    partition_cells: usize,
    partition_records: usize,
    partition_records_with_mesh: usize,
    partition_records_fog_volume: usize,
    partition_records_stores_no_geometry: usize,
    empty_cells: usize,
    objects: usize,
    objects_with_mesh: usize,
    objects_in_a_sector: usize,
    objects_resident: usize,
    objects_solid: usize,
    objects_unindexed_none: usize,
    objects_unindexed_fog: usize,
    objects_unindexed_unresolved: usize,
    objects_unresolved_collision: usize,
    sectors: usize,
    sectors_without_extent: usize,
    mesh_binding_records_elsewhere: usize,
    matrix_disagreements: usize,
    meters_per_unit: f64,
    unit_class: ClaimStatus,
    axis_map: String,
    axis_map_preserves_orientation: bool,
    angle_unit: AngleUnit,
    rotation_sense: RotationSense,
    axis_class: ClaimStatus,
}

impl WorldImportReport {
    /// The world record's stored slot.
    #[must_use]
    pub const fn world_node(&self) -> u32 {
        self.world_node
    }

    /// How many records the world node's own stored child list named.
    ///
    /// Measured to be the world's **non-spatial** content: over the eight
    /// containers these are the horizon, the volumetric fog volumes, vegetation
    /// instances and zeppelins, and none of them appears in the partition grid.
    #[must_use]
    pub const fn stored_child_list(&self) -> usize {
        self.stored_child_list
    }

    /// How many cells the partition grid holds.
    #[must_use]
    pub const fn partition_cells(&self) -> usize {
        self.partition_cells
    }

    /// How many records the grid names, distinct.
    #[must_use]
    pub const fn partition_records(&self) -> usize {
        self.partition_records
    }

    /// How many of the grid's records bind a mesh.
    ///
    /// Measured: in every container the grid records that bind **no** mesh are
    /// exactly the ones whose stored bounding box is all zero, so the two
    /// statements are the same set and neither is a reading this conversion had
    /// to invent.
    #[must_use]
    pub const fn partition_records_with_mesh(&self) -> usize {
        self.partition_records_with_mesh
    }

    /// How many of the grid's records are the original's fog volumes.
    ///
    /// **A measured disagreement, settled rather than only counted.** Task #716
    /// measured that the image's only name-keyed consumer of the four-byte
    /// `fvol` prefix is its fog system, while [`INDEXED_RECORD_IS_STATIC`] had
    /// resolved every grid-named record to `Solid`. Task #727 then measured
    /// what the image does with the grid itself — a broad-phase *candidate*
    /// index, never a solidity statement
    /// (`docs/findings/2026-10-07-f18-grid-collision-origin.md`) — and task
    /// #771 measured the candidate's own filter: with the record's
    /// [`INTERSECTION_NARROW_PHASE_FLAG`] clear the walk drops it before any
    /// box test, so these records now resolve role `None` under
    /// [`FOG_VOLUME_RECORD_NEVER_BLOCKS`] the way their unindexed siblings
    /// already did, and [`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`] stays only
    /// for a grid-named `fvol*` record that stores that bit. **This counter is
    /// the overlap, not an open question**: it says how many grid-named records
    /// the original's fog consumer keys — four in `c1c`, two in `c5`, none
    /// anywhere else — and a verdict that wants to know what is still
    /// unanswered must read [`Self::objects_unresolved_collision`] instead.
    #[must_use]
    pub const fn partition_records_fog_volume(&self) -> usize {
        self.partition_records_fog_volume
    }

    /// How many of the grid's records store **no geometry at all**: no mesh
    /// index and no non-empty stored box ([`GRID_RECORD_STORES_NO_GEOMETRY`]).
    ///
    /// This is the answered half of the grid's mesh-less records: the store
    /// gives them nothing a collider or a drawing could come from, so they
    /// resolve `None` and are never a skip. What a verdict asks about a
    /// grid record that stores **no mesh** is therefore
    ///
    /// ```text
    /// partition_records() - partition_records_with_mesh()
    ///                     - partition_records_stores_no_geometry()
    /// ```
    ///
    /// — grid records that bind no mesh **and** do store a box, which this
    /// claim does not speak for. Measured over the original installation:
    /// thirteen in `c1`, eight in `c1b`, one in `c1c`, twenty-three in `c2`,
    /// one in `c2b`, eighteen in `c3`, four in `c4` and eighty in `c5`.
    #[must_use]
    pub const fn partition_records_stores_no_geometry(&self) -> usize {
        self.partition_records_stores_no_geometry
    }

    /// How many cells name no record.
    #[must_use]
    pub const fn empty_cells(&self) -> usize {
        self.empty_cells
    }

    /// How many world objects the import produced.
    #[must_use]
    pub const fn objects(&self) -> usize {
        self.objects
    }

    /// How many of them resolved a mesh reference.
    #[must_use]
    pub const fn objects_with_mesh(&self) -> usize {
        self.objects_with_mesh
    }

    /// How many of them belong to at least one sector.
    #[must_use]
    pub const fn objects_in_a_sector(&self) -> usize {
        self.objects_in_a_sector
    }

    /// How many belong to no sector and are therefore always resident.
    #[must_use]
    pub const fn objects_resident(&self) -> usize {
        self.objects_resident
    }

    /// How many carry a resolved `Solid` collision role.
    ///
    /// Equal to [`Self::partition_records`] minus
    /// [`Self::partition_records_fog_volume`] minus
    /// [`Self::partition_records_stores_no_geometry`]: the role follows the
    /// spatial index (see [`INDEXED_RECORD_IS_STATIC`]) except for the
    /// grid-named fog volumes, which the original's own consumer takes as fog
    /// ([`FOG_VOLUME_RECORD_NEVER_BLOCKS`], settled by tasks #716/#727/#771),
    /// and the grid records that store no geometry at all
    /// ([`GRID_RECORD_STORES_NO_GEOMETRY`]). Subtracting the two measured
    /// sets from the index is what lets a consumer tell a world's static
    /// geometry from its fog volumes and its empty records with three numbers
    /// instead of one.
    #[must_use]
    pub const fn objects_solid(&self) -> usize {
        self.objects_solid
    }

    /// How many imported objects still carry `Unknown` for their **collision
    /// role** — the count a launch verdict reads to learn what is unanswered.
    ///
    /// A record the partition grid names resolves one through the index, the
    /// fog consumer or the store's own silence — **except** a grid-named
    /// `fvol*` record that stores [`INTERSECTION_NARROW_PHASE_FLAG`], which
    /// stays an explicit unknown under
    /// [`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`] and is counted here too, so
    /// this is [`Self::objects_unindexed_unresolved`] **plus** any such
    /// record. No record on the original installation stores that bit (task
    /// #771's retail test asserts both facts per container), so the two are
    /// equal there and this one is `0` for `zbd/c1c`. Either way a report
    /// where it is zero has no record whose collision behavior this conversion
    /// had to leave open. A record can still carry an explicit unknown for its
    /// **shape** (fog volumes and empty records do, on purpose); that is a
    /// statement about which geometry, not about whether it blocks.
    #[must_use]
    pub const fn objects_unresolved_collision(&self) -> usize {
        self.objects_unresolved_collision
    }

    /// How many unindexed records resolved to `None` — the anchors, groups and
    /// dummies that store no geometry of their own
    /// ([`UNINDEXED_RECORD_STORES_NO_GEOMETRY`], task #677).
    #[must_use]
    pub const fn objects_unindexed_none(&self) -> usize {
        self.objects_unindexed_none
    }

    /// How many unindexed records resolve to `None` because they are fog
    /// volumes — the `fvol*` records whose only measured consumer is the
    /// engine's fog system ([`FOG_VOLUME_RECORD_NEVER_BLOCKS`], task #716).
    ///
    /// Unlike [`Self::objects_unindexed_none`] these records **do** store a
    /// mesh and an extent: the store draws them, and what the measurement
    /// settled is that nothing measured reports a contact for them. Their mesh
    /// stays a known mesh reference, so a consumer can tell "no collider
    /// because the store states no geometry" from "no collider because the
    /// record is a fog volume" by the object's shape claim.
    #[must_use]
    pub const fn objects_unindexed_fog(&self) -> usize {
        self.objects_unindexed_fog
    }

    /// How many unindexed records still carry `Unknown` — the ones that store
    /// collision geometry (a mesh, an extent or both) without a class saying
    /// what the engine did with it ([`UNINDEXED_ROLE_UNMEASURED`], task #677's
    /// census, narrowed by task #716: the `fvol*` half left for the fog-volume
    /// measurement, and what remains here is every mesh-bearing record whose
    /// name carries no measured prefix and about which the container still
    /// says nothing).
    #[must_use]
    pub const fn objects_unindexed_unresolved(&self) -> usize {
        self.objects_unindexed_unresolved
    }

    /// How many sectors the definition declares.
    ///
    /// Equal to the cell count minus [`Self::sectors_without_extent`]: a cell
    /// whose members store no extent cannot be given a box, and its records stay
    /// resident rather than being attached to a sector with a made-up extent.
    #[must_use]
    pub const fn sectors(&self) -> usize {
        self.sectors
    }

    /// How many cells could not be given an extent.
    #[must_use]
    pub const fn sectors_without_extent(&self) -> usize {
        self.sectors_without_extent
    }

    /// How many mesh-binding records the container holds that the world node
    /// does not own.
    ///
    /// These are the effect hierarchies, the aircraft and the rest of the
    /// container's content: they bind meshes and have nothing to do with this
    /// world, so they are counted here rather than imported as world objects.
    #[must_use]
    pub const fn mesh_binding_records_elsewhere(&self) -> usize {
        self.mesh_binding_records_elsewhere
    }

    /// How many imported records store a matrix their own euler triple
    /// disagrees with.
    ///
    /// The disagreement is the store's, not this conversion's; the count is
    /// reported because the import follows the stored matrix (the format
    /// reader's own precedence rule) and a reader deserves to know how often
    /// that choice was a choice.
    #[must_use]
    pub const fn matrix_disagreements(&self) -> usize {
        self.matrix_disagreements
    }

    /// The stored-unit-to-metre factor the import used, from the caller's
    /// adapter.
    #[must_use]
    pub const fn meters_per_unit(&self) -> f64 {
        self.meters_per_unit
    }

    /// How strong the evidence behind that factor is.
    ///
    /// The **scale quantity's** own class, not the whole convention's: a
    /// source whose axis map or angle unit is unmeasured can still have a
    /// measured unit, and this accessor names which. `Unknown` for every
    /// declared fixture conversion ([`WORLD_UNIT_UNMEASURED`]); the measured
    /// GameZ source reports `ObservedTool`
    /// ([`crate::coordinates::GAMEZ_VERTEX_UNIT_IS_THE_METRE`], task #677).
    #[must_use]
    pub const fn unit_class(&self) -> ClaimStatus {
        self.unit_class
    }

    /// The axis map this import applied, spelled one entry per canonical axis.
    ///
    /// `"identity"` is the measured GameZ map ([`WORLD_AXIS_CONVENTION_MEASURED`]):
    /// every stored component feeds its own canonical axis with a positive sign.
    /// Anything else is spelled out as `"[+z, -x, +y]"` — canonical x is fed by
    /// stored `+z`, canonical y by stored `−x`, canonical z by stored `+y` — so
    /// a reader of an imported definition never has to infer which frame its
    /// positions came from.
    #[must_use]
    pub fn axis_map(&self) -> &str {
        &self.axis_map
    }

    /// Whether the applied axis map preserves orientation (its determinant is
    /// `+1`), i.e. whether stored content and canonical content are the same
    /// handedness.
    #[must_use]
    pub const fn axis_map_preserves_orientation(&self) -> bool {
        self.axis_map_preserves_orientation
    }

    /// The angle unit the source convention this import converted through
    /// carries, as the import did **not** convert angles: `.zrd` documents and
    /// GameZ nodes keep their own units, and this is the source's own answer
    /// for the GameZ side (`Radians` for `retail.gamez`).
    #[must_use]
    pub const fn angle_unit(&self) -> AngleUnit {
        self.angle_unit
    }

    /// The rotation sense the source convention carries
    /// ([`RotationSense::RightHandRule`] for the measured GameZ source).
    #[must_use]
    pub const fn rotation_sense(&self) -> RotationSense {
        self.rotation_sense
    }

    /// How strong the evidence is that the axis map, handedness and angle unit
    /// this import applied **are the original's**.
    ///
    /// `ObservedTool` when an installation-backed source applied exactly the
    /// map the owner's static analysis measured ([`WORLD_AXIS_CONVENTION_MEASURED`],
    /// task #436) — code-derived evidence over the decrypted image, never a
    /// run, so it never reaches [`ClaimStatus::VerifiedOriginal`].
    /// `Contradicted` when an installation-backed source applied anything
    /// else, because the measurement and the applied map disagree.
    /// `Unknown` for every designed or synthetic source: nothing about those
    /// conventions was measured ([`WORLD_UNIT_UNMEASURED`]).
    #[must_use]
    pub const fn axis_class(&self) -> ClaimStatus {
        self.axis_class
    }
}

/// One world container imported: the definition the runtime consumes and the
/// measurement the import was made under.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedWorld {
    definition: WorldDefinition,
    report: WorldImportReport,
}

impl ImportedWorld {
    /// The imported definition.
    #[must_use]
    pub const fn definition(&self) -> &WorldDefinition {
        &self.definition
    }

    /// The measurement.
    #[must_use]
    pub const fn report(&self) -> &WorldImportReport {
        &self.report
    }

    /// The two halves, for a caller that wants to keep them apart.
    #[must_use]
    pub fn into_parts(self) -> (WorldDefinition, WorldImportReport) {
        (self.definition, self.report)
    }
}

/// The stable identity of one imported world object: its container's node slot.
///
/// The slot is what the store addresses the record by, what the partition grid
/// names it by, and the one value the key grammar always accepts. See
/// [`OBJECT_ID_IS_THE_NODE_SLOT`] for why the record's display name is not used.
fn object_key(slot: u32) -> String {
    format!("node-{slot}")
}

/// The stable identity of one imported sector: its cell's grid coordinates.
fn sector_key(cell: &WorldPartitionCell) -> String {
    format!("partition-{:02}-{:02}", cell.grid_x(), cell.grid_y())
}

/// Whether a record's stored display name carries the engine's `fvol` prefix.
///
/// **The engine's own rule, mirrored.** The decrypted image compares names
/// with `strncmp(name, "fvol", 4)` — one reference in the whole image, inside
/// the routine that pairs those records with `fogvol.zrd`'s fog keys — so the
/// prefix, and only the prefix, is what the original keys on. Four bytes is
/// deliberately the same length the image compares: a record named `fvol` or
/// `fvol17` matches, a record named `volume` or `ffvol` does not.
///
/// See [`FOG_VOLUME_RECORD_NEVER_BLOCKS`] for the measurement and for what the
/// classification does and does not decide.
fn is_fog_volume(name: &str) -> bool {
    name.starts_with("fvol")
}

/// The record's stored world-space bounding box (`unk140`), in stored units.
///
/// The box is `None` when the record stores an all-zero one, which is the
/// store's own statement that the record has no extent — measured to hold for
/// exactly the records that bind no mesh.
fn stored_extent(record: &RawNode) -> Result<Option<StoredVolume>, WorldImportError> {
    let min = record.info.unk140[0].map(f64::from);
    let max = record.info.unk140[1].map(f64::from);
    let volume = StoredVolume::new(min, max)?;
    Ok(if volume.is_empty() {
        None
    } else {
        Some(volume)
    })
}

/// Whether a record stores **no geometry at all**: the store's own `-1` mesh
/// index and, in each of the record's three stored boxes, a zero extent on
/// every axis.
///
/// This is the store state task #771 resolved for grid-named records
/// ([`GRID_RECORD_STORES_NO_GEOMETRY`]): with no mesh and no non-empty box
/// there is nothing a collider or a drawing could come from. A box this
/// function cannot even parse (non-finite or inverted) is **not** "empty" —
/// the record keeps its container's silence rather than being told it stores
/// nothing.
fn stores_no_stored_geometry(record: &RawNode) -> bool {
    if record.mesh_index() >= 0 {
        return false;
    }
    [record.info.unk116, record.info.unk140, record.info.unk164]
        .iter()
        .all(|stored| {
            let min = stored[0].map(f64::from);
            let max = stored[1].map(f64::from);
            matches!(StoredVolume::new(min, max), Ok(volume) if volume.is_empty())
        })
}

/// Converts a stored box into canonical metres through `adapter`.
fn canonical_bounds(
    volume: &StoredVolume,
    adapter: &SourceAdapter,
) -> Result<Aabb, WorldImportError> {
    let low = adapter.position_to_canonical(volume.min())?.to_array();
    let high = adapter.position_to_canonical(volume.max())?.to_array();
    // The axis map is a signed permutation and the scale is positive, so the two
    // corners can swap: a min is taken after the conversion, never before it.
    let mut min = [0.0f64; 3];
    let mut max = [0.0f64; 3];
    for axis in 0..3 {
        min[axis] = low[axis].min(high[axis]);
        max[axis] = low[axis].max(high[axis]);
    }
    Aabb::try_new(min, max).map_err(WorldImportError::Bounds)
}

/// Converts one object record's authored transform into canonical metres.
///
/// The stored 3×3 wins over the record's own euler triple, which is the format
/// reader's precedence rule
/// ([`RawObject3dData::matrix`](cs_formats::gamez::RawObject3dData::matrix));
/// the per-axis scale multiplies the matrix's columns; the whole linear map is
/// then conjugated through the adapter's signed permutation and the translation
/// scaled by its length factor. A record that stores the identity is the store
/// saying so, not a default this conversion chose.
fn canonical_transform(
    object: &RawObject3dData,
    adapter: &SourceAdapter,
) -> Result<CanonicalTransform, WorldImportError> {
    if object.stores_identity() {
        return Ok(CanonicalTransform::IDENTITY);
    }
    let mut source = object.matrix.map(|row| row.map(f64::from));
    for (column, scale) in object.scale.iter().enumerate() {
        for row in &mut source {
            row[column] *= f64::from(*scale);
        }
    }
    let axes = adapter.source().convention().axes();
    let mut linear = [[0.0f64; 3]; 3];
    for (canonical_row, row) in axes.iter().enumerate() {
        for (canonical_column, column) in axes.iter().enumerate() {
            linear[canonical_row][canonical_column] = row.sign.factor()
                * column.sign.factor()
                * source[row.axis.index()][column.axis.index()];
        }
    }
    let translation = adapter
        .position_to_canonical(object.translation.map(f64::from))?
        .to_array();
    CanonicalTransform::try_new(linear, translation).map_err(WorldImportError::Space)
}

/// Imports one world container's node array and its own partition grid into a
/// [`WorldDefinition`].
///
/// **What this reads.** The world record's partition grid, through
/// [`WorldPartitionGrid::read`]; every record that record owns — measured to be
/// exactly the grid's slots plus the world node's own stored child list, and
/// exactly the records that name the world node as their parent, with the three
/// statements cross-checked against each other rather than one assumed; each
/// owned record's stored mesh index, transform and world-space bounding box.
///
/// **What this refuses to guess.** Everything the container does not state:
///
/// * the world's boundary, floor and ceiling ([`WORLD_BOUNDARY_UNMEASURED`]);
/// * every object's gameplay surface ([`WORLD_SURFACE_UNMEASURED`]);
/// * the collision role of an unindexed record that stores collision
///   geometry — a mesh, an extent or both — but no class saying what the
///   engine did with it ([`UNINDEXED_ROLE_UNMEASURED`]), and with one
///   measured exception: a record whose name carries the `fvol` prefix is a
///   fog volume and resolves to `None` ([`FOG_VOLUME_RECORD_NEVER_BLOCKS`]);
/// * the length unit, which the caller's [`SourceAdapter`] supplies and the
///   report names ([`WORLD_UNIT_UNMEASURED`], or a measured claim id when the
///   adapter's source has one).
///
/// An object the grid names becomes the world's static geometry
/// ([`INDEXED_RECORD_IS_STATIC`]) — a **designed** rule over a measured fact,
/// not a measurement of how the 2000 engine collided — with two measured
/// exceptions, both about what the container itself states: a grid-named
/// record whose name the original's fog-volume consumer keys, and which the
/// image's intersection walk drops before any box test, resolves role `None`
/// ([`FOG_VOLUME_RECORD_NEVER_BLOCKS`], the overlap reported by
/// [`WorldImportReport::partition_records_fog_volume`]); and a grid-named
/// record that stores no mesh and no non-empty box resolves `None` because the
/// store states no geometry for it ([`GRID_RECORD_STORES_NO_GEOMETRY`],
/// [`WorldImportReport::partition_records_stores_no_geometry`]). A grid-named
/// `fvol*` record that stores the narrow-phase bit still resolves role-unknown
/// ([`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`]). An unindexed object
/// that binds no mesh and stores no extent is the store saying this record has
/// no geometry, so its role resolves to `None`
/// ([`UNINDEXED_RECORD_STORES_NO_GEOMETRY`]) — measured to be exactly the
/// anchors, transform groups and dummies. An unindexed object whose stored
/// name carries the `fvol` prefix resolves to `None` under
/// [`FOG_VOLUME_RECORD_NEVER_BLOCKS`], because the image's only name-keyed
/// consumer of that prefix is its fog system. Its id is its node slot
/// ([`OBJECT_ID_IS_THE_NODE_SLOT`]). Every object whose role this function
/// leaves open is counted by [`WorldImportReport::objects_unresolved_collision`].
///
/// **The axis convention is bound, not assumed.** The report states the axis
/// map this conversion applied, whether it preserves orientation, the source
/// convention's angle unit and rotation sense, and how strong the evidence is
/// that those are the original's ([`WorldImportReport::axis_map`],
/// [`WorldImportReport::axis_class`], [`WORLD_AXIS_CONVENTION_MEASURED`]): an
/// installation-backed source that applied the measured identity map reports
/// `ObservedTool`, one that applied anything else reports `Contradicted`, and
/// a designed source reports `Unknown`.
///
/// `meshes` is the caller's mesh-slot table, exactly as
/// [`world_hierarchy_from_gamez`] takes it: which catalog element a stored
/// mesh-array slot stands for is discovery's answer, not this function's.
///
/// # Errors
///
/// Every [`WorldImportError`]: the grid refusals, the world-ownership
/// cross-check ([`WorldImportError::OwnershipDisagreement`]), a stored
/// coordinate or box that is not one
/// ([`WorldImportError::Space`], [`WorldImportError::Volume`],
/// [`WorldImportError::Bounds`]), a mesh index the caller's table does not hold
/// ([`WorldImportError::MeshSlotMissing`]), and whatever the definition's own
/// validation refuses
/// ([`WorldImportError::Definition`]).
pub fn import_world_container(
    id: WorldId,
    origin: Origin,
    records: &GameZNodes,
    container: &[u8],
    meshes: &[MeshSlot],
    adapter: &SourceAdapter,
    provenance: Provenance,
) -> Result<ImportedWorld, WorldImportError> {
    let grid = WorldPartitionGrid::read(records, container)?;
    let world_slot = grid.world_node();
    let stored = records
        .get(world_slot)
        .ok_or(WorldImportError::NoWorldNode)?;
    let indexed: BTreeSet<u32> = grid.indexed_slots().into_iter().collect();
    let child_list: BTreeSet<u32> = stored.children.iter().copied().collect();
    let naming: BTreeSet<u32> = records
        .nodes
        .iter()
        .filter(|node| node.parent == Some(world_slot))
        .map(|node| node.index)
        .collect();
    let union: BTreeSet<u32> = indexed.union(&child_list).copied().collect();
    if union != naming {
        return Err(WorldImportError::OwnershipDisagreement {
            grid: indexed.len(),
            child_list: child_list.len(),
            naming: naming.len(),
        });
    }

    // Sectors: one per cell that can be given an extent, which is the union of
    // the stored boxes its members state. A cell whose members all store a zero
    // box gets no sector, and its records stay resident.
    let mut sectors: Vec<Sector> = Vec::with_capacity(grid.cells().len());
    let mut sector_of_cell: BTreeMap<u32, SectorId> = BTreeMap::new();
    let mut sectors_without_extent = 0usize;
    for cell in grid.cells() {
        let mut bounds: Option<Aabb> = None;
        for slot in cell.slots() {
            let Some(record) = records.get(*slot) else {
                continue;
            };
            let Some(extent) = stored_extent(record)? else {
                continue;
            };
            let converted = canonical_bounds(&extent, adapter)?;
            bounds = Some(match bounds {
                None => converted,
                Some(previous) => {
                    let min = [
                        previous.min()[0].min(converted.min()[0]),
                        previous.min()[1].min(converted.min()[1]),
                        previous.min()[2].min(converted.min()[2]),
                    ];
                    let max = [
                        previous.max()[0].max(converted.max()[0]),
                        previous.max()[1].max(converted.max()[1]),
                        previous.max()[2].max(converted.max()[2]),
                    ];
                    Aabb::try_new(min, max)?
                }
            });
        }
        let Some(bounds) = bounds else {
            sectors_without_extent += 1;
            continue;
        };
        let id = SectorId::new(&sector_key(cell))?;
        sector_of_cell.insert(cell.index(), id.clone());
        sectors.push(Sector::new(id, bounds));
    }

    // Objects: every record the world node owns, in stored order.
    let mut objects: Vec<WorldObjectInstance> = Vec::with_capacity(union.len());
    let mut objects_with_mesh = 0usize;
    let mut objects_in_a_sector = 0usize;
    let mut objects_resident = 0usize;
    let mut objects_solid = 0usize;
    let mut objects_unindexed_none = 0usize;
    let mut objects_unindexed_fog = 0usize;
    let mut objects_unindexed_unresolved = 0usize;
    let mut matrix_disagreements = 0usize;
    let mut partition_records_with_mesh = 0usize;
    let mut partition_records_fog_volume = 0usize;
    let mut partition_records_stores_no_geometry = 0usize;
    let mut objects_unresolved_collision = 0usize;
    for record in records
        .nodes
        .iter()
        .filter(|node| union.contains(&node.index))
    {
        let StoredNodeKind::Object3d(object) = record.kind else {
            return Err(WorldImportError::WorldOwnedNotAnObject {
                slot: record.index,
                kind: record.kind.label(),
            });
        };
        let indexed_record = indexed.contains(&record.index);
        if object.matrix_disagrees() {
            matrix_disagreements += 1;
        }
        let mesh = if record.mesh_index() < 0 {
            Resolved::Unknown {
                claim_id: claim(OBJECT_STORES_NO_MESH),
                reason: format!(
                    "node slot {} stores no mesh index, so this record draws geometry of its \
                     own nowhere in the container",
                    record.index
                ),
            }
        } else {
            let index = u32::try_from(record.mesh_index()).unwrap_or(u32::MAX);
            let slot = meshes
                .get(index as usize)
                .ok_or(WorldImportError::MeshSlotMissing {
                    index,
                    slots: meshes.len(),
                })?;
            if indexed_record {
                partition_records_with_mesh += 1;
            }
            objects_with_mesh += 1;
            Resolved::Known(Known::new(slot.id().clone(), provenance.clone()))
        };
        let (collision, shape) = if indexed_record && is_fog_volume(&record.name) {
            // Measured (task #716), the candidate index settled (task #727),
            // and the candidate's own filter settled (task #771): the image's
            // only name-keyed consumer of the `fvol` prefix is its fog system,
            // and the grid this record is named by is read by the image as a
            // broad-phase **candidate** index — `cls_di.c`'s builder walks
            // `0x4cb579` and filters each candidate by node flags, a zone
            // whitelist and an optional name before any box is tested — never
            // as a solidity statement. The filter is decisive for these
            // records: at `0x4cb635` the walk reads the record's own flags
            // word and takes the narrow phase (the only branch that copies a
            // box through `[node+0x70]`) only when bit `0x40` is set, and
            // otherwise recurses into children — so a grid-named `fvol*`
            // record without that bit is **dropped before any box test**. The
            // installation's six all store it clear (`0x0308831c`, no
            // children), so their role resolves the same way an unindexed fog
            // volume's does. A record that *did* store the bit would still be
            // a live candidate, and keeps #727's explicit unknown.
            partition_records_fog_volume += 1;
            if record.flags() & INTERSECTION_NARROW_PHASE_FLAG == 0 {
                let reason = format!(
                    "node slot {} is named by the world record's partition grid and its name is \
                     one the original's fog-volume consumer takes as fog: the grid is a \
                     broad-phase candidate index, and the image's intersection walk reads this \
                     record's flags word at 0x4cb635 and, with its narrow-phase bit 0x{:08x} \
                     clear, drops it before any box test (0x4cb63c-0x4cb642), so nothing \
                     measured reports a contact for it",
                    record.index, INTERSECTION_NARROW_PHASE_FLAG,
                );
                (
                    Resolved::Known(Known::new(WorldCollisionRole::None, provenance.clone())),
                    Resolved::Unknown {
                        claim_id: claim(FOG_VOLUME_RECORD_NEVER_BLOCKS),
                        reason,
                    },
                )
            } else {
                let reason = format!(
                    "node slot {} is named by the world record's partition grid, but its name is \
                     one the original's fog-volume consumer takes as fog, and the container \
                     states no collision role for it",
                    record.index
                );
                (
                    Resolved::Unknown {
                        claim_id: claim(GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED),
                        reason: reason.clone(),
                    },
                    Resolved::Unknown {
                        claim_id: claim(GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED),
                        reason,
                    },
                )
            }
        } else if indexed_record && stores_no_stored_geometry(record) {
            // Measured (task #771): this record binds no mesh (`mesh_index` is
            // the store's own `-1` = "no mesh") **and** every one of its three
            // stored boxes is empty, so the container states no geometry a
            // collider or a drawing could come from — the same store state
            // task #677 resolved for unindexed records, here on a record the
            // grid names. The index can only make it a *candidate* (#727), and
            // a candidate that stores nothing has nothing to test: it is
            // presented and never blocks, with its absent mesh kept as an
            // explicit unknown so "the store states no mesh" stays readable.
            partition_records_stores_no_geometry += 1;
            let reason = format!(
                "node slot {} is named by the world record's partition grid and stores no mesh \
                 index and no non-empty stored box, so the container states no geometry for it",
                record.index
            );
            (
                Resolved::Known(Known::new(WorldCollisionRole::None, provenance.clone())),
                Resolved::Unknown {
                    claim_id: claim(GRID_RECORD_STORES_NO_GEOMETRY),
                    reason,
                },
            )
        } else if indexed_record {
            objects_solid += 1;
            (
                Resolved::Known(Known::new(WorldCollisionRole::Solid, provenance.clone())),
                Resolved::Known(Known::new(
                    WorldCollisionShape::FromMesh,
                    provenance.clone(),
                )),
            )
        } else if is_fog_volume(&record.name) {
            // Measured (task #716): the image's only name-keyed consumer of the
            // `fvol` prefix is the fog system, so this record is presented and
            // never blocks — while its mesh stays a known mesh reference, the
            // box it draws is still drawn.
            objects_unindexed_fog += 1;
            (
                Resolved::Known(Known::new(WorldCollisionRole::None, provenance.clone())),
                Resolved::Unknown {
                    claim_id: claim(FOG_VOLUME_RECORD_NEVER_BLOCKS),
                    reason: format!(
                        "node slot {} carries the measured `fvol` prefix, so its only \
                         measured consumer is the engine's fog system: the record is \
                         drawn and nothing measured reports a contact for it",
                        record.index
                    ),
                },
            )
        } else if record.mesh_index() < 0 && stored_extent(record)?.is_none() {
            // Measured (task #677): the partition grid omits either both of a
            // record's geometry fields or neither, so this arm is exactly the
            // anchors, groups and dummies — a record with no mesh and no
            // extent has nothing a collider could be built from.
            objects_unindexed_none += 1;
            (
                Resolved::Known(Known::new(WorldCollisionRole::None, provenance.clone())),
                Resolved::Unknown {
                    claim_id: claim(UNINDEXED_RECORD_STORES_NO_GEOMETRY),
                    reason: format!(
                        "node slot {} binds no mesh and stores no bounding box, so the \
                         container states no shape for it",
                        record.index
                    ),
                },
            )
        } else {
            objects_unindexed_unresolved += 1;
            let reason = format!(
                "node slot {} is not named by the world record's partition grid and the \
                 container states no collision role for it",
                record.index
            );
            (
                Resolved::Unknown {
                    claim_id: claim(UNINDEXED_ROLE_UNMEASURED),
                    reason: reason.clone(),
                },
                Resolved::Unknown {
                    claim_id: claim(UNINDEXED_ROLE_UNMEASURED),
                    reason,
                },
            )
        };
        // What is still open, counted once per object: a role this container
        // states nothing about is the whole of the import's unanswered
        // collision behavior (task #771).
        if matches!(&collision, Resolved::Unknown { .. }) {
            objects_unresolved_collision += 1;
        }
        let surface = Resolved::Unknown {
            claim_id: claim(WORLD_SURFACE_UNMEASURED),
            reason: format!(
                "node slot {} stores no gameplay surface class",
                record.index
            ),
        };
        let mut sectors_of: Vec<SectorId> = Vec::new();
        for cell in grid.cells() {
            if cell.slots().contains(&record.index)
                && let Some(id) = sector_of_cell.get(&cell.index())
                && !sectors_of.contains(id)
            {
                sectors_of.push(id.clone());
            }
        }
        if sectors_of.is_empty() {
            objects_resident += 1;
        } else {
            objects_in_a_sector += 1;
        }
        objects.push(WorldObjectInstance::try_new(
            WorldObjectId::new(&object_key(record.index))?,
            mesh,
            canonical_transform(&object, adapter)?,
            collision,
            shape,
            surface,
            sectors_of.into_iter().collect(),
            provenance.clone(),
        )?);
    }

    let boundary = Resolved::Unknown {
        claim_id: claim(WORLD_BOUNDARY_UNMEASURED),
        reason: "the world record stores no floor, ceiling or lateral rule this stage can read"
            .to_owned(),
    };
    let definition = WorldDefinition::try_new(id, origin, boundary, sectors, objects, provenance)?;
    let mesh_binding_records_elsewhere = records
        .nodes
        .iter()
        .filter(|node| !union.contains(&node.index) && node.mesh_index() >= 0)
        .count();
    let report = WorldImportReport {
        world_node: world_slot,
        stored_child_list: child_list.len(),
        partition_cells: grid.cells().len(),
        partition_records: indexed.len(),
        partition_records_with_mesh,
        partition_records_fog_volume,
        partition_records_stores_no_geometry,
        empty_cells: grid.empty_cells().len(),
        objects: definition.objects().len(),
        objects_with_mesh,
        objects_in_a_sector,
        objects_resident,
        objects_solid,
        objects_unindexed_none,
        objects_unindexed_fog,
        objects_unindexed_unresolved,
        objects_unresolved_collision,
        sectors: definition.sectors().len(),
        sectors_without_extent,
        mesh_binding_records_elsewhere,
        matrix_disagreements,
        meters_per_unit: adapter.source().convention().meters_per_unit(),
        unit_class: adapter
            .source()
            .calibration()
            .quantity_status(CalibratedQuantity::Scale),
        axis_map: axis_map_label(adapter),
        axis_map_preserves_orientation: adapter.source().convention().is_orientation_preserving(),
        angle_unit: adapter.source().convention().angle_unit(),
        rotation_sense: adapter.source().convention().rotation_sense(),
        axis_class: axis_class(adapter),
    };
    Ok(ImportedWorld { definition, report })
}

/// The axis map [`import_world_container`] applied, as the report states it.
///
/// `"identity"` when every stored component feeds its own canonical axis with a
/// positive sign — the map task #436 measured the original to use — and the
/// spelled-out permutation otherwise, so a reader never infers the frame.
fn axis_map_label(adapter: &SourceAdapter) -> String {
    let axes = adapter.source().convention().axes();
    let identity = axes
        .iter()
        .enumerate()
        .all(|(index, source)| source.axis.index() == index && !source.sign.is_negative());
    if identity {
        return "identity".to_owned();
    }
    let spelled: Vec<String> = axes
        .iter()
        .map(|source| {
            format!(
                "{}{}",
                if source.sign.is_negative() { "-" } else { "+" },
                source.axis.label()
            )
        })
        .collect();
    format!("[{}]", spelled.join(", "))
}

/// The evidence class for "the axis convention this import applied **is** the
/// original's", as [`WorldImportReport::axis_class`] documents it.
fn axis_class(adapter: &SourceAdapter) -> ClaimStatus {
    let Origin::Installation { .. } = adapter.source().origin() else {
        // Designed or synthetic content declares its own convention; nothing
        // about the original was measured through it.
        return ClaimStatus::Unknown;
    };
    let axes = adapter.source().convention().axes();
    let identity = axes
        .iter()
        .enumerate()
        .all(|(index, source)| source.axis.index() == index && !source.sign.is_negative());
    if identity {
        ClaimStatus::ObservedTool
    } else {
        // The measurement (task #436) says identity; this source applied
        // something else over original bytes. The two disagree, and the report
        // says so rather than quietly trusting one of them.
        ClaimStatus::Contradicted
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
