//! The player as a damage victim in M01 (PARITY-M01-OUTCOME-A, #1653).
//!
//! The mission's scripted aircraft already fly, arm and fire
//! ([`crate::mission_ai`]), but a round they fire sweeps only
//! [`PartSweptBox`] entities, and the player body carried none: the player
//! was not registered with the shared damage resolver either. An idle player
//! therefore could never be hurt, whatever the hostile Kestrels did. This
//! module makes the player a victim through the same production path every
//! other target uses: a damage actor registered with the session's resolver, a
//! [`DamageActorBinding`] on the body, and one swept box hung under it.
//!
//! # What is designed, and says so
//!
//! * **The integrity pool** ([`DESIGNED_PLAYER_INTEGRITY`]). No M01 or
//!   per-airframe damage table is measured (`mission_actors` leaves every
//!   scripted actor's pool an explicit unknown for the same reason), so the
//!   player's hull pool is project design under
//!   [`M01_PLAYER_DAMAGE_CLAIM`], and the measurement it stands in for is
//!   carried beside it as an explicit unknown ([`PlayerVictim::unmeasured`]).
//!   The value is **not** fitted to the retail capture's first-damage or
//!   failure times.
//! * **The hull box** ([`DESIGNED_PLAYER_HALF_EXTENTS_M`]) is the same
//!   development box the playtest's airframe collider uses; the original's
//!   collision shape is unmeasured.

use bevy::prelude::{
    Component, Entity, GlobalTransform, Query, Resource, Transform, With, Without, World,
};
use cs_sim::damage::{
    AttributionRule, DamageGraph, DamageNode, DamageNodeKey, DamageNodeKind, DamagePolicy,
};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::ActorId;

use crate::airframe_damage::AirframeDamageRegion;
use crate::damage::DamageActorBinding;
use crate::scene::{NodeVisualTransform, SceneGeneration};
use crate::weapons::{PartSweptBox, SessionDamageResolver};

use super::weapons::{MISSION_DAMAGE_PRODUCER, MISSION_TARGET_NODE};

/// The claim the player's designed victim values are filed under.
pub const M01_PLAYER_DAMAGE_CLAIM: &str = "m01-player-victim.designed-hull-integrity";

/// The claim the unmeasured player damage table is filed under.
pub const M01_PLAYER_DAMAGE_UNKNOWN_CLAIM: &str = "m01-player-victim.damage-table-unmeasured";

/// The designed integrity of the player's hull, in the damage unit the gun
/// records declare. Project design, not a measurement, and not fitted to any
/// capture: a hundred of the designed gun's twelve-unit rounds.
pub const DESIGNED_PLAYER_INTEGRITY: f64 = 1_200.0;

/// The claim the designed region integrity is filed under.
pub const M01_PLAYER_REGION_CLAIM: &str = "m01-player-victim.designed-region-integrity";

/// The claim the unmeasured per-part integrity and threshold table is filed
/// under.
pub const M01_PLAYER_REGION_UNKNOWN_CLAIM: &str = "m01-player-victim.region-integrity-unmeasured";

/// The designed integrity of each airframe region node: a quarter of the
/// hull pool. Project design, not a measurement; the `destroyable_parts`
/// records of `player_pfighter` carry no recovered integrity.
pub const DESIGNED_REGION_INTEGRITY: f64 = DESIGNED_PLAYER_INTEGRITY / 4.0;

/// The designed half extents of the player's hull box, in metres.
pub const DESIGNED_PLAYER_HALF_EXTENTS_M: [f64; 3] = [6.0, 1.0, 5.0];

/// The designed values the player became a victim with, and the measurement
/// they stand in for.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct PlayerVictim {
    /// The player's damage actor.
    pub actor: ActorId,
    /// The designed hull integrity.
    pub integrity: Known<f64>,
    /// The original's per-airframe damage table, an explicit unknown.
    pub unmeasured: Resolved<f64>,
    /// The original's per-part integrity and level thresholds, an explicit
    /// unknown standing beside the designed region pools.
    pub region_unmeasured: Resolved<f64>,
}

/// Component: the player's hull box, whose [`NodeVisualTransform`] follows the
/// body every fixed tick.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PlayerHullBox;

fn claim() -> ClaimId {
    ClaimId::new(M01_PLAYER_DAMAGE_CLAIM).expect("a claim id this module declares")
}

/// Registers the player with the shared damage resolver and hangs the hull
/// box under `player`.
///
/// # Errors
///
/// The refusal, spelled for the weapon report: a graph the resolver rejects or
/// a registration it refuses.
pub fn attach_player_victim(
    world: &mut World,
    player: Entity,
    actor: ActorId,
    generation: SceneGeneration,
) -> Result<PlayerVictim, String> {
    let subject = ContentId::from_source(ContentKind::Blueprint, "damage-graph.m01-player")
        .map_err(|error| error.to_string())?;
    let node_key = DamageNodeKey::new(MISSION_TARGET_NODE).map_err(|error| error.to_string())?;
    let integrity = Known::new(DESIGNED_PLAYER_INTEGRITY, Provenance::designed(claim()));
    let node = DamageNode::new(
        node_key.clone(),
        DamageNodeKind::InternalStructure,
        Resolved::Known(integrity.clone()),
    )
    .with_lethal(true);
    let mut nodes = vec![node];
    nodes.extend(region_nodes());
    let graph = DamageGraph::try_new(subject.clone(), nodes)
        .map_err(|error| format!("the player's damage graph refuses: {error}"))?;
    let policy = DamagePolicy {
        attribution: AttributionRule::FirstLethalHit,
    };
    let session = actor.session;
    if world
        .get_resource::<SessionDamageResolver>()
        .is_none_or(|resolver| resolver.resolver().session() != session)
    {
        world.insert_resource(SessionDamageResolver::new(
            cs_sim::damage::DamageResolver::new(session, MISSION_DAMAGE_PRODUCER),
        ));
    }
    world
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .register_actor(actor, graph, policy)
        .map_err(|error| format!("the damage resolver refuses the player: {error}"))?;

    let at = world
        .get::<GlobalTransform>(player)
        .copied()
        .unwrap_or_default();
    world.entity_mut(player).insert(DamageActorBinding {
        actor,
        graph: subject,
        generation,
    });
    world.spawn((
        PlayerHullBox,
        PartSweptBox::new(actor, node_key, DESIGNED_PLAYER_HALF_EXTENTS_M),
        NodeVisualTransform(at),
        Transform::IDENTITY,
        bevy::prelude::ChildOf(player),
    ));
    let unmeasured = Resolved::<f64>::unknown(
        ClaimId::new(M01_PLAYER_DAMAGE_UNKNOWN_CLAIM).expect("a claim id this module declares"),
        "no per-airframe or M01 damage table is measured: the original's hull pool for the \
         player's Devastator, its armor and its hit boxes are unrecovered, so the designed pool \
         and box stand in for them",
    )
    .map_err(|error| error.to_string())?;
    let region_unmeasured = Resolved::<f64>::unknown(
        ClaimId::new(M01_PLAYER_REGION_UNKNOWN_CLAIM).expect("a claim id this module declares"),
        "the `destroyable_parts` of `player_pfighter` (nose, tail, leftwing, rightwing; each \
         `critical`) carry no recovered integrity, damage threshold or lethality: the designed \
         region pools are non-lethal and stand in until a measurement replaces them",
    )
    .map_err(|error| error.to_string())?;
    Ok(PlayerVictim {
        actor,
        integrity,
        unmeasured,
        region_unmeasured,
    })
}

/// The four airframe region nodes of the player, named as measured in
/// `ZBD/planes.zbd`. `taildamage` is an engine node because the vehicle
/// record names the tail part `engine`; the pools are designed.
fn region_nodes() -> Vec<DamageNode> {
    let claim = ClaimId::new(M01_PLAYER_REGION_CLAIM).expect("a claim id this module declares");
    AirframeDamageRegion::ALL
        .into_iter()
        .map(|region| {
            let key = DamageNodeKey::new(region.node_name())
                .expect("a measured region node name is a valid key");
            let kind = match region {
                AirframeDamageRegion::Tail => DamageNodeKind::Engine,
                _ => DamageNodeKind::InternalStructure,
            };
            DamageNode::new(
                key,
                kind,
                Resolved::Known(Known::new(
                    DESIGNED_REGION_INTEGRITY,
                    Provenance::designed(claim.clone()),
                )),
            )
        })
        .collect()
}

/// Composes the hull box's [`NodeVisualTransform`] from the live body, so a
/// round sweeps the box where the player actually is.
pub fn compose_player_hull_box(
    mut hulls: Query<(&bevy::prelude::ChildOf, &mut NodeVisualTransform), With<PlayerHullBox>>,
    bodies: Query<&GlobalTransform, Without<PlayerHullBox>>,
) {
    for (parent, mut node) in &mut hulls {
        if let Ok(global) = bodies.get(parent.parent()) {
            node.0 = *global;
        }
    }
}
