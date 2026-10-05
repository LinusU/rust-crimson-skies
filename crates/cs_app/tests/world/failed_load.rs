//! Task #502: a world load that fails part-way leaves the app as it found it —
//! no entities, no residency record and no engine mesh assets.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-B`, following #425 (`F18-B-followup-shared-mesh-asset`). Task
//! test prefix: `accept_f18_b_`.
//!
//! # How the failure is produced, and why this one
//!
//! `load_world` decomposes every activated object's matrix through
//! `instance_placements` before the first entity exists, and `spawn_object`'s
//! only refusal (`WorldSpawnError::UnplaceableAffine`) comes from the same two
//! functions on the same record. A record `spawn_object` refuses is therefore
//! refused by the pre-flight *before* any object has spawned (pinned for
//! `spawn_world` by `spawn::accept_f18_a_spawn_refuses_a_matrix_no_runtime_transform_can_hold_before_spawning_anything`)
//! — it cannot be the failure that happens part-way.
//!
//! The refusal that *can* come after objects exist is
//! `WorldLoadError::VanishedEntity`: something outside the load transaction
//! despawned an entity of the object the load is stamping. [`vanish_on_stamp`]
//! is that outside actor — an observer on the [`ObjectCondition`] the load
//! stamps on each entity. When the load stamps the first entity of a named
//! two-entity object (a cuboid: presentation plus collider), it despawns the
//! object's *other* entity, so the stamp's next step finds it gone. It also
//! records how many *other* objects had already spawned at that moment.
//! (Despawning at spawn time instead is not possible: the spawn path inserts
//! components after `spawn`, and Bevy panics on an insert into a despawned
//! entity, which would test Bevy rather than the rollback.)
//! Production code does everything else: the fixtures are the production
//! builders, the load is `load_world`, and the rollback is the one under test.
//!
//! No original data and no `CS_GAME_DIR` access: every world here is
//! `Origin::SyntheticFixture`.

use std::collections::BTreeSet;

use bevy::asset::Assets;
use bevy::mesh::Mesh;
use bevy::prelude::{Add, App, Commands, Entity, On, Query, ResMut, Resource, Transform, With};

use cs_app::world::{
    HARBOR_OBJECT_ABSENT, HARBOR_OBJECT_BANNER, HARBOR_OBJECT_GROUND, HARBOR_OBJECT_HANGAR,
    HARBOR_OBJECT_SENSOR, HARBOR_OBJECT_WATER, HARBOR_SECTOR_APPROACH, HARBOR_SECTOR_YARD,
    MESH_SETTLE_UPDATES, ObjectCondition, TWIN_OBJECT_BANNER, TWIN_OBJECT_GROUND,
    TWIN_OBJECT_PANEL, TWIN_OBJECT_SHELL_A, TWIN_OBJECT_SHELL_B, TWIN_OBJECT_TRIGGER,
    WorldLoadError, WorldMeshAssets, WorldObjectBinding, harbor_meshes, harbor_world, load_sector,
    load_world, residency, spawn_world, twin_harbor_meshes, twin_harbor_world, unload_sector,
    world_app, world_instance,
};
use cs_content::world::{SectorId, WorldDefinition, WorldInstance, WorldObjectId};

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object key is valid")
}

/// The twin harbor world: five mesh-backed records over two meshes, then a
/// cuboid, in definition order — so a failure on the cuboid comes after both
/// meshes were uploaded.
fn twin() -> (WorldDefinition, WorldInstance) {
    let definition = twin_harbor_world().expect("the twin harbor world is well formed");
    let instance = world_instance(
        &definition,
        None,
        &[
            TWIN_OBJECT_SHELL_A,
            TWIN_OBJECT_SHELL_B,
            TWIN_OBJECT_PANEL,
            TWIN_OBJECT_BANNER,
            TWIN_OBJECT_TRIGGER,
            TWIN_OBJECT_GROUND,
        ],
        &[],
    )
    .expect("a valid fixture load record");
    (definition, instance)
}

/// The harbor world, every object activated.
fn harbor() -> (WorldDefinition, WorldInstance) {
    let definition = harbor_world().expect("the synthetic harbor world is well formed");
    let instance = world_instance(
        &definition,
        None,
        &[
            HARBOR_OBJECT_HANGAR,
            HARBOR_OBJECT_SENSOR,
            HARBOR_OBJECT_BANNER,
            HARBOR_OBJECT_WATER,
            HARBOR_OBJECT_GROUND,
            HARBOR_OBJECT_ABSENT,
        ],
        &[],
    )
    .expect("a valid fixture load record");
    (definition, instance)
}

/// The one-shot outside despawn, and what it saw when it fired.
#[derive(Resource)]
struct Vanish {
    /// The object whose second entity is despawned while the first is stamped.
    target: WorldObjectId,
    /// Every other object whose entity had been added before the despawn.
    seen: BTreeSet<WorldObjectId>,
    /// The entity the observer despawned, once it has.
    vanished: Option<Entity>,
    /// How many other objects had spawned when it fired.
    spawned_before: Option<usize>,
}

/// Installs the outside actor: when the load stamps the first entity of
/// `target`, the object's other entity is despawned straight away, once.
fn vanish_on_stamp(app: &mut App, target: &str) {
    let world = app.world_mut();
    world.insert_resource(Vanish {
        target: object(target),
        seen: BTreeSet::new(),
        vanished: None,
        spawned_before: None,
    });
    world.add_observer(
        |add: On<Add, WorldObjectBinding>,
         bindings: Query<&WorldObjectBinding>,
         mut vanish: ResMut<Vanish>| {
            if let Ok(binding) = bindings.get(add.entity)
                && binding.object() != &vanish.target
                && vanish.vanished.is_none()
            {
                vanish.seen.insert(binding.object().clone());
            }
        },
    );
    world.add_observer(
        |add: On<Add, ObjectCondition>,
         bindings: Query<(Entity, &WorldObjectBinding)>,
         mut vanish: ResMut<Vanish>,
         mut commands: Commands| {
            if vanish.vanished.is_some() {
                return;
            }
            let Ok((_, binding)) = bindings.get(add.entity) else {
                return;
            };
            if binding.object() != &vanish.target {
                return;
            }
            let other = bindings
                .iter()
                .find(|(entity, other)| *entity != add.entity && other.object() == &vanish.target)
                .map(|(entity, _)| entity)
                .expect("the target is a two-entity object");
            vanish.vanished = Some(other);
            vanish.spawned_before = Some(vanish.seen.len());
            commands.entity(other).despawn();
        },
    );
}

/// Every entity that carries a world-object binding.
fn world_entities(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<WorldObjectBinding>>();
    query.iter(world).collect()
}

/// How many mesh assets the engine currently holds.
fn asset_count(app: &App) -> usize {
    app.world().resource::<Assets<Mesh>>().len()
}

/// How many entries the loader's owning-handle record holds, or `None` when the
/// record is absent.
fn loader_assets(app: &App) -> Option<usize> {
    app.world()
        .get_resource::<WorldMeshAssets>()
        .map(WorldMeshAssets::len)
}

fn settle(app: &mut App) {
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
}

/// A first world load that fails on its last object gives back everything it
/// took: the five objects it had already spawned, the half-built object it
/// failed on, and the two engine mesh assets those objects uploaded.
///
/// Observable failure if the rollback is removed (the `?`/despawn-only shape
/// `load_world` had before #502): world entities survive the failed load, or
/// `WorldMeshAssets` is still present and the engine still holds two assets
/// after the settle updates.
#[test]
fn accept_f18_b_a_world_load_that_fails_part_way_leaves_no_entities_and_no_mesh_assets() {
    let (definition, instance) = twin();
    let mut app = world_app();
    settle(&mut app);
    let assets_before = asset_count(&app);
    assert_eq!(
        assets_before, 0,
        "nothing is loaded yet, so the counts below are about this load"
    );
    assert!(world_entities(&mut app).is_empty());
    vanish_on_stamp(&mut app, TWIN_OBJECT_GROUND);

    let error = load_world(&mut app, &definition, &instance, &twin_harbor_meshes())
        .expect_err("the load must fail when one of its entities vanishes");

    let vanish = app.world().resource::<Vanish>();
    let vanished = vanish.vanished.expect("the outside despawn fired");
    let spawned_before = vanish.spawned_before.expect("it recorded what it saw");
    assert!(
        matches!(&error, WorldLoadError::VanishedEntity { object: failed, entity, .. }
            if *failed == object(TWIN_OBJECT_GROUND) && *entity == vanished),
        "the load names the object and the entity that vanished, got {error:?}"
    );
    assert_eq!(
        spawned_before, 5,
        "the failure came after the five objects ahead of the ground had spawned, so \
         the rollback below had something to take back"
    );
    assert!(
        world_entities(&mut app).is_empty(),
        "no world-object entity survives the failed load — neither the five objects \
         spawned before the failure nor the ground's own surviving presentation"
    );
    assert!(
        residency(app.world()).is_none(),
        "no half-written residency record is left"
    );
    assert_eq!(
        loader_assets(&app),
        None,
        "the loader's owning handles go with the failed load, because no residency is \
         left for an unload to release them through"
    );
    settle(&mut app);
    assert_eq!(
        asset_count(&app),
        assets_before,
        "and with no strong handle left the engine frees the meshes the failed load \
         uploaded"
    );

    // The app really is as it was: the same world now loads cleanly over it.
    let report = load_world(&mut app, &definition, &instance, &twin_harbor_meshes())
        .expect("a load after the failed one starts from nothing");
    settle(&mut app);
    assert_eq!(report.objects().len(), 6, "every activated object spawns");
    assert_eq!(
        asset_count(&app),
        2,
        "one asset per named mesh again, not two leftovers plus two"
    );
}

/// A failed world load puts back exactly the owning-handle record it found:
/// entries a direct `spawn_world` left stay, and only the ones this load added
/// are released.
///
/// Observable failure if the rollback simply removes `WorldMeshAssets`: the twin
/// world's two entries vanish with it and its geometry is freed while its
/// entities still draw it. If the rollback forgets the record, the harbor's four
/// entries stay.
#[test]
fn accept_f18_b_a_failed_world_load_restores_the_mesh_record_it_found() {
    let mut app = world_app();
    let twin_definition = twin_harbor_world().expect("the twin harbor world is well formed");
    let twin_spawn = spawn_world(&mut app, &twin_definition, &twin_harbor_meshes())
        .expect("the twin world spawns");
    settle(&mut app);
    assert_eq!(loader_assets(&app), Some(2));
    assert_eq!(asset_count(&app), 2);
    let twin_entities = world_entities(&mut app);

    let (definition, instance) = harbor();
    vanish_on_stamp(&mut app, HARBOR_OBJECT_GROUND);
    let error = load_world(&mut app, &definition, &instance, &harbor_meshes())
        .expect_err("the harbor load fails on its ground slab");
    assert!(
        matches!(&error, WorldLoadError::VanishedEntity { object: failed, .. }
            if *failed == object(HARBOR_OBJECT_GROUND)),
        "got {error:?}"
    );
    assert_eq!(
        app.world().resource::<Vanish>().spawned_before,
        Some(4),
        "the hangar, sensor, banner and water — four meshes of the harbor's own — \
         had spawned before the ground failed"
    );

    assert_eq!(
        world_entities(&mut app),
        twin_entities,
        "the failed load took back its own entities and only those"
    );
    assert!(residency(app.world()).is_none());
    assert_eq!(
        loader_assets(&app),
        Some(2),
        "the record holds exactly the two entries it held before the load"
    );
    settle(&mut app);
    assert_eq!(
        asset_count(&app),
        2,
        "the harbor's meshes are freed and the twin world's are kept"
    );
    let shell = twin_spawn
        .visual_for(&object(TWIN_OBJECT_SHELL_A))
        .expect("the twin shell is presented");
    assert!(
        app.world().get::<Transform>(shell).is_some(),
        "the twin world's own objects are untouched"
    );
}

/// A failed **sector** load keeps its own rollback: it takes back only what it
/// spawned, and releases none of the resident world's mesh assets.
///
/// Observable failure if a sector load were routed through the world-load
/// rollback: the loader's record disappears and the engine frees the meshes the
/// still-present water draws. If the rollback were removed, the hangar respawned
/// before the failure would survive unclaimed.
///
/// Both sectors are unloaded first, so the ground slab — the harbor's one
/// two-entity object, in both sectors — comes back with the yard, after the
/// hangar; the water names no sector and stays throughout.
#[test]
fn accept_f18_b_a_failed_sector_load_keeps_the_resident_world_and_its_assets() {
    let (definition, instance) = harbor();
    let mut app = world_app();
    load_world(&mut app, &definition, &instance, &harbor_meshes()).expect("the harbor loads");
    settle(&mut app);
    let yard = SectorId::new(HARBOR_SECTOR_YARD).expect("the fixture sector key is valid");
    let approach = SectorId::new(HARBOR_SECTOR_APPROACH).expect("the fixture sector key is valid");
    unload_sector(&mut app, &approach).expect("the approach unloads");
    unload_sector(&mut app, &yard).expect("the yard unloads");
    settle(&mut app);
    let present_before = present(&app);
    let entities_before = world_entities(&mut app);
    let records_before = loader_assets(&app);
    let assets_before = asset_count(&app);
    assert_eq!(
        present_before,
        vec![object(HARBOR_OBJECT_WATER)],
        "only the sectorless water is present before the sector load"
    );
    assert!(
        records_before.is_some_and(|count| count > 0),
        "the resident world holds mesh assets a wrong rollback could release"
    );

    // The yard brings back the hangar, then the ground; the ground fails.
    vanish_on_stamp(&mut app, HARBOR_OBJECT_GROUND);
    let error = load_sector(&mut app, &yard, &harbor_meshes())
        .expect_err("the sector load fails on the ground");
    assert!(
        matches!(&error, WorldLoadError::VanishedEntity { object: failed, .. }
            if *failed == object(HARBOR_OBJECT_GROUND)),
        "got {error:?}"
    );
    assert_eq!(
        app.world().resource::<Vanish>().spawned_before,
        Some(1),
        "the hangar had respawned before the ground failed"
    );

    assert_eq!(
        present(&app),
        present_before,
        "the record claims exactly the objects it claimed before the call"
    );
    assert_eq!(
        world_entities(&mut app),
        entities_before,
        "the hangar this call spawned is gone, and every other object's entity is \
         still live"
    );
    assert!(
        !residency(app.world())
            .expect("the world is still resident")
            .resident()
            .loaded_sectors()
            .contains(&yard),
        "the sector stays unloaded"
    );
    assert_eq!(
        loader_assets(&app),
        records_before,
        "the resident world's owning handles are untouched"
    );
    settle(&mut app);
    assert_eq!(
        asset_count(&app),
        assets_before,
        "and the engine still holds every mesh the world loaded"
    );
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
