//! `accept_f17_c_paint_` tests for the F17-C follow-up: how a per-instance
//! paint reaches the GPU.
//!
//! The decision under test (see `crates/cs_app/src/render/paint.rs` and
//! `docs/findings/2026-09-30-f17-c-followup-per-instance-paint.md`): the
//! original stores a paint as mask planes, not as a baked texture, and the
//! observed composition produces a whole RGB8 image per variant — so the
//! paint reaches the GPU **in the texels** of a per-variant image, bound per
//! batch at sync time. These tests prove that path and fail when the
//! per-instance selection is removed: with the paint binding taken out, every
//! batch samples the one shared canonical image again and the texels of the
//! two paints compare equal.
//!
//! The synthetic fixture is authored here and proves production behavior
//! only; it does not certify original renderer behavior. The retail test
//! exercises the real library at `$CS_GAME_DIR` and is `#[ignore]`d for CI.

use bevy::asset::Assets;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::prelude::World;
use bevy::image::{Image, ImageAddressMode, ImageSampler};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::render::render_resource::TextureFormat;

use cs_app::livery::{LiveryRuntime, LiverySession, ModelInstanceId, PaintChoice};
use cs_app::render::additive::AdditiveMaterial;
use cs_app::render::batch::{
    BatchedFrame, BatchingLimitation, InstanceBatch, InstanceVisual, InstanceVisuals, PartRef,
    SubmittedDraw, batch_frame, limitation_codes,
};
use cs_app::render::bevy_image::upload_image;
use cs_app::render::capture::{SceneOutcome, SceneSurface, upload_surface};
use cs_app::render::material::{
    AddressMode, Coverage, DeclaredClass, MaterialClass, MaterialFacts, RenderPhase,
    TextureAddress, classify,
};
use cs_app::render::paint::upload_paint;
use cs_app::render::plan::{DrawItem, DrawItemKey, DrawPlan, SceneView};
use cs_app::render::profile::RenderProfile;
use cs_app::render::sync::{
    BatchDraw, BatchInstancePlacement, RenderProfileRequest, RenderSession, SyncError,
    process_render_profile_request, sync_frame,
};
use cs_app::scene::AirframeDamageState;
use cs_content::livery::{LiveryPaint, LiveryVariantStore};
use cs_content::scene::SceneNodeId;
use cs_formats::bm::PaintColor;
use cs_formats::io::AllocationBudget;
use cs_formats::texture::{AlphaSource, AlphaTest};
use cs_formats::{ParseContext, read_bm};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimStatus;

use super::fixture::{ImageShape, QuadShape, decoded_image, quad_mesh};

/// The tick every frame in this file is at.
const TICK: Tick = Tick(4_243);

/// The render session the consumer tests open.
const SESSION: RenderSession = RenderSession(11);

/// The three aircraft the fixture scene draws. `a` and `c` share one
/// committed paint, `b` has its own.
const PLANE_A: ModelInstanceId = ModelInstanceId(1);
const PLANE_B: ModelInstanceId = ModelInstanceId(2);
const PLANE_C: ModelInstanceId = ModelInstanceId(3);

/// The paint of `a` and `c`.
const RED: LiveryPaint = LiveryPaint::new([
    PaintColor::new(255, 0, 0),
    PaintColor::WHITE,
    PaintColor::WHITE,
]);
/// The paint of `b`.
const BLUE: LiveryPaint = LiveryPaint::new([
    PaintColor::new(0, 0, 255),
    PaintColor::WHITE,
    PaintColor::WHITE,
]);

const REPEAT: TextureAddress = TextureAddress {
    u: AddressMode::Repeat,
    v: AddressMode::Repeat,
};
const CLAMP: TextureAddress = TextureAddress {
    u: AddressMode::Clamp,
    v: AddressMode::Clamp,
};

fn budget() -> AllocationBudget {
    AllocationBudget::with_defaults("synthetic/f17-c-paint.bm")
}

/// A 2x2 BM with a full mask on every plane, so the paint colors change the
/// composed bytes. Authored here; nothing original.
fn stored_bm() -> Vec<u8> {
    let base = [[90u8, 20, 30], [40, 140, 60], [20, 60, 150], [200, 190, 80]];
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

fn faction(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Faction, key).expect("a valid faction id")
}

fn part(key: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, key).expect("a valid scene node id"),
    )
    .expect("a root-level node id")
}

/// Which part of its aircraft one fixture item draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PartChoice {
    Body,
    Wing,
    Tail,
}

/// One item of the fixture scene, authored.
struct Spec {
    key: &'static str,
    instance: ModelInstanceId,
    part: PartChoice,
    class: MaterialClass,
    coverage: Coverage,
    image: bool,
    address: TextureAddress,
    center_m: [f32; 3],
}

/// The fixture scene: three aircraft, one alpha-cut wing each, `c` an extra
/// opaque but textured tail, and `a` and `b` an opaque body that samples no
/// image at all.
///
/// Every item is submitted with the same geometry and — for the textured
/// items — the same stored image, so the *only* thing that can bind different
/// texels to two items is the per-instance paint this stage is about. The
/// tail gives the shared paint a second batch in another phase: two batches
/// carrying one variant must bind one image, not one each.
fn specs() -> [Spec; 6] {
    let wing = |key, instance, x, z| Spec {
        key,
        instance,
        part: PartChoice::Wing,
        class: MaterialClass::Masked,
        coverage: Coverage::Texture(AlphaSource::Channel),
        image: true,
        address: REPEAT,
        center_m: [x, 0.0, z],
    };
    let tail = |key, instance, x, z| Spec {
        key,
        instance,
        part: PartChoice::Tail,
        class: MaterialClass::Opaque,
        coverage: Coverage::Opaque,
        image: true,
        address: REPEAT,
        center_m: [x, -1.0, z],
    };
    let body = |key, instance, x, z| Spec {
        key,
        instance,
        part: PartChoice::Body,
        class: MaterialClass::Opaque,
        coverage: Coverage::Opaque,
        image: false,
        address: REPEAT,
        center_m: [x, -1.0, z],
    };
    [
        wing("a.wing", PLANE_A, 1.0, -21.0),
        wing("c.wing", PLANE_C, 5.0, -22.0),
        wing("b.wing", PLANE_B, 3.0, -23.0),
        tail("c.tail", PLANE_C, 5.0, -24.0),
        body("a.body", PLANE_A, 1.0, -25.0),
        body("b.body", PLANE_B, 3.0, -26.0),
    ]
}

/// The same scene with `c`'s tail sampling under a different addressing:
/// the same variant then fills two texture contracts.
fn specs_clamped_tail() -> [Spec; 6] {
    let mut specs = specs();
    specs[3].address = CLAMP;
    specs
}

/// The synthetic scene, its uploads, its plan and its per-instance records.
struct Fixture {
    items: Vec<DrawItem>,
    instances: Vec<ModelInstanceId>,
    choices: Vec<PartChoice>,
    outcomes: Vec<SceneOutcome>,
    plan: DrawPlan,
    visuals: InstanceVisuals,
    parts: Parts,
    runtime: LiveryRuntime,
}

/// The three part identities the scene draws.
struct Parts {
    body: SceneNodeId,
    wing: SceneNodeId,
    tail: SceneNodeId,
}

fn classify_item(spec: &Spec) -> cs_app::render::material::ClassifiedMaterial {
    let declared = DeclaredClass::new(spec.class, ClaimStatus::Designed).expect("Designed asserts");
    let facts = MaterialFacts {
        declared: Some(declared),
        coverage: spec.coverage,
        alpha_test: if spec.class == MaterialClass::Masked {
            AlphaTest::Threshold(0x80)
        } else {
            AlphaTest::Disabled
        },
        two_sided: Some(false),
        addressing: Some(spec.address),
        vertex_colors: false,
        unknown_flag_bits: 0,
    };
    match classify(&facts) {
        cs_app::render::material::Classification::Classified(material) => material,
        cs_app::render::material::Classification::Unclassified { reasons } => {
            panic!(
                "authored item {:?} stopped classifying: {reasons:?}",
                spec.key
            )
        }
    }
}

/// Builds the scene through the production readers: the canonical mesh
/// through `cs_content`, the stored image through `cs_formats`, the paint
/// through the F09-C livery runtime.
fn fixture() -> Fixture {
    fixture_of(&specs())
}

/// The fixture built from `specs` rather than the default scene.
fn fixture_of(specs: &[Spec]) -> Fixture {
    let view = SceneView::new([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]).expect("a finite view");
    let parts = Parts {
        body: part("planes.fuselage"),
        wing: part("planes.wing_l"),
        tail: part("planes.tail"),
    };
    let mut items = Vec::new();
    let mut instances = Vec::new();
    let mut choices = Vec::new();
    let mut outcomes = Vec::new();
    for spec in specs {
        let item = DrawItem::new(
            DrawItemKey::new(spec.key).expect("authored keys are valid"),
            classify_item(spec),
            spec.center_m,
            None,
        )
        .expect("authored geometry is finite");
        let mesh = quad_mesh(QuadShape::full(), 0);
        let image = spec.image.then(|| decoded_image(ImageShape::rgba8_srgb()));
        outcomes.push(upload_surface(&SceneSurface {
            item: &item,
            mesh: &mesh,
            group: 0,
            image: image.as_ref(),
            unknowns: &[],
        }));
        items.push(item);
        instances.push(spec.instance);
        choices.push(spec.part);
    }
    let plan = DrawPlan::build(&items, &view);

    // The producer for paint: the F09-C runtime, one instance bound per
    // paint. `a` and `c` share the red variant, `b` gets the blue one.
    let bytes = stored_bm();
    let mut context = ParseContext::with_defaults("synthetic/f17-c-paint.bm");
    let file = read_bm(&mut context, &bytes).expect("the synthetic image parses");
    let session = LiverySession(5);
    let mut runtime = LiveryRuntime::new(session);
    for (instance, paint, key) in [
        (PLANE_A, RED, "red"),
        (PLANE_B, BLUE, "blue"),
        (PLANE_C, RED, "red"),
    ] {
        runtime
            .bind(
                session,
                instance,
                &file,
                PaintChoice::faction(faction(key), paint),
                &mut budget(),
            )
            .expect("the paint fits");
    }

    let mut visuals = InstanceVisuals::new();
    for instance in [PLANE_A, PLANE_B, PLANE_C] {
        visuals
            .bind(&runtime, session, instance, &AirframeDamageState::new())
            .expect("the livery session is this one");
    }

    Fixture {
        items,
        instances,
        choices,
        outcomes,
        plan,
        visuals,
        parts,
        runtime,
    }
}

impl Fixture {
    /// The scene in submission order, as the batcher and the consumer take it.
    fn submitted(&self) -> Vec<SubmittedDraw<'_>> {
        self.items
            .iter()
            .enumerate()
            .map(|(index, item)| SubmittedDraw {
                item,
                outcome: &self.outcomes[index],
                instance: self.instances[index],
                part: PartRef::Known(match self.choices[index] {
                    PartChoice::Body => &self.parts.body,
                    PartChoice::Wing => &self.parts.wing,
                    PartChoice::Tail => &self.parts.tail,
                }),
            })
            .collect()
    }

    /// Batches the fixture scene under `profile`.
    fn batch(&self, profile: &RenderProfile) -> BatchedFrame {
        batch_frame(&self.submitted(), &self.plan, &self.visuals, profile, TICK)
            .expect("the fixture scene batches")
    }
}

/// The RGB8 bytes of `rgb` widened to opaque RGBA8: the byte shape a
/// [`PaintUpload`](cs_app::render::paint::PaintUpload) holds.
fn rgba(rgb: &[u8]) -> Vec<u8> {
    rgb.as_chunks::<3>()
        .0
        .iter()
        .flat_map(|texel| [texel[0], texel[1], texel[2], u8::MAX])
        .collect()
}

/// The batch `instance` appears in under `phase`.
fn batch_of(frame: &BatchedFrame, phase: RenderPhase, instance: ModelInstanceId) -> &InstanceBatch {
    batches_of(frame, phase)
        .into_iter()
        .find(|batch| batch.row(instance).is_some())
        .unwrap_or_else(|| panic!("{instance} is in no {phase} batch"))
}

fn batches_of(frame: &BatchedFrame, phase: RenderPhase) -> Vec<&InstanceBatch> {
    frame
        .batches()
        .iter()
        .filter(|batch| batch.phase() == phase)
        .collect()
}

/// A Bevy world with the asset stores the consumer binds into.
fn render_world() -> World {
    let mut world = World::new();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<bevy::mesh::Mesh>::default());
    world.insert_resource(Assets::<StandardMaterial>::default());
    world.insert_resource(Assets::<AdditiveMaterial>::default());
    world
}

/// A world with the fixture's profile applied, ready to sync.
fn session_world(fixture: &Fixture) -> World {
    let mut world = render_world();
    world.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut world);
    let submitted = fixture.submitted();
    let frame = fixture.batch(&RenderProfile::faithful());
    sync_frame(&mut world, &submitted, &frame, SESSION, &fixture.runtime).expect("the frame syncs");
    world
}

/// The entity that draws the batch `instance` appears in.
fn batch_entity(
    world: &World,
    frame: &BatchedFrame,
    phase: RenderPhase,
    instance: ModelInstanceId,
) -> Entity {
    let key = cs_app::render::sync::batch_key(batch_of(frame, phase, instance));
    world
        .iter_entities()
        .find(|entity| {
            entity
                .get::<BatchDraw>()
                .is_some_and(|draw| draw.key() == key)
        })
        .map_or_else(
            || panic!("no entity draws {instance} in {phase}"),
            |entity| entity.id(),
        )
}

/// The image handle one batch entity binds.
fn batch_image(world: &World, entity: Entity) -> bevy::asset::Handle<Image> {
    world
        .get::<BatchDraw>(entity)
        .and_then(BatchDraw::image)
        .expect("the batch binds an image")
        .clone()
}

/// The material a batch entity draws with, read out of the world's store.
fn stored_material(world: &World, entity: Entity) -> StandardMaterial {
    let handle = world
        .get::<MeshMaterial3d<StandardMaterial>>(entity)
        .expect("the batch draws a material")
        .0
        .clone();
    world
        .resource::<Assets<StandardMaterial>>()
        .get(&handle)
        .expect("the handle resolves to a stored material")
        .clone()
}

/// The per-instance entities placed under a batch entity.
fn placements(world: &World, batch: Entity) -> Vec<Entity> {
    world
        .iter_entities()
        .filter(|entity| {
            entity.contains::<BatchInstancePlacement>()
                && entity
                    .get::<ChildOf>()
                    .is_some_and(|parent| parent.parent() == batch)
        })
        .map(|entity| entity.id())
        .collect()
}

fn batch_entities(world: &World) -> usize {
    world
        .iter_entities()
        .filter(|entity| entity.contains::<BatchDraw>())
        .count()
}

/// AC03's texel half: three aircraft on one geometry and one canonical image,
/// two paints. The batcher keeps the paints apart, and the consumer binds
/// each batch's composed variant as its texture — so the two paints reach the
/// GPU as two different images, and every placed row of a batch samples the
/// paint its aircraft committed to.
#[test]
fn accept_f17_c_paint_two_paints_batched_sample_different_texels() {
    let fixture = fixture();
    let mut world = render_world();
    world.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut world);

    let submitted = fixture.submitted();
    let frame = fixture.batch(&RenderProfile::faithful());
    let report = sync_frame(&mut world, &submitted, &frame, SESSION, &fixture.runtime)
        .expect("the frame syncs");

    // Five batches: the two wing paints, `c`'s painted tail and the two
    // untextured bodies. The three textured batches bind a paint.
    assert_eq!(frame.batch_count(), 5);
    assert_eq!(report.spawned, 5);
    assert_eq!(report.placed, 6, "one placed draw per row");
    assert_eq!(
        report.painted, 3,
        "the textured painted batches bound a paint"
    );
    assert_eq!(batch_entities(&world), 5);

    let shared = batch_of(&frame, RenderPhase::Masked, PLANE_A);
    assert_eq!(shared.len(), 2, "a and c share the red wing draw");
    let own = batch_of(&frame, RenderPhase::Masked, PLANE_B);
    assert_eq!(own.len(), 1, "b's blue wing is a draw of its own");
    assert_ne!(
        shared.key().paint(),
        own.key().paint(),
        "the batch keys carry the variants"
    );

    // The two batches of `c`'s red variant — the shared wing and its own
    // tail — bind *the same* paint image: one variant, one texture.
    let shared_e = batch_entity(&world, &frame, RenderPhase::Masked, PLANE_A);
    let own_e = batch_entity(&world, &frame, RenderPhase::Masked, PLANE_B);
    let tail_e = batch_entity(&world, &frame, RenderPhase::Opaque, PLANE_C);
    let shared_h = batch_image(&world, shared_e);
    let own_h = batch_image(&world, own_e);
    let tail_h = batch_image(&world, tail_e);
    assert_eq!(
        shared_h, tail_h,
        "two batches of one variant bind one paint image"
    );
    assert_ne!(shared_h, own_h, "a different paint is a different image");
    assert_eq!(
        world.resource::<Assets<Image>>().len(),
        2,
        "one texture per variant in the store"
    );

    // The bound texels are the composed variants exactly — not the shared
    // canonical image every textured item uploaded.
    let red = world
        .resource::<Assets<Image>>()
        .get(&shared_h)
        .expect("the red paint is stored");
    let blue = world
        .resource::<Assets<Image>>()
        .get(&own_h)
        .expect("the blue paint is stored");
    assert_eq!(
        red.data.as_deref(),
        Some(rgba(fixture.runtime.image(PLANE_A).expect("a paints").rgb()).as_slice()),
        "the shared batch samples the red variant's composed texels"
    );
    assert_eq!(
        blue.data.as_deref(),
        Some(rgba(fixture.runtime.image(PLANE_B).expect("b paints").rgb()).as_slice()),
        "b's batch samples the blue variant's composed texels"
    );
    let canonical = upload_image(&decoded_image(ImageShape::rgba8_srgb()), Some(REPEAT))
        .expect("the canonical upload");
    assert_ne!(
        red.data,
        canonical.image().data,
        "the bound texels are the paint, not the canonical image"
    );
    assert_ne!(red.data, blue.data, "two paints sample different texels");

    // The bound image is what the material samples, and every placed row of a
    // batch shares that material — the paint reaches each instance's draw.
    for entity in [shared_e, own_e, tail_e] {
        let material = stored_material(&world, entity);
        assert_eq!(
            material.base_color_texture.as_ref(),
            world.get::<BatchDraw>(entity).and_then(BatchDraw::image),
            "the material samples the batch's paint image"
        );
        assert!(!placements(&world, entity).is_empty());
        for child in placements(&world, entity) {
            assert_eq!(
                world.get::<MeshMaterial3d<StandardMaterial>>(child),
                world.get::<MeshMaterial3d<StandardMaterial>>(entity),
                "every placed row samples the batch's paint"
            );
        }
    }

    // A painted aircraft's *untextured* part binds nothing: the paint is a
    // texture and the material has no texture slot for it.
    for instance in [PLANE_A, PLANE_B] {
        let body_e = batch_entity(&world, &frame, RenderPhase::Opaque, instance);
        assert!(world.get::<BatchDraw>(body_e).unwrap().image().is_none());
        assert!(
            stored_material(&world, body_e).base_color_texture.is_none(),
            "an untextured surface has no texels to paint"
        );
    }
}

/// A re-synced frame keeps its paint bindings: the store grows once per
/// variant, not once per frame.
#[test]
fn accept_f17_c_paint_a_resynced_frame_binds_no_new_textures() {
    let fixture = fixture();
    let mut world = session_world(&fixture);
    assert_eq!(world.resource::<Assets<Image>>().len(), 2);

    let submitted = fixture.submitted();
    let frame = fixture.batch(&RenderProfile::faithful());
    for _ in 0..3 {
        let report = sync_frame(&mut world, &submitted, &frame, SESSION, &fixture.runtime)
            .expect("the frame resyncs");
        assert_eq!(report.reused, 5);
        assert_eq!(report.spawned, 0);
        assert_eq!(report.painted, 3);
    }
    assert_eq!(
        world.resource::<Assets<Image>>().len(),
        2,
        "the same two variant textures, still"
    );
    assert_eq!(
        world.resource::<Assets<StandardMaterial>>().len(),
        5,
        "one material per batch, kept"
    );
}

/// A paint sampled under two different addressings is two textures: the
/// variant fills each batch's sampling contract, so deduplicating on the
/// variant alone would hand one batch's sampler to the other.
#[test]
fn accept_f17_c_paint_one_variant_under_two_addressings_is_two_textures() {
    let fixture = fixture_of(&specs_clamped_tail());
    let mut world = render_world();
    world.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut world);
    let submitted = fixture.submitted();
    let frame = fixture.batch(&RenderProfile::faithful());
    let report = sync_frame(&mut world, &submitted, &frame, SESSION, &fixture.runtime)
        .expect("the frame syncs");
    assert_eq!(report.painted, 3);

    // `c`'s red paint fills two batches: the shared masked wing (repeat)
    // and its own opaque tail (clamp). The variant is the same, the
    // sampling contract is not — the tail binds its own texture, carrying
    // the tail's addressing and the same composed texels.
    let shared_e = batch_entity(&world, &frame, RenderPhase::Masked, PLANE_A);
    let tail_e = batch_entity(&world, &frame, RenderPhase::Opaque, PLANE_C);
    let shared_h = batch_image(&world, shared_e);
    let tail_h = batch_image(&world, tail_e);
    assert_ne!(
        shared_h, tail_h,
        "two samplings of one variant are two textures"
    );
    let assets = world.resource::<Assets<Image>>();
    let shared = assets.get(&shared_h).expect("the repeat upload is stored");
    let tail = assets.get(&tail_h).expect("the clamp upload is stored");
    let ImageSampler::Descriptor(descriptor) = &tail.sampler else {
        panic!("the paint upload declares its sampler");
    };
    assert_eq!(descriptor.address_mode_u, ImageAddressMode::ClampToEdge);
    assert_eq!(descriptor.address_mode_v, ImageAddressMode::ClampToEdge);
    assert_eq!(
        tail.data, shared.data,
        "the texels are the same composed variant either way"
    );
    assert_eq!(
        assets.len(),
        3,
        "red twice under two addressings, plus blue"
    );
}

/// An instance with no established paint samples the canonical image: the
/// batcher reports the gap, and the consumer binds the surface's own texture
/// — there is no paint variant to put in its place, and inventing one would
/// paint an aircraft a paint nobody chose.
#[test]
fn accept_f17_c_paint_an_unbound_instance_samples_the_canonical_image() {
    let mut fixture = fixture();
    fixture.visuals.insert(InstanceVisual::unbound(PLANE_B));

    let mut world = render_world();
    world.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut world);
    let submitted = fixture.submitted();
    let frame = fixture.batch(&RenderProfile::faithful());
    let report = sync_frame(&mut world, &submitted, &frame, SESSION, &fixture.runtime)
        .expect("the frame syncs");

    // `b`'s gap is reported for both its parts and its draws never merge.
    assert_eq!(
        frame
            .limitations()
            .iter()
            .filter(|limitation| {
                matches!(
                    limitation,
                    BatchingLimitation::UnboundInstance { instance, .. } if *instance == PLANE_B
                )
            })
            .count(),
        2
    );
    assert!(
        frame
            .limitations()
            .iter()
            .all(|limitation| limitation.code() == limitation_codes::UNBOUND_INSTANCE)
    );
    let wing = batch_of(&frame, RenderPhase::Masked, PLANE_B);
    assert_eq!(wing.len(), 1);
    assert!(!wing.mergeable());
    assert_eq!(wing.key().paint(), None);
    assert_eq!(report.painted, 2, "only the two red batches bind a paint");

    // What it binds is the canonical image the surface uploaded, byte for
    // byte — not a paint.
    let wing_e = batch_entity(&world, &frame, RenderPhase::Masked, PLANE_B);
    let bound = world
        .resource::<Assets<Image>>()
        .get(&batch_image(&world, wing_e))
        .expect("the image is stored");
    let canonical = upload_image(&decoded_image(ImageShape::rgba8_srgb()), Some(REPEAT))
        .expect("the canonical upload");
    assert_eq!(bound.data, canonical.image().data);
    assert_eq!(
        world.resource::<Assets<Image>>().len(),
        2,
        "the canonical image and the red variant: `b`'s paint never existed"
    );
}

/// A committed paint nobody composed is a refusal, not a fallback: the frame
/// writes nothing, and the caller can compose and retry.
#[test]
fn accept_f17_c_paint_an_uncomposed_variant_is_refused_and_the_retry_binds_it() {
    let fixture = fixture();
    let mut world = render_world();
    world.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut world);
    let submitted = fixture.submitted();
    let frame = fixture.batch(&RenderProfile::faithful());

    // A runtime of the right shape that never composed a variant.
    let empty = LiveryRuntime::new(LiverySession(9));
    let error = sync_frame(&mut world, &submitted, &frame, SESSION, &empty)
        .expect_err("a paint with no composed bytes cannot be bound");
    assert_eq!(error.code(), "paint_not_composed");
    let SyncError::PaintNotComposed { batch, variant } = &error else {
        panic!("the refusal names the batch and the missing variant: {error}");
    };
    assert_eq!(
        *batch,
        cs_app::render::sync::batch_key(
            frame
                .batches()
                .iter()
                .find(|batch| batch.key().paint().is_some() && batch.key().image().is_some())
                .expect("a textured painted batch exists")
        ),
        "the refusal names the batch whose paint is missing"
    );
    let digests: Vec<_> = [PLANE_A, PLANE_B]
        .iter()
        .map(|instance| {
            fixture
                .runtime
                .livery(*instance)
                .expect("a paint is committed")
                .key()
                .digest()
        })
        .collect();
    assert!(
        digests.contains(variant),
        "the refusal names a committed variant"
    );
    assert_eq!(batch_entities(&world), 0, "the refusal spawned nothing");
    assert!(
        world.resource::<Assets<Image>>().is_empty(),
        "the refusal stored no texture"
    );

    // Composing the variant and retrying binds it.
    let report = sync_frame(&mut world, &submitted, &frame, SESSION, &fixture.runtime)
        .expect("the retry syncs");
    assert_eq!(report.spawned, 5);
    assert_eq!(report.painted, 3);
}

/// The upload itself: the composed RGB8 as opaque RGBA8, the surface's
/// declared addressing, and a fingerprint that follows the variant.
#[test]
fn accept_f17_c_paint_upload_is_the_composed_rgb_widened_to_opaque_rgba() {
    let bytes = stored_bm();
    let mut context = ParseContext::with_defaults("synthetic/f17-c-paint.bm");
    let file = read_bm(&mut context, &bytes).expect("the synthetic image parses");
    let mut store = LiveryVariantStore::new();
    let red_key = *store
        .compose(&file, &RED, &mut budget())
        .expect("the paint fits")
        .key();
    let blue_key = *store
        .compose(&file, &BLUE, &mut budget())
        .expect("the paint fits")
        .key();
    let red = store.get(&red_key).expect("the red variant stays");
    let blue = store.get(&blue_key).expect("the blue variant stays");

    let upload = upload_paint(red, REPEAT);
    assert_eq!(upload.variant(), &red_key, "the upload is for this variant");
    let image = upload.image();
    assert_eq!((image.width(), image.height()), (2, 2));
    assert_eq!(
        image.texture_descriptor.format,
        TextureFormat::Rgba8UnormSrgb,
        "the authored bytes read as sRGB — one correction, the inferred class"
    );
    assert_eq!(
        image.data.as_deref().map(<[u8]>::len),
        Some(red.rgb().len() / 3 * 4)
    );
    assert_eq!(image.data.as_deref(), Some(rgba(red.rgb()).as_slice()));
    assert!(
        image
            .data
            .as_deref()
            .expect("texels")
            .as_chunks::<4>()
            .0
            .iter()
            .all(|texel| texel[3] == u8::MAX),
        "the composition is opaque by construction"
    );
    let ImageSampler::Descriptor(descriptor) = &image.sampler else {
        panic!("the upload declares its sampler");
    };
    assert_eq!(descriptor.address_mode_u, ImageAddressMode::Repeat);
    assert_eq!(descriptor.address_mode_v, ImageAddressMode::Repeat);

    // The fingerprint follows the variant's identity: another paint is
    // another upload, and so is another sampling contract.
    assert_ne!(
        upload_paint(blue, REPEAT).fingerprint(),
        upload.fingerprint()
    );
    assert_ne!(upload_paint(red, CLAMP).fingerprint(), upload.fingerprint());
    assert_eq!(
        upload_paint(red, REPEAT).fingerprint(),
        upload.fingerprint()
    );
}

// ------------------------------------------------------ original library ---

/// The two paints the retail measurement applies to every member: different
/// on the first and third mask planes, the same on the second, so an
/// unchanged composition can only mean the masks themselves allow no
/// difference.
const PAINT_ONE: LiveryPaint = LiveryPaint::new([
    PaintColor::new(255, 40, 30),
    PaintColor::new(200, 100, 50),
    PaintColor::WHITE,
]);
const PAINT_TWO: LiveryPaint = LiveryPaint::new([
    PaintColor::WHITE,
    PaintColor::new(200, 100, 50),
    PaintColor::new(30, 60, 200),
]);

/// The data side of the decision, on the real library: every `.bm` member of
/// the airframe library parses through the production reader, composes under
/// both paints through the production store, and the two paints upload as
/// two different images exactly where the masks allow them to differ — which
/// is what "the paint is stored in the masks, never as a baked texture"
/// means on the original bytes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f17_c_paint_retail_library_composes_paint_dependent_texels() {
    let root = retail_source();
    let catalog = cs_content::livery::StockLiveryCatalog::discover(&root);
    assert!(catalog.findings().is_empty(), "the library is covered");
    assert_eq!(catalog.assets().len(), 184, "the whole member set");

    let mut store = LiveryVariantStore::new();
    let mut differing = 0usize;
    let mut identical = Vec::new();
    for asset in catalog.assets() {
        let key = cs_types::asset_id::AssetKey::from_spelling(
            root.namespace().as_str(),
            asset.spelling(),
            "default",
        )
        .expect("a valid member key");
        let read = root
            .read(&key)
            .unwrap_or_else(|error| panic!("{}: {error}", asset.spelling()));
        let mut context = ParseContext::with_defaults(asset.spelling());
        let file = read_bm(&mut context, &read.data)
            .unwrap_or_else(|error| panic!("{}: {error}", asset.spelling()));

        let one_key = *store
            .compose(&file, &PAINT_ONE, &mut budget())
            .expect("the paint fits")
            .key();
        let two_key = *store
            .compose(&file, &PAINT_TWO, &mut budget())
            .expect("the paint fits")
            .key();
        let (one, two) = (
            store.get(&one_key).expect("the first variant stays"),
            store.get(&two_key).expect("the second variant stays"),
        );
        assert_ne!(
            one.key(),
            two.key(),
            "{}: the paint is always part of the variant identity",
            asset.spelling()
        );
        if one.rgb() == two.rgb() {
            // The two paints differ only on mask planes 1 and 3, so an
            // unchanged composition is only explained by those planes
            // carrying nothing — measured, not assumed.
            for y in 0..file.height() {
                for x in 0..file.width() {
                    assert_eq!(
                        file.mask(cs_formats::bm::BmPlane::Mask1, x, y),
                        Some(0),
                        "{} ({x},{y}): an unchanged composition with a nonzero first mask",
                        asset.spelling()
                    );
                    assert_eq!(
                        file.mask(cs_formats::bm::BmPlane::Mask3, x, y),
                        Some(0),
                        "{} ({x},{y}): an unchanged composition with a nonzero third mask",
                        asset.spelling()
                    );
                }
            }
            identical.push(asset.spelling().to_owned());
        } else {
            differing += 1;
        }

        // Each composed variant is an image a batch could bind, and the two
        // uploads differ wherever the texels do.
        let one_upload = upload_paint(one, REPEAT);
        let two_upload = upload_paint(two, REPEAT);
        assert_eq!(
            (one_upload.image().width(), one_upload.image().height()),
            (file.width(), file.height())
        );
        assert_eq!(
            one_upload.image().data.as_deref().map(<[u8]>::len),
            Some(file.width() as usize * file.height() as usize * 4)
        );
        assert_ne!(
            one_upload.fingerprint(),
            two_upload.fingerprint(),
            "{}: the upload's identity is the variant, always paint-keyed",
            asset.spelling()
        );
        assert_eq!(
            one_upload.image().data == two_upload.image().data,
            one.rgb() == two.rgb(),
            "{}: identical uploads only when the texels themselves are",
            asset.spelling()
        );
    }

    // Measured on the original library: 159 of 184 members' texels depend
    // on the paint; the 25 unchanged ones are members whose first and third
    // masks are all-zero (asserted above), so only the shared second mask
    // contributes. The pin is in
    // `docs/findings/2026-09-30-f17-c-followup-per-instance-paint.md`.
    assert_eq!(differing, 159, "paint-dependent members of the library");
    println!(
        "paint-dependent members: {differing} of {}; unchanged: {identical:?}",
        catalog.assets().len()
    );
}

/// The shared airframe library mounted read-only through the production
/// discovery and ROF mount.
fn retail_source() -> cs_assets::rof::RofSource {
    use cs_assets::install;
    use cs_assets::rof::mount_rof_into;
    use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
    use cs_types::asset_id::{MountId, MountNamespace, PrecedenceClass, ResolveContext};

    const CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";
    let game_dir = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR is not set: this test measures the original installation"),
    );
    let found = install::discover(&game_dir)
        .expect("production discovery must read the original installation");
    let context = ResolveContext::new(install::fingerprint(&found.manifest));
    let mut builder = SessionBuilder::new(context);
    let mount = MountBuilder::new(
        MountId::new("rof-gosdata-assets-crimson-rof").expect("a valid mount id"),
        MountNamespace::new(INSTALL_NAMESPACE).expect("a valid namespace"),
        PrecedenceClass::Shared,
        CONTAINER,
    )
    .retail();
    mount_rof_into(&mut builder, mount, &game_dir.join(CONTAINER))
        .expect("the airframe library mounts")
}

// ---------------------------------------------------- evidence harness ---

/// Evidence-report harness for this task (`docs/contracts/CLI-EVIDENCE.md`,
/// schema `schemas/evidence.schema.json`).
///
/// This test is deliberately **not** named `accept_f17_c_paint_*`: it is not
/// part of the acceptance suite, it fails loudly when its inputs are missing
/// instead of passing vacuously, and the task selection must never pick it
/// up.
///
/// Run from the workspace root, after the acceptance suite, exactly as:
///
/// 1. ```sh
///    cargo test --workspace --locked -- accept_f17_c_paint_ --include-ignored \
///      > private/evidence/F17-C-PAINT/cargo-test.log 2>&1
///    ```
///    (record that command's exit status — it is passed to this harness as
///    `CS_EVIDENCE_EXIT_CODE`.)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/F17-C-PAINT \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f17_c_paint_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status from step 1> \
///      cargo test --locked -p cs_app --test render -- evidence_report_f17_c_paint -- --ignored
///    ```
/// 3. ```sh
///    python3 tools/validate_evidence.py \
///      private/evidence/F17-C-PAINT/acceptance.json \
///      --artifact-root private/evidence/F17-C-PAINT --require-pass
///    ```
/// 4. Commit a copy of `acceptance.json` as
///    `docs/findings/evidence/F17-C-PAINT.json`.
#[cfg(test)]
mod evidence {
    use std::fs;
    use std::path::PathBuf;

    use cs_assets::install;
    use cs_assets::rof::RofSource;
    use cs_content::livery::StockLiveryCatalog;
    use cs_formats::{ParseContext, read_bm};
    use cs_types::asset_id::AssetKey;

    use super::evidence_helpers::*;

    use super::{PAINT_ONE, PAINT_TWO, budget};

    /// The measured corpus the report asserts against the production catalog,
    /// so a catalog change that discovers fewer members cannot silently pass.
    const EXPECTED_ASSETS: usize = 184;

    /// The retail acceptance test this task's `retail` capability rests on.
    /// It must be in the recorded log, and it must have passed.
    const RETAIL_TESTS: [&str; 1] =
        ["accept_f17_c_paint_retail_library_composes_paint_dependent_texels"];

    /// The scope boundaries this stage does not resolve, each naming the
    /// affected content and the resolving task, recorded in `review.method`
    /// and in the committed finding rather than in `unknowns`: they are
    /// follow-up limitations, not unresolved issues with this report's
    /// `implemented` claim.
    const DEFERRED_BOUNDARIES: [&str; 4] = [
        "original_binding_mechanism: what the original renderer did with the \
         composed image — bind a texture per variant, select an atlas region, \
         select a texture-array layer — is not measured anywhere in the \
         observed data; the per-variant binding this stage ships is Designed. \
         Affected content: every painted aircraft. Resolving task: F17-D (the \
         GPU consumer plus an owner-run capture). Gates: any verified_original \
         or release claim about painted-aircraft appearance.",
        "painted_surface_coverage: the composed variant is RGB8 with no \
         coverage plane, so a painted surface that declares texture coverage \
         samples an opaque alpha; where coverage for a painted surface comes \
         from is unknown. Affected content: any painted masked surface. \
         Resolving task: F17-D.",
        "livery_surface_association: the .bm member names are per airframe \
         part (BRI_WING, DEV_ENGINE, ...), but which scene node a part names \
         is not established, so the consumer binds the committed variant on \
         every sampled surface of the instance. Affected content: every \
         painted multi-part aircraft. Resolving task: F09-PREFIX (Rally #386) \
         and the later surface wiring it enables.",
        "paint_color_space: the upload reads the authored 8-bit composed bytes \
         as sRGB, the same single-correction rule as the canonical adapter; \
         the original renderer's color handling is unmeasured. Affected \
         content: every painted surface. Resolving task: F17-D.",
    ];

    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_GAME_DIR"]
    fn evidence_report_f17_c_paint_writes_the_acceptance_report() {
        let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
        let candidate_tree = env_var("CS_CANDIDATE_TREE");
        let argv: Vec<String> = env_var("CS_EVIDENCE_ARGV")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        assert!(
            !argv.is_empty(),
            "CS_EVIDENCE_ARGV must hold the acceptance command (space-separated)"
        );
        let exit_code: i32 = env_var("CS_EVIDENCE_EXIT_CODE")
            .parse()
            .expect("CS_EVIDENCE_EXIT_CODE must be the exit status of the acceptance run");
        let game_dir = PathBuf::from(env_var("CS_GAME_DIR"));

        let head_tree = git(&["rev-parse", "HEAD^{tree}"]);
        assert_eq!(
            candidate_tree, head_tree,
            "CS_CANDIDATE_TREE must be `git rev-parse 'HEAD^{{tree}}'` of the tested commit; \
             old reports cannot be reused for new code"
        );

        // The acceptance suite is the evidence: parse its recorded output.
        let log_path = evidence_dir.join("cargo-test.log");
        let log = fs::read_to_string(&log_path).unwrap_or_else(|error| {
            panic!(
                "cannot read the acceptance log {}: {error} (step 1 must write its output there)",
                log_path.display()
            )
        });
        let suite = parse_suite(&log);
        assert!(
            suite.passed > 0 && !suite.assertions.is_empty(),
            "no `accept_f17_c_paint_` tests were recorded in {}",
            log_path.display()
        );
        for retail_test in RETAIL_TESTS {
            let status = suite
                .assertions
                .iter()
                .find(|(name, _)| name == retail_test)
                .map(|(_, status)| *status)
                .unwrap_or_else(|| {
                    panic!(
                        "{retail_test} did not run: this task requires capability `retail`, run \
                         step 1 with `--include-ignored` and CS_GAME_DIR set"
                    )
                });
            assert_eq!(status, "pass", "{retail_test} must pass; got {status}");
        }
        assert!(
            suite
                .assertions
                .iter()
                .any(|(name, _)| name.starts_with("accept_f17_c_paint_")
                    && !name.contains("_retail_")),
            "synthetic task tests must be present alongside the retail one"
        );

        // The installation and content hashes come from the **production**
        // discovery and fingerprint code, not from a hash this harness
        // computes.
        let found = install::discover(&game_dir).expect(
            "production discovery must read the original installation for the evidence record",
        );
        let install_sha256 = install::fingerprint(&found.manifest).to_hex();
        let content_sha256 = install::content_fingerprint(&found.manifest).to_hex();

        // The substantive measurement: the production catalog and composition
        // over the whole airframe library, asserted and written beside the
        // report.
        let source = super::retail_source();
        let catalog = StockLiveryCatalog::discover(&source);
        assert_eq!(
            catalog.assets().len(),
            EXPECTED_ASSETS,
            "the original airframe library holds {EXPECTED_ASSETS} stock livery members"
        );
        assert!(catalog.findings().is_empty());
        let measurement = measure_members(&source, &catalog);
        let measurement_path = evidence_dir.join("paint-members.json");
        fs::write(
            &measurement_path,
            measurement_json(&candidate_tree, &source, &catalog, &measurement),
        )
        .unwrap_or_else(|error| panic!("write {}: {error}", measurement_path.display()));
        let mut artifacts = vec![artifact(&log_path, "log", &evidence_dir)];
        artifacts.push(artifact(&measurement_path, "json", &evidence_dir));

        let engine = format!(
            "{{\"rust\": {}, \"bevy\": {}, \"avian\": {}}}",
            jstr(&rustc_version()),
            jstr(&locked_version("bevy")),
            jstr(&locked_version("avian3d")),
        );

        let report = format!(
            "{{\n\
             \x20\"schema_version\": 1,\n\
             \x20\"task_id\": \"F17-C-PAINT\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"engine\": {},\n\
             \x20\"created_at\": {},\n\
             \x20\"command\": {{\"argv\": {}, \"cwd\": {}, \"exit_code\": {}}},\n\
             \x20\"source\": {{\"install_sha256\": {}, \"content_sha256\": {}}},\n\
             \x20\"seed\": 0,\n\
             \x20\"ticks\": {{\"start\": 0, \"end\": 0}},\n\
             \x20\"overrides\": [],\n\
             \x20\"capabilities\": [\"retail\", \"synthetic\"],\n\
             \x20\"tests\": {{\"discovered\": {}, \"executed\": {}, \"passed\": {}, \"failed\": {}, \"ignored\": {}}},\n\
             \x20\"assertions\": [{}],\n\
             \x20\"artifacts\": [{}],\n\
             \x20\"unknowns\": [],\n\
             \x20\"review\": {{\"identity\": {}, \"method\": {}}},\n\
             \x20\"claim\": \"implemented\"\n\
             }}\n",
            jstr(&candidate_tree),
            engine,
            jstr(&iso_utc_now()),
            str_array(&argv),
            jstr(&git(&["rev-parse", "--show-toplevel"])),
            exit_code,
            jstr(&install_sha256),
            jstr(&content_sha256),
            suite.discovered,
            suite.executed,
            suite.passed,
            suite.failed,
            suite.ignored,
            assertion_array(&suite.assertions),
            artifact_array(&artifacts),
            jstr(
                "implemented by devin-1 (Rally #410); reviewed in the same session by the same \
                 agent identity, so this is not independent evidence in the owner directive's \
                 sense, and no agent review replaces the owner's human approval."
            ),
            jstr(&format!(
                "acceptance suite run locally with the retail capability; this harness derives \
                 every field from the recorded log, production discovery and fingerprint of \
                 $CS_GAME_DIR, the production StockLiveryCatalog::discover plus a per-member \
                 two-paint composition over GOSDATA/ASSETS/crimson.rof ({} members, {} \
                 paint-dependent, {} unchanged under PAINT_ONE vs PAINT_TWO; per-member detail \
                 in paint-members.json), rustc and Cargo.lock. The measurement proves how the \
                 paint is *stored* and that the composed texels depend on it; how the original \
                 *renderer* bound a composed texture is unmeasured, so the per-variant binding \
                 is Designed and the claim is `implemented` only. Deferred follow-up boundaries \
                 (tracked by their own Rally tasks so they survive this task; also recorded in \
                 docs/findings/2026-09-30-f17-c-followup-per-instance-paint.md): {}. Validated \
                 with tools/validate_evidence.py --require-pass.",
                measurement.total,
                measurement.differing,
                measurement.identical.len(),
                DEFERRED_BOUNDARIES.join(" | "),
            )),
        );

        let out = evidence_dir.join("acceptance.json");
        fs::write(&out, &report).unwrap_or_else(|error| panic!("write {}: {error}", out.display()));
        let written = fs::read_to_string(&out).expect("the report reads back");
        for needle in [
            "\"schema_version\": 1",
            "\"task_id\": \"F17-C-PAINT\"",
            "\"claim\": \"implemented\"",
            "\"install_sha256\"",
            "\"content_sha256\"",
            "\"assertions\": [",
            "\"artifacts\": [",
            "\"unknowns\": []",
            "original_binding_mechanism",
            "painted_surface_coverage",
            "livery_surface_association",
            "paint_color_space",
        ] {
            assert!(
                written.contains(needle),
                "the written report is missing {needle:?}:\n{written}"
            );
        }
        assert!(
            suite.failed == 0 && exit_code == 0,
            "the acceptance run failed (exit {exit_code}, {} failed): the report was written \
             honestly and must NOT validate; fix the tests first",
            suite.failed
        );
        println!("wrote {}", out.display());
    }

    // ------------------------------------------------------- measurement ---

    /// What the per-member two-paint composition measured.
    struct Measurement {
        total: usize,
        differing: usize,
        identical: Vec<String>,
        /// `(spelling, variant digest of PAINT_ONE, differs)`.
        members: Vec<(String, String, bool)>,
    }

    /// Composes every library member under both paints through the production
    /// store and counts where the composed texels depend on the paint.
    fn measure_members(source: &RofSource, catalog: &StockLiveryCatalog) -> Measurement {
        let mut store = cs_content::livery::LiveryVariantStore::new();
        let mut differing = 0usize;
        let mut identical = Vec::new();
        let mut members = Vec::new();
        for asset in catalog.assets() {
            let key =
                AssetKey::from_spelling(source.namespace().as_str(), asset.spelling(), "default")
                    .expect("a valid member key");
            let read = source
                .read(&key)
                .unwrap_or_else(|error| panic!("{}: {error}", asset.spelling()));
            let mut context = ParseContext::with_defaults(asset.spelling());
            let file = read_bm(&mut context, &read.data)
                .unwrap_or_else(|error| panic!("{}: {error}", asset.spelling()));
            let one_key = *store
                .compose(&file, &PAINT_ONE, &mut budget())
                .expect("the paint fits")
                .key();
            let two_key = *store
                .compose(&file, &PAINT_TWO, &mut budget())
                .expect("the paint fits")
                .key();
            assert_ne!(one_key, two_key);
            let (one, two) = (
                store.get(&one_key).expect("the first variant stays"),
                store.get(&two_key).expect("the second variant stays"),
            );
            let differs = one.rgb() != two.rgb();
            if differs {
                differing += 1;
            } else {
                identical.push(asset.spelling().to_owned());
            }
            members.push((
                asset.spelling().to_owned(),
                one.key().digest().to_hex(),
                differs,
            ));
        }
        Measurement {
            total: catalog.assets().len(),
            differing,
            identical,
            members,
        }
    }

    /// The measured membership as a JSON artifact: per-member spelling,
    /// variant digest and whether the two paints produced different texels.
    /// No pixels, no file bytes; the artifact stays in `private/`.
    fn measurement_json(
        candidate_tree: &str,
        source: &RofSource,
        catalog: &StockLiveryCatalog,
        measurement: &Measurement,
    ) -> String {
        let members: Vec<String> = measurement
            .members
            .iter()
            .map(|(spelling, digest, differs)| {
                format!(
                    "{{\"spelling\": {}, \"variant\": {}, \"paint_dependent\": {differs}}}",
                    jstr(spelling),
                    jstr(digest),
                )
            })
            .collect();
        let identical: Vec<String> = measurement.identical.iter().map(|s| jstr(s)).collect();
        format!(
            "{{\n\
             \x20\"task_id\": \"F17-C-PAINT\",\n\
             \x20\"candidate_tree\": {},\n\
             \x20\"created_at\": {},\n\
             \x20\"reader\": \"cs_content::livery::StockLiveryCatalog::discover + \
             LiveryVariantStore::compose + cs_app::render::paint::upload_paint\",\n\
             \x20\"claim\": \"implemented\",\n\
             \x20\"evidence_class\": \"documented for storage, observed_tool for composition, \
             designed for binding\",\n\
             \x20\"note\": \"per-member spelling, PAINT_ONE variant digest and paint dependence; \
             no pixels, no file bytes\",\n\
             \x20\"container\": \"GOSDATA/ASSETS/crimson.rof\",\n\
             \x20\"namespace\": {},\n\
             \x20\"totals\": {{\"assets\": {}, \"findings\": {}, \"paint_dependent\": {}, \
             \"unchanged\": {}}},\n\
             \x20\"unchanged\": [{}],\n\
             \x20\"members\": [\n  {}\n ]\n\
             }}\n",
            jstr(candidate_tree),
            jstr(&iso_utc_now()),
            jstr(source.namespace().as_str()),
            catalog.assets().len(),
            catalog.findings().len(),
            measurement.differing,
            measurement.identical.len(),
            identical.join(", "),
            members.join(",\n  "),
        )
    }
}

/// Helpers shared by the evidence harness. Kept at the file root so the
/// harness module stays focused on what it measures.
mod evidence_helpers {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use cs_assets::install;

    pub fn env_var(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| {
            panic!(
                "{name} is not set: this harness only runs through the sequence in its module doc \
                 (crates/cs_app/tests/render/paint.rs)"
            )
        })
    }

    /// Cargo runs a test binary with its working directory set to the
    /// *package* root, so a path written relative to the workspace root must
    /// be re-anchored.
    pub fn workspace_path(as_described: &str) -> PathBuf {
        let path = PathBuf::from(as_described);
        if path.is_absolute() {
            return path;
        }
        Path::new(&git(&["rev-parse", "--show-toplevel"])).join(path)
    }

    pub fn git(args: &[&str]) -> String {
        let output = Command::new("git").args(args).output().expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    pub fn rustc_version() -> String {
        let output = Command::new("rustc")
            .arg("--version")
            .output()
            .expect("rustc runs");
        assert!(output.status.success(), "rustc --version failed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// The locked version of one `Cargo.lock` package: read, never asserted
    /// from memory.
    pub fn locked_version(package: &str) -> String {
        let lock_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/")
            .parent()
            .expect("workspace root")
            .join("Cargo.lock");
        let lock = fs::read_to_string(&lock_path)
            .unwrap_or_else(|error| panic!("read {}: {error}", lock_path.display()));
        let mut wanted = false;
        for line in lock.lines() {
            let line = line.trim();
            if line == "[[package]]" {
                wanted = false;
            } else if let Some(name) = line.strip_prefix("name = \"") {
                wanted = name.trim_end_matches('"') == package;
            } else if let Some(version) = line.strip_prefix("version = \"")
                && wanted
            {
                return version.trim_end_matches('"').to_owned();
            }
        }
        panic!("package {package:?} is not in {}", lock_path.display());
    }

    /// What the recorded `cargo test` output says actually happened.
    #[derive(Debug, Default)]
    pub struct Suite {
        pub discovered: u64,
        pub executed: u64,
        pub passed: u64,
        pub failed: u64,
        pub ignored: u64,
        /// `(test name, "pass" | "fail")`, in log order, deduplicated.
        pub assertions: Vec<(String, &'static str)>,
    }

    /// Extracts the libtest summaries and the per-test results of the
    /// `accept_f17_c_paint_` tests from a recorded `cargo test` output.
    pub fn parse_suite(log: &str) -> Suite {
        let mut suite = Suite::default();
        let mut pending: Vec<String> = Vec::new();
        for line in log.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("test result:") {
                for (count, kind) in summary_fields(trimmed) {
                    match kind {
                        "passed" => suite.passed += count,
                        "failed" => suite.failed += count,
                        "ignored" => suite.ignored += count,
                        _ => {}
                    }
                }
                continue;
            }
            if !pending.is_empty() {
                if trimmed == "ok" {
                    let name = pending.remove(0);
                    record(&mut suite, name, "pass");
                    continue;
                }
                if trimmed == "FAILED" {
                    let name = pending.remove(0);
                    record(&mut suite, name, "fail");
                    continue;
                }
            }
            let mut cursor = trimmed;
            while let Some(position) = cursor.find("test ") {
                let after = &cursor[position + 5..];
                let Some(separator) = after.find(" ... ") else {
                    break;
                };
                let full = &after[..separator];
                if !full.contains("accept_f17_c_paint_") {
                    cursor = &after[separator + 5..];
                    continue;
                }
                let name = full.rsplit("::").next().expect("a name").to_owned();
                let tail = &after[separator + 5..];
                cursor = tail;
                match tail.split_whitespace().next() {
                    Some("ok") => record(&mut suite, name, "pass"),
                    Some("FAILED") => record(&mut suite, name, "fail"),
                    _ => pending.push(name),
                }
            }
        }
        suite.assertions.dedup_by(|left, right| left.0 == right.0);
        suite.executed = suite.passed + suite.failed;
        suite.discovered = suite.passed + suite.failed + suite.ignored;
        suite
    }

    /// `(count, kind)` pairs of one `test result:` summary line.
    fn summary_fields(line: &str) -> Vec<(u64, &str)> {
        let mut fields = Vec::new();
        for segment in line["test result:".len()..].split(';') {
            let words: Vec<&str> = segment.split_whitespace().collect();
            for pair in words.windows(2) {
                if let Ok(count) = pair[0].parse::<u64>()
                    && matches!(pair[1], "passed" | "failed" | "ignored")
                {
                    fields.push((count, pair[1]));
                    break;
                }
            }
        }
        fields
    }

    fn record(suite: &mut Suite, name: String, status: &'static str) {
        if suite.assertions.iter().any(|(seen, _)| *seen == name) {
            return;
        }
        suite.assertions.push((name, status));
    }

    /// One referenced artifact, hashed with the **production** SHA-256. The
    /// validator re-hashes it with `hashlib` independently, so a wrong digest
    /// here fails validation rather than passing quietly.
    pub fn artifact(source: &Path, kind: &str, evidence_dir: &Path) -> (String, String, String) {
        let name = source
            .file_name()
            .expect("artifact has a file name")
            .to_string_lossy()
            .into_owned();
        let target = evidence_dir.join(&name);
        if source != target {
            fs::copy(source, &target).unwrap_or_else(|error| {
                panic!("copy {} -> {}: {error}", source.display(), target.display())
            });
        }
        let bytes =
            fs::read(&target).unwrap_or_else(|error| panic!("read {}: {error}", target.display()));
        (name, install::sha256(&bytes).to_hex(), kind.to_owned())
    }

    pub fn assertion_array(assertions: &[(String, &'static str)]) -> String {
        let items: Vec<String> = assertions
            .iter()
            .map(|(name, status)| {
                format!(
                    "{{\"id\": {}, \"status\": {status:?}, \"evidence\": [\"cargo-test.log\"]}}",
                    jstr(name)
                )
            })
            .collect();
        items.join(", ")
    }

    pub fn artifact_array(artifacts: &[(String, String, String)]) -> String {
        let items: Vec<String> = artifacts
            .iter()
            .map(|(name, digest, kind)| {
                format!(
                    "{{\"path\": {}, \"sha256\": {digest:?}, \"kind\": {kind:?}}}",
                    jstr(name)
                )
            })
            .collect();
        items.join(", ")
    }

    pub fn str_array(items: &[String]) -> String {
        let quoted: Vec<String> = items.iter().map(|item| jstr(item)).collect();
        format!("[{}]", quoted.join(", "))
    }

    /// A JSON string literal: quoted and escaped, so no report field can
    /// break out of its string.
    pub fn jstr(value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        for character in value.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                control if (control as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", control as u32));
                }
                other => out.push(other),
            }
        }
        out.push('"');
        out
    }

    /// RFC 3339 with whole seconds and `Z`, which `datetime.fromisoformat`
    /// accepts after the validator's `Z` -> `+00:00` replacement.
    pub fn iso_utc_now() -> String {
        let epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the system clock is after 1970")
            .as_secs() as i64;
        let (year, month, day, hour, minute, second) = civil_from_unix(epoch);
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    }

    /// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to a UTC
    /// calendar date, because `std` has no date formatting.
    fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
        let days = seconds.div_euclid(86_400);
        let rest = seconds.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year_of_day = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_prime = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
        let month = (if month_prime < 10 {
            month_prime + 3
        } else {
            month_prime - 9
        }) as u32;
        let year = if month <= 2 {
            year_of_day + 1
        } else {
            year_of_day
        };
        (
            year,
            month,
            day,
            (rest / 3_600) as u32,
            ((rest % 3_600) / 60) as u32,
            (rest % 60) as u32,
        )
    }
}
