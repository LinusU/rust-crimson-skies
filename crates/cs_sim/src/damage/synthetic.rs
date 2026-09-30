//! The minimal synthetic damage fixture (F29-A).
//!
//! One declared synthetic airframe damage graph the acceptance tests drive.
//! Every value is newly authored fixture content — `Origin::SyntheticFixture`
//! on the declared record, designed provenance on every pool — never a
//! stand-in for missing retail data.

use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use super::graph::{DamageGraph, DamageNode, DamageNodeKey, DamageNodeKind};

/// The catalog id of the synthetic airframe the fixture describes.
pub const SYNTHETIC_AIRFRAME_KEY: &str = "synthetic.devastator";

/// The fixture's armor zone: it guards [`SYNTHETIC_HULL_NODE`].
pub const SYNTHETIC_ARMOR_NODE: &str = "nose_armor";
/// The fixture's lethal internal structure.
pub const SYNTHETIC_HULL_NODE: &str = "hull";
/// The fixture's engine.
pub const SYNTHETIC_ENGINE_NODE: &str = "engine_1";
/// The fixture's weapon mount.
pub const SYNTHETIC_MOUNT_NODE: &str = "gun_mount_1";

/// The nose armor's integrity pool.
pub const SYNTHETIC_ARMOR_INTEGRITY: f64 = 20.0;
/// The hull's integrity pool.
pub const SYNTHETIC_HULL_INTEGRITY: f64 = 40.0;
/// The engine's integrity pool.
pub const SYNTHETIC_ENGINE_INTEGRITY: f64 = 15.0;
/// The gun mount's integrity pool.
pub const SYNTHETIC_MOUNT_INTEGRITY: f64 = 10.0;

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("fixture node keys are valid")
}

fn designed_integrity(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(
            ClaimId::new("f29a.synthetic-devastator").expect("fixture claim id is valid"),
        ),
    ))
}

/// The minimal synthetic airframe damage graph:
///
/// * `nose_armor` — an [`DamageNodeKind::ArmorZone`] guarding `hull`,
///   overflowing into it once depleted;
/// * `hull` — the lethal [`DamageNodeKind::InternalStructure`], armor-guarded;
/// * `engine_1` — an [`DamageNodeKind::Engine`] disabling
///   [`crate::damage::SystemKind::Propulsion`];
/// * `gun_mount_1` — a [`DamageNodeKind::WeaponMount`] disabling
///   [`crate::damage::SystemKind::Weapon`].
///
/// The graph exercises every declared mechanism: armor routing, overflow,
/// a lethal part and two disabling parts.
#[must_use]
pub fn synthetic_airframe_graph() -> DamageGraph {
    DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY)
            .expect("fixture subject id is valid"),
        vec![
            DamageNode::new(
                key(SYNTHETIC_ARMOR_NODE),
                DamageNodeKind::ArmorZone,
                designed_integrity(SYNTHETIC_ARMOR_INTEGRITY),
            )
            .with_overflow(key(SYNTHETIC_HULL_NODE)),
            DamageNode::new(
                key(SYNTHETIC_HULL_NODE),
                DamageNodeKind::InternalStructure,
                designed_integrity(SYNTHETIC_HULL_INTEGRITY),
            )
            .with_lethal(true)
            .with_guard(key(SYNTHETIC_ARMOR_NODE)),
            DamageNode::new(
                key(SYNTHETIC_ENGINE_NODE),
                DamageNodeKind::Engine,
                designed_integrity(SYNTHETIC_ENGINE_INTEGRITY),
            )
            .with_disables(crate::damage::SystemKind::Propulsion),
            DamageNode::new(
                key(SYNTHETIC_MOUNT_NODE),
                DamageNodeKind::WeaponMount,
                designed_integrity(SYNTHETIC_MOUNT_INTEGRITY),
            )
            .with_disables(crate::damage::SystemKind::Weapon),
        ],
    )
    .expect("the synthetic airframe damage graph is valid")
}
