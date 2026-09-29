//! Collision layer declarations and contact classification (F23-A).
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! section "Collision and ballistic tests".
//!
//! This module is the **typed vocabulary only**: which collision layers the
//! game declares, which of them the designed matrix lets interact, when an
//! overlap is a sensor report instead of a solid contact, and which layers
//! need swept/continuous detection. It creates no Avian body and reads no
//! original data; the Avian binding is `cs_app::physics`.
//!
//! **Designed vocabulary, not original data.** The six layer names come from
//! the spec's non-negotiable behavior 2 (aircraft, projectiles, static world,
//! debris, triggers, cameras). Their bits, the designed interaction matrix and
//! the sensor-versus-solid split are newly authored project design. Which
//! layers the original 2000 PC game used, which pairs it let interact and
//! whether its triggers ever dealt damage directly are **unknown** until the
//! compatibility work measures them, so nothing here claims to reproduce the
//! original. See
//! `docs/findings/2026-09-29-f23-a-avian-schedule-adapter-and-collision-layers.md`.
//!
//! `cs_sim` is not the physics adapter (`docs/01-ARCHITECTURE.md`): this module
//! stays dependency-free so gameplay can label an actor without linking Avian.

use std::fmt;

/// One declared collision layer.
///
/// Every dynamic actor, projectile, trigger and camera is assigned exactly one
/// of these. The discriminants are the layer's bit index; use
/// [`CollisionLayer::bit`] or [`CollisionLayers`] rather than the raw value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum CollisionLayer {
    /// Player and AI aircraft bodies.
    Aircraft = 0,
    /// Bullets, rockets and other fast ordnance bodies.
    Projectile = 1,
    /// Terrain, buildings and other immovable world geometry.
    StaticWorld = 2,
    /// Detached, tumbling wreckage.
    Debris = 3,
    /// Sensor volumes that report overlap (checkpoints, activation zones).
    Trigger = 4,
    /// The presentation camera query filter; it must never generate contacts.
    Camera = 5,
}

impl CollisionLayer {
    /// Every declared layer, in ascending bit order.
    pub const ALL: [Self; 6] = [
        Self::Aircraft,
        Self::Projectile,
        Self::StaticWorld,
        Self::Debris,
        Self::Trigger,
        Self::Camera,
    ];

    /// The stable label used in reports and persisted layer sets.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Aircraft => "aircraft",
            Self::Projectile => "projectile",
            Self::StaticWorld => "static_world",
            Self::Debris => "debris",
            Self::Trigger => "trigger",
            Self::Camera => "camera",
        }
    }

    /// Looks a layer up by its label; `None` for an unknown label.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|layer| layer.label() == label)
    }

    /// This layer's single bit, `1 << discriminant`.
    pub const fn bit(self) -> u8 {
        1 << self as u8
    }

    /// Whether objects on this layer may move fast enough to tunnel through a
    /// thin obstacle or a narrow trigger within one tick.
    ///
    /// The spec's non-negotiable behavior 3 requires swept/continuous tests
    /// for fast projectiles and narrow triggers specifically, so those two
    /// layers carry the requirement rather than every dynamic body.
    pub const fn requires_continuous_detection(self) -> bool {
        matches!(self, Self::Projectile | Self::Trigger)
    }

    /// Whether the designed collision matrix lets these two layers generate
    /// contacts.
    ///
    /// The matrix is symmetric and leaves [`CollisionLayer::Camera`] inert:
    /// the presentation camera is a query filter, never a collider pair. The
    /// pair list is designed project content, not measured original behavior.
    pub const fn designed_collides_with(self, other: Self) -> bool {
        use CollisionLayer::{Aircraft, Debris, Projectile, StaticWorld, Trigger};

        // Normalize so each unordered pair is written once.
        let (a, b) = if (self as u8) <= (other as u8) {
            (self, other)
        } else {
            (other, self)
        };

        matches!(
            (a, b),
            (Aircraft, Aircraft)
                | (Aircraft, Projectile)
                | (Aircraft, StaticWorld)
                | (Aircraft, Debris)
                | (Aircraft, Trigger)
                | (Projectile, Projectile)
                | (Projectile, StaticWorld)
                | (Projectile, Debris)
                | (Projectile, Trigger)
                | (StaticWorld, StaticWorld)
                | (StaticWorld, Debris)
                | (StaticWorld, Trigger)
                | (Debris, Debris)
                | (Debris, Trigger)
        )
    }
}

impl fmt::Display for CollisionLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A set of collision layers, stored as a bitmask over the declared bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CollisionLayers(u8);

impl CollisionLayers {
    /// The empty set.
    pub const NONE: Self = Self(0);

    /// Every declared layer.
    pub const ALL: Self = Self(0b0011_1111);

    /// The bits that [`CollisionLayers`] actually stores.
    const MASK: u8 = Self::ALL.0;

    /// Builds a set from raw bits, dropping any bit not assigned to a layer.
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & Self::MASK)
    }

    /// The stored bits.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether the set contains `layer`.
    pub const fn contains(self, layer: CollisionLayer) -> bool {
        self.0 & layer.bit() != 0
    }

    /// Whether the two sets share at least one layer.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// This set plus `layer`.
    pub const fn with(self, layer: CollisionLayer) -> Self {
        Self(self.0 | layer.bit())
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl From<CollisionLayer> for CollisionLayers {
    fn from(layer: CollisionLayer) -> Self {
        Self(layer.bit())
    }
}

impl FromIterator<CollisionLayer> for CollisionLayers {
    fn from_iter<T: IntoIterator<Item = CollisionLayer>>(iter: T) -> Self {
        iter.into_iter().fold(Self::NONE, Self::with)
    }
}

impl fmt::Display for CollisionLayers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        f.write_str("[")?;
        for layer in CollisionLayer::ALL {
            if self.contains(layer) {
                if !first {
                    f.write_str(", ")?;
                }
                f.write_str(layer.label())?;
                first = false;
            }
        }
        f.write_str("]")
    }
}

/// The physical shape class of a collider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeClass {
    /// A solid collider that resolves contacts and can impart forces.
    Solid,
    /// A sensor collider that only reports overlap.
    Sensor,
}

impl ShapeClass {
    /// Whether this shape is a sensor.
    pub const fn is_sensor(self) -> bool {
        matches!(self, Self::Sensor)
    }
}

/// The designed classification of one overlap between two colliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactKind {
    /// The layers do not interact; no contact or overlap is reported.
    Ignored,
    /// A solid contact between interacting layers; the solver resolves it.
    SolidContact,
    /// A sensor overlap. It is reported as an event and is **never** damage on
    /// its own (F23 non-negotiable behavior 2).
    SensorOverlap,
}

impl ContactKind {
    /// Whether this classification is a sensor overlap.
    pub const fn is_sensor_overlap(self) -> bool {
        matches!(self, Self::SensorOverlap)
    }

    /// Whether this classification is a resolved solid contact.
    pub const fn is_solid_contact(self) -> bool {
        matches!(self, Self::SolidContact)
    }
}

/// Classifies one overlap between colliders on layers `a` and `b`.
///
/// A sensor on either side makes an interacting pair a [`ContactKind::SensorOverlap`]
/// rather than a [`ContactKind::SolidContact`], which is how the sensor/damage
/// boundary is enforced in code instead of by convention. Non-interacting
/// layers are [`ContactKind::Ignored`] whether or not a sensor is present.
pub const fn classify_contact(
    a: CollisionLayer,
    b: CollisionLayer,
    a_shape: ShapeClass,
    b_shape: ShapeClass,
) -> ContactKind {
    if !CollisionLayer::designed_collides_with(a, b) {
        return ContactKind::Ignored;
    }
    if a_shape.is_sensor() || b_shape.is_sensor() {
        return ContactKind::SensorOverlap;
    }
    ContactKind::SolidContact
}
