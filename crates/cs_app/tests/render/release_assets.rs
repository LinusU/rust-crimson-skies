//! `accept_t512_` tests: a released batch hands its mesh and its material back
//! to the stores they came from.
//!
//! Rally #512, filed by the #503 reviewer. Before this, `release_entity` in
//! `crates/cs_app/src/render/sync.rs` despawned a batch entity and its
//! placements and stopped there: the `Assets<Mesh>` entry the spawn uploaded and
//! the material entry `add_material` created stayed in their stores for the life
//! of the session. The reviewer's probe measured exactly one of each per
//! release/respawn cycle, monotone. That was a rare path before #503 — a batch
//! was released only when the frame stopped naming it — and a
//! gameplay-frequency path now: the F20-C draw consumer releases a batch whenever
//! the composed visibility verdict withholds *all* of its rows and spawns it
//! again when the rows come back, so a clip that hides a whole batch, or damage
//! and repair cycling over every row of one, reaches the leak every few ticks.
//!
//! The rule under test, from `sync.rs`: a batch entity **owns** the store
//! entries its spawn added, and its placements **borrow** the mesh and material
//! handles. A release despawns the entity and its placements first and only
//! then hands an unowned entry back, so a handle a live entity still draws with
//! is never pulled out from under it, and an entry is handed back exactly once.
//! Together with the reuse guard that already existed — a frame that reuses
//! every batch adds no material — neither path grows a store.
//!
//! The fixture binds **no image**, so `images` is asserted as `0` everywhere
//! here: the third entry a spawn can add, `Assets<Image>`, has its own rule
//! (shared between batches, and named from inside the material entry) and its
//! own file, `released_image.rs`, Rally #514.
//!
//! The `accept_f17_c_reused_` selection extends the same rule to the one path
//! #512 left unfinished (Rally #516): a **reused** batch entity whose material
//! component went missing. The material is an entry this module owns, so a reused
//! entity that took a replacement would hold an entry no owner record names while
//! the record it kept named the entry it no longer draws with. `reuse_batch` now
//! treats a missing matching material component as "not usable" — the same
//! release-and-respawn the lost-`BatchDraw` repair path already used — so a
//! replacement is always added by a spawn that records it and the stores stay
//! flat.
//!
//! Every input is newly authored synthetic content decoded through the
//! production readers and driven through the production `sync_frame`. No
//! `CS_GAME_DIR` and no original-behavior claim: nothing here is
//! `verified_original`.

use bevy::asset::{Assets, Handle};
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::prelude::World;
use bevy::image::Image;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};

use cs_app::livery::{LiveryRuntime, LiverySession, ModelInstanceId, PaintChoice};
use cs_app::render::additive::AdditiveMaterial;
use cs_app::render::batch::{BatchedFrame, InstanceVisuals, PartRef, SubmittedDraw, batch_frame};
use cs_app::render::capture::{SceneOutcome, SceneSurface, SurfaceRefusal, upload_surface};
use cs_app::render::material::{
    AddressMode, ClassifiedMaterial, Coverage, DeclaredClass, MaterialClass, MaterialFacts,
    TextureAddress, classify,
};
use cs_app::render::plan::{DrawItem, DrawItemKey, DrawPlan, SceneView};
use cs_app::render::profile::{Enhancement, RenderProfile};
use cs_app::render::sync::{
    BatchDraw, BatchInstancePlacement, ReclaimedAssets, RenderProfileRequest, RenderSession,
    SyncError, process_render_profile_request, sync_frame, teardown,
};
use cs_app::scene::AirframeDamageState;
use cs_content::livery::LiveryPaint;
use cs_formats::bm::PaintColor;
use cs_formats::io::AllocationBudget;
use cs_formats::texture::AlphaTest;
use cs_formats::{ParseContext, read_bm};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimStatus;

use super::fixture::{QuadShape, quad_mesh};

/// The tick every frame in this file is at.
const TICK: Tick = Tick(77_001);

/// The render session the consumer tests open.
const SESSION: RenderSession = RenderSession(31);

/// The livery session the fixture's paints are bound in.
const LIVERY_SESSION: LiverySession = LiverySession(9);

/// The instances the batched scene draws. One committed paint, so the rows share
/// a batch key and merge into one draw: one mesh, one material, and four placed
/// draws that *borrow* those two handles.
const FLEET: [ModelInstanceId; 4] = [
    ModelInstanceId(1),
    ModelInstanceId(2),
    ModelInstanceId(3),
    ModelInstanceId(4),
];

/// The one paint every aircraft carries, so the rows merge.
const RED: LiveryPaint = LiveryPaint::new([
    PaintColor::new(255, 0, 0),
    PaintColor::WHITE,
    PaintColor::WHITE,
]);

const REPEAT: TextureAddress = TextureAddress {
    u: AddressMode::Repeat,
    v: AddressMode::Repeat,
};

/// A released batch's mesh and material go back to their stores, once, however
/// often the frame path releases and respawns it — and a frame that reuses every
/// batch still adds nothing.
///
/// Against the old `release_entity` this fails on the first cycle: the mesh and
/// material counts are 2 after the first release and respawn instead of 1.
#[test]
fn accept_t512_a_released_batch_hands_its_mesh_and_material_back_to_the_stores() {
    let scene = fleet_scene();
    let mut world = open_world();
    let drawn = &scene.drawn_frame;
    let refused = &scene.refused_frame;

    // The scene really is one batch of four rows: one mesh, one material, four
    // placed draws. A test that released four separate batches would not show one
    // owner holding the handles that four live entities draw with.
    assert_eq!(drawn.batches().len(), 1, "four rows, one draw");
    assert_eq!(drawn.batches()[0].len(), 4, "one row per aircraft");
    assert_eq!(scene.planned, 4, "one placed draw per row");

    // The first drawn sync: one batch spawned, nothing released, and both stores
    // hold exactly the one entry the spawn added.
    let first = sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect("the drawn frame syncs");
    assert_eq!(first.spawned, 1);
    assert_eq!(first.reused, 0);
    assert_eq!(first.released, 0);
    assert_eq!(first.placed, 4);
    assert_eq!(
        first.reclaimed,
        ReclaimedAssets::default(),
        "nothing was released, so nothing was handed back"
    );
    assert_eq!(meshes(&world), 1);
    assert_eq!(standard_materials(&world), 1);
    assert_eq!(
        additive_materials(&world),
        0,
        "no additive class in this scene"
    );
    assert_eq!(placed_draws(&world), 4);
    every_handle_resolves(&world);

    // While the batch is live, its four placements draw with the owner's very
    // handles and the stores resolve them. This is the "a still-referenced handle
    // is not removed" half, in the form the module can actually produce: the
    // entry is only ever handed back after the entities naming it are gone.
    let (mesh, material) = live_batch_handles(&world);
    assert!(
        world.resource::<Assets<Mesh>>().get(&mesh).is_some(),
        "the mesh the live batch draws with is in its store"
    );
    assert!(
        world
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .is_some(),
        "and so is its material"
    );
    for placement in placements(&world, batch_entity(&world)) {
        assert_eq!(
            world.get::<Mesh3d>(placement).map(|held| held.0.clone()),
            Some(mesh.clone()),
            "every placement borrows the owner's mesh handle"
        );
        assert_eq!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(placement)
                .map(|held| held.0.clone()),
            Some(material.clone()),
            "and the owner's material handle"
        );
    }

    // A frame that reuses every batch adds nothing: the reuse guard and the
    // release rule are two halves of one promise, and neither may grow a store.
    for _ in 0..3 {
        let repeat = sync_frame(
            &mut world,
            &scene.drawn_submitted(),
            drawn,
            SESSION,
            &scene.runtime,
        )
        .expect("the drawn frame resyncs");
        assert_eq!(repeat.spawned, 0);
        assert_eq!(repeat.reused, 1);
        assert_eq!(repeat.released, 0);
        assert_eq!(repeat.placed, 4);
        assert_eq!(repeat.reclaimed, ReclaimedAssets::default());
    }
    assert_eq!(meshes(&world), 1, "a reused batch keeps its mesh");
    assert_eq!(
        standard_materials(&world),
        1,
        "a reused batch keeps its material"
    );

    // Three release/respawn cycles, the shape #503 puts on a gameplay path: a
    // frame that withholds every row releases the batch, and the frame after it
    // spawns it again.
    for cycle in 1..=3 {
        let released = sync_frame(
            &mut world,
            &scene.refused_submitted(),
            refused,
            SESSION,
            &scene.runtime,
        )
        .expect("the frame that draws nothing syncs");
        assert_eq!(released.spawned, 0, "cycle {cycle}: nothing to draw");
        assert_eq!(released.reused, 0);
        assert_eq!(released.released, 1, "cycle {cycle}: the batch is released");
        assert_eq!(released.placed, 0);
        assert_eq!(
            released.reclaimed,
            ReclaimedAssets {
                meshes: 1,
                materials: 1,
                additive_materials: 0,
                images: 0,
            },
            "cycle {cycle}: exactly the two entries the batch added, once each"
        );
        assert_eq!(meshes(&world), 0, "cycle {cycle}: its mesh went back");
        assert_eq!(
            standard_materials(&world),
            0,
            "cycle {cycle}: its material went back"
        );
        assert_eq!(
            placed_draws(&world),
            0,
            "cycle {cycle}: its four placements went with it"
        );
        every_handle_resolves(&world);

        let respawn = sync_frame(
            &mut world,
            &scene.drawn_submitted(),
            drawn,
            SESSION,
            &scene.runtime,
        )
        .expect("the drawn frame syncs again");
        assert_eq!(respawn.spawned, 1, "cycle {cycle}: the batch comes back");
        assert_eq!(respawn.released, 0);
        assert_eq!(respawn.placed, 4);
        assert_eq!(
            respawn.reclaimed,
            ReclaimedAssets::default(),
            "cycle {cycle}: a spawn hands nothing back"
        );
        assert_eq!(
            meshes(&world),
            1,
            "cycle {cycle}: one mesh, not one per cycle"
        );
        assert_eq!(
            standard_materials(&world),
            1,
            "cycle {cycle}: one material, not one per cycle"
        );
        assert_eq!(placed_draws(&world), 4);
        every_handle_resolves(&world);
    }

    // The session ends with its entries still in the stores, so the teardown
    // takes those too — and a second teardown is the no-op it always was, not a
    // second removal of the same entries.
    let ended = teardown(&mut world);
    assert_eq!(ended.entities, 1);
    assert!(
        ended.sessions,
        "a teardown of an open session drops its state"
    );
    assert_eq!(
        ended.reclaimed,
        ReclaimedAssets {
            meshes: 1,
            materials: 1,
            additive_materials: 0,
            images: 0,
        },
        "the teardown hands back what the last batch owned"
    );
    assert_eq!(meshes(&world), 0);
    assert_eq!(standard_materials(&world), 0);
    assert_eq!(placed_draws(&world), 0);
    every_handle_resolves(&world);

    let again = teardown(&mut world);
    assert_eq!(again.entities, 0);
    assert_eq!(
        again.reclaimed,
        ReclaimedAssets::default(),
        "an entry is handed back once, however often a teardown passes"
    );
    assert!(!again.sessions, "and the session state was already gone");
}

/// The additive class's material lives in a different store, so its release
/// branch has to find it there: the additive entry is handed back by the same
/// release, and the standard stores are untouched by it.
#[test]
fn accept_t512_the_additive_classes_material_is_handed_back_by_the_same_release() {
    let scene = additive_scene();
    let mut world = open_world();
    let drawn = &scene.drawn_frame;
    let refused = &scene.refused_frame;

    let first = sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect("the additive frame syncs");
    assert_eq!(first.spawned, 1);
    assert_eq!(first.placed, 1);
    assert_eq!(meshes(&world), 1);
    assert_eq!(
        additive_materials(&world),
        1,
        "the additive class's entry is in its own store"
    );
    assert_eq!(standard_materials(&world), 0, "and not in the standard one");
    every_handle_resolves(&world);

    for cycle in 1..=3 {
        let released = sync_frame(
            &mut world,
            &scene.refused_submitted(),
            refused,
            SESSION,
            &scene.runtime,
        )
        .expect("the frame that draws nothing syncs");
        assert_eq!(released.released, 1);
        assert_eq!(
            released.reclaimed,
            ReclaimedAssets {
                meshes: 1,
                materials: 0,
                additive_materials: 1,
                images: 0,
            },
            "cycle {cycle}: the additive entry came out of its own store"
        );
        assert_eq!(meshes(&world), 0);
        assert_eq!(additive_materials(&world), 0);
        assert_eq!(standard_materials(&world), 0);
        every_handle_resolves(&world);

        let respawn = sync_frame(
            &mut world,
            &scene.drawn_submitted(),
            drawn,
            SESSION,
            &scene.runtime,
        )
        .expect("the additive frame syncs again");
        assert_eq!(respawn.spawned, 1);
        assert_eq!(meshes(&world), 1);
        assert_eq!(additive_materials(&world), 1);
        every_handle_resolves(&world);
    }
}

/// The repair path is the same release path. A batch entity that lost the
/// components that make it this draw is released and respawned rather than
/// repaired in place, so `reuse_batch` is a second place a mesh and a material
/// could be orphaned; it goes through `release_entity` too, so a batch entity
/// damaged behind the sync path's back neither grows a store nor loses an entry a
/// live entity still draws with.
///
/// The damage is authored here — one component is removed from the batch entity —
/// because it is exactly what the repair path looks for and nothing else in the
/// workspace produces it.
#[test]
fn accept_t512_the_repair_of_a_damaged_batch_entity_hands_back_what_the_dead_one_owned() {
    let scene = fleet_scene();
    let mut world = open_world();
    let drawn = &scene.drawn_frame;

    sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect("the drawn frame syncs");
    assert_eq!(meshes(&world), 1);
    assert_eq!(standard_materials(&world), 1);
    let (first_mesh, first_material) = live_batch_handles(&world);

    for cycle in 1..=3 {
        // The entity under the batch key stops carrying the draw: the repair
        // path releases it and spawns a fresh one for the same batch.
        world.entity_mut(batch_entity(&world)).remove::<BatchDraw>();
        let report = sync_frame(
            &mut world,
            &scene.drawn_submitted(),
            drawn,
            SESSION,
            &scene.runtime,
        )
        .expect("the drawn frame syncs again");
        assert_eq!(report.spawned, 1, "cycle {cycle}: a fresh entity");
        assert_eq!(
            report.reused, 0,
            "cycle {cycle}: the damaged entity cannot serve this draw"
        );
        assert_eq!(report.released, 1, "cycle {cycle}: it was released");
        assert_eq!(report.placed, 4);
        assert_eq!(
            report.reclaimed,
            ReclaimedAssets {
                meshes: 1,
                materials: 1,
                additive_materials: 0,
                images: 0,
            },
            "cycle {cycle}: what the dead entity owned went back before the respawn"
        );
        assert_eq!(meshes(&world), 1, "cycle {cycle}: no mesh per repair");
        assert_eq!(
            standard_materials(&world),
            1,
            "cycle {cycle}: no material per repair"
        );
        assert_eq!(placed_draws(&world), 4);
        every_handle_resolves(&world);

        // The respawn really did add new entries rather than keeping the dead
        // entity's handles, which a repair that reused them would show here.
        let (mesh, material) = live_batch_handles(&world);
        assert_ne!(
            mesh.id(),
            first_mesh.id(),
            "cycle {cycle}: a new mesh entry, the old one is out of the store"
        );
        assert_ne!(
            material.id(),
            first_material.id(),
            "cycle {cycle}: a new material entry"
        );
    }
}

/// A batch that lost its **material** component — not its [`BatchDraw`] — is
/// not repaired in place either. The material is one of the entries this module
/// owns, so a reused entity that took a replacement would hold an entry no
/// record names (nothing to hand back) while the record it kept naming the entry
/// it no longer draws with. The repair path treats the entity as not usable, so
/// it goes through the same release-and-respawn as the lost-`BatchDraw` case and
/// neither store grows.
///
/// Against the `sync.rs` before this change the first cycle reuses the damaged
/// entity, adds one `StandardMaterial` that no owner record names, and reports
/// `reused: 1` / `released: 0`; the material count climbs by one per damaged
/// frame.
#[test]
fn accept_f17_c_reused_a_damaged_material_component_is_released_not_orphaned() {
    let scene = fleet_scene();
    let mut world = open_world();
    let drawn = &scene.drawn_frame;

    sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect("the drawn frame syncs");
    assert_eq!(meshes(&world), 1);
    assert_eq!(standard_materials(&world), 1);
    every_handle_resolves(&world);
    let (first_mesh, first_material) = live_batch_handles(&world);

    for cycle in 1..=3 {
        // The entity under the batch key loses the material component the sync
        // path itself owns, behind the sync path's back. Nothing else in the
        // workspace removes it; this is the one way in, exactly as the
        // lost-`BatchDraw` case is for the repair path.
        world
            .entity_mut(batch_entity(&world))
            .remove::<MeshMaterial3d<StandardMaterial>>();
        let report = sync_frame(
            &mut world,
            &scene.drawn_submitted(),
            drawn,
            SESSION,
            &scene.runtime,
        )
        .expect("the drawn frame syncs again");
        assert_eq!(report.spawned, 1, "cycle {cycle}: a fresh entity draws it");
        assert_eq!(
            report.reused, 0,
            "cycle {cycle}: an entity without its material is not this draw"
        );
        assert_eq!(
            report.released, 1,
            "cycle {cycle}: the damaged entity goes through the one release path"
        );
        assert_eq!(report.placed, 4);
        assert_eq!(
            report.reclaimed,
            ReclaimedAssets {
                meshes: 1,
                materials: 1,
                additive_materials: 0,
                images: 0,
            },
            "cycle {cycle}: exactly the entries the damaged entity owned went back"
        );
        assert_eq!(
            meshes(&world),
            1,
            "cycle {cycle}: no mesh per damaged frame"
        );
        assert_eq!(
            standard_materials(&world),
            1,
            "cycle {cycle}: no material per damaged frame"
        );
        assert_eq!(additive_materials(&world), 0);
        assert_eq!(placed_draws(&world), 4);
        every_handle_resolves(&world);

        // The replacement really is a fresh spawn: the live entity draws with
        // entries that were not the damaged entity's, and the record it carries
        // names them.
        let (mesh, material) = live_batch_handles(&world);
        assert_ne!(
            mesh.id(),
            first_mesh.id(),
            "cycle {cycle}: the replacement is a new mesh entry"
        );
        assert_ne!(
            material.id(),
            first_material.id(),
            "cycle {cycle}: and a new material entry"
        );
    }

    // The record names the entry the live entity draws with: the teardown hands
    // back exactly one of each and leaves the stores empty, so nothing the
    // replacements added was left behind.
    let ended = teardown(&mut world);
    assert_eq!(ended.entities, 1);
    assert_eq!(
        ended.reclaimed,
        ReclaimedAssets {
            meshes: 1,
            materials: 1,
            additive_materials: 0,
            images: 0,
        },
        "the last spawned batch owned exactly the entries it added"
    );
    assert_eq!(meshes(&world), 0);
    assert_eq!(standard_materials(&world), 0);
    assert_eq!(placed_draws(&world), 0);
    every_handle_resolves(&world);
}

/// The same rule for the additive class, whose material entry lives in its own
/// store: a live additive batch that loses its component is released and
/// respawned, and `Assets<AdditiveMaterial>` does not grow.
#[test]
fn accept_f17_c_reused_the_additive_classes_damaged_material_is_released_not_orphaned() {
    let scene = additive_scene();
    let mut world = open_world();
    let drawn = &scene.drawn_frame;

    let first = sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect("the additive frame syncs");
    assert_eq!(first.spawned, 1);
    assert_eq!(first.placed, 1);
    assert_eq!(meshes(&world), 1);
    assert_eq!(additive_materials(&world), 1);
    assert_eq!(standard_materials(&world), 0);
    every_handle_resolves(&world);
    let first_material = live_additive_material(&world);

    for cycle in 1..=3 {
        world
            .entity_mut(batch_entity(&world))
            .remove::<MeshMaterial3d<AdditiveMaterial>>();
        let report = sync_frame(
            &mut world,
            &scene.drawn_submitted(),
            drawn,
            SESSION,
            &scene.runtime,
        )
        .expect("the additive frame syncs again");
        assert_eq!(report.spawned, 1, "cycle {cycle}: a fresh entity draws it");
        assert_eq!(
            report.reused, 0,
            "cycle {cycle}: an entity without its additive material is not this draw"
        );
        assert_eq!(report.released, 1, "cycle {cycle}: released once");
        assert_eq!(report.placed, 1);
        assert_eq!(
            report.reclaimed,
            ReclaimedAssets {
                meshes: 1,
                materials: 0,
                additive_materials: 1,
                images: 0,
            },
            "cycle {cycle}: the additive entry went back to its own store"
        );
        assert_eq!(meshes(&world), 1);
        assert_eq!(
            additive_materials(&world),
            1,
            "cycle {cycle}: no additive entry per damaged frame"
        );
        assert_eq!(standard_materials(&world), 0);
        assert_eq!(placed_draws(&world), 1);
        every_handle_resolves(&world);
        assert_ne!(
            live_additive_material(&world).id(),
            first_material.id(),
            "cycle {cycle}: the replacement is a new additive entry"
        );
    }

    let ended = teardown(&mut world);
    assert_eq!(
        ended.reclaimed,
        ReclaimedAssets {
            meshes: 1,
            materials: 0,
            additive_materials: 1,
            images: 0,
        }
    );
    assert_eq!(meshes(&world), 0);
    assert_eq!(additive_materials(&world), 0);
    every_handle_resolves(&world);
}

/// A frame that is refused before anything is written leaves the stores exactly
/// as it found them: a refusal that handed an entry back would leave a live batch
/// drawing a removed asset.
#[test]
fn accept_t512_a_refused_frame_hands_back_nothing_and_leaves_the_stores_alone() {
    let scene = fleet_scene();
    let mut world = World::new();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<StandardMaterial>::default());
    world.insert_resource(Assets::<AdditiveMaterial>::default());
    let drawn = &scene.drawn_frame;

    // No session is open yet: the frame is refused before a single store is
    // touched.
    let refusal = sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect_err("no session is open");
    assert_eq!(refusal, SyncError::NoSession);
    assert_eq!(meshes(&world), 0);
    assert_eq!(standard_materials(&world), 0);

    world.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut world);
    sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect("the drawn frame syncs");
    assert_eq!(meshes(&world), 1);
    assert_eq!(standard_materials(&world), 1);

    // A frame built under a profile nobody applied is refused the same way, with
    // the live batch and both stores untouched.
    let enhanced = RenderProfile::faithful()
        .with(Enhancement::Antialiasing { samples: 4 })
        .expect("four samples is expressible");
    let other = scene.batched_under(&enhanced);
    let refusal = sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        &other,
        SESSION,
        &scene.runtime,
    )
    .expect_err("the enhanced frame is refused");
    assert_eq!(refusal.code(), "profile_mismatch");
    assert_eq!(meshes(&world), 1, "a refused frame hands nothing back");
    assert_eq!(standard_materials(&world), 1);
    assert_eq!(placed_draws(&world), 4);
    every_handle_resolves(&world);
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// One authored surface of a scene.
struct ItemSpec {
    key: &'static str,
    class: MaterialClass,
    coverage: Coverage,
    center_m: [f32; 3],
}

/// An opaque surface at its own view depth, named `hull_N`.
fn hull(index: usize) -> ItemSpec {
    const KEYS: [&str; 4] = ["hull_0", "hull_1", "hull_2", "hull_3"];
    ItemSpec {
        key: KEYS[index],
        class: MaterialClass::Opaque,
        coverage: Coverage::Opaque,
        center_m: [f32::from(index as u16) - 1.5, 0.0, -10.0 - index as f32],
    }
}

/// The scene under test, in both of the states the sync path sees it.
struct Scene {
    items: Vec<DrawItem>,
    plan: DrawPlan,
    visuals: InstanceVisuals,
    runtime: LiveryRuntime,
    /// The scene as the adapters uploaded it.
    drawn: Vec<SceneOutcome>,
    /// The same scene with every surface refused, so it draws nothing and a sync
    /// of it releases every batch the previous one spawned.
    refused: Vec<SceneOutcome>,
    drawn_frame: BatchedFrame,
    refused_frame: BatchedFrame,
    /// How many placed draws the drawn frame puts in the world.
    planned: usize,
}

impl Scene {
    /// The drawn scene in submission order, as the batcher and the consumer take
    /// it. Built per call because a `SubmittedDraw` borrows its outcome, and the
    /// two lists have to exist at the same time.
    fn drawn_submitted(&self) -> Vec<SubmittedDraw<'_>> {
        submitted(&self.items, &self.drawn)
    }

    /// The same scene against the refused outcomes.
    fn refused_submitted(&self) -> Vec<SubmittedDraw<'_>> {
        submitted(&self.items, &self.refused)
    }

    /// The drawn scene batched under another profile, so a frame nobody applied
    /// can be offered to a session that applied another one.
    fn batched_under(&self, profile: &RenderProfile) -> BatchedFrame {
        batch_frame(
            &self.drawn_submitted(),
            &self.plan,
            &self.visuals,
            profile,
            TICK,
        )
        .expect("the scene batches")
    }
}

/// The fixture scene, built through the production readers: the canonical mesh
/// through `cs_content` and the paint through the F09-C livery runtime.
fn scene(specs: &[ItemSpec]) -> Scene {
    let view = SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]).expect("a finite view");
    let items: Vec<DrawItem> = specs
        .iter()
        .map(|spec| {
            DrawItem::new(
                DrawItemKey::new(spec.key).expect("an authored key is valid"),
                classified(spec.class, spec.coverage),
                spec.center_m,
                None,
            )
            .expect("authored geometry is finite")
        })
        .collect();

    // Uploaded through the production adapter, once and owned: a `SubmittedDraw`
    // borrows its outcome and both lists are read repeatedly. No image, so the
    // scene is about the mesh and material stores alone.
    let drawn: Vec<SceneOutcome> = items
        .iter()
        .map(|item| {
            upload_surface(&SceneSurface {
                item,
                mesh: &quad_mesh(QuadShape::full(), 0),
                group: 0,
                image: None,
                unknowns: &[],
            })
        })
        .collect();
    let refused: Vec<SceneOutcome> = items
        .iter()
        .map(|item| {
            SceneOutcome::Refused(SurfaceRefusal::new(
                item.key().clone(),
                vec!["fixture_surface_refused"],
            ))
        })
        .collect();

    // One committed paint per aircraft. Rows with one paint, one geometry, one
    // state and no image are one batch key, so they merge into one draw — which
    // is what makes the four placed draws borrow *one* mesh and *one* material.
    let (runtime, visuals) = bound_paints(specs.len());
    let plan = DrawPlan::build(&items, &view);
    let profile = RenderProfile::faithful();
    let frame_of = |outcomes: &[SceneOutcome]| {
        batch_frame(
            &submitted(&items, outcomes),
            &plan,
            &visuals,
            &profile,
            TICK,
        )
        .expect("the scene batches")
    };
    let drawn_frame = frame_of(&drawn);
    let refused_frame = frame_of(&refused);
    assert!(
        drawn_frame.withheld().is_empty(),
        "the drawn frame withholds nothing"
    );
    assert!(
        refused_frame.batches().is_empty(),
        "the refused frame draws nothing"
    );
    let planned = drawn_frame.batches().iter().map(|batch| batch.len()).sum();

    Scene {
        items,
        plan,
        visuals,
        runtime,
        drawn,
        refused,
        drawn_frame,
        refused_frame,
        planned,
    }
}

/// Four opaque aircraft sharing one paint: one batch of four rows.
fn fleet_scene() -> Scene {
    let scene = scene(&[hull(0), hull(1), hull(2), hull(3)]);
    assert_eq!(
        scene.drawn_frame.batches().len(),
        1,
        "four rows with one paint are one draw"
    );
    assert_eq!(scene.planned, 4, "one placed draw per row");
    scene
}

/// One additive surface: its material entry lives in a different store.
fn additive_scene() -> Scene {
    scene(&[ItemSpec {
        key: "sprite",
        class: MaterialClass::Additive,
        coverage: Coverage::Opaque,
        center_m: [0.0, 0.0, -2.0],
    }])
}

/// The scene in submission order against one list of outcomes.
fn submitted<'a>(items: &'a [DrawItem], outcomes: &'a [SceneOutcome]) -> Vec<SubmittedDraw<'a>> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| SubmittedDraw {
            item,
            outcome: &outcomes[index],
            instance: ModelInstanceId(index as u64 + 1),
            part: PartRef::Unresolved("fixture_has_no_part_identity"),
        })
        .collect()
}

/// Commits one paint per aircraft and returns the producer and the per-instance
/// records the batcher reads.
fn bound_paints(instances: usize) -> (LiveryRuntime, InstanceVisuals) {
    let bytes = stored_bm();
    let mut context = ParseContext::with_defaults("synthetic/t512.bm");
    let file = read_bm(&mut context, &bytes).expect("the synthetic image parses");
    let mut runtime = LiveryRuntime::new(LIVERY_SESSION);
    let mut visuals = InstanceVisuals::new();
    for instance in FLEET.iter().take(instances) {
        runtime
            .bind(
                LIVERY_SESSION,
                *instance,
                &file,
                PaintChoice::faction(faction(), RED),
                &mut AllocationBudget::with_defaults("synthetic/t512.bm"),
            )
            .expect("the paint fits");
        visuals
            .bind(
                &runtime,
                LIVERY_SESSION,
                *instance,
                &AirframeDamageState::new(),
            )
            .expect("the livery session is this one");
    }
    (runtime, visuals)
}

/// A 2x2 BM with a full mask on every plane, so the paint colors change the
/// composed bytes. Authored here; nothing original.
fn stored_bm() -> Vec<u8> {
    let base = [[10u8, 20, 30], [40, 50, 60], [70, 80, 90], [100, 110, 120]];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2u16.to_le_bytes()); // height
    bytes.extend_from_slice(&2u16.to_le_bytes()); // width
    for texel in base {
        bytes.extend_from_slice(&texel);
    }
    for _ in 0..3 {
        bytes.extend_from_slice(&[255u8; 4]);
    }
    bytes.extend_from_slice(&[0u8; 16]); // transparent overlay
    bytes
}

fn faction() -> ContentId {
    ContentId::from_source(ContentKind::Faction, "kestrel").expect("a valid faction id")
}

/// One classified surface, authored here.
fn classified(class: MaterialClass, coverage: Coverage) -> ClassifiedMaterial {
    let facts = MaterialFacts {
        declared: Some(
            DeclaredClass::new(class, ClaimStatus::Designed).expect("Designed asserts a class"),
        ),
        coverage,
        alpha_test: if class == MaterialClass::Masked {
            AlphaTest::Threshold(0x80)
        } else {
            AlphaTest::Disabled
        },
        two_sided: Some(false),
        addressing: Some(REPEAT),
        vertex_colors: false,
        unknown_flag_bits: 0,
    };
    match classify(&facts) {
        cs_app::render::material::Classification::Classified(material) => material,
        cs_app::render::material::Classification::Unclassified { reasons } => {
            panic!("the authored {class} surface stopped classifying: {reasons:?}")
        }
    }
}

// ---------------------------------------------------------------------------
// Observations
// ---------------------------------------------------------------------------

/// A world with the four asset stores the consumer binds into, and a session
/// already open under the faithful profile.
fn open_world() -> World {
    let mut world = World::new();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<StandardMaterial>::default());
    world.insert_resource(Assets::<AdditiveMaterial>::default());
    world.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut world);
    world
}

fn meshes(world: &World) -> usize {
    world.resource::<Assets<Mesh>>().len()
}

fn standard_materials(world: &World) -> usize {
    world.resource::<Assets<StandardMaterial>>().len()
}

fn additive_materials(world: &World) -> usize {
    world.resource::<Assets<AdditiveMaterial>>().len()
}

/// How many placed draws the world holds in total.
fn placed_draws(world: &World) -> usize {
    world
        .iter_entities()
        .filter(|entity| entity.contains::<BatchInstancePlacement>())
        .count()
}

/// The per-instance entities placed under `batch`.
fn placements(world: &World, batch: Entity) -> Vec<Entity> {
    world
        .iter_entities()
        .filter(|entity| {
            entity.get::<BatchInstancePlacement>().is_some()
                && entity
                    .get::<ChildOf>()
                    .is_some_and(|parent| parent.parent() == batch)
        })
        .map(|entity| entity.id())
        .collect()
}

/// The one entity that draws a batch.
fn batch_entity(world: &World) -> Entity {
    let found: Vec<Entity> = world
        .iter_entities()
        .filter(|entity| entity.contains::<BatchDraw>())
        .map(|entity| entity.id())
        .collect();
    assert_eq!(
        found.len(),
        1,
        "the scene is one batch, so one entity draws it: {found:?}"
    );
    found[0]
}

/// The mesh and the material the one live batch draws with, with the four
/// placements under it asserted to share both.
fn live_batch_handles(world: &World) -> (Handle<Mesh>, Handle<StandardMaterial>) {
    let entity = batch_entity(world);
    let children = placements(world, entity);
    assert_eq!(children.len(), 4, "four rows, four placed draws");
    let mesh = world
        .get::<Mesh3d>(entity)
        .expect("the batch draws a mesh")
        .0
        .clone();
    let material = world
        .get::<MeshMaterial3d<StandardMaterial>>(entity)
        .expect("the batch draws a standard material")
        .0
        .clone();
    (mesh, material)
}

/// The additive material the one live batch draws with.
fn live_additive_material(world: &World) -> Handle<AdditiveMaterial> {
    world
        .get::<MeshMaterial3d<AdditiveMaterial>>(batch_entity(world))
        .expect("the batch draws an additive material")
        .0
        .clone()
}

/// Asserts that every asset handle any entity in `world` draws with still
/// resolves in its store.
///
/// This is where a release that handed an entry back *before* the entities naming
/// it were despawned would show up: a placement left holding a handle into an
/// empty store. It is checked after every step of every cycle, because the
/// invariant is about the world and not about one call's report.
fn every_handle_resolves(world: &World) {
    for entity in world.iter_entities() {
        let id = entity.id();
        if let Some(mesh) = entity.get::<Mesh3d>() {
            assert!(
                world.resource::<Assets<Mesh>>().get(&mesh.0).is_some(),
                "{id} draws a mesh that is not in the store"
            );
        }
        if let Some(material) = entity.get::<MeshMaterial3d<StandardMaterial>>() {
            assert!(
                world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&material.0)
                    .is_some(),
                "{id} draws a standard material that is not in the store"
            );
        }
        if let Some(material) = entity.get::<MeshMaterial3d<AdditiveMaterial>>() {
            assert!(
                world
                    .resource::<Assets<AdditiveMaterial>>()
                    .get(&material.0)
                    .is_some(),
                "{id} draws an additive material that is not in the store"
            );
        }
    }
}
