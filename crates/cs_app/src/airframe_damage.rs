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
use bevy::prelude::{Entity, Visibility, World};
use cs_sim::damage::{ActorId, DamageNodeKey, DamageResolver, PartState};

use crate::scene::SceneGeneration;
use crate::weapons::SessionDamageResolver;

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

/// Component: the airframe's measured `wing_flare` fire quad
/// (PARITY-M01-PLAYER-VIEW-C2).
///
/// `planes.zbd` stores one single-quad mesh per wingtip — the player's
/// `wing_flare1` (slot 208, mesh 105, port tip) and `wing_flare2` (slot 209,
/// mesh 106, starboard tip; handedness measured against the `l_aileron*`/
/// `r_aileron*` translations) — both textured `oil_liteflare.tif` under
/// `healthy > dontmove`. The vehicle record's `injure_anims` bind the quads
/// to the original's `*_firetrail`/`fuel_trail` anim effects
/// (`docs/findings/2026-10-19-parity-m01-player-view-c2-wing-fire-smoke-source.md`),
/// the owner capture `orig25.png` shows no flare on the intact aircraft and
/// `orig70.png` shows the fire at the damaged port wingtip, so the quads are
/// damage-gated in the original. The anim-driven sprite trail itself is
/// follow-up work; its emitter parameters are unmeasured and recorded
/// unknown.
///
/// The marker rides on the entity that draws the quad and carries the
/// measured region it answers to; [`update_wing_flares`] declares the
/// [`RegionDamageBinding`] once the session exists (the player body spawns
/// before `MissionWeapons` is installed) and is the only writer of the
/// quad's visibility.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WingFlare {
    /// The damage region this quad anchors.
    pub region: AirframeDamageRegion,
}

/// Shows every [`WingFlare`] exactly while its entity's published
/// [`RegionDamagePresentation`] is `Damaged` or `Destroyed` — the gate the
/// task names. `Intact`, `Unknown` and a flare with no published record yet
/// all keep the quad hidden.
///
/// A flare whose spawn ran before the session's weapons marker existed gets
/// its [`RegionDamageBinding`] declared here, on the first pass that can read
/// one — binding the measured region name, never a derived one. Returns how
/// many flares changed visibility.
#[must_use]
pub fn update_wing_flares(world: &mut World) -> u32 {
    declare_wing_flare_bindings(world);
    let flares: Vec<(Entity, PartState)> = world
        .iter_entities()
        .filter_map(|entity| {
            entity.get::<WingFlare>()?;
            let state = entity
                .get::<RegionDamagePresentation>()
                .map_or(PartState::Unknown, |record| record.state);
            Some((entity.id(), state))
        })
        .collect();
    let mut changed = 0;
    for (entity, state) in flares {
        let wanted = match state {
            PartState::Damaged | PartState::Destroyed => Visibility::Inherited,
            PartState::Unknown | PartState::Intact => Visibility::Hidden,
        };
        if world.get::<Visibility>(entity) != Some(&wanted) {
            world.entity_mut(entity).insert(wanted);
            changed += 1;
        }
    }
    changed
}

/// Declares the [`RegionDamageBinding`] of every [`WingFlare`] that lacks one.
///
/// The flare entities spawn with the player body, before `MissionWeapons`
/// names the session's player actor and generation, so the spawn path cannot
/// stamp the binding itself. The first pass that can read the marker declares
/// it — the measured region's node name on the player actor of the current
/// generation, never a name or a position derived at runtime.
fn declare_wing_flare_bindings(world: &mut World) {
    let Some((actor, generation)) = world
        .get_resource::<crate::mission_session::MissionWeapons>()
        .map(|weapons| (weapons.player(), weapons.generation()))
    else {
        return;
    };
    let unbound: Vec<(Entity, AirframeDamageRegion)> = world
        .iter_entities()
        .filter_map(|entity| {
            let flare = entity.get::<WingFlare>()?;
            entity
                .get::<RegionDamageBinding>()
                .is_none()
                .then_some((entity.id(), flare.region))
        })
        .collect();
    for (entity, region) in unbound {
        if let Ok(node) = DamageNodeKey::new(region.node_name()) {
            world.entity_mut(entity).insert(RegionDamageBinding {
                actor,
                region,
                node,
                generation,
            });
        }
    }
}

/// The scheduled driver of the airframe damage visual (PARITY-M01-PLAYER-VIEW-C2).
///
/// One scheduled step declares every flare's [`RegionDamageBinding`], then
/// publishes every bound actor's [`RegionDamagePresentation`] through
/// [`apply_region_damage_state`] against the session's own
/// [`SessionDamageResolver`], then lets [`update_wing_flares`] gate each
/// measured quad on the record that names it — declare, publish, gate, so a
/// fresh binding meets its record in the same pass. Both passes are
/// idempotent and convergent, so a frame in which the authority published
/// nothing new changes nothing; with no session resolver installed — the
/// synthetic playtest — the publish is skipped and the gate still keeps a
/// declared binding's visibility in step with its existing record.
pub fn drive_airframe_damage_visuals(world: &mut World) {
    declare_wing_flare_bindings(world);
    let mut actors: Vec<ActorId> = world
        .iter_entities()
        .filter_map(|entity| entity.get::<RegionDamageBinding>().map(|b| b.actor))
        .collect();
    actors.sort_unstable();
    actors.dedup();
    if !actors.is_empty() && world.contains_resource::<SessionDamageResolver>() {
        world.resource_scope(|world, damage: bevy::prelude::Mut<SessionDamageResolver>| {
            for actor in actors {
                let _ = apply_region_damage_state(world, damage.resolver(), actor);
            }
        });
    }
    let _ = update_wing_flares(world);
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
