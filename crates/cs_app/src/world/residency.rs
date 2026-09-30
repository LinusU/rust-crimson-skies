//! One world's residency: which sectors are loaded, and the state that outlives
//! their entities (F18-B, F18-C).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-B` and `### F18-C`, acceptance scenario **AC02** — *unload and
//! reload a sector containing a damaged objective; state persists correctly.*
//!
//! # What this layer is, and what it is not
//!
//! It is a **transaction**: [`load_world`] brings a [`WorldInstance`] into a
//! Bevy world, [`unload_sector`] and [`load_sector`] move one sector's geometry
//! in and out, [`unload_world`] takes the whole thing away, and
//! [`damage_object`] records that gameplay damaged an object. Every call
//! decides everything before it changes anything, and each reports what it
//! spawned, despawned or refused.
//!
//! It is **not** the streaming *policy*: nothing here decides which sector
//! *should* be resident from a camera, a mission overlay or a trigger. That is
//! [`super::visibility`], and the separation is deliberate — this layer only has
//! to be right about *what a load means*, so a policy on top of it cannot
//! quietly redefine it. What this layer *does* own is the record a policy reads
//! and cannot rebuild: the load's per-object conditions and, since F18-C, the
//! overlays it has already applied.
//!
//! # Why the condition lives here and not on an entity
//!
//! F18 non-negotiable behavior 3 requires object identity to survive sector
//! streaming, and AC02 requires a damaged objective to still be damaged after
//! its sector was unloaded and loaded again. Neither is satisfiable when the
//! state lives on the entities: despawning a sector destroys them, and
//! re-spawning invents a fresh, undamaged object. So the condition is a value in
//! the [`WorldResidency`] resource, keyed by the authored [`WorldObjectId`], and
//! every entity an object owns carries the same value in its [`ObjectCondition`]
//! component — the resource is the truth, the component is what a query can see.
//!
//! The residency rule is the record's own: an object is present while at least
//! one of its sectors is loaded, and an object that names **no** sector is
//! resident — it never streams away (see
//! `WorldDefinition::resident_objects`).
//!
//! # No leftovers
//!
//! [`load_world`] refuses to load over a world that is already loaded, and
//! [`unload_world`] is the only way to release it. That is F18 non-negotiable
//! behavior 5 made structural: a second mission's population, variant and
//! authored damage can only be established by a load that starts from nothing,
//! so the previous run has nowhere to survive. An object's condition, by
//! contrast, is the *load's* memory and deliberately does not outlive its own
//! unload — the next load seeds it from its own initial damage. The same is
//! true of the applied overlays: they belong to this load, and the next load
//! gets a world whose door is shut again.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::{App, Component, Entity, Resource, World};
use cs_content::world::{
    MissionOverlay, SectorId, WorldDefinition, WorldError, WorldId, WorldInstance,
    WorldObjectCondition, WorldObjectId,
};
use cs_types::content::Resolved;

use super::contacts::WorldObjectBinding;
use super::meshes::WorldMeshes;
use super::overlays::reapply_object;
use super::spawn::{
    SpawnedObject, SpawnedWorld, WorldSpawnError, instance_placements, spawn_object,
};

/// The condition of the object an entity belongs to.
///
/// Stamped on **every** entity the object owns, so a query over
/// [`WorldObjectBinding`] can read the object's state without a second lookup.
/// The authority is [`WorldResidency`]; this component is that value where a
/// system can see it, and an unload removes the entities and the record together.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectCondition(pub WorldObjectCondition);

impl ObjectCondition {
    /// The condition this component carries.
    #[must_use]
    pub const fn condition(self) -> WorldObjectCondition {
        self.0
    }

    /// Whether the object is damaged.
    #[must_use]
    pub const fn is_damaged(self) -> bool {
        self.0.is_damaged()
    }
}

impl From<WorldObjectCondition> for ObjectCondition {
    fn from(condition: WorldObjectCondition) -> Self {
        Self(condition)
    }
}

/// What one loaded world is, right now.
#[derive(Clone, Debug)]
pub struct ResidentWorld {
    definition: WorldDefinition,
    variant: Resolved<WorldId>,
    population: BTreeSet<WorldObjectId>,
    declared: BTreeSet<SectorId>,
    loaded: BTreeSet<SectorId>,
    conditions: BTreeMap<WorldObjectId, WorldObjectCondition>,
    objects: BTreeMap<WorldObjectId, SpawnedObject>,
    /// The mission-local overlays this load declared, keyed by their trigger
    /// object. The load's own copy, so a load that starts from nothing starts
    /// with no overlay state at all (F18 non-negotiable behavior 5).
    overlays: BTreeMap<WorldObjectId, MissionOverlay>,
    /// The triggers whose overlay this load has already applied, in stable
    /// order. This is what makes "once" a property of the load rather than of
    /// the frame: a second crossing of the same volume is not a second
    /// displacement.
    applied: BTreeSet<WorldObjectId>,
    /// The objects this load declares gameplay-required, in stable order: the
    /// ones a streaming policy must not take away (F18 non-negotiable
    /// behavior 3).
    required: BTreeSet<WorldObjectId>,
}

impl ResidentWorld {
    /// The definition this load reads from.
    #[must_use]
    pub fn definition(&self) -> &WorldDefinition {
        &self.definition
    }

    /// The world's identity.
    #[must_use]
    pub fn id(&self) -> &WorldId {
        self.definition.id()
    }

    /// The authored variant this load applies, or the explicit unknown it left.
    #[must_use]
    pub fn variant(&self) -> &Resolved<WorldId> {
        &self.variant
    }

    /// The objects this load activates, whether or not they are present now.
    #[must_use]
    pub const fn population(&self) -> &BTreeSet<WorldObjectId> {
        &self.population
    }

    /// The sectors that are loaded, in stable order.
    #[must_use]
    pub const fn loaded_sectors(&self) -> &BTreeSet<SectorId> {
        &self.loaded
    }

    /// The objects that currently have entities, in stable order.
    #[must_use]
    pub fn present_objects(&self) -> Vec<&WorldObjectId> {
        self.objects.keys().collect()
    }

    /// The condition `object` is in, whether it is present or streamed out.
    #[must_use]
    pub fn condition(&self, object: &WorldObjectId) -> Option<WorldObjectCondition> {
        self.conditions.get(object).copied()
    }

    /// The spawn record of a present object.
    #[must_use]
    pub fn object(&self, object: &WorldObjectId) -> Option<&SpawnedObject> {
        self.objects.get(object)
    }

    /// Whether `object` is active in this load *and* currently present.
    #[must_use]
    pub fn is_present(&self, object: &WorldObjectId) -> bool {
        self.objects.contains_key(object)
    }

    /// The overlay whose trigger is `trigger`, when this load declares one.
    ///
    /// The single way to read a declared overlay: the record is a map keyed by
    /// its own trigger, and there is one overlay per trigger by construction
    /// ([`WorldInstance::with_mission_layer`] refuses a second).
    #[must_use]
    pub fn overlay_for(&self, trigger: &WorldObjectId) -> Option<&MissionOverlay> {
        self.overlays.get(trigger)
    }

    /// Whether this load has already applied the overlay fired by `trigger`.
    #[must_use]
    pub fn is_applied(&self, trigger: &WorldObjectId) -> bool {
        self.applied.contains(trigger)
    }

    /// The triggers whose overlay this load has applied, in stable order.
    #[must_use]
    pub const fn applied_overlays(&self) -> &BTreeSet<WorldObjectId> {
        &self.applied
    }

    /// The objects this load declares gameplay-required, in stable order.
    ///
    /// The whole declaration, and the only way to read it: which *sectors* a
    /// required object holds is the streaming policy's own question and is
    /// answered by [`super::visibility::retained_sectors`], which is the tested
    /// path. A second spelling of that question here would be untested
    /// arithmetic that could disagree with the policy.
    #[must_use]
    pub const fn required_objects(&self) -> &BTreeSet<WorldObjectId> {
        &self.required
    }

    /// Records that this load has applied the overlay fired by `trigger`.
    ///
    /// Crate-private on purpose: only [`super::overlays::apply_overlay`] may
    /// mark an overlay applied, and only after it has moved the entities. A
    /// public setter would let a caller record an application that never
    /// happened, which is the one lie this record must not tell — "once" is
    /// only meaningful while it is written on the same step as the movement.
    pub(crate) fn mark_applied(&mut self, trigger: WorldObjectId) {
        self.applied.insert(trigger);
    }
}

/// The one world a Bevy world has loaded, and its residency.
///
/// The resource exists exactly while a world is loaded: [`load_world`] inserts
/// it, [`unload_world`] removes it, and every other entry point refuses when it
/// is absent. There is therefore no state in which a caller can read a
/// residency without a world, or find world entities with no record of them.
#[derive(Resource, Clone, Debug)]
pub struct WorldResidency {
    resident: ResidentWorld,
}

impl WorldResidency {
    /// The loaded world.
    #[must_use]
    pub const fn resident(&self) -> &ResidentWorld {
        &self.resident
    }

    /// The loaded world, mutably.
    ///
    /// Crate-private, like [`ResidentWorld::mark_applied`]: the only state a
    /// caller outside this module may change is the load's own per-object
    /// condition, and that goes through [`damage_object`] so that every change
    /// is stamped onto the entities a query can see.
    pub(crate) fn resident_mut(&mut self) -> &mut ResidentWorld {
        &mut self.resident
    }
}

/// Why a load, unload or state change was refused.
///
/// Every refusal names what was asked for and what is actually there, because
/// "it did nothing" is the one outcome a caller cannot recover from.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldLoadError {
    /// The load record does not read from this definition.
    Instance(WorldError),
    /// An object's authored matrix no runtime transform can hold.
    Spawn(WorldSpawnError),
    /// A mission overlay could not be re-applied to a freshly spawned object,
    /// which means the load's own record and the world disagree about what is
    /// there. Handled rather than swallowed: the sector load is rolled back so
    /// the residency still describes the world.
    Overlay(super::overlays::OverlayError),
    /// A world is already loaded in this Bevy world. Loading a second one over
    /// it is how the last run's population and damage survive into this one, so
    /// it is refused by name instead of merged.
    WorldAlreadyResident {
        /// The world that is loaded.
        resident: WorldId,
        /// The world the caller asked for.
        requested: WorldId,
    },
    /// No world is loaded, so there is nothing to act on.
    NoResidentWorld,
    /// The sector is not declared by the loaded definition.
    UnknownSector {
        /// The sector the caller named.
        sector: SectorId,
    },
    /// The sector is already loaded.
    SectorAlreadyResident {
        /// The sector the caller named.
        sector: SectorId,
    },
    /// The sector is not loaded, so it cannot be unloaded.
    SectorNotResident {
        /// The sector the caller named.
        sector: SectorId,
    },
    /// The object is not part of this load's population, so the load holds no
    /// condition for it to change.
    ObjectNotLoaded {
        /// The object the caller named.
        object: WorldObjectId,
    },
    /// A presented object had lost its entity: something outside this module
    /// despawned part of a world. The residency record is left untouched so the
    /// inconsistency stays visible instead of being papered over.
    VanishedEntity {
        /// The object whose entity was gone.
        object: WorldObjectId,
        /// The entity that was gone.
        entity: Entity,
        /// What the world reported.
        reason: String,
    },
}

impl std::fmt::Display for WorldLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Instance(err) => write!(f, "the load record is not valid: {err}"),
            Self::Spawn(err) => write!(f, "could not spawn the world: {err}"),
            Self::Overlay(err) => {
                write!(f, "could not re-apply the load's mission overlays: {err}")
            }
            Self::WorldAlreadyResident {
                resident,
                requested,
            } => write!(
                f,
                "`{resident}` is already loaded, so `{requested}` was not loaded on top of it; \
                 unload the first world rather than merging two"
            ),
            Self::NoResidentWorld => {
                write!(f, "no world is loaded, so there is nothing to act on")
            }
            Self::UnknownSector { sector } => {
                write!(f, "the loaded world declares no sector `{sector}`")
            }
            Self::SectorAlreadyResident { sector } => {
                write!(f, "sector `{sector}` is already loaded")
            }
            Self::SectorNotResident { sector } => {
                write!(f, "sector `{sector}` is not loaded")
            }
            Self::ObjectNotLoaded { object } => {
                write!(f, "object `{object}` is not part of this load's population")
            }
            Self::VanishedEntity {
                object,
                entity,
                reason,
            } => write!(
                f,
                "object `{object}` was presented but its entity {entity} is gone ({reason})"
            ),
        }
    }
}

impl std::error::Error for WorldLoadError {}

impl From<WorldError> for WorldLoadError {
    fn from(err: WorldError) -> Self {
        Self::Instance(err)
    }
}

impl From<WorldSpawnError> for WorldLoadError {
    fn from(err: WorldSpawnError) -> Self {
        Self::Spawn(err)
    }
}

/// What one call moved: which objects it spawned, which it despawned, and what
/// [`super::spawn`] produced for the ones it spawned.
#[derive(Clone, Debug, Default)]
pub struct SectorLoad {
    /// The sector this call acted on, or `None` for a whole-world unload.
    pub sector: Option<SectorId>,
    /// The objects this call spawned, in the order it spawned them.
    pub spawned: Vec<WorldObjectId>,
    /// The objects this call despawned, in the order it despawned them.
    pub despawned: Vec<WorldObjectId>,
    /// What [`super::spawn`] produced for the objects this call spawned.
    pub report: SpawnedWorld,
}

impl SectorLoad {
    /// Whether the call changed nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spawned.is_empty() && self.despawned.is_empty()
    }
}

/// The residency a Bevy world holds, if one is loaded.
#[must_use]
pub fn residency(world: &World) -> Option<&WorldResidency> {
    world.get_resource::<WorldResidency>()
}

/// The condition `object` is in for the loaded world, if one is loaded at all.
///
/// This is the query gameplay uses after an object was damaged: it answers for
/// an object whose sector is *not* loaded, which is the whole point.
#[must_use]
pub fn condition_of(world: &World, object: &WorldObjectId) -> Option<WorldObjectCondition> {
    residency(world)?.resident().condition(object)
}

/// Records that gameplay damaged `object`, whether or not its sector is loaded.
///
/// F18 non-negotiable behavior 3 asks for exactly this: an object outside render
/// visibility stays *correctly summarized*, and the only honest summary of
/// "damaged" is the condition itself. Because the condition belongs to the load,
/// a sector reloaded afterwards brings the object back damaged instead of
/// re-authoring it (AC02).
///
/// # Errors
///
/// [`WorldLoadError::NoResidentWorld`] when nothing is loaded,
/// [`WorldLoadError::ObjectNotLoaded`] when the object is not part of this
/// load's population — a condition for an object the load never activates would
/// name a state nothing could show — and [`WorldLoadError::VanishedEntity`] when
/// a present object has lost an entity somewhere else. The whole call is decided
/// before anything changes, so a refusal leaves the condition exactly as it was
/// rather than half applied.
pub fn damage_object(app: &mut App, object: &WorldObjectId) -> Result<(), WorldLoadError> {
    let world = app.world_mut();
    // 1. Decide. The population, and every entity the stamp below would touch,
    //    are checked while nothing has changed yet.
    let entities = {
        let resident = resident(world)?;
        if !resident.population.contains(object) {
            return Err(WorldLoadError::ObjectNotLoaded {
                object: object.clone(),
            });
        }
        let entities = entities_of(world, object);
        for entity in &entities {
            world
                .get_entity(*entity)
                .map_err(|reason| vanished(object, *entity, &reason))?;
        }
        entities
    };
    // 2. Change. The record first, then the components, so a query and the
    //    resource cannot disagree about the same object.
    {
        let Some(mut residency) = world.get_resource_mut::<WorldResidency>() else {
            return Err(WorldLoadError::NoResidentWorld);
        };
        residency
            .resident
            .conditions
            .insert(object.clone(), WorldObjectCondition::Damaged);
    }
    for entity in entities {
        stamp_condition(world, object, entity, WorldObjectCondition::Damaged)?;
    }
    Ok(())
}

/// Loads `instance` — its variant, its population and its authored damage —
/// into `app`, with every declared sector resident.
///
/// The refusal order is why this is safe to call from a load screen: the record
/// is checked against the definition, the residency record is checked for a
/// world that is already loaded, and *every* activated object's authored matrix
/// is decomposed — all before the first entity exists. A refusal leaves the app
/// exactly as it was.
///
/// An object the record's population does not activate is never spawned, even
/// when the definition declares it. An object whose role, shape or mesh this
/// load cannot build is reported in [`SpawnedWorld::skipped`] next to the
/// objects that *were* built, and every object is stamped with the condition the
/// load holds for it.
///
/// # Errors
///
/// [`WorldLoadError::Instance`] when the load record does not read from this
/// definition, [`WorldLoadError::WorldAlreadyResident`] when a world is already
/// loaded, and [`WorldLoadError::Spawn`] when an activated object's matrix has
/// no runtime form. Nothing is spawned in any of those cases.
pub fn load_world(
    app: &mut App,
    definition: &WorldDefinition,
    instance: &WorldInstance,
    meshes: &WorldMeshes,
) -> Result<SpawnedWorld, WorldLoadError> {
    instance.validate_against(definition)?;

    if let Some(resident) = residency(app.world()) {
        return Err(WorldLoadError::WorldAlreadyResident {
            resident: resident.resident().id().clone(),
            requested: definition.id().clone(),
        });
    }

    let population: Vec<_> = definition
        .objects()
        .iter()
        .filter(|object| instance.activates(object.id()))
        .collect();
    instance_placements(&population)?;

    let sectors: BTreeSet<SectorId> = definition
        .sectors()
        .iter()
        .map(|sector| sector.id().clone())
        .collect();
    let mut resident = ResidentWorld {
        definition: definition.clone(),
        variant: instance.variant().clone(),
        population: population
            .iter()
            .map(|object| object.id().clone())
            .collect(),
        declared: sectors.clone(),
        loaded: sectors,
        conditions: population
            .iter()
            .map(|object| (object.id().clone(), instance.initial_condition(object.id())))
            .collect(),
        objects: BTreeMap::new(),
        // A load starts with nothing applied, and nothing left over from the
        // last run: the previous run's record is gone with the world, so this
        // mission's door starts shut (F18 non-negotiable behavior 5).
        overlays: instance
            .overlays()
            .iter()
            .map(|overlay| (overlay.trigger().clone(), overlay.clone()))
            .collect(),
        applied: BTreeSet::new(),
        required: instance.required_objects().clone(),
    };

    let mut report = SpawnedWorld::of(definition.id());
    for object in population {
        let spawned = spawn_object(app, definition, object, meshes)?;
        let condition = resident.condition(object.id());
        if let Err(err) = stamp(app.world_mut(), &spawned, condition) {
            // Unreachable after `instance_placements` above, and handled rather
            // than assumed: a half-loaded world is the one outcome this module
            // exists to prevent.
            despawn_all(app.world_mut(), resident.objects.values());
            return Err(err);
        }
        resident
            .objects
            .insert(spawned.object.clone(), spawned.clone());
        report.record(spawned);
    }

    app.world_mut().insert_resource(WorldResidency { resident });
    Ok(report)
}

/// Unloads one sector: every object whose sectors are *all* unloaded is
/// despawned, and an object that still belongs to a loaded sector stays exactly
/// where it is.
///
/// Object conditions are untouched — the load keeps them, so a reload finds them
/// again (AC02). Nothing else about the object is kept: the entities go, and a
/// reload rebuilds them from the same record.
///
/// # Errors
///
/// [`WorldLoadError::NoResidentWorld`] when nothing is loaded,
/// [`WorldLoadError::UnknownSector`] when the definition declares no such
/// sector, [`WorldLoadError::SectorNotResident`] when it is already unloaded,
/// and [`WorldLoadError::VanishedEntity`] when a presented object has lost an
/// entity somewhere else. Every refusal leaves the residency untouched.
pub fn unload_sector(app: &mut App, sector: &SectorId) -> Result<SectorLoad, WorldLoadError> {
    // 1. Decide, and check that every entity involved still exists. Read-only,
    //    so a refusal here has changed nothing at all.
    let outgoing: Vec<WorldObjectId> = {
        let world = app.world();
        let resident = resident(world)?;
        check_declared(resident, sector)?;
        if !resident.loaded.contains(sector) {
            return Err(WorldLoadError::SectorNotResident {
                sector: sector.clone(),
            });
        }
        let still_loaded: BTreeSet<SectorId> = resident
            .loaded
            .iter()
            .filter(|candidate| *candidate != sector)
            .cloned()
            .collect();
        let mut outgoing = Vec::new();
        for (object, spawned) in &resident.objects {
            if resident
                .definition
                .object(object)
                .is_some_and(|record| !present_in(&still_loaded, record.sectors()))
            {
                for entity in spawned.entities() {
                    world
                        .get_entity(entity)
                        .map_err(|reason| vanished(object, entity, &reason))?;
                }
                outgoing.push(object.clone());
            }
        }
        outgoing
    };

    // 2. Despawn, then update the record. Nothing below can fail, and the
    //    order means a world never claims a sector is unloaded while its
    //    entities are still live.
    let world = app.world_mut();
    let spawned: Vec<SpawnedObject> = {
        let resident = resident(world)?;
        outgoing
            .iter()
            .filter_map(|object| resident.objects.get(object).cloned())
            .collect()
    };
    despawn_all(world, spawned.iter());
    {
        let Some(mut residency) = world.get_resource_mut::<WorldResidency>() else {
            return Err(WorldLoadError::NoResidentWorld);
        };
        for object in &outgoing {
            residency.resident.objects.remove(object);
        }
        residency.resident.loaded.remove(sector);
    }

    Ok(SectorLoad {
        sector: Some(sector.clone()),
        spawned: Vec::new(),
        despawned: outgoing,
        report: SpawnedWorld::of(definition_id(app.world())?),
    })
}

/// Loads one sector: every object that becomes present is spawned, stamped with
/// the condition the load already holds for it — never with the definition's, so
/// a reloaded damaged objective comes back damaged.
///
/// An object that belongs to two sectors and is already present through the
/// other one is not spawned a second time.
///
/// # Errors
///
/// [`WorldLoadError::NoResidentWorld`] when nothing is loaded,
/// [`WorldLoadError::UnknownSector`] for a sector the definition does not
/// declare, [`WorldLoadError::SectorAlreadyResident`] when it is already
/// loaded, and [`WorldLoadError::Spawn`] when an incoming object's matrix has no
/// runtime form. Every refusal leaves the residency untouched, and the last one
/// spawns nothing: all incoming transforms are decomposed before the first
/// entity.
pub fn load_sector(
    app: &mut App,
    sector: &SectorId,
    meshes: &WorldMeshes,
) -> Result<SectorLoad, WorldLoadError> {
    // 1. Decide which objects become present, and check every transform before
    //    anything is spawned. Read-only.
    let (definition, records) = {
        let world = app.world();
        let resident = resident(world)?;
        check_declared(resident, sector)?;
        if resident.loaded.contains(sector) {
            return Err(WorldLoadError::SectorAlreadyResident {
                sector: sector.clone(),
            });
        }
        // The definition is cloned out of the record because spawning needs a
        // `&mut App` while the record lives in the world, and a world's records
        // are small next to the geometry they name.
        let definition = resident.definition.clone();
        let records: Vec<_> = resident
            .population
            .iter()
            .filter(|object| !resident.is_present(object))
            .filter_map(|object| definition.object(object))
            .filter(|record| record.sectors().contains(sector))
            .cloned()
            .collect();
        (definition, records)
    };
    {
        let borrowed: Vec<_> = records.iter().collect();
        instance_placements(&borrowed)?;
    }

    // 2. Spawn. Each object is stamped with the condition the load already holds
    //    and entered in the record before the next one starts, so an abort
    //    leaves the world as this call found it: [`rollback`] takes back exactly
    //    what this call spawned and leaves everything that was already present
    //    where it was.
    //
    //    An object joins `incoming` the moment its entities exist, **before** the
    //    steps that can still fail. Everything after the spawn is a refusal that
    //    has already created entities, so an object that was not yet in
    //    `incoming` would survive its own abort: still live, and — for the two
    //    steps that touch the record — still claimed by it, which is the one
    //    outcome this module exists to prevent.
    let mut report = SpawnedWorld::of(definition.id());
    let mut incoming: Vec<SpawnedObject> = Vec::new();
    for record in &records {
        let condition = resident(app.world())?.condition(record.id());
        let spawned = match spawn_object(app, &definition, record, meshes) {
            Ok(spawned) => spawned,
            Err(err) => return Err(rollback(app, &incoming, WorldLoadError::Spawn(err))),
        };
        incoming.push(spawned.clone());
        if let Err(err) = stamp(app.world_mut(), &spawned, condition) {
            return Err(rollback(app, &incoming, err));
        }
        {
            let Some(mut residency) = app.world_mut().get_resource_mut::<WorldResidency>() else {
                return Err(rollback(app, &incoming, WorldLoadError::NoResidentWorld));
            };
            residency
                .resident
                .objects
                .insert(spawned.object.clone(), spawned.clone());
        }
        // The load's memory is the condition *and* the effect it has already
        // applied: a door that opened before its sector streamed out is still
        // open when the sector comes back, beside the damaged objective that is
        // still damaged. AC02's rule, applied to an overlay.
        if let Err(err) = reapply_object(app.world_mut(), &spawned) {
            return Err(rollback(app, &incoming, WorldLoadError::Overlay(err)));
        }
        report.record(spawned);
    }
    {
        let Some(mut residency) = app.world_mut().get_resource_mut::<WorldResidency>() else {
            return Err(rollback(app, &incoming, WorldLoadError::NoResidentWorld));
        };
        residency.resident.loaded.insert(sector.clone());
    }

    Ok(SectorLoad {
        sector: Some(sector.clone()),
        spawned: incoming
            .iter()
            .map(|spawned| spawned.object.clone())
            .collect(),
        despawned: Vec::new(),
        report,
    })
}

/// Takes the whole world out of `app` and forgets its load record.
///
/// This is the only way a new mission's population, variant and authored damage
/// can be established (F18 non-negotiable behavior 5), and it is deliberately
/// total: every object the load activated is despawned and the residency record
/// is gone, so nothing of the previous run can be read or inherited.
///
/// Unloading a world that is not loaded is a no-op that reports `None`.
#[must_use]
pub fn unload_world(app: &mut App) -> Option<SectorLoad> {
    let world = app.world_mut();
    let residency = world.remove_resource::<WorldResidency>()?;
    let resident = residency.resident;
    let despawned: Vec<WorldObjectId> = resident.objects.keys().cloned().collect();
    let spawned: Vec<SpawnedObject> = resident.objects.values().cloned().collect();
    despawn_all(world, spawned.iter());
    Some(SectorLoad {
        sector: None,
        spawned: Vec::new(),
        despawned,
        report: SpawnedWorld::of(resident.id()),
    })
}

/// Whether an object with these sector memberships is present in a world whose
/// resident sectors are `loaded`.
///
/// An object that names no sector is resident: it is never streamed away.
fn present_in(loaded: &BTreeSet<SectorId>, sectors: &[SectorId]) -> bool {
    sectors.is_empty() || sectors.iter().any(|sector| loaded.contains(sector))
}

/// The loaded world, or [`WorldLoadError::NoResidentWorld`].
fn resident(world: &World) -> Result<&ResidentWorld, WorldLoadError> {
    residency(world)
        .map(WorldResidency::resident)
        .ok_or(WorldLoadError::NoResidentWorld)
}

/// The loaded world's identity, or [`WorldLoadError::NoResidentWorld`].
fn definition_id(world: &World) -> Result<&WorldId, WorldLoadError> {
    resident(world).map(ResidentWorld::id)
}

/// Whether the loaded world declares `sector`.
fn check_declared(resident: &ResidentWorld, sector: &SectorId) -> Result<(), WorldLoadError> {
    if resident.declared.iter().any(|declared| declared == sector) {
        Ok(())
    } else {
        Err(WorldLoadError::UnknownSector {
            sector: sector.clone(),
        })
    }
}

/// Stamps every entity of a freshly spawned object with a condition.
fn stamp(
    world: &mut World,
    spawned: &SpawnedObject,
    condition: Option<WorldObjectCondition>,
) -> Result<(), WorldLoadError> {
    let condition = ObjectCondition(condition.unwrap_or(WorldObjectCondition::Authored));
    for entity in spawned.entities() {
        stamp_condition(world, &spawned.object, entity, condition.0)?;
    }
    Ok(())
}

/// Stamps one entity with its object's condition.
fn stamp_condition(
    world: &mut World,
    object: &WorldObjectId,
    entity: Entity,
    condition: WorldObjectCondition,
) -> Result<(), WorldLoadError> {
    world
        .get_entity_mut(entity)
        .map_err(|reason| vanished(object, entity, &reason))?
        .insert(ObjectCondition(condition));
    Ok(())
}

/// Every entity that carries `object`'s binding, in query order.
fn entities_of(world: &mut World, object: &WorldObjectId) -> Vec<Entity> {
    let mut query = world.query::<(Entity, &WorldObjectBinding)>();
    query
        .iter(world)
        .filter(|(_, binding)| binding.object() == object)
        .map(|(entity, _)| entity)
        .collect()
}

/// Despawns several objects' entities.
///
/// Every entity the spawn reported is named here and a missing one is skipped
/// rather than turned into a failure: by the time this runs the caller has
/// already established that the load owns these entities. The list is the
/// report's own ([`SpawnedObject::entities`]), so it follows the entity layout
/// — a mesh object is one entity because its body *is* its collider node (the
/// collider-on-body rule, see [`crate::asset_stack`]), and a cuboid object is a
/// presentation entity plus a collider entity.
fn despawn_all<'a>(world: &mut World, spawned: impl IntoIterator<Item = &'a SpawnedObject>) {
    for object in spawned {
        for entity in object.entities() {
            if let Ok(entity) = world.get_entity_mut(entity) {
                entity.despawn();
            }
        }
    }
}

/// Takes back exactly what one aborted [`load_sector`] spawned, and leaves
/// everything that was already present exactly where it was.
///
/// The objects this call did *not* spawn belong to the load that put them there;
/// despawning them would leave a record claiming objects the world no longer
/// holds, which is the one outcome this module exists to prevent. Each of this
/// call's own spawns is despawned **and** removed from the record, so the
/// residency still describes the world when the call returns, and the sector
/// stays unloaded.
fn rollback(app: &mut App, incoming: &[SpawnedObject], error: WorldLoadError) -> WorldLoadError {
    let world = app.world_mut();
    despawn_all(world, incoming.iter());
    if let Some(mut residency) = world.get_resource_mut::<WorldResidency>() {
        for spawned in incoming {
            residency.resident.objects.remove(&spawned.object);
        }
    }
    error
}

/// The one named error a vanished entity produces.
fn vanished(
    object: &WorldObjectId,
    entity: Entity,
    reason: &dyn std::fmt::Debug,
) -> WorldLoadError {
    WorldLoadError::VanishedEntity {
        object: object.clone(),
        entity,
        reason: format!("{reason:?}"),
    }
}
