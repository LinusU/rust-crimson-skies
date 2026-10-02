//! Task #425: one **engine** mesh asset per named mesh, shared by every object
//! record that names it.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-B`, following `F18-B-followup-mesh-source-from-catalog`
//! (#422). Task test prefix: `accept_f18_b_`.
//!
//! The **source** already de-duplicates — two records naming one mesh resolve to
//! one `WorldMesh`, one upload, one fingerprint — and `import.rs` proves it. What
//! was not shared was the engine asset: `spawn_mesh_presentation` and
//! `spawn_mesh_collider` each cloned the upload into `Assets<Mesh>`, so N records
//! naming one mesh added N identical copies of the same geometry. These tests
//! drive the twin harbor world [`twin_harbor_world`], whose four mesh records —
//! two solids, a presentation-only banner and a trigger volume — all name **one**
//! stored mesh, through the production [`spawn_world`] and [`load_world`].
//!
//! What is pinned here:
//!
//! * four records naming one mesh hold **one** strong handle and one engine asset,
//!   across all four mesh layouts (solid, trigger, presentation-only);
//! * a record naming a **different** mesh gets a different asset, and a cuboid
//!   record — which resolves no mesh at all — adds none;
//! * every derived collider still carries every stored triangle of the merged
//!   mesh, so sharing an asset changed nothing about the geometry;
//! * unloading the world releases the shared asset, and reloading draws and
//!   collides from it again.
//!
//! No original data and no `CS_GAME_DIR` access: the fixture is
//! `Origin::SyntheticFixture` and proves the asset bookkeeping, never the original
//! game.

use avian3d::prelude::{Collider, RigidBody, Sensor};
use bevy::asset::{Assets, Handle};
use bevy::mesh::{Indices, Mesh, Mesh3d};
use bevy::prelude::{App, Entity, Transform, Vec3, World};
use cs_app::world::{
    HARBOR_HANGAR_TRIANGLES, MESH_SETTLE_UPDATES, TWIN_OBJECT_BANNER, TWIN_OBJECT_GROUND,
    TWIN_OBJECT_PANEL, TWIN_OBJECT_SHELL_A, TWIN_OBJECT_SHELL_B, TWIN_OBJECT_TRIGGER,
    WorldMeshAssets, harbor_world, load_world, residency, spawn_world, twin_harbor_meshes,
    twin_harbor_world, unload_world, world_app, world_instance,
};
use cs_content::world::{WorldDefinition, WorldObjectId};

/// The panel mesh is a plain box: six quad faces, twelve triangles. It is here so
/// the "different mesh, different asset" assertion can name a triangle count of
/// its own instead of relying on "not the same handle".
const PANEL_TRIANGLES: usize = 12;

/// The twin harbor world, built by production code.
fn twin() -> WorldDefinition {
    twin_harbor_world().expect("the twin harbor world is well formed")
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object key is valid")
}

/// Every object the twin world declares, in definition order.
fn population() -> [&'static str; 6] {
    [
        TWIN_OBJECT_SHELL_A,
        TWIN_OBJECT_SHELL_B,
        TWIN_OBJECT_PANEL,
        TWIN_OBJECT_BANNER,
        TWIN_OBJECT_TRIGGER,
        TWIN_OBJECT_GROUND,
    ]
}

/// A mission load record activating the whole twin world.
fn mission(definition: &WorldDefinition) -> cs_content::world::WorldInstance {
    world_instance(definition, None, &population(), &[]).expect("a valid fixture load record")
}

/// How many mesh assets the engine currently holds.
fn asset_count(app: &App) -> usize {
    app.world().resource::<Assets<Mesh>>().len()
}

/// The `Mesh3d` handle `entity` presents.
fn presented(app: &App, entity: Entity) -> Handle<Mesh> {
    app.world()
        .get::<Mesh3d>(entity)
        .unwrap_or_else(|| panic!("entity {entity} presents the mesh it collides with"))
        .0
        .clone()
}

/// Triangles the engine asset `handle` resolves to, counted from its index
/// buffer.
///
/// This is the asset's own count, independent of any collider derived from it,
/// so "the shared asset is the whole merged mesh" and "the colliders built from
/// it are complete" are two separate facts rather than one measurement twice.
fn asset_triangles(app: &App, handle: &Handle<Mesh>) -> usize {
    let mesh = app
        .world()
        .resource::<Assets<Mesh>>()
        .get(handle)
        .unwrap_or_else(|| panic!("handle {handle:?} resolves in the engine's asset stack"));
    let indices = match mesh.indices() {
        Some(Indices::U16(values)) => values.len(),
        Some(Indices::U32(values)) => values.len(),
        None => 0,
    };
    // A triangle list indexes three corners per triangle, and the count this test
    // compares against is a triangle count like `WorldMesh::triangles`, so the
    // two are the same unit here and not two units that happen to agree.
    assert_eq!(
        indices % 3,
        0,
        "a triangle list's index buffer is a whole number of triangles, got {indices}"
    );
    indices / 3
}

/// Triangles the collider Avian derived onto `entity` carries.
fn collider_triangles(world: &World, entity: Entity) -> usize {
    world
        .get::<Collider>(entity)
        .unwrap_or_else(|| panic!("entity {entity} has the collider Avian derived"))
        .shape()
        .as_trimesh()
        .unwrap_or_else(|| {
            panic!(
                "entity {entity}: the derived collider is a triangle mesh, not a \
                 substitute primitive"
            )
        })
        .indices()
        .len()
}

/// The twin world spawned through the production conversion and settled, so
/// Avian's mesh constructors have run.
fn spawned() -> (App, cs_app::world::SpawnedWorld) {
    let mut app = world_app();
    let report =
        spawn_world(&mut app, &twin(), &twin_harbor_meshes()).expect("the twin world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    (app, report)
}

/// **One stored mesh, one engine asset, four records — on every mesh layout.**
///
/// Two solids, a presentation-only banner and a trigger volume all name the same
/// stored mesh, and all four must end up on the *same* strong handle in
/// `Assets<Mesh>`: the geometry F17-B uploaded once is not copied once per
/// instance that draws or collides with it. The fifth record names a different
/// mesh and must get a different asset, and the cuboid record — which resolves no
/// mesh at all — must add none, so "one asset per named mesh" is not
/// "one asset for the whole world" and not "one asset per object".
///
/// The shared asset is asserted to carry the **merged** mesh's own triangle
/// count, so sharing cannot have swapped in one material group of it (the shell
/// is three groups; the whole is 36 triangles, one group is 12), and every
/// derived collider is checked to carry the same triangles, so the change is
/// bookkeeping and not geometry.
///
/// Observable failure if the sharing is removed or widened: `asset_count` is 4
/// instead of 2 (or 1, if keyed by world rather than by mesh), one of the four
/// handles differs from the others, or a collider reports 12 triangles instead of
/// 36.
#[test]
fn accept_f18_b_records_naming_one_mesh_share_one_engine_asset_and_others_do_not() {
    let (app, report) = spawned();

    // Nothing was in the engine's asset stack before the world was spawned, so
    // the count below is about this conversion alone.
    assert_eq!(
        asset_count(&app),
        2,
        "six records over two stored meshes add exactly two engine assets: the \
         shared shell and the panel's own, and no asset for the cuboid"
    );
    assert_eq!(
        app.world().resource::<WorldMeshAssets>().len(),
        2,
        "and the loader's own record of them holds the same two"
    );

    // The four consumers of one mesh, one handle.
    let shell_a = report
        .visual_for(&object(TWIN_OBJECT_SHELL_A))
        .expect("the first shell is presented");
    let shell_b = report
        .visual_for(&object(TWIN_OBJECT_SHELL_B))
        .expect("the second shell is presented");
    let banner = report
        .visual_for(&object(TWIN_OBJECT_BANNER))
        .expect("the banner is presented");
    let trigger = report
        .visual_for(&object(TWIN_OBJECT_TRIGGER))
        .expect("the trigger volume is presented");
    let panel = report
        .visual_for(&object(TWIN_OBJECT_PANEL))
        .expect("the panel is presented");

    let shared = presented(&app, shell_a);
    for (key, entity) in [
        (TWIN_OBJECT_SHELL_A, shell_a),
        (TWIN_OBJECT_SHELL_B, shell_b),
        (TWIN_OBJECT_BANNER, banner),
        (TWIN_OBJECT_TRIGGER, trigger),
    ] {
        assert_eq!(
            presented(&app, entity),
            shared,
            "`{key}` names the same stored mesh, so it must present the same engine \
             asset rather than a second copy of the same geometry"
        );
    }

    // A record naming a different mesh gets a different asset, with different
    // geometry in it: the two handles must not be equal and neither resolves the
    // other's mesh.
    let panel_handle = presented(&app, panel);
    assert_ne!(
        panel_handle, shared,
        "the panel names its own mesh, so it must get its own engine asset"
    );
    assert_eq!(
        asset_triangles(&app, &shared),
        HARBOR_HANGAR_TRIANGLES,
        "the shared asset is the whole merged shell, not one material group of it"
    );
    assert_eq!(
        asset_triangles(&app, &panel_handle),
        PANEL_TRIANGLES,
        "and the panel's asset is the panel's own geometry"
    );

    // The cuboid record resolves no mesh, so it names no asset at all — and its
    // presentation is the F18-A marker with no `Mesh3d`.
    let ground = report
        .visual_for(&object(TWIN_OBJECT_GROUND))
        .expect("the ground slab is presented");
    assert!(
        app.world().get::<Mesh3d>(ground).is_none(),
        "a cuboid record presents the F18-A marker, which carries no geometry, so \
         it must not acquire a mesh handle"
    );
    assert!(
        report
            .object(&object(TWIN_OBJECT_GROUND))
            .expect("the ground slab is in the report")
            .mesh
            .is_none(),
        "and it reports no mesh provenance, because it resolved none"
    );

    // Sharing changed no geometry: every mesh record's derived collider carries
    // every triangle its own upload stored. The four records on the shared asset
    // each derive their **own** trimesh from it, which is the whole point — one
    // asset, four colliders, all of them complete.
    for (key, entity, triangles) in [
        (TWIN_OBJECT_SHELL_A, shell_a, HARBOR_HANGAR_TRIANGLES),
        (TWIN_OBJECT_SHELL_B, shell_b, HARBOR_HANGAR_TRIANGLES),
        (TWIN_OBJECT_TRIGGER, trigger, HARBOR_HANGAR_TRIANGLES),
        (TWIN_OBJECT_PANEL, panel, PANEL_TRIANGLES),
    ] {
        assert_eq!(
            collider_triangles(app.world(), entity),
            triangles,
            "`{key}`: a `TrimeshFromMesh` collider must still carry every triangle \
             of the mesh it is built from"
        );
    }

    // The banner presented the shared asset and was still never given a collider:
    // sharing an asset must not turn a role-`None` record into geometry a body
    // can reach.
    assert!(
        app.world().get::<Collider>(banner).is_none(),
        "the banner shares the shell's asset but must still never collide"
    );
    assert!(
        report.collider_for(&object(TWIN_OBJECT_BANNER)).is_none(),
        "and the report still says so, rather than presenting the share as a \
         collider it does not have"
    );

    // The trigger volume shares the asset too, and is still a body-less sensor:
    // one shared asset must not make two roles the same layout.
    assert!(
        app.world().get::<RigidBody>(trigger).is_none(),
        "the trigger volume shares the shell's asset but must still carry no rigid \
         body, or swept CCD could hold a body at its face"
    );
    assert!(
        app.world().get::<Sensor>(trigger).is_some(),
        "and it is still marked as a sensor"
    );
    assert_eq!(
        app.world().get::<RigidBody>(shell_a),
        Some(&RigidBody::Static),
        "while a shared asset leaves the solid shell on its own static body"
    );
    assert_ne!(
        shell_a, shell_b,
        "and the two solids are two entities, not one entity drawn twice"
    );
    assert_eq!(
        (
            app.world().get::<Transform>(shell_a).map(|t| t.translation),
            app.world().get::<Transform>(shell_b).map(|t| t.translation),
        ),
        (
            Some(Vec3::new(0.0, 0.0, 0.0)),
            Some(Vec3::new(100.0, 0.0, 0.0))
        ),
        "each placed from its own record: a hundred metres apart, so the shared \
         asset is not a shared pose"
    );

    // Each record's own provenance is still reported, and the four records that
    // share an asset report the *same* fingerprint and triangle count — the
    // source was already shared, and sharing the asset did not lose that.
    for key in [
        TWIN_OBJECT_SHELL_A,
        TWIN_OBJECT_SHELL_B,
        TWIN_OBJECT_BANNER,
        TWIN_OBJECT_TRIGGER,
    ] {
        let mesh = report
            .object(&object(key))
            .unwrap_or_else(|| panic!("`{key}` is in the report"))
            .mesh
            .clone()
            .unwrap_or_else(|| panic!("`{key}` resolved a mesh"));
        assert_eq!(
            mesh.triangles, HARBOR_HANGAR_TRIANGLES,
            "`{key}` reports the upload's own triangle count"
        );
    }
    let reference = report
        .object(&object(TWIN_OBJECT_SHELL_A))
        .expect("the first shell is in the report")
        .mesh
        .clone()
        .expect("it resolved a mesh")
        .id;
    for key in [TWIN_OBJECT_SHELL_B, TWIN_OBJECT_BANNER, TWIN_OBJECT_TRIGGER] {
        assert_eq!(
            report
                .object(&object(key))
                .expect("the record is in the report")
                .mesh
                .clone()
                .expect("it resolved a mesh")
                .id,
            reference,
            "`{key}` names the same authored reference the first shell does"
        );
    }
}

/// **The harbor world's own mesh path is unaffected by the sharing.** The twin
/// fixture is built for this question, so the pre-existing mesh world is checked
/// here too: one record per mesh, so the count must equal the number of distinct
/// references, and the arch's opening must still be a hole in the collision
/// rather than a hull of the same corners.
///
/// Observable failure if sharing collapsed distinct meshes together or if the
/// shared-handle path stopped uploading the whole merged mesh: the hangar's
/// collider would carry 12 triangles instead of 36, or two records would resolve
/// one another's geometry.
#[test]
fn accept_f18_b_a_world_with_one_record_per_mesh_keeps_one_asset_each_and_every_triangle() {
    let definition = harbor_world().expect("the harbor world is well formed");
    let hangar = object("objective.hangar");
    let mut app = world_app();
    let report = spawn_world(&mut app, &definition, &cs_app::world::harbor_meshes())
        .expect("the harbor world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }

    assert_eq!(
        asset_count(&app),
        app.world().resource::<WorldMeshAssets>().len(),
        "the loader's record and the engine's asset stack agree, whatever the count"
    );
    assert_eq!(
        asset_count(&app),
        4,
        "the harbor world names four resolvable meshes across four mesh records, so \
         there is nothing to share and nothing to fold together"
    );
    let hangar_entity = report.visual_for(&hangar).expect("the hangar is presented");
    assert_eq!(
        collider_triangles(app.world(), hangar_entity),
        HARBOR_HANGAR_TRIANGLES,
        "the arch's collider must carry all three material groups' triangles, so the \
         opening stays a hole rather than a hull of the same corners"
    );
}

/// **The reverse holds: unloading a world releases the shared asset, and a reload
/// draws and collides from it again.**
///
/// A shared handle is an **owning** one (see `WorldMeshAssets`), so a world
/// that forgot to release it would leave the geometry of a mission that is over
/// resident in the engine. This is measured through the load transaction, not by
/// reaching into the asset stack: after `unload_world` the loader's record is gone
/// and the asset count is back to what it was before the load, and a second load
/// of the same world produces the same shared handle, the same one asset per
/// mesh, and colliders carrying every stored triangle.
///
/// Observable failure if the unload leaks the asset: the count after the unload is
/// 2 rather than 0, or the reload's handle is not the shared one — a leaked asset
/// and a re-uploaded one are the same bug seen from two ends.
#[test]
fn accept_f18_b_unloading_a_world_releases_the_shared_mesh_assets_and_a_reload_rebuilds_them() {
    let definition = twin();
    let instance = mission(&definition);
    let mut app = world_app();
    assert_eq!(
        asset_count(&app),
        0,
        "the engine's asset stack is empty before anything is loaded, so the counts \
         below are about this load"
    );

    let first = load_world(&mut app, &definition, &instance, &twin_harbor_meshes())
        .expect("the twin world loads");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    assert_eq!(
        asset_count(&app),
        2,
        "the load put one asset per named mesh into the engine's stack"
    );
    assert!(
        residency(app.world()).is_some(),
        "and the load's record is what the unload will take"
    );

    let shell_a = first
        .visual_for(&object(TWIN_OBJECT_SHELL_A))
        .expect("the first shell is presented");
    assert!(
        app.world().get::<Mesh3d>(shell_a).is_some(),
        "the loaded shell presents the shared asset, so there is something for the \
         unload to take away"
    );

    let unload = unload_world(&mut app).expect("the twin world was loaded");
    assert_eq!(
        unload.despawned.len(),
        population().len(),
        "the unload takes every activated object of the run"
    );
    assert!(
        residency(app.world()).is_none(),
        "and forgets the load record"
    );
    assert!(
        app.world().get_resource::<WorldMeshAssets>().is_none(),
        "the loader's owning handle is released with the world, which is the only \
         place it is released"
    );
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    assert_eq!(
        asset_count(&app),
        0,
        "and with no strong handle left the engine frees the shared assets, rather \
         than the next mission inheriting the last one's geometry"
    );

    // A reload brings the geometry back, shared again and complete.
    let second = load_world(&mut app, &definition, &instance, &twin_harbor_meshes())
        .expect("the twin world loads again");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    assert_eq!(
        asset_count(&app),
        2,
        "a reload rebuilds one asset per named mesh, not one per record"
    );
    let reloaded = second
        .visual_for(&object(TWIN_OBJECT_SHELL_A))
        .expect("the first shell is presented again");
    for key in [
        TWIN_OBJECT_SHELL_A,
        TWIN_OBJECT_SHELL_B,
        TWIN_OBJECT_BANNER,
        TWIN_OBJECT_TRIGGER,
    ] {
        let entity = second
            .visual_for(&object(key))
            .unwrap_or_else(|| panic!("`{key}` is presented again"));
        assert_eq!(
            presented(&app, entity),
            presented(&app, reloaded),
            "`{key}` shares the one asset after a reload as it did before"
        );
    }
    assert_eq!(
        collider_triangles(app.world(), reloaded),
        HARBOR_HANGAR_TRIANGLES,
        "and the reloaded object collides from every triangle of the merged mesh, \
         which is what a re-upload that lost a material group would not give"
    );
    // The first run's entity is gone, so nothing is being drawn twice.
    assert!(
        app.world().get_entity(shell_a).is_err(),
        "the first run's entities are gone, so the reload cannot be the old world \
         left in place"
    );
    assert!(
        app.world().get::<Mesh3d>(reloaded).is_some(),
        "and the reloaded shell really presents an asset again"
    );
    assert_ne!(
        reloaded, shell_a,
        "the reload spawned a new entity rather than reusing the despawned one"
    );
}
