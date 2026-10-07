//! `accept_t514_` tests: a released batch returns the image it bound to
//! `Assets<Image>`.
//!
//! Rally #514, the follow-up #512's finding filed against itself
//! (`docs/findings/2026-10-02-t512-released-batch-store-assets.md`, "Not closed
//! here"): `release_entity` hands a released batch's mesh and material back and
//! leaves the `Assets<Image>` entry its spawn added behind. Two shapes of that
//! leak, both on the production `sync_frame` — a paint image is deduplicated
//! *within one call* only, so a batch released and respawned once per cycle adds
//! one image per cycle, and the canonical path deduplicates not at all.
//!
//! The rule under test, from `sync.rs`: the spawn that binds an image records it
//! with the batch that added it, and a release hands it back only when **no live
//! batch and no live material entry names it**. The two halves need two cases,
//! because an image is unlike a mesh or a material in both directions:
//!
//! * it is **shared** — every spawn of one paint fingerprint in a frame binds the
//!   one texture — so releasing one of two batches that bind it must leave it in
//!   the store ([`accept_t514_an_image_shared_with_another_live_batch_stays_in_the_store`]);
//! * it is referenced from *inside* the material entry (`base_color_texture`), so
//!   the batch's own material has to leave the store before the image is asked
//!   about. That reference is the reason the removal asks both questions; the
//!   case where the material store alone decides is checked in `sync.rs`,
//!   beside the counter it shares.
//!
//! Nothing here changes what is drawn or which rows are placed: every assertion
//! about the world beyond the stores is the placement and handle-resolution
//! invariant the `accept_t512_` selection already holds, plus the bound image.
//!
//! Every input is newly authored synthetic content decoded through the
//! production readers and driven through the production `sync_frame`. No
//! `CS_GAME_DIR` and no original-behavior claim: nothing here is
//! `verified_original`.

use bevy::asset::{Assets, Handle};
use bevy::ecs::entity::Entity;
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
    RenderPhase, TextureAddress, classify,
};
use cs_app::render::plan::{DrawItem, DrawItemKey, DrawPlan, SceneView};
use cs_app::render::profile::RenderProfile;
use cs_app::render::sync::{
    BatchDraw, ReclaimedAssets, RenderProfileRequest, RenderSession,
    process_render_profile_request, sync_frame, teardown,
};
use cs_app::scene::AirframeDamageState;
use cs_content::livery::LiveryPaint;
use cs_formats::bm::PaintColor;
use cs_formats::io::AllocationBudget;
use cs_formats::texture::{AlphaSource, AlphaTest};
use cs_formats::{ParseContext, read_bm};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimStatus;

use super::fixture::{ImageShape, QuadShape, decoded_image, quad_mesh};

/// The tick every frame in this file is at.
const TICK: Tick = Tick(77_014);

/// The render session the consumer tests open.
const SESSION: RenderSession = RenderSession(34);

/// The livery session the fixture's paints are bound in.
const LIVERY_SESSION: LiverySession = LiverySession(12);

/// The one paint every aircraft in this file carries, so rows merge and the two
/// parts of the shared scene bind one variant.
const RED: LiveryPaint = LiveryPaint::new([
    PaintColor::new(255, 0, 0),
    PaintColor::WHITE,
    PaintColor::WHITE,
]);

const REPEAT: TextureAddress = TextureAddress {
    u: AddressMode::Repeat,
    v: AddressMode::Repeat,
};

/// A painted batch released and respawned once per cycle hands its image back
/// every time: the `Assets<Image>` length does not grow per cycle, and a frame
/// that reuses every batch adds nothing.
///
/// Against `release_entity` before #514 this fails in the first cycle: the
/// texture is still in the store after the release (`images` is 1 where 0 is
/// required) and the second respawn leaves two, which is the monotone growth the
/// task measured.
#[test]
fn accept_t514_a_released_painted_batch_returns_its_image_to_the_store() {
    let scene = fleet_scene();
    let mut world = open_world();
    let drawn = &scene.drawn_frame;
    let withheld = &scene.withheld_frame;

    // The scene really is one painted batch: four rows, one texture, one
    // material, one mesh — so one owner holds what four placed draws borrow.
    assert_eq!(drawn.batches().len(), 1, "four rows, one draw");
    assert_eq!(drawn.batches()[0].len(), 4);
    assert!(
        withheld.batches().is_empty(),
        "the withheld frame draws nothing, so it releases the batch"
    );

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
    assert_eq!(
        first.painted, 1,
        "the batch binds its composed paint as its texture"
    );
    assert_eq!(first.placed, 4);
    assert_eq!(
        first.reclaimed,
        ReclaimedAssets::default(),
        "nothing was released, so nothing was handed back"
    );
    assert_eq!(images(&world), 1, "one painted batch binds one texture");
    assert_eq!(meshes(&world), 1);
    assert_eq!(standard_materials(&world), 1);
    every_handle_resolves(&world);

    // A frame that reuses every batch adds nothing — the image included: the
    // reuse path reads the handle the entity already has instead of adding one.
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
        assert_eq!(repeat.reclaimed, ReclaimedAssets::default());
    }
    assert_eq!(
        images(&world),
        1,
        "a frame that reuses every batch adds no image"
    );

    // Three release/respawn cycles, the shape #503 puts on a gameplay path: a
    // frame that withholds every row releases the batch, and the frame after it
    // spawns it again.
    for cycle in 1..=3 {
        let released = sync_frame(
            &mut world,
            &scene.withheld_submitted(),
            withheld,
            SESSION,
            &scene.runtime,
        )
        .expect("the frame that draws nothing syncs");
        assert_eq!(released.spawned, 0, "cycle {cycle}: nothing to draw");
        assert_eq!(released.reused, 0);
        assert_eq!(released.released, 1, "cycle {cycle}: the batch is released");
        assert_eq!(released.placed, 0);
        // The observable fact first: the store itself, not the report of it.
        assert_eq!(
            images(&world),
            0,
            "cycle {cycle}: its texture went back with it"
        );
        assert_eq!(meshes(&world), 0, "cycle {cycle}: its mesh went back");
        assert_eq!(
            standard_materials(&world),
            0,
            "cycle {cycle}: its material went back"
        );
        assert_eq!(
            released.reclaimed,
            ReclaimedAssets {
                meshes: 1,
                materials: 1,
                additive_materials: 0,
                images: 1,
            },
            "cycle {cycle}: exactly the three entries the spawn added, once each"
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
        assert_eq!(
            respawn.painted, 1,
            "cycle {cycle}: it binds its paint again"
        );
        assert_eq!(
            images(&world),
            1,
            "cycle {cycle}: one texture, not one per cycle"
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
        every_handle_resolves(&world);
    }

    // The session ends with its entries still in the stores, so the teardown
    // takes the texture too — and a second teardown hands nothing back.
    let ended = teardown(&mut world);
    assert_eq!(ended.entities, 1);
    assert!(
        ended.sessions,
        "a teardown of an open session drops its state"
    );
    // The observable fact first: the stores, then the report of what left them.
    assert_eq!(images(&world), 0, "the texture went back with the session");
    assert_eq!(meshes(&world), 0);
    assert_eq!(standard_materials(&world), 0);
    assert_eq!(
        ended.reclaimed,
        ReclaimedAssets {
            meshes: 1,
            materials: 1,
            additive_materials: 0,
            images: 1,
        },
        "the teardown hands back what the last batch owned, texture included"
    );

    let again = teardown(&mut world);
    assert_eq!(again.entities, 0);
    assert_eq!(
        again.reclaimed,
        ReclaimedAssets::default(),
        "an entry is handed back once, however often a teardown passes"
    );
}

/// One texture bound by two live batches is handed back by neither of them
/// alone: releasing one batch leaves the image in the store for the other, and
/// a release that ends the *last* of them hands it back.
///
/// This is the half of the rule a mesh and a material never exercise — a spawn
/// adds a fresh one of those per batch, while `sync_frame` deduplicates the
/// paint upload by fingerprint across the frame — so it needs its own case: the
/// shared texture must survive the first release, and must not survive the last.
#[test]
fn accept_t514_an_image_shared_with_another_live_batch_stays_in_the_store() {
    let scene = shared_scene();
    let mut world = open_world();
    let drawn = &scene.drawn_frame;
    let partial = &scene.partial_frame;
    let withheld = &scene.withheld_frame;

    // Two parts of one aircraft in two phases, one committed paint: two batches,
    // one texture — the second batch binds the entry the first one added.
    assert_eq!(drawn.batches().len(), 2, "two phases, two draws");
    assert_eq!(
        partial.batches().len(),
        1,
        "the partial frame withholds the wing's whole batch"
    );
    assert!(withheld.batches().is_empty());

    let first = sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect("the drawn frame syncs");
    assert_eq!(first.spawned, 2);
    assert_eq!(first.painted, 2, "both batches bind their composed paint");
    assert_eq!(first.placed, 2);
    assert_eq!(
        images(&world),
        1,
        "one paint fingerprint under one addressing is one texture"
    );
    let wing = batch_entity(&world, RenderPhase::Masked);
    let tail = batch_entity(&world, RenderPhase::Opaque);
    let shared = bound_image(&world, wing);
    assert_eq!(
        bound_image(&world, tail),
        shared,
        "both batches bind that very texture"
    );
    assert_eq!(
        standard_materials(&world),
        2,
        "a material per batch, each sampling the shared texture"
    );
    every_handle_resolves(&world);

    // Release one of the two: the other is still a live batch, its material is
    // still in the store, and both name the texture — so it stays.
    let released = sync_frame(
        &mut world,
        &scene.partial_submitted(),
        partial,
        SESSION,
        &scene.runtime,
    )
    .expect("the partial frame syncs");
    assert_eq!(released.released, 1, "the wing's batch is released");
    assert_eq!(released.reused, 1, "the tail's batch is untouched");
    // The observable fact first: the shared texture is still in its store, and
    // the report says why — it had a live owner left.
    assert_eq!(
        images(&world),
        1,
        "releasing one of two batches keeps the shared image in its store"
    );
    assert_eq!(
        released.reclaimed,
        ReclaimedAssets {
            meshes: 1,
            materials: 1,
            additive_materials: 0,
            images: 0,
        },
        "the shared texture has a live owner left, so it is not handed back"
    );
    assert_eq!(
        bound_image(&world, tail),
        shared,
        "the surviving batch still binds the same texture"
    );
    assert!(
        world.resource::<Assets<Image>>().get(&shared).is_some(),
        "and it resolves in Assets<Image>"
    );
    every_handle_resolves(&world);

    // The wing comes back. Its spawn binds the paint again, and the dedup is
    // per call, so the store holds the tail's original and the wing's own — two,
    // both owned, neither accumulating across cycles.
    let respawn = sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect("the drawn frame syncs again");
    assert_eq!(respawn.spawned, 1, "the wing's batch comes back");
    assert_eq!(respawn.reused, 1);
    assert_eq!(images(&world), 2, "each live batch binds a texture it owns");

    // And a frame that reuses every batch still adds nothing.
    let reused = images(&world);
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
        assert_eq!(repeat.reused, 2);
        assert_eq!(repeat.released, 0);
        assert_eq!(repeat.reclaimed, ReclaimedAssets::default());
    }
    assert_eq!(
        images(&world),
        reused,
        "a frame that reuses every batch adds no image"
    );

    // The wing is released again — and this time its texture is its own, since
    // the respawn added one the tail does not sample. The count comes back down
    // instead of climbing.
    let released = sync_frame(
        &mut world,
        &scene.partial_submitted(),
        partial,
        SESSION,
        &scene.runtime,
    )
    .expect("the partial frame syncs again");
    assert_eq!(released.released, 1);
    // The observable fact first: the store came back down instead of climbing.
    assert_eq!(
        images(&world),
        1,
        "the store is back to the tail's own texture — no growth per cycle"
    );
    assert_eq!(
        released.reclaimed,
        ReclaimedAssets {
            meshes: 1,
            materials: 1,
            additive_materials: 0,
            images: 1,
        },
        "this cycle's texture has no live batch and no live material left"
    );
    every_handle_resolves(&world);

    // The last live batch goes, and the last texture goes with it.
    let last = sync_frame(
        &mut world,
        &scene.withheld_submitted(),
        withheld,
        SESSION,
        &scene.runtime,
    )
    .expect("the frame that draws nothing syncs");
    assert_eq!(last.released, 1, "only the tail's batch was still live");
    assert_eq!(images(&world), 0, "and no material sampled it any more");
    assert_eq!(last.reclaimed.images, 1, "its texture had no owner left");
    assert_eq!(meshes(&world), 0);
    assert_eq!(standard_materials(&world), 0);
    every_handle_resolves(&world);

    // Both batches respawn together — one call, one fingerprint, one texture
    // again — and the teardown hands that one back exactly once.
    let back = sync_frame(
        &mut world,
        &scene.drawn_submitted(),
        drawn,
        SESSION,
        &scene.runtime,
    )
    .expect("the drawn frame syncs a third time");
    assert_eq!(back.spawned, 2);
    assert_eq!(images(&world), 1, "two batches, one shared texture again");
    let ended = teardown(&mut world);
    assert_eq!(ended.entities, 2);
    assert_eq!(
        images(&world),
        0,
        "the shared texture went back with the session"
    );
    assert_eq!(
        ended.reclaimed,
        ReclaimedAssets {
            meshes: 2,
            materials: 2,
            additive_materials: 0,
            images: 1,
        },
        "the shared texture is handed back once, by the last release to end"
    );
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// One authored surface of a scene, sampling the stored image.
struct Spec {
    key: &'static str,
    instance: ModelInstanceId,
    class: MaterialClass,
    coverage: Coverage,
    center_m: [f32; 3],
}

/// Four opaque aircraft, one item each: one batch of four rows once their
/// paints merge, in one phase.
fn fleet_specs() -> Vec<Spec> {
    (0..4)
        .map(|index| Spec {
            key: KEYS[index],
            instance: ModelInstanceId(index as u64 + 1),
            class: MaterialClass::Opaque,
            coverage: Coverage::Opaque,
            center_m: [f32::from(index as u16) - 1.5, 0.0, -10.0 - index as f32],
        })
        .collect()
}

/// The keys of the four fleet items, in submission order.
const KEYS: [&str; 4] = ["hull_0", "hull_1", "hull_2", "hull_3"];

/// One aircraft with a masked wing and an opaque tail: two phases, so two
/// batches — both sampling the same stored image under the same addressing and
/// carrying the same committed paint, so both bind the one texture.
fn shared_specs() -> Vec<Spec> {
    vec![
        Spec {
            key: "plane.wing",
            instance: ModelInstanceId(1),
            class: MaterialClass::Masked,
            coverage: Coverage::Texture(AlphaSource::Channel),
            center_m: [1.0, 0.0, -21.0],
        },
        Spec {
            key: "plane.tail",
            instance: ModelInstanceId(1),
            class: MaterialClass::Opaque,
            coverage: Coverage::Opaque,
            center_m: [1.0, -1.0, -24.0],
        },
    ]
}

/// The scene under test, in the three states the sync path sees it.
struct Scene {
    items: Vec<DrawItem>,
    /// The instance each item draws, parallel to [`Scene::items`].
    instances: Vec<ModelInstanceId>,
    runtime: LiveryRuntime,
    /// Every surface uploaded.
    drawn: Vec<SceneOutcome>,
    /// Every surface refused, so the frame names no batch at all.
    withheld: Vec<SceneOutcome>,
    /// The first surface refused and the rest uploaded, so the frame keeps the
    /// batches the first item is not in and releases the one it is.
    partial: Vec<SceneOutcome>,
    drawn_frame: BatchedFrame,
    withheld_frame: BatchedFrame,
    partial_frame: BatchedFrame,
}

impl Scene {
    /// The scene in submission order against one list of outcomes.
    fn submitted<'a>(&'a self, outcomes: &'a [SceneOutcome]) -> Vec<SubmittedDraw<'a>> {
        submitted(&self.items, &self.instances, outcomes)
    }

    /// The scene in submission order against the fully uploaded outcomes.
    fn drawn_submitted(&self) -> Vec<SubmittedDraw<'_>> {
        self.submitted(&self.drawn)
    }

    /// The same scene with every surface refused.
    fn withheld_submitted(&self) -> Vec<SubmittedDraw<'_>> {
        self.submitted(&self.withheld)
    }

    /// The same scene with only its first surface refused.
    fn partial_submitted(&self) -> Vec<SubmittedDraw<'_>> {
        self.submitted(&self.partial)
    }
}

/// The fixture scene, built through the production readers: the canonical mesh
/// through `cs_content`, the stored image through `cs_formats`, the paint
/// through the F09-C livery runtime, and every frame through the production
/// batcher.
///
/// Every surface samples the same stored image under the same addressing, and
/// every instance carries the same committed paint, so the *only* thing that
/// can put two textures in the store is two spawns in two different frames.
fn scene(specs: Vec<Spec>) -> Scene {
    let view = SceneView::new([0.0, 0.0, 4.0], [0.0, 0.0, -1.0]).expect("a finite view");
    // One row per spec, drawing the instance the spec authored for it — two
    // specs may well be two parts of one aircraft, so the list that reaches the
    // submissions repeats an instance and the list the livery session binds
    // does not.
    let instances: Vec<ModelInstanceId> = specs.iter().map(|spec| spec.instance).collect();
    let mut bound = Vec::new();
    for instance in &instances {
        if !bound.contains(instance) {
            bound.push(*instance);
        }
    }
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

    // One stored image for every surface, so a batch's canonical upload and its
    // paint are the only textures in play.
    let stored = decoded_image(ImageShape::rgba8_srgb());
    let upload = |item: &DrawItem| {
        upload_surface(&SceneSurface {
            item,
            mesh: &quad_mesh(QuadShape::full(), 0),
            group: 0,
            image: Some(&stored),
            unknowns: &[],
        })
    };
    let drawn: Vec<SceneOutcome> = items.iter().map(upload).collect();
    let refused = |item: &DrawItem| {
        SceneOutcome::Refused(SurfaceRefusal::new(
            item.key().clone(),
            vec!["fixture_surface_refused"],
        ))
    };
    let withheld: Vec<SceneOutcome> = items.iter().map(refused).collect();
    let partial: Vec<SceneOutcome> = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            if index == 0 {
                refused(item)
            } else {
                upload(item)
            }
        })
        .collect();

    let (runtime, visuals) = bound_paints(&bound);
    let plan = DrawPlan::build(&items, &view);
    let profile = RenderProfile::faithful();
    let frame_of = |outcomes: &[SceneOutcome]| {
        batch_frame(
            &submitted(&items, &instances, outcomes),
            &plan,
            &visuals,
            &profile,
            TICK,
        )
        .expect("the scene batches")
    };
    let drawn_frame = frame_of(&drawn);
    let withheld_frame = frame_of(&withheld);
    let partial_frame = frame_of(&partial);

    Scene {
        items,
        instances,
        runtime,
        drawn,
        withheld,
        partial,
        drawn_frame,
        withheld_frame,
        partial_frame,
    }
}

/// Four aircraft in one batch, each with a stored image and the red paint.
fn fleet_scene() -> Scene {
    let scene = scene(fleet_specs());
    assert_eq!(
        scene.drawn_frame.batches().len(),
        1,
        "four rows with one paint and one texture are one draw"
    );
    assert!(
        scene.withheld_frame.batches().is_empty(),
        "and refusing every surface draws nothing"
    );
    scene
}

/// One aircraft whose two parts share one texture across two batches.
fn shared_scene() -> Scene {
    scene(shared_specs())
}

/// The scene in submission order against one list of outcomes.
fn submitted<'a>(
    items: &'a [DrawItem],
    instances: &[ModelInstanceId],
    outcomes: &'a [SceneOutcome],
) -> Vec<SubmittedDraw<'a>> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| SubmittedDraw {
            item,
            outcome: &outcomes[index],
            instance: instances[index],
            part: PartRef::Unresolved("fixture_has_no_part_identity"),
        })
        .collect()
}

/// Commits one paint per instance and returns the producer and the per-instance
/// records the batcher reads.
fn bound_paints(instances: &[ModelInstanceId]) -> (LiveryRuntime, InstanceVisuals) {
    let bytes = stored_bm();
    let mut context = ParseContext::with_defaults("synthetic/t514.bm");
    let file = read_bm(&mut context, &bytes).expect("the synthetic image parses");
    let mut runtime = LiveryRuntime::new(LIVERY_SESSION);
    let mut visuals = InstanceVisuals::new();
    for instance in instances {
        runtime
            .bind(
                LIVERY_SESSION,
                *instance,
                &file,
                PaintChoice::faction(faction(), RED),
                &mut AllocationBudget::with_defaults("synthetic/t514.bm"),
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

fn images(world: &World) -> usize {
    world.resource::<Assets<Image>>().len()
}

fn meshes(world: &World) -> usize {
    world.resource::<Assets<Mesh>>().len()
}

fn standard_materials(world: &World) -> usize {
    world.resource::<Assets<StandardMaterial>>().len()
}

/// The one entity of `phase` that draws a batch.
fn batch_entity(world: &World, phase: RenderPhase) -> Entity {
    let found: Vec<Entity> = world
        .iter_entities()
        .filter(|entity| {
            entity
                .get::<BatchDraw>()
                .is_some_and(|draw| draw.phase() == phase)
        })
        .map(|entity| entity.id())
        .collect();
    assert_eq!(found.len(), 1, "one batch draws in {phase:?}: {found:?}");
    found[0]
}

/// The image the batch entity `entity` binds.
fn bound_image(world: &World, entity: Entity) -> Handle<Image> {
    world
        .get::<BatchDraw>(entity)
        .and_then(BatchDraw::image)
        .expect("the batch binds an image")
        .clone()
}

/// Asserts that every asset handle any entity in `world` draws with still
/// resolves in its store — and, for a material, that the texture it samples is
/// still there too.
///
/// The second half is this file's half: `Assets::remove` does not cascade, so
/// an image handed back while a material still samples it would be a live
/// entity drawing an asset that is gone. It is checked after every step of
/// every cycle, because the invariant is about the world and not about one
/// call's report.
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
            let stored = world
                .resource::<Assets<StandardMaterial>>()
                .get(&material.0)
                .unwrap_or_else(|| {
                    panic!("{id} draws a standard material that is not in the store")
                });
            if let Some(texture) = &stored.base_color_texture {
                assert!(
                    world.resource::<Assets<Image>>().get(texture).is_some(),
                    "{id}'s material samples a texture that is not in the store"
                );
            }
        }
        if let Some(material) = entity.get::<MeshMaterial3d<AdditiveMaterial>>() {
            let stored = world
                .resource::<Assets<AdditiveMaterial>>()
                .get(&material.0)
                .unwrap_or_else(|| {
                    panic!("{id} draws an additive material that is not in the store")
                });
            if let Some(texture) = &stored.base_color_texture {
                assert!(
                    world.resource::<Assets<Image>>().get(texture).is_some(),
                    "{id}'s material samples a texture that is not in the store"
                );
            }
        }
        if let Some(image) = entity.get::<BatchDraw>().and_then(BatchDraw::image) {
            assert!(
                world.resource::<Assets<Image>>().get(image).is_some(),
                "{id} binds a texture that is not in the store"
            );
        }
    }
}
