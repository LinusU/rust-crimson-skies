//! The airframe damage-region presentation hook (PARITY-M01-PLAYER-VIEW-C1).
//!
//! The original shows hit damage on the player's wings as a trailing fire and
//! smoke plume. Nothing measured says when that starts or what it looks like
//! (`docs/findings/2026-10-19-parity-m01-player-view-c1-wing-damage-hook.md`),
//! so this module is **not** a visual. It is the state-driven hook a visual
//! reads: for each of the airframe's four measured damage regions
//! ([`AirframeDamageRegion`]) it publishes the damage authority's own
//! [`PartState`] on the entity the region maps to, stamped with the
//! [`SceneGeneration`] that bound it.
//!
//! * The region → damage-node mapping is **declared by the spawn path** as a
//!   [`RegionDamageBinding`]; this pass never derives it from a name or a
//!   position. No spawn path declares one for the M01 player yet: the
//!   player's actor has no graph in the session resolver, which is recorded
//!   in the finding rather than papered over.
//! * [`apply_region_damage_state`] copies [`DamageResolver::part_state`]
//!   verbatim. `Damaged` and `Destroyed` come straight from the resolver, and
//!   `Unknown` stays `Unknown` (a node the resolver does not describe is
//!   published as `Unknown` and reported, never as `Intact`).
//! * The pass is convergent and idempotent, the same shape as
//!   [`crate::debris::apply_debris_state`]; [`release_region_damage`] drops
//!   every record of an actor for repair, reload and scene release.

use bevy::ecs::component::Component;
use bevy::prelude::{Entity, World};
use cs_sim::damage::{ActorId, DamageNodeKey, DamageResolver, PartState};

use crate::scene::SceneGeneration;

/// One of the airframe's four measured damage regions.
///
/// The names are the node names measured in `ZBD/planes.zbd`
/// (`docs/findings/2026-10-07-f29-d1-airframe-damage-vocabulary.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AirframeDamageRegion {
    /// `leftwingdamage`.
    LeftWing,
    /// `rightwingdamage`.
    RightWing,
    /// `nosedamage`.
    Nose,
    /// `taildamage`.
    Tail,
}

impl AirframeDamageRegion {
    /// Every region, in the order the finding lists them.
    pub const ALL: [Self; 4] = [Self::LeftWing, Self::RightWing, Self::Nose, Self::Tail];

    /// The measured node name of the region.
    #[must_use]
    pub const fn node_name(self) -> &'static str {
        match self {
            Self::LeftWing => "leftwingdamage",
            Self::RightWing => "rightwingdamage",
            Self::Nose => "nosedamage",
            Self::Tail => "taildamage",
        }
    }

    /// The region a measured node name denotes, if it is one of the four.
    #[must_use]
    pub fn from_node_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|region| region.node_name().eq_ignore_ascii_case(name))
    }
}

/// Component: which damage node one entity's airframe region answers to.
///
/// Published by the spawn path beside the entity; an entity without it is not
/// a region and gets no record.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct RegionDamageBinding {
    /// The session-qualified actor the region belongs to.
    pub actor: ActorId,
    /// The region the entity is.
    pub region: AirframeDamageRegion,
    /// The damage graph node the region answers to.
    pub node: DamageNodeKey,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

/// Component: the presentation record of one region's damage state.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct RegionDamagePresentation {
    /// The session-qualified actor the region belongs to.
    pub actor: ActorId,
    /// The region.
    pub region: AirframeDamageRegion,
    /// The damage authority's state of the mapped node, verbatim.
    pub state: PartState,
    /// The scene generation of the binding the record was published for.
    pub generation: SceneGeneration,
}

/// What one pass did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegionDamageOutcome {
    /// Records inserted or whose state changed.
    pub published: u32,
    /// Bound regions whose record already matched.
    pub unchanged: u32,
    /// Regions published as `Unknown` because the resolver could not answer,
    /// and records dropped for a binding that left; one line each.
    pub refusals: Vec<String>,
}

/// Publishes the resolver's part state on every region entity of `actor`.
///
/// A foreign session or an actor the resolver does not register publishes
/// `Unknown` for each bound region, never a state the authority did not give.
#[must_use]
pub fn apply_region_damage_state(
    world: &mut World,
    resolver: &DamageResolver,
    actor: ActorId,
) -> RegionDamageOutcome {
    let mut outcome = RegionDamageOutcome::default();
    let foreign = actor.session != resolver.session();

    let bound: Vec<(
        Entity,
        RegionDamageBinding,
        Option<RegionDamagePresentation>,
    )> = world
        .iter_entities()
        .filter_map(|entity| {
            let binding = entity.get::<RegionDamageBinding>()?;
            (binding.actor == actor).then(|| {
                (
                    entity.id(),
                    binding.clone(),
                    entity.get::<RegionDamagePresentation>().cloned(),
                )
            })
        })
        .collect();

    for (entity, binding, existing) in bound {
        let state = if foreign {
            None
        } else {
            resolver.part_state(&actor, &binding.node)
        };
        let state = state.unwrap_or_else(|| {
            outcome.refusals.push(format!(
                "{:?}: the resolver does not describe node {:?}; published Unknown",
                binding.region, binding.node
            ));
            PartState::Unknown
        });
        let record = RegionDamagePresentation {
            actor,
            region: binding.region,
            state,
            generation: binding.generation,
        };
        if existing.as_ref() == Some(&record) {
            outcome.unchanged += 1;
        } else {
            world.entity_mut(entity).insert(record);
            outcome.published += 1;
        }
    }
    outcome
}

/// Drops every presentation record of `actor` and reports how many went.
///
/// The repair / reload / scene-release entry; a second call reports `0`.
#[must_use]
pub fn release_region_damage(world: &mut World, actor: ActorId) -> u32 {
    let entities: Vec<Entity> = world
        .iter_entities()
        .filter_map(|entity| {
            let record = entity.get::<RegionDamagePresentation>()?;
            (record.actor == actor).then_some(entity.id())
        })
        .collect();
    let released = u32::try_from(entities.len()).unwrap_or(u32::MAX);
    for entity in entities {
        world
            .entity_mut(entity)
            .remove::<RegionDamagePresentation>();
    }
    released
}
