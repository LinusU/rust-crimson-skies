//! F18-B: the load transaction, and the state that survives a sector
//! (acceptance scenario **AC02**).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-B`. Task test prefix: `accept_f18_b_`.
//!
//! These tests drive `cs_app::world::{load_world, load_sector, unload_sector,
//! unload_world, damage_object}` on the same harbor world
//! `cs_app::world::harbor_world` the import tests fly through. Nothing here
//! carries its own world, its own streaming rule or its own state store.
//!
//! What is pinned here:
//!
//! * **AC02**: an objective damaged *while its sector is unloaded* comes back
//!   damaged when the sector is loaded again, and its identity survives the
//!   round trip.
//! * **residency is the record's own rule**: an object that belongs to two
//!   sectors stays while either is loaded, and an object that names none is
//!   resident. A reload never spawns a second copy.
//! * **F18 non-negotiable behavior 5**: a second mission's population, variant
//!   and authored damage are established by a load that starts from nothing, and
//!   loading over a resident world is refused rather than merged.

use bevy::prelude::{App, Entity, With};
use cs_app::world::{
    HARBOR_OBJECT_ABSENT, HARBOR_OBJECT_BANNER, HARBOR_OBJECT_GROUND, HARBOR_OBJECT_HANGAR,
    HARBOR_OBJECT_SENSOR, HARBOR_OBJECT_WATER, HARBOR_SECTOR_YARD, MESH_SETTLE_UPDATES,
    ObjectCondition, SpawnedWorld, WorldLoadError, WorldObjectBinding, WorldResidency,
    condition_of, damage_object, harbor_meshes, harbor_world, load_sector, load_world, residency,
    unload_sector, unload_world, world_app, world_instance,
};
use cs_content::world::{
    SectorId, WorldDefinition, WorldError, WorldInstance, WorldObjectCondition, WorldObjectId,
};

/// The harbor world, built by production code.
fn harbor() -> WorldDefinition {
    harbor_world().expect("the synthetic harbor world is well formed")
}

/// Every object of the harbor world, in definition order.
fn population() -> [&'static str; 6] {
    [
        HARBOR_OBJECT_HANGAR,
        HARBOR_OBJECT_SENSOR,
        HARBOR_OBJECT_BANNER,
        HARBOR_OBJECT_WATER,
        HARBOR_OBJECT_GROUND,
        HARBOR_OBJECT_ABSENT,
    ]
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object key is valid")
}

fn sector(key: &str) -> SectorId {
    SectorId::new(key).expect("the fixture sector key is valid")
}

/// A mission load record with the given variant, population and authored damage.
fn mission(
    definition: &WorldDefinition,
    variant: Option<&str>,
    population: &[&str],
    damaged: &[&str],
) -> WorldInstance {
    world_instance(definition, variant, population, damaged).expect("a valid fixture load record")
}

/// Every object the residency says is present, in stable order.
fn present(app: &App) -> Vec<WorldObjectId> {
    residency(app.world())
        .expect("a world is loaded")
        .resident()
        .present_objects()
        .into_iter()
        .cloned()
        .collect()
}

/// Every entity the Bevy world currently holds for `object`, in query order.
///
/// The residency record is a *claim* about the world; this is the world itself,
/// read through the binding every entity an object owns carries. A despawn that
/// despawns nothing, or a reload that appends, shows up here and nowhere else.
fn entities_of(app: &mut App, object: &WorldObjectId) -> Vec<Entity> {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<WorldObjectBinding>>();
    let mut entities: Vec<Entity> = query
        .iter(world)
        .filter(|entity| {
            world
                .get::<WorldObjectBinding>(*entity)
                .is_some_and(|binding| binding.object() == object)
        })
        .collect();
    entities.sort_unstable();
    entities
}

/// The headless world with the harbor world loaded and settled.
fn loaded(definition: &WorldDefinition, instance: &WorldInstance) -> (App, SpawnedWorld) {
    let mut app = world_app();
    let report = load_world(&mut app, definition, instance, &harbor_meshes())
        .expect("the harbor world loads");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    (app, report)
}

/// **AC02.** A damaged objective's state survives unloading its sector and
/// loading it again — including damage that happened *while the sector was not
/// loaded*, which is the only version of this that a respawn cannot fake.
///
/// The test is deliberately strict about the "while it was out" part: the load
/// record authors the hangar sound, the objective is damaged with its sector
/// unloaded, and the reload must bring back a damaged hangar. A reload that
/// re-read the record would hand back a sound one, which is the observable
/// failure.
///
/// Observable failure if the condition lived on the entities, if the reload
/// re-seeded it from the record, or if the object's identity did not survive:
/// the hangar comes back sound, or with a different id, or not at all.
#[test]
fn accept_f18_b_a_damaged_object_survives_a_sector_unload_and_reload() {
    let definition = harbor();
    let instance = mission(
        &definition,
        Some("synthetic.harbor_world.mission_01"),
        &population(),
        &[],
    );
    let (mut app, report) = loaded(&definition, &instance);
    let hangar = object(HARBOR_OBJECT_HANGAR);
    assert!(
        report.collider_for(&hangar).is_some(),
        "the objective is a collided object before anything streams"
    );
    assert_eq!(
        condition_of(app.world(), &hangar),
        Some(WorldObjectCondition::Authored),
        "the load authors the objective sound"
    );

    // The sector goes away: its entities with it.
    let unload = unload_sector(&mut app, &sector(HARBOR_SECTOR_YARD))
        .expect("the yard sector is loaded and unloads");
    assert_eq!(
        unload.despawned,
        vec![hangar.clone(), object(HARBOR_OBJECT_SENSOR)],
        "only the objects that belong to no loaded sector leave"
    );
    assert!(
        unload.spawned.is_empty(),
        "an unload spawns nothing, saw {:?}",
        unload.spawned
    );
    assert_eq!(
        present(&app),
        vec![
            object(HARBOR_OBJECT_WATER),
            object(HARBOR_OBJECT_GROUND),
            object(HARBOR_OBJECT_BANNER),
            object(HARBOR_OBJECT_ABSENT),
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>(),
        "the water patch, the ground slab, the banner and the gap remain: the \
         ground belongs to the approach sector too, and the water names none"
    );

    // Gameplay damages the objective while it is not there at all.
    damage_object(&mut app, &hangar).expect("the objective is part of the load");
    assert_eq!(
        condition_of(app.world(), &hangar),
        Some(WorldObjectCondition::Damaged),
        "an object outside render visibility must still be summarized correctly"
    );

    // The sector comes back, and the objective with it.
    let reload = load_sector(&mut app, &sector(HARBOR_SECTOR_YARD), &harbor_meshes())
        .expect("the yard sector reloads");
    assert_eq!(
        reload.spawned,
        vec![hangar.clone(), object(HARBOR_OBJECT_SENSOR)],
        "the same objects come back, in the same ids"
    );
    assert_eq!(
        reload.despawned,
        Vec::<WorldObjectId>::new(),
        "a reload despawns nothing"
    );
    assert_eq!(
        condition_of(app.world(), &hangar),
        Some(WorldObjectCondition::Damaged),
        "the reloaded objective is still the damaged one: the load's memory \
         outlived its sector"
    );
    let resident = residency(app.world())
        .expect("a world is loaded")
        .resident();
    assert_eq!(
        resident
            .object(&hangar)
            .map(|spawned| spawned.collider.is_some()),
        Some(true),
        "and it is collided again, under its own id"
    );
    // Every entity of the reloaded object carries the condition, so a query
    // agrees with the record.
    let world = app.world_mut();
    let mut query = world.query::<(Entity, &ObjectCondition)>();
    let conditions: Vec<WorldObjectCondition> = query
        .iter(world)
        .filter(|(entity, _)| {
            world
                .get::<cs_app::world::WorldObjectBinding>(*entity)
                .is_some_and(|binding| binding.object() == &hangar)
        })
        .map(|(_, condition)| condition.condition())
        .collect();
    assert!(
        !conditions.is_empty()
            && conditions
                .iter()
                .all(|c| *c == WorldObjectCondition::Damaged),
        "every entity of the reloaded objective reports it damaged, saw {conditions:?}"
    );
}

/// **The residency record and the Bevy world are the same fact, not two.** An
/// unload really takes the object's entities out of the world — every one of
/// them, the body and the node the derived collider hangs from — and a reload
/// brings the object back under the same id with the same number of entities, so
/// nothing accumulates and nothing is left behind.
///
/// The other residency tests read the *record*; this one reads the world. A
/// despawn that despawns nothing passes every record-level assertion, because
/// the record is a claim about the world rather than the world itself, so this
/// is where the two can only agree if the transaction really moves geometry.
///
/// Observable failure if the unload left entities behind, if a reload appended a
/// second copy, or if the despawn took entities another object still needed: the
/// entity count would not return to its starting value, or the ground slab —
/// which stays resident — would lose the entity it keeps.
#[test]
fn accept_f18_b_an_unload_really_despawns_the_objects_entities_and_a_reload_restores_them() {
    let definition = harbor();
    let instance = mission(&definition, None, &population(), &[]);
    let (mut app, _) = loaded(&definition, &instance);
    let hangar = object(HARBOR_OBJECT_HANGAR);
    let sensor = object(HARBOR_OBJECT_SENSOR);
    let ground = object(HARBOR_OBJECT_GROUND);
    let yard = sector(HARBOR_SECTOR_YARD);

    let hangar_before = entities_of(&mut app, &hangar);
    assert!(
        hangar_before.len() >= 2,
        "a mesh object owns a body and a node, so the unload has two entities to \
         take; saw {hangar_before:?}"
    );
    let ground_before = entities_of(&mut app, &ground);
    assert_eq!(
        ground_before.len(),
        2,
        "a cuboid object owns a presentation entity and a collider entity, and \
         both belong to it; saw {ground_before:?}"
    );

    for _ in 0..2 {
        unload_sector(&mut app, &yard).expect("the yard unloads");
        for object in [&hangar, &sensor] {
            assert!(
                entities_of(&mut app, object).is_empty(),
                "unloading the yard must leave no entity behind for `{object}`: \
                 saw {:?}",
                entities_of(&mut app, object)
            );
        }
        assert_eq!(
            entities_of(&mut app, &ground),
            ground_before,
            "the ground belongs to the approach sector too, so its entity must \
             survive the yard's unload untouched"
        );

        load_sector(&mut app, &yard, &harbor_meshes()).expect("the yard reloads");
        assert_eq!(
            entities_of(&mut app, &hangar).len(),
            hangar_before.len(),
            "a reload rebuilds the object, it does not append a second copy"
        );
        assert_eq!(
            entities_of(&mut app, &ground),
            ground_before,
            "and an object that never left is not rebuilt on top of itself"
        );
    }

    // The whole world can also be taken away: every activated object's entities
    // go, and nothing of the run is left in the world to inherit.
    let present_before: Vec<WorldObjectId> = population().iter().map(|key| object(key)).collect();
    unload_world(&mut app).expect("the harbor world is loaded");
    for object in &present_before {
        assert!(
            entities_of(&mut app, object).is_empty(),
            "unloading the world must take `{object}`'s entities with it"
        );
    }
    assert!(
        residency(app.world()).is_none(),
        "and forget the load record, so the next load starts from nothing"
    );
}

/// The residency rule is the record's own: an object that belongs to two
/// sectors stays while either is loaded, an object that names none is resident,
/// and a reload never produces a second copy of anything.
///
/// Observable failure if residency were "everything in the unloaded sector goes",
/// or if a reload appended instead of restoring: the ground slab would vanish
/// with the yard, or the hangar would exist twice after two round trips.
#[test]
fn accept_f18_b_reloading_a_sector_keeps_one_entity_per_object_and_residency_by_membership() {
    let definition = harbor();
    let instance = mission(&definition, None, &population(), &[]);
    let (mut app, _) = loaded(&definition, &instance);
    let ground = object(HARBOR_OBJECT_GROUND);
    let hangar = object(HARBOR_OBJECT_HANGAR);
    let water = object(HARBOR_OBJECT_WATER);

    for _ in 0..2 {
        unload_sector(&mut app, &sector(HARBOR_SECTOR_YARD)).expect("the yard unloads");
        assert!(
            !present(&app).contains(&hangar),
            "the hangar belongs only to the yard, so it leaves with it"
        );
        assert!(
            present(&app).contains(&ground),
            "the ground belongs to the approach sector as well, so it stays"
        );
        assert!(
            present(&app).contains(&water),
            "the water patch names no sector, so it is resident"
        );
        let reload = load_sector(&mut app, &sector(HARBOR_SECTOR_YARD), &harbor_meshes())
            .expect("the yard reloads");
        assert_eq!(
            reload.spawned,
            vec![hangar.clone(), object(HARBOR_OBJECT_SENSOR)]
        );
        assert_eq!(
            reload.spawned.len(),
            present(&app)
                .iter()
                .filter(|id| **id == hangar || **id == object(HARBOR_OBJECT_SENSOR))
                .count(),
            "a reload must not spawn a second copy of what is already there"
        );
    }

    let resident = residency(app.world())
        .expect("a world is loaded")
        .resident();
    assert_eq!(
        resident.present_objects().len(),
        population().len(),
        "after two unload/reload round trips the world holds exactly its \
         population again, saw {:?}",
        resident
            .present_objects()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        resident.loaded_sectors().len(),
        2,
        "both sectors are loaded again: {:?}",
        resident
            .loaded_sectors()
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>()
    );
}

/// **F18 non-negotiable behavior 5:** a second mission's population, variant and
/// authored damage replace the first's, and nothing of the first run survives —
/// neither its objects nor its state.
///
/// Observable failure if a load could be layered over another, or if state
/// survived a world unload: the first mission's sound objective would be the
/// second mission's damaged one, and the second's population would include the
/// first's extra objects.
#[test]
fn accept_f18_b_another_mission_loads_its_own_population_and_damage_with_no_leftovers() {
    let definition = harbor();
    let first = mission(
        &definition,
        Some("synthetic.harbor_world.mission_01"),
        &population(),
        &[HARBOR_OBJECT_HANGAR],
    );
    let (mut app, _) = loaded(&definition, &first);
    let hangar = object(HARBOR_OBJECT_HANGAR);
    let banner = object(HARBOR_OBJECT_BANNER);
    assert_eq!(
        condition_of(app.world(), &hangar),
        Some(WorldObjectCondition::Damaged),
        "the first mission starts its objective damaged"
    );

    // The second mission is narrower: no banner, no trigger, and it damages the
    // water patch instead. Everything it says must replace the first's.
    let second = mission(
        &definition,
        Some("synthetic.harbor_world.mission_02"),
        &[
            HARBOR_OBJECT_HANGAR,
            HARBOR_OBJECT_WATER,
            HARBOR_OBJECT_GROUND,
        ],
        &[HARBOR_OBJECT_WATER],
    );
    let unload = unload_world(&mut app).expect("the first mission is loaded");
    assert_eq!(
        unload.despawned.len(),
        population().len(),
        "unloading takes every activated object of the first run"
    );
    assert!(
        app.world().get_resource::<WorldResidency>().is_none(),
        "and forgets the load record entirely: nothing may be inherited"
    );

    let report = load_world(&mut app, &definition, &second, &harbor_meshes())
        .expect("the second mission loads");
    let spawned: Vec<&WorldObjectId> = report.objects().iter().map(|o| &o.object).collect();
    assert_eq!(
        spawned,
        vec![
            &hangar,
            &object(HARBOR_OBJECT_WATER),
            &object(HARBOR_OBJECT_GROUND)
        ],
        "the second run's population replaces the first's, and only its own"
    );
    assert!(
        !present(&app).contains(&banner),
        "an object the second population does not activate must not survive from \
         the first run"
    );
    assert_eq!(
        condition_of(app.world(), &hangar),
        Some(WorldObjectCondition::Authored),
        "the second mission authors the objective sound: the first run's damage \
         did not carry over"
    );
    assert_eq!(
        condition_of(app.world(), &object(HARBOR_OBJECT_WATER)),
        Some(WorldObjectCondition::Damaged),
        "and it starts the water patch damaged, which is its own authored state"
    );
    assert_eq!(
        residency(app.world())
            .expect("a world is loaded")
            .resident()
            .variant()
            .clone()
            .known()
            .map(|variant| variant.key().to_owned()),
        Some("synthetic.harbor_world.mission_02".to_owned()),
        "the load states its own variant, and it is the second mission's"
    );
}

/// Loading over a resident world is refused by name, not merged: two populations
/// in one Bevy world is how "leftovers from the last run" would happen.
///
/// Observable failure if the second load proceeded, or if the refusal were
/// silent: the world would carry two worlds' objects under one residency record.
#[test]
fn accept_f18_b_a_second_load_over_a_resident_world_is_refused_rather_than_merged() {
    let definition = harbor();
    let first = mission(&definition, None, &population(), &[HARBOR_OBJECT_HANGAR]);
    let (mut app, _) = loaded(&definition, &first);
    let before = present(&app);

    let second = mission(
        &definition,
        Some("synthetic.harbor_world.mission_02"),
        &[HARBOR_OBJECT_HANGAR],
        &[],
    );
    let error = load_world(&mut app, &definition, &second, &harbor_meshes())
        .expect_err("a second load over a resident world must be refused");
    assert!(
        matches!(
            &error,
            WorldLoadError::WorldAlreadyResident { resident, requested }
                if resident == definition.id() && requested == definition.id()
        ),
        "the refusal must name the resident and the requested world, got {error:?}"
    );
    assert_eq!(
        present(&app),
        before,
        "and it must change nothing: the first load is untouched"
    );
    assert_eq!(
        condition_of(app.world(), &object(HARBOR_OBJECT_HANGAR)),
        Some(WorldObjectCondition::Damaged),
        "including the first run's own authored damage"
    );
}

/// Every sector call refuses by name and changes nothing when the sector is not
/// in the state the call needs, and a load record that damages an object its own
/// population never activates is refused at the record.
///
/// Observable failure if these were no-ops or if they mutated: a caller could
/// not tell a sector it never declared from one that is merely unloaded, and an
/// object could be given a condition nothing could ever show.
#[test]
fn accept_f18_b_every_load_refusal_names_what_it_refused_and_changed_nothing() {
    let definition = harbor();
    let instance = mission(&definition, None, &population(), &[HARBOR_OBJECT_HANGAR]);
    let (mut app, _) = loaded(&definition, &instance);
    let before = present(&app);
    let conditions: Vec<WorldObjectId> = present(&app)
        .iter()
        .map(|id| {
            assert!(condition_of(app.world(), id).is_some());
            id.clone()
        })
        .collect();
    assert_eq!(conditions.len(), before.len());

    let unknown = sector("no_such_sector");
    assert!(
        matches!(
            unload_sector(&mut app, &unknown),
            Err(WorldLoadError::UnknownSector { sector }) if sector == unknown
        ),
        "unloading a sector the definition never declared must name it"
    );
    assert!(
        matches!(
            load_sector(&mut app, &unknown, &harbor_meshes()),
            Err(WorldLoadError::UnknownSector { sector }) if sector == unknown
        ),
        "loading one must name it too"
    );
    let yard = sector(HARBOR_SECTOR_YARD);
    assert!(
        matches!(
            load_sector(&mut app, &yard, &harbor_meshes()),
            Err(WorldLoadError::SectorAlreadyResident { sector }) if sector == yard
        ),
        "loading a sector that is already loaded must say so, not spawn it twice"
    );
    unload_sector(&mut app, &yard).expect("the yard unloads");
    assert!(
        matches!(
            unload_sector(&mut app, &yard),
            Err(WorldLoadError::SectorNotResident { sector }) if sector == yard
        ),
        "unloading it twice must say so"
    );
    assert_eq!(
        present(&app),
        before
            .iter()
            .filter(
                |id| **id != object(HARBOR_OBJECT_HANGAR) && **id != object(HARBOR_OBJECT_SENSOR)
            )
            .cloned()
            .collect::<Vec<_>>(),
        "the refusals themselves changed nothing; only the one real unload did"
    );

    // An object outside the load's population has no condition to change.
    let other = harbor();
    let narrow = mission(&other, None, &[HARBOR_OBJECT_HANGAR], &[]);
    let (mut narrow_app, _) = loaded(&other, &narrow);
    let trigger = object(HARBOR_OBJECT_SENSOR);
    assert!(
        matches!(
            damage_object(&mut narrow_app, &trigger),
            Err(WorldLoadError::ObjectNotLoaded { object }) if object == trigger
        ),
        "damaging an object the population never activates must be refused"
    );

    // And the record itself refuses to name one when it is checked, which is
    // what every load does before it spawns anything.
    let phantom = world_instance(
        &definition,
        None,
        &[HARBOR_OBJECT_HANGAR],
        &[HARBOR_OBJECT_SENSOR],
    )
    .expect("the record itself is well formed");
    let sensor = object(HARBOR_OBJECT_SENSOR);
    assert!(
        matches!(
            phantom.validate_against(&definition),
            Err(WorldError::DamagedObjectNotActivated { object }) if object == sensor
        ),
        "a load that starts an unactivated object damaged must be refused by name"
    );
    let mut refused = world_app();
    assert!(
        matches!(
            load_world(&mut refused, &definition, &phantom, &harbor_meshes()),
            Err(WorldLoadError::Instance(
                WorldError::DamagedObjectNotActivated { .. }
            ))
        ),
        "and a load must refuse it before spawning anything"
    );
    assert!(
        residency(refused.world()).is_none(),
        "the refused load left no residency behind"
    );

    // With nothing loaded at all, every sector call says so.
    let mut empty = world_app();
    assert!(
        matches!(
            unload_sector(&mut empty, &yard),
            Err(WorldLoadError::NoResidentWorld)
        ),
        "there is nothing to act on when no world is loaded"
    );
    assert!(
        matches!(
            load_sector(&mut empty, &yard, &harbor_meshes()),
            Err(WorldLoadError::NoResidentWorld)
        ),
        "and a load must say that too"
    );
    assert!(
        matches!(
            damage_object(&mut empty, &object(HARBOR_OBJECT_HANGAR)),
            Err(WorldLoadError::NoResidentWorld)
        ),
        "a state change needs a load to own it"
    );
}
