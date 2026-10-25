//! #423: a sector load must not copy the world's records.
//!
//! Task test prefix: `accept_f18_b_`. The world here is built from the public
//! `cs_content::world` records; the load is the production [`load_world`] /
//! [`unload_sector`] / [`load_sector`] path. The timing line this test prints
//! (`cargo test ... -- --nocapture`) is what
//! `docs/findings/2026-10-25-f18-b-sector-load-definition-clone.md` records.

use std::time::Instant;

use cs_app::world::{
    WorldMeshes, fixture_provenance, load_sector, load_world, residency, unload_sector, world_app,
    world_instance,
};
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    Aabb, Sector, SectorId, SurfaceRole, WorldBoundary, WorldCollisionRole, WorldCollisionShape,
    WorldDefinition, WorldId, WorldObjectId, WorldObjectInstance,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Resolved};

const SECTORS: usize = 8;
const PER_SECTOR: usize = 500;

fn known<T>(value: T, key: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, fixture_provenance(key)))
}

/// A retail-sized world of solid cuboids: `SECTORS * PER_SECTOR` instances.
fn large_world() -> (WorldDefinition, Vec<String>) {
    let sector_ids: Vec<SectorId> = (0..SECTORS)
        .map(|i| SectorId::new(&format!("large.sector{i}")).expect("valid sector key"))
        .collect();
    let sectors = sector_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let x = i as f64 * 100.0;
            Sector::new(
                id.clone(),
                Aabb::try_new([x, -2.0, 0.0], [x + 99.0, 12.0, 99.0]).expect("valid bounds"),
            )
        })
        .collect();
    let mut keys = Vec::new();
    let mut objects = Vec::new();
    for (i, sector) in sector_ids.iter().enumerate() {
        for n in 0..PER_SECTOR {
            let key = format!("large.object.{i}.{n}");
            let transform = CanonicalTransform::try_new(
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                [
                    i as f64 * 100.0 + (n % 50) as f64,
                    0.0,
                    (n / 50) as f64 * 5.0,
                ],
            )
            .expect("finite translation");
            objects.push(
                WorldObjectInstance::try_new(
                    WorldObjectId::new(&key).expect("valid object key"),
                    known(
                        ContentId::from_source(ContentKind::Mesh, &format!("synthetic.{key}"))
                            .expect("valid mesh id"),
                        &format!("{key}.mesh"),
                    ),
                    transform,
                    known(WorldCollisionRole::Solid, "large.role"),
                    known(
                        WorldCollisionShape::cuboid([0.5, 0.5, 0.5]).expect("positive extents"),
                        "large.shape",
                    ),
                    known(SurfaceRole::Ground, "large.surface"),
                    vec![sector.clone()],
                    fixture_provenance(&format!("{key}.record")),
                )
                .expect("no duplicate sectors"),
            );
            keys.push(key);
        }
    }
    let definition = WorldDefinition::try_new(
        WorldId::from_key("synthetic.large_world").expect("valid world key"),
        Origin::SyntheticFixture,
        known(
            WorldBoundary::try_new(Some(-50.0), Some(500.0), None).expect("valid boundary"),
            "large.boundary",
        ),
        sectors,
        objects,
        fixture_provenance("large_world.record"),
    )
    .expect("a well formed large world");
    (definition, keys)
}

#[test]
fn accept_f18_b_sector_reload_of_a_large_world_keeps_every_record() {
    let (definition, keys) = large_world();
    let objects = definition.objects().len();
    assert_eq!(objects, SECTORS * PER_SECTOR);
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let instance =
        world_instance(&definition, Some("large"), &key_refs, &[]).expect("a valid load record");

    let mut app = world_app();
    load_world(&mut app, &definition, &instance, &WorldMeshes::new()).expect("the world loads");

    let sector = SectorId::new("large.sector3").expect("valid sector key");
    let clone_start = Instant::now();
    let copy = definition.clone();
    let clone_cost = clone_start.elapsed();
    assert_eq!(copy.objects().len(), objects);

    unload_sector(&mut app, &sector).expect("the sector unloads");
    let load_start = Instant::now();
    let load = load_sector(&mut app, &sector, &WorldMeshes::new()).expect("the sector reloads");
    let load_cost = load_start.elapsed();

    assert_eq!(load.spawned.len(), PER_SECTOR);
    assert_eq!(
        residency(app.world())
            .expect("a world is loaded")
            .resident()
            .definition()
            .objects()
            .len(),
        objects,
        "the resident record still holds the whole definition"
    );
    println!(
        "F18-B #423: objects={objects} definition.clone()={clone_cost:?} \
         load_sector({PER_SECTOR} objects)={load_cost:?}"
    );
}
