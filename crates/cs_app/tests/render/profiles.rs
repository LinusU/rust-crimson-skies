//! `accept_f17_c_` tests for the rendering profiles, instance batching and the
//! ECS consumer.
//!
//! The stage's minimum scenario is AC03 — "Two instances with different
//! paint/damage remain visually independent after batching" — so the first
//! test here draws three aircraft that share one mesh and one image, gives two
//! of them the same committed paint and one a different one, and destroys a
//! part of one aircraft. It then requires that
//!
//! * the two aircraft with the same paint *are* batched together, because a
//!   test that passes by never batching anything would prove nothing;
//! * the third aircraft is in a batch of its own, because its paint is part of
//!   the batch key and a batcher that ignored it would draw two aircraft with
//!   one paint;
//! * the destroyed part is withheld with its reason, while the identical part
//!   of the two intact aircraft is drawn — and repairing or re-damaging it
//!   moves only that aircraft's row.
//!
//! The rest cover the profile rules and the consumer: every enhancement
//! independently switchable, the two options the sheet forbids refused by the
//! type, an enhanced profile refused as comparison evidence all the way down
//! the chain into F17-B's capture, no enhancement changing a draw decision,
//! the batching refusals, and the consumer's teardown, refusal and retry.

use bevy::asset::Assets;
use bevy::core_pipeline::tonemapping::Tonemapping as BevyTonemapping;
use bevy::ecs::entity::Entity;
use bevy::ecs::prelude::World;
use bevy::image::Image;
use bevy::light::DirectionalLight;
use bevy::mesh::Mesh;
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::render::view::Msaa;
use bevy::window::{Window, WindowResolution};

use cs_app::livery::{LiveryRuntime, LiverySession, ModelInstanceId, PaintChoice};
use cs_app::render::batch::{
    BatchedFrame, BatchingLimitation, InstanceVisual, InstanceVisuals, PartRef, SubmittedDraw,
    batch_frame, limitation_codes, withheld_codes,
};
use cs_app::render::capture::{CaptureError, Projection, SceneSurface, capture, upload_surface};
use cs_app::render::material::{
    AddressMode, Coverage, DeclaredClass, MaterialClass, MaterialFacts, RenderPhase,
    TextureAddress, classify,
};
use cs_app::render::plan::{DrawItem, DrawItemKey, DrawPlan, SceneView};
use cs_app::render::profile::{
    Enhancement, EnhancementKind, ProfileError, ProfileParity, RenderProfile, Resolution,
    bevy_tonemapping, msaa_for,
};
use cs_app::render::sync::{
    BatchDraw, ProfileEvent, RenderProfileLog, RenderProfileRequest, RenderSession, SyncError,
    batch_key, process_render_profile_request, sync_frame, teardown,
};
use cs_app::scene::AirframeDamageState;
use cs_content::livery::LiveryPaint;
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

/// The three aircraft the fixture scene draws. `a` and `c` share one committed
/// paint, `b` has its own.
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

/// The four texels a synthetic stored image is built from: the same table the
/// F17-B fixture uses, so the decoded image is byte-identical to one a
/// previous stage already proved.
fn budget() -> AllocationBudget {
    AllocationBudget::with_defaults("synthetic/f17-c.bm")
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
    /// The item's part identity was not established.
    Unresolved,
}

/// One item of the fixture scene, authored.
struct ItemSpec {
    key: &'static str,
    instance: ModelInstanceId,
    part: PartChoice,
    class: MaterialClass,
    coverage: Coverage,
    image: bool,
    center_m: [f32; 3],
}

/// The fixture scene: three aircraft, two parts each, one mesh and one image
/// shared by all of them.
///
/// Every item is submitted with the same geometry and — for the alpha-cut
/// wings — the same image, so the *only* thing that can keep two items in
/// different batches is the per-instance state this stage is about. The
/// submission order is scrambled on purpose and puts `a.body` next to
/// `c.body`, the pair that must merge.
fn specs() -> [ItemSpec; 6] {
    // Every item gets its own view depth, so a row's depth is a fact about
    // that aircraft and not a constant of the fixture.
    let wing = |key, instance, x, z| ItemSpec {
        key,
        instance,
        part: PartChoice::Wing,
        class: MaterialClass::Masked,
        coverage: Coverage::Texture(AlphaSource::Channel),
        image: true,
        center_m: [x, 0.0, z],
    };
    let body = |key, instance, x, z| ItemSpec {
        key,
        instance,
        part: PartChoice::Body,
        class: MaterialClass::Opaque,
        coverage: Coverage::Opaque,
        image: false,
        center_m: [x, -1.0, z],
    };
    [
        wing("b.wing", PLANE_B, 3.0, -21.0),
        wing("a.wing", PLANE_A, 1.0, -22.0),
        wing("c.wing", PLANE_C, 5.0, -23.0),
        body("b.body", PLANE_B, 3.0, -24.0),
        body("a.body", PLANE_A, 1.0, -25.0),
        body("c.body", PLANE_C, 5.0, -26.0),
    ]
}

/// The synthetic scene, its uploads, its plan and its per-instance records.
struct Fixture {
    items: Vec<DrawItem>,
    /// The aircraft each submitted item draws for, in submission order.
    instances: Vec<ModelInstanceId>,
    outcomes: Vec<cs_app::render::capture::SceneOutcome>,
    plan: DrawPlan,
    view: SceneView,
    visuals: InstanceVisuals,
    parts: BTreeMapPart,
    choices: Vec<PartChoice>,
    runtime: LiveryRuntime,
    session: LiverySession,
    /// The one part identity of `a`'s damage record.
    damaged: AirframeDamageState,
    intact: AirframeDamageState,
}

/// The two part identities the scene draws. Kept beside the scene so the
/// submitted draws can borrow them.
struct BTreeMapPart {
    body: SceneNodeId,
    wing: SceneNodeId,
}

fn classify_item(spec: &ItemSpec) -> cs_app::render::material::ClassifiedMaterial {
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
        addressing: Some(REPEAT),
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

/// Builds the scene through the production readers: the canonical mesh through
/// `cs_content`, the stored image through `cs_formats`, the paint through the
/// F09-C livery runtime and the damage through the F11-C record.
fn fixture() -> Fixture {
    let view = SceneView::new([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]).expect("a finite view");
    let parts = BTreeMapPart {
        body: part("planes.fuselage"),
        wing: part("planes.wing_l"),
    };
    let specs = specs();
    let mut items = Vec::new();
    let mut instances = Vec::new();
    let mut images = Vec::new();
    let mut outcomes = Vec::new();
    let mut choices = Vec::new();
    for spec in &specs {
        let item = DrawItem::new(
            DrawItemKey::new(spec.key).expect("authored keys are valid"),
            classify_item(spec),
            spec.center_m,
            None,
        )
        .expect("authored geometry is finite");
        // Every item is uploaded from the same quad and, for the wings, the
        // same stored image, so only the per-instance state can separate them.
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
        images.push(image);
        choices.push(spec.part);
    }
    let plan = DrawPlan::build(&items, &view);

    // The producer for paint: the F09-C runtime, one instance bound per paint.
    let bytes = stored_bm();
    let mut context = ParseContext::with_defaults("synthetic/f17-c.bm");
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

    // The producer for damage: the F11-C record, one per aircraft. `a` has its
    // left wing destroyed; the other two are intact.
    let mut damaged = AirframeDamageState::new();
    damaged.destroy(parts.wing.clone());

    let mut visuals = InstanceVisuals::new();
    for (instance, damage) in [
        (PLANE_A, &damaged),
        (PLANE_B, &AirframeDamageState::new()),
        (PLANE_C, &AirframeDamageState::new()),
    ] {
        visuals
            .bind(&runtime, session, instance, damage)
            .expect("the livery session is this one");
    }

    Fixture {
        items,
        instances,
        outcomes,
        plan,
        view,
        visuals,
        parts,
        choices,
        runtime,
        session,
        damaged,
        intact: AirframeDamageState::new(),
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
                part: match self.choices[index] {
                    PartChoice::Body => PartRef::Known(&self.parts.body),
                    PartChoice::Wing => PartRef::Known(&self.parts.wing),
                    PartChoice::Unresolved => {
                        PartRef::Unresolved("mesh_group_has_no_part_identity")
                    }
                },
            })
            .collect()
    }

    /// The committed paint digest of one aircraft, from the producer.
    fn livery(&self, instance: ModelInstanceId) -> cs_types::evidence::ContentHash {
        self.visuals
            .get(instance)
            .and_then(InstanceVisual::livery)
            .expect("the fixture binds every aircraft")
    }

    /// Batches the scene under `profile`.
    fn batch(&self, profile: &RenderProfile) -> BatchedFrame {
        let submitted = self.submitted();
        batch_frame(&submitted, &self.plan, &self.visuals, profile, TICK)
            .expect("the fixture scene batches")
    }
}

/// The batches of one phase, in draw order.
fn batches_of(
    frame: &BatchedFrame,
    phase: RenderPhase,
) -> Vec<&cs_app::render::batch::InstanceBatch> {
    frame
        .batches()
        .iter()
        .filter(|batch| batch.phase() == phase)
        .collect()
}

/// The one batch that holds `instance`.
fn batch_of(
    frame: &BatchedFrame,
    phase: RenderPhase,
    instance: ModelInstanceId,
) -> &cs_app::render::batch::InstanceBatch {
    frame
        .batches()
        .iter()
        .find(|batch| batch.phase() == phase && batch.row(instance).is_some())
        .unwrap_or_else(|| panic!("no {phase} batch draws {instance}"))
}

/// AC03: two instances with different paint and different damage stay
/// visually independent after batching, and the ones that *can* share a batch
/// do.
#[test]
fn accept_f17_c_two_instances_with_different_paint_and_damage_stay_independent_after_batching() {
    let fixture = fixture();
    let red = fixture.livery(PLANE_A);
    let blue = fixture.livery(PLANE_B);
    assert_eq!(red, fixture.livery(PLANE_C), "a and c share one paint");
    assert_ne!(red, blue, "b has its own paint");
    assert_eq!(
        fixture.runtime.image(PLANE_A).expect("a").rgb(),
        fixture.runtime.image(PLANE_C).expect("c").rgb(),
        "the shared paint really is the same composed image"
    );
    assert_ne!(
        fixture.runtime.image(PLANE_A).expect("a").rgb(),
        fixture.runtime.image(PLANE_B).expect("b").rgb(),
        "b's paint is a different composed image"
    );

    let frame = fixture.batch(&RenderProfile::faithful());

    // The destroyed part is withheld with its reason, and only it: the same
    // part of the two intact aircraft is drawn.
    assert_eq!(frame.withheld().len(), 1);
    let withheld = &frame.withheld()[0];
    assert_eq!(withheld.item().as_str(), "a.wing");
    assert_eq!(withheld.instance(), PLANE_A);
    assert_eq!(withheld.reasons(), [withheld_codes::DESTROYED_PART]);
    assert_eq!(frame.instance_count(), 5, "six items, one withheld");
    assert!(frame.limitations().is_empty(), "every fact was established");

    // Opaque bodies: `a` and `c` share one batch, `b` is in one of its own.
    // The merge is the proof that batching happens at all; the split is the
    // proof that the paint is part of the key.
    let bodies = batches_of(&frame, RenderPhase::Opaque);
    assert_eq!(bodies.len(), 2, "two paints, two draws");
    let shared = batch_of(&frame, RenderPhase::Opaque, PLANE_A);
    assert_eq!(shared.len(), 2, "a and c are batched together");
    assert_eq!(shared.key().livery(), Some(red));
    assert!(shared.row(PLANE_C).is_some());
    assert!(
        shared.row(PLANE_B).is_none(),
        "b must not be drawn with a's and c's paint"
    );
    assert_eq!(shared.key().image(), None, "the bodies carry no image");
    let own = batch_of(&frame, RenderPhase::Opaque, PLANE_B);
    assert_eq!(own.len(), 1, "b's body is a draw of its own");
    assert_eq!(own.key().livery(), Some(blue));
    assert_ne!(
        batch_key(shared),
        batch_key(own),
        "two paints are two draws even with one mesh and one state"
    );
    // Every row keeps its own place and depth, so the batch is not one
    // instance wearing three identities.
    assert_eq!(
        shared.row(PLANE_A).expect("a").center_m(),
        [1.0, -1.0, -25.0]
    );
    assert_eq!(
        shared.row(PLANE_C).expect("c").center_m(),
        [5.0, -1.0, -26.0]
    );
    assert_ne!(
        shared.row(PLANE_A).expect("a").depth_m(),
        shared.row(PLANE_C).expect("c").depth_m()
    );

    // Alpha-cut wings: `b` and `c` are in different batches (different paint),
    // and `a`'s wing is not drawn at all.
    let wings = batches_of(&frame, RenderPhase::Masked);
    assert_eq!(wings.len(), 2, "two intact wings, two paints");
    assert_eq!(batch_of(&frame, RenderPhase::Masked, PLANE_C).len(), 1);
    assert_eq!(batch_of(&frame, RenderPhase::Masked, PLANE_B).len(), 1);
    assert!(
        frame
            .batches()
            .iter()
            .all(|batch| batch.row(PLANE_A).is_none() || batch.phase() != RenderPhase::Masked)
    );
    assert!(
        wings.iter().all(|batch| batch.key().image().is_some()),
        "both wings sample the shared image"
    );

    // Repainting `b` with the red variant merges it into the shared batch: the
    // batch key follows the paint, and the merge is real.
    let mut repainted = fixture.visuals.clone();
    repainted.insert(InstanceVisual::bound(
        PLANE_B,
        fixture.runtime.livery(PLANE_A).expect("a is bound"),
    ));
    let submitted = fixture.submitted();
    let repainted_frame = batch_frame(
        &submitted,
        &fixture.plan,
        &repainted,
        &RenderProfile::faithful(),
        TICK,
    )
    .expect("the repainted scene batches");
    let merged = batch_of(&repainted_frame, RenderPhase::Opaque, PLANE_A);
    assert_eq!(merged.len(), 3, "three aircraft, one paint, one draw");
    assert!(merged.row(PLANE_B).is_some());
    // `a`'s own row is byte-identical across the two frames: changing `b` did
    // not move it.
    assert_eq!(
        merged.row(PLANE_A).expect("a"),
        shared.row(PLANE_A).expect("a"),
        "one aircraft's repaint must not move another's row"
    );
    assert_ne!(repainted_frame.fingerprint(), frame.fingerprint());

    // Repairing `a`'s wing draws it again, into `c`'s wing batch.
    let mut repaired = fixture.visuals.clone();
    repaired.insert(InstanceVisual::bound(
        PLANE_A,
        fixture.runtime.livery(PLANE_A).expect("a is bound"),
    ));
    let repaired_frame = batch_frame(
        &submitted,
        &fixture.plan,
        &repaired,
        &RenderProfile::faithful(),
        TICK,
    )
    .expect("the repaired scene batches");
    assert!(repaired_frame.withheld().is_empty(), "nothing is withheld");
    assert_eq!(repaired_frame.instance_count(), 6);
    let wings = batch_of(&repaired_frame, RenderPhase::Masked, PLANE_C);
    assert_eq!(wings.len(), 2, "a and c's intact wings share a paint");
    assert!(wings.row(PLANE_A).is_some());
    assert_eq!(fixture.damaged.len(), 1, "the record is unchanged");
    assert!(
        fixture.intact.is_empty(),
        "the other two records are intact"
    );
    assert_ne!(repaired_frame.fingerprint(), frame.fingerprint());

    // The producer refuses a foreign session, so a paint bound for a finished
    // session is never read into a frame, and the caller can retry.
    let mut foreign = InstanceVisuals::new();
    assert_eq!(
        foreign
            .bind(&fixture.runtime, LiverySession(6), PLANE_A, &fixture.intact,)
            .expect_err("another session is refused")
            .code(),
        "foreign_session"
    );
    assert!(foreign.is_empty(), "the refusal recorded nothing");
    assert_eq!(
        fixture.session,
        LiverySession(5),
        "the fixture's own session"
    );
    foreign
        .bind(&fixture.runtime, fixture.session, PLANE_A, &fixture.intact)
        .expect("the open session binds");
    assert_eq!(foreign.len(), 1);

    // The reason a part is not drawn is part of what the frame records: the
    // same five draws are a different frame when the sixth is withheld because
    // the adapters refused it rather than because the part is destroyed.
    let mut refused = fixture;
    refused.outcomes[1] = cs_app::render::capture::SceneOutcome::Refused(
        cs_app::render::capture::SurfaceRefusal::new(
            DrawItemKey::new("a.wing").expect("valid"),
            vec!["two_sided_unknown"],
        ),
    );
    let refused_frame = refused.batch(&RenderProfile::faithful());
    assert_eq!(
        refused_frame.batches(),
        frame.batches(),
        "the same five draws"
    );
    assert_eq!(refused_frame.withheld().len(), 1);
    assert_eq!(refused_frame.withheld()[0].item().as_str(), "a.wing");
    assert_eq!(
        refused_frame.withheld()[0].reasons(),
        [withheld_codes::REFUSED, "two_sided_unknown"],
        "an adapter refusal is reported as a refusal, with its own reason"
    );
    assert_ne!(
        refused_frame.fingerprint(),
        frame.fingerprint(),
        "why a draw is missing is part of the frame's identity"
    );
}

/// AC03 in the consumer: the two aircraft with the same paint are one entity
/// carrying two rows, the third is a second entity, and the destroyed part
/// spawned nothing.
#[test]
fn accept_f17_c_the_consumer_draws_one_entity_per_batch_with_every_row() {
    let fixture = fixture();
    let mut world = render_world();
    world.insert_resource(RenderProfileRequest::set(
        SESSION,
        RenderProfile::faithful(),
    ));
    process_render_profile_request(&mut world);

    let submitted = fixture.submitted();
    let frame = fixture.batch(&RenderProfile::faithful());
    let report = sync_frame(&mut world, &submitted, &frame, SESSION).expect("the frame syncs");

    // Four draws: two wing batches and two body batches. The additive gap is
    // not in this scene, so nothing is unmaterialed.
    assert_eq!(report.spawned, 4);
    assert_eq!(report.reused, 0);
    assert_eq!(report.released, 0);
    assert_eq!(report.unmaterialed, 0);
    assert_eq!(report.withheld, 1);
    assert_eq!(batch_entities(&world), 4);

    let body = batch_entity(&world, &frame, RenderPhase::Opaque, PLANE_A);
    let draw = world.get::<BatchDraw>(body).expect("the body batch draws");
    assert_eq!(draw.phase(), RenderPhase::Opaque);
    assert_eq!(draw.instances().len(), 2, "the batch keeps both rows");
    assert!(draw.row(PLANE_A).is_some() && draw.row(PLANE_C).is_some());
    assert!(draw.row(PLANE_B).is_none());
    assert!(draw.image().is_none(), "the bodies carry no image");
    // Its material is a real `StandardMaterial` in the world's store.
    let material = stored_material(&world, body);
    assert!(
        material.base_color_texture.is_none(),
        "an untextured surface binds no texture"
    );

    // A wing batch binds its own image handle, and it is not the other wing
    // batch's handle: the two aircraft with different paints are two draws.
    let wing_c = batch_entity(&world, &frame, RenderPhase::Masked, PLANE_C);
    let wing_b = batch_entity(&world, &frame, RenderPhase::Masked, PLANE_B);
    let handle_c = world
        .get::<BatchDraw>(wing_c)
        .and_then(BatchDraw::image)
        .expect("a textured batch binds an image")
        .clone();
    let handle_b = world
        .get::<BatchDraw>(wing_b)
        .and_then(BatchDraw::image)
        .expect("a textured batch binds an image")
        .clone();
    assert_ne!(handle_c, handle_b, "two draws, two bindings");
    assert!(stored_material(&world, wing_c).base_color_texture.is_some());
    let texture = world
        .resource::<Assets<Image>>()
        .get(&handle_c)
        .expect("the handle resolves to stored texels");
    assert_eq!(texture.width(), 2, "the canonical image reached the store");
    assert!(world.get::<bevy::mesh::Mesh3d>(body).is_some());

    // No entity draws a's destroyed wing.
    assert!(
        frame
            .batches()
            .iter()
            .all(|batch| batch.phase() != RenderPhase::Masked || batch.row(PLANE_A).is_none()),
        "the destroyed part is in no batch"
    );

    // The same frame again reuses every entity and grows nothing.
    let images_before = world.resource::<Assets<Image>>().len();
    let repeat = sync_frame(&mut world, &submitted, &frame, SESSION).expect("the frame resyncs");
    assert_eq!(repeat.spawned, 0);
    assert_eq!(repeat.reused, 4);
    assert_eq!(repeat.released, 0);
    assert_eq!(batch_entities(&world), 4, "no duplicate geometry");
    assert_eq!(
        world.resource::<Assets<Image>>().len(),
        images_before,
        "a reused batch keeps its handle instead of growing the store"
    );

    // A different frame releases what it does not claim: repainting `b` with
    // the red variant collapses the two body draws into one and gives `b` wings
    // of the red paint, so every batch of the previous frame is gone.
    let mut repainted = fixture.visuals.clone();
    repainted.insert(InstanceVisual::bound(
        PLANE_B,
        fixture.runtime.livery(PLANE_A).expect("a is bound"),
    ));
    let next = batch_frame(
        &submitted,
        &fixture.plan,
        &repainted,
        &RenderProfile::faithful(),
        TICK,
    )
    .expect("the repainted frame batches");
    let swapped = sync_frame(&mut world, &submitted, &next, SESSION).expect("the next frame syncs");
    assert_eq!(
        swapped.reused, 1,
        "c's wing is the one draw that is genuinely unchanged"
    );
    assert_eq!(swapped.released, 3, "every stale draw is despawned");
    assert_eq!(swapped.spawned, 2, "b's red wing and the merged body draw");
    assert_eq!(batch_entities(&world), 3, "no entity of the old frame is left");
    let merged = batch_entity(&world, &next, RenderPhase::Opaque, PLANE_B);
    assert_eq!(
        world
            .get::<BatchDraw>(merged)
            .expect("the merged draw exists")
            .instances()
            .len(),
        3,
        "all three aircraft are in the one draw their paint allows"
    );
}

/// A frame built under a profile nobody applied is refused before anything is
/// touched; applying that profile and syncing again succeeds. That is the
/// retry, and it is the same path a mission switch uses.
#[test]
fn accept_f17_c_a_frame_under_an_unapplied_profile_is_refused_and_retries_after_applying_it() {
    let fixture = fixture();
    let faithful = RenderProfile::faithful();
    let enhanced = faithful
        .with(Enhancement::Antialiasing { samples: 4 })
        .expect("four samples is expressible");
    let mut world = render_world();
    world.insert_resource(RenderProfileRequest::set(SESSION, faithful.clone()));
    process_render_profile_request(&mut world);

    let submitted = fixture.submitted();
    let faithful_frame = fixture.batch(&faithful);
    sync_frame(&mut world, &submitted, &faithful_frame, SESSION).expect("the frame syncs");
    let live = batch_entities(&world);
    assert_eq!(live, 4);

    // A frame built under the enhanced profile, synced under the faithful one.
    let enhanced_frame = fixture.batch(&enhanced);
    assert_ne!(enhanced_frame.fingerprint(), faithful_frame.fingerprint());
    let error = sync_frame(&mut world, &submitted, &enhanced_frame, SESSION)
        .expect_err("the applied profile does not match the frame");
    assert_eq!(error.code(), "profile_mismatch");
    assert_eq!(
        batch_entities(&world),
        live,
        "the refusal left the live frame alone"
    );
    assert_eq!(
        world
            .get_resource::<cs_app::render::sync::RenderSessionState>()
            .expect("a session is open")
            .profile(),
        &faithful,
        "a refused sync changes no profile"
    );

    // A foreign session is refused too, and changes nothing.
    let foreign = sync_frame(&mut world, &submitted, &faithful_frame, RenderSession(99))
        .expect_err("another session is refused");
    assert_eq!(foreign.code(), "foreign_session");

    // Retry: apply the enhanced profile, then the same frame syncs.
    world.insert_resource(RenderProfileRequest::set(SESSION, enhanced.clone()));
    process_render_profile_request(&mut world);
    let retried = sync_frame(&mut world, &submitted, &enhanced_frame, SESSION)
        .expect("the retry syncs the frame it was built for");
    assert_eq!(retried.reused, 4, "the geometry is the same either way");
    assert_eq!(
        batch_entities(&world),
        live,
        "a profile switch redraws nothing"
    );
    assert_eq!(
        world
            .get_resource::<cs_app::render::sync::RenderSessionState>()
            .expect("a session is open")
            .applied(),
        2,
        "the state counts the applications"
    );

    // A session switch ends the old one: nothing of it is left drawn.
    world.insert_resource(RenderProfileRequest::set(
        RenderSession(12),
        faithful.clone(),
    ));
    process_render_profile_request(&mut world);
    assert_eq!(
        batch_entities(&world),
        0,
        "a new session leaves no entity of the old one"
    );
    let log = world
        .get_resource::<RenderProfileLog>()
        .expect("the log records the hand-off");
    assert_eq!(log.len(), 3);
    assert!(matches!(
        log.events()[2],
        ProfileEvent::Applied {
            session: RenderSession(12),
            ..
        }
    ));

    // Tearing down another session is refused and reported; tearing this one
    // down is a no-op-safe teardown, twice.
    world.insert_resource(RenderProfileRequest::tear_down(RenderSession(77)));
    process_render_profile_request(&mut world);
    assert_eq!(batch_entities(&world), 0);
    assert_eq!(batch_entities(&world), 0, "teardown is idempotent");
    world.insert_resource(RenderProfileRequest::tear_down(RenderSession(12)));
    process_render_profile_request(&mut world);
    let log = world
        .get_resource::<RenderProfileLog>()
        .expect("the log records the hand-off");
    assert_eq!(
        log.refusals().map(SyncError::code).collect::<Vec<_>>(),
        ["foreign_session"]
    );
    assert!(matches!(log.last(), Some(ProfileEvent::TornDown { .. })));
    // And with no session open there is nothing to sync under.
    assert_eq!(
        sync_frame(&mut world, &submitted, &faithful_frame, SESSION)
            .expect_err("no session is open")
            .code(),
        "no_render_session"
    );
    let again = teardown(&mut world);
    assert_eq!(again.entities, 0);
    assert!(!again.sessions, "a second teardown releases nothing");
}

/// The applied presentation reaches the entities that own each decision, and a
/// world with no asset store is refused instead of drawn with unbound
/// textures.
#[test]
fn accept_f17_c_the_applied_presentation_reaches_the_camera_light_and_window() {
    let fixture = fixture();
    let mut world = render_world();
    let (camera, light, window) = presentation_targets(&mut world);

    let enhanced = RenderProfile::faithful()
        .with(Enhancement::Antialiasing { samples: 4 })
        .and_then(|profile| {
            profile.with(Enhancement::ToneMapping {
                curve: cs_app::render::capture::Tonemap::Filmic,
            })
        })
        .and_then(|profile| profile.with(Enhancement::ShadowMapping))
        .and_then(|profile| {
            profile.with(Enhancement::RenderResolution {
                resolution: Resolution::new(1_600, 900).expect("a positive resolution"),
            })
        })
        .expect("every option is expressible");
    let faithful = RenderProfile::faithful();

    for (profile, samples, curve, shadows, resolution) in [
        (
            faithful.clone(),
            Msaa::Off,
            BevyTonemapping::None,
            false,
            None,
        ),
        (
            enhanced.clone(),
            Msaa::Sample4,
            BevyTonemapping::BlenderFilmic,
            true,
            Some(WindowResolution::new(1_600, 900)),
        ),
    ] {
        world.insert_resource(RenderProfileRequest::set(SESSION, profile.clone()));
        process_render_profile_request(&mut world);
        let submitted = fixture.submitted();
        let frame = fixture.batch(&profile);
        let report = sync_frame(&mut world, &submitted, &frame, SESSION).expect("the frame syncs");
        assert_eq!(report.presentation.cameras, 1, "one camera was set");
        assert_eq!(report.presentation.lights, 1, "one light was set");
        assert_eq!(
            report.presentation.windows,
            usize::from(resolution.is_some()),
            "only a profile that sets a resolution touches the window"
        );
        assert_eq!(*world.get::<Msaa>(camera).expect("the camera"), samples);
        assert_eq!(
            *world
                .get::<BevyTonemapping>(camera)
                .expect("the camera has a tone curve"),
            curve
        );
        let light = world
            .get::<DirectionalLight>(light)
            .expect("the light is a directional light");
        assert_eq!(light.shadow_maps_enabled, shadows);
        assert_eq!(
            world.get::<Window>(window).expect("the window").resolution,
            resolution.unwrap_or_default()
        );
    }

    // A world with no asset store is refused before anything is spawned.
    let mut bare = World::new();
    let submitted = fixture.submitted();
    let frame = fixture.batch(&faithful);
    assert_eq!(
        sync_frame(&mut bare, &submitted, &frame, SESSION)
            .expect_err("no session is open")
            .code(),
        "no_render_session"
    );
    bare.insert_resource(cs_app::render::sync::RenderSessionState::new(
        SESSION, faithful, 1,
    ));
    assert_eq!(
        sync_frame(&mut bare, &submitted, &frame, SESSION)
            .expect_err("the world has no asset store")
            .code(),
        "no_asset_store"
    );
    assert_eq!(batch_entities(&bare), 0, "a refused sync spawns nothing");
}

/// No enhancement changes a draw decision: the same scene batched under the
/// fully enhanced profile has the same batches, rows, withheld draws and
/// limitations as the faithful one, and the profile digest is visibly
/// different.
#[test]
fn accept_f17_c_enhancements_change_no_draw_decision() {
    let fixture = fixture();
    let faithful = RenderProfile::faithful();
    let enhanced = RenderProfile::new([
        Enhancement::Antialiasing { samples: 8 },
        Enhancement::ToneMapping {
            curve: cs_app::render::capture::Tonemap::Filmic,
        },
        Enhancement::ShadowMapping,
        Enhancement::RenderResolution {
            resolution: Resolution::new(1_920, 1_080).expect("positive"),
        },
    ])
    .expect("every option is expressible");

    let baseline = fixture.batch(&faithful);
    let improved = fixture.batch(&enhanced);
    assert_eq!(
        baseline.profile(),
        ProfileParity::FidelityBaseline,
        "the baseline is comparison evidence"
    );
    assert_eq!(
        improved.profile(),
        ProfileParity::DesignedImprovement,
        "an enhanced profile is not"
    );
    assert_ne!(
        baseline.profile_fingerprint(),
        improved.profile_fingerprint(),
        "the two profiles really are different"
    );
    assert_ne!(baseline.fingerprint(), improved.fingerprint());

    // The draw content is identical: same batches, same shared resources, same
    // per-instance rows, same withheld draws, same limitations. Only the
    // profile the frame records differs.
    assert_eq!(baseline.batches(), improved.batches());
    assert_eq!(baseline.withheld(), improved.withheld());
    assert_eq!(baseline.limitations(), improved.limitations());
    assert_eq!(baseline.instance_count(), improved.instance_count());
    assert_eq!(baseline.batch_count(), improved.batch_count());
    for (batch, other) in baseline.batches().iter().zip(improved.batches()) {
        assert_eq!(batch_key(batch), batch_key(other));
    }

    // The enhancements reached the presentation and nothing else.
    let presentation = enhanced.presentation();
    assert_eq!(presentation.msaa_samples(), 8);
    assert_eq!(
        presentation.tonemap(),
        cs_app::render::capture::Tonemap::Filmic
    );
    assert!(presentation.shadows());
    assert_eq!(
        presentation.render_resolution(),
        Some(Resolution {
            width: 1_920,
            height: 1_080
        })
    );
    assert!(!presentation.is_fidelity());

    // Two *different* enhanced profiles record different frames although the
    // draw content is identical, so the profile the frame was built under is
    // part of what a comparison sees — not only the faithful/improved verdict.
    let other = RenderProfile::new([
        Enhancement::Antialiasing { samples: 4 },
        Enhancement::ToneMapping {
            curve: cs_app::render::capture::Tonemap::Filmic,
        },
        Enhancement::ShadowMapping,
        Enhancement::RenderResolution {
            resolution: Resolution::new(1_920, 1_080).expect("positive"),
        },
    ])
    .expect("every option is expressible");
    let also_improved = fixture.batch(&other);
    assert_eq!(also_improved.profile(), improved.profile());
    assert_eq!(also_improved.batches(), improved.batches());
    assert_ne!(
        also_improved.fingerprint(),
        improved.fingerprint(),
        "the recorded profile is part of the frame's identity"
    );
    assert_ne!(
        also_improved.profile_fingerprint(),
        improved.profile_fingerprint()
    );
}

/// An enhanced profile is refused as comparison evidence by the profile and,
/// independently, by F17-B's capture: the refusal chain a caller actually
/// walks.
#[test]
fn accept_f17_c_an_enhanced_profile_is_refused_as_comparison_evidence() {
    let fixture = fixture();
    let faithful = RenderProfile::faithful();
    let settings = faithful
        .comparison_settings()
        .expect("the baseline is the fixed set");
    assert_eq!(
        settings,
        cs_app::render::capture::ComparisonSettings::comparison()
    );
    let frame = fixture.batch(&faithful);
    assert_eq!(
        frame.profile(),
        ProfileParity::FidelityBaseline,
        "the frame records the profile it was built under"
    );
    capture(
        &fixture.outcomes,
        &fixture.plan,
        &fixture.view,
        &Projection::comparison(),
        TICK,
        &settings,
    )
    .expect("the faithful frame captures");

    // Shadows alone are enough: an enhancement is an enhancement.
    for (profile, expected) in [
        (faithful.with(Enhancement::ShadowMapping), "shadow_mapping"),
        (
            faithful.with(Enhancement::Antialiasing { samples: 2 }),
            "antialiasing",
        ),
        (
            faithful.with(Enhancement::ToneMapping {
                curve: cs_app::render::capture::Tonemap::Filmic,
            }),
            "tone_mapping",
        ),
    ] {
        let profile = profile.expect("expressible");
        let error = profile
            .comparison_settings()
            .expect_err("an enhanced profile compares nothing");
        assert_eq!(error.code(), "not_comparison_evidence");
        let cs_app::render::profile::ProfileError::NotComparisonEvidence { options } = &error
        else {
            panic!("the refusal names the options that are on: {error}");
        };
        assert_eq!(options, &[expected], "the refusal names what is on");
        assert_eq!(profile.parity(), ProfileParity::DesignedImprovement);

        // And the settings the profile *does* produce are refused by the
        // capture itself, so a caller that ignores the profile's refusal still
        // cannot produce a comparable frame. The fixed set carries every
        // presentation decision the profile owns, so even shadows alone — which
        // changes no field F17-B knew about — are refused.
        let unfixed = profile.settings();
        assert!(!unfixed.is_fixed());
        let error = capture(
            &fixture.outcomes,
            &fixture.plan,
            &fixture.view,
            &Projection::comparison(),
            TICK,
            &unfixed,
        )
        .expect_err("the capture refuses unpinned settings");
        let CaptureError::SettingsNotFixed {
            msaa_samples,
            shadows,
            render_resolution,
            ..
        } = &error
        else {
            panic!("an unpinned setting is SettingsNotFixed, got {error}");
        };
        assert_eq!(*msaa_samples, unfixed.msaa_samples());
        assert_eq!(*shadows, unfixed.shadows());
        assert_eq!(*render_resolution, unfixed.render_resolution());
    }

    // The fixed set pins every decision the profile owns, so switching any one
    // of them off the fixed set.
    assert!(!faithful.settings().shadows(), "no shadows in the baseline");
    assert_eq!(faithful.settings().render_resolution(), None);
    assert!(faithful.settings().is_fixed());
    for settings in [
        cs_app::render::capture::ComparisonSettings::with_shadows(true),
        cs_app::render::capture::ComparisonSettings::with_render_resolution(Some(
            Resolution::new(1_920, 1_080).expect("positive"),
        )),
        cs_app::render::capture::ComparisonSettings::with_msaa_samples(4),
    ] {
        assert!(!settings.is_fixed());
    }
}

/// Every option is switchable on its own, the two options the sheet forbids
/// are refused by the type, and an order of switching makes no difference.
#[test]
fn accept_f17_c_enhancements_are_independently_switchable_and_the_forbidden_ones_refused() {
    let faithful = RenderProfile::faithful();
    assert!(faithful.is_faithful());
    assert!(faithful.enhancements().is_empty());
    assert_eq!(
        faithful.presentation(),
        cs_app::render::profile::Presentation::fidelity()
    );
    assert!(faithful.presentation().is_fidelity());

    // One option at a time: it changes its own decision and no other.
    let shadows = faithful
        .with(Enhancement::ShadowMapping)
        .expect("shadows are expressible");
    assert_eq!(shadows.enhancements(), [Enhancement::ShadowMapping]);
    assert!(shadows.presentation().shadows());
    assert_eq!(shadows.presentation().msaa_samples(), 1);
    assert_eq!(
        shadows.presentation().tonemap(),
        cs_app::render::capture::Tonemap::None
    );
    assert_eq!(shadows.presentation().render_resolution(), None);

    let antialiased = shadows
        .with(Enhancement::Antialiasing { samples: 4 })
        .expect("four samples are expressible");
    assert_eq!(antialiased.enhancements().len(), 2, "both options are on");
    assert_eq!(antialiased.presentation().msaa_samples(), 4);
    assert!(
        antialiased.presentation().shadows(),
        "the other option survives"
    );
    assert_eq!(
        antialiased.enhancement(EnhancementKind::Antialiasing),
        Some(Enhancement::Antialiasing { samples: 4 })
    );

    // Switching one off leaves the other on, and switching off an option that
    // is not on is a no-op.
    let without_aa = antialiased.without(EnhancementKind::Antialiasing);
    assert_eq!(without_aa, shadows);
    assert_eq!(without_aa.without(EnhancementKind::ToneMapping), shadows);

    // Setting the same decision again replaces it instead of doubling it.
    let resampled = antialiased
        .with(Enhancement::Antialiasing { samples: 8 })
        .expect("eight samples are expressible");
    assert_eq!(resampled.presentation().msaa_samples(), 8);
    assert_eq!(resampled.enhancements().len(), 2, "the same decision, once");

    // The order of switching makes no difference: the profile is canonical.
    let a = RenderProfile::new([
        Enhancement::ShadowMapping,
        Enhancement::RenderResolution {
            resolution: Resolution::new(1_280, 720).expect("positive"),
        },
    ]);
    let b = RenderProfile::new([
        Enhancement::RenderResolution {
            resolution: Resolution::new(1_280, 720).expect("positive"),
        },
        Enhancement::ShadowMapping,
    ]);
    let (a, b) = (a.expect("expressible"), b.expect("expressible"));
    assert_eq!(a, b);
    assert_eq!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.enhancements()[0].kind(), EnhancementKind::ShadowMapping);
    assert_eq!(
        a.enhancements()[1].kind(),
        EnhancementKind::RenderResolution
    );

    // A different setting of the same decision is a different profile.
    assert_ne!(
        a.fingerprint(),
        faithful
            .with(Enhancement::RenderResolution {
                resolution: Resolution::new(1_280, 721).expect("positive"),
            })
            .expect("expressible")
            .fingerprint()
    );

    // The two options non-negotiable 5 forbids are refused, with the option's
    // own code, and no profile is built from them.
    for (option, code) in [
        (Enhancement::TexturalUpscaling, "textural_upscaling"),
        (Enhancement::AssetRedistribution, "asset_redistribution"),
    ] {
        assert!(option.is_refused());
        assert_eq!(
            faithful
                .with(option)
                .expect_err("the sheet forbids this option"),
            ProfileError::RefusedOption { option: code }
        );
        assert_eq!(
            RenderProfile::new([option])
                .expect_err("the sheet forbids this option")
                .code(),
            code
        );
        assert_eq!(faithful.enhancements().len(), 0, "nothing was applied");
    }

    // A sample count and a resolution the engine cannot express are refused,
    // not rounded or clamped.
    assert_eq!(
        faithful
            .with(Enhancement::Antialiasing { samples: 3 })
            .expect_err("bevy has no three-sample mode"),
        ProfileError::SampleCount { samples: 3 }
    );
    assert_eq!(
        faithful
            .with(Enhancement::RenderResolution {
                resolution: Resolution {
                    width: 0,
                    height: 720
                }
            })
            .expect_err("a zero extent cannot be allocated"),
        ProfileError::Resolution {
            width: 0,
            height: 720
        }
    );
    assert_eq!(
        Resolution::new(0, 0).expect_err("zero").code(),
        "invalid_render_resolution"
    );

    // The engine mapping is total over the counts it accepts and refuses the
    // rest, with the faithful baseline among the accepted ones.
    assert_eq!(msaa_for(1).expect("one sample is off"), Msaa::Off);
    assert_eq!(msaa_for(2).expect("two"), Msaa::Sample2);
    assert_eq!(msaa_for(4).expect("four"), Msaa::Sample4);
    assert_eq!(msaa_for(8).expect("eight"), Msaa::Sample8);
    assert_eq!(
        msaa_for(16).expect_err("no sixteen-sample mode"),
        ProfileError::SampleCount { samples: 16 }
    );
    assert_eq!(
        msaa_for(0).expect_err("no zero-sample mode"),
        ProfileError::SampleCount { samples: 0 }
    );
    assert_eq!(
        faithful.presentation().msaa_samples(),
        settings_msaa(),
        "the baseline is one sample per pixel, which is Msaa::Off"
    );
    assert_eq!(
        bevy_tonemapping(cs_app::render::capture::Tonemap::None),
        BevyTonemapping::None
    );
    assert_eq!(
        bevy_tonemapping(cs_app::render::capture::Tonemap::Filmic),
        BevyTonemapping::BlenderFilmic
    );
}

/// The comparison baseline's sample count, read from the fixed set rather than
/// repeated here, so a change to it is visible in this test.
fn settings_msaa() -> u32 {
    cs_app::render::capture::COMPARISON_MSAA_SAMPLES
}

/// Two facts this stage cannot establish are reported instead of assumed: an
/// item whose instance has no paint record is drawn on its own and never
/// merged, and an item whose part identity is unresolved is drawn with its gap
/// named.
#[test]
fn accept_f17_c_unresolved_paint_and_part_identity_are_reported_not_assumed() {
    // `b` has no committed paint, so its items are never merged with anything.
    let mut without_paint = fixture();
    without_paint
        .visuals
        .insert(InstanceVisual::unbound(PLANE_B));
    let frame = without_paint.batch(&RenderProfile::faithful());
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
        2,
        "both of b's items report the gap"
    );
    assert_eq!(
        frame.instance_count(),
        5,
        "an unresolved paint is not a drop"
    );
    assert!(
        frame
            .limitations()
            .iter()
            .all(|limitation| limitation.code() == limitation_codes::UNBOUND_INSTANCE)
    );
    let own = batch_of(&frame, RenderPhase::Opaque, PLANE_B);
    assert_eq!(own.len(), 1, "an unresolved paint is a draw of its own");
    assert_eq!(own.key().livery(), None);
    assert!(!own.mergeable(), "and it never merges with the next item");
    assert!(own.row(PLANE_B).is_some());
    assert_ne!(
        batch_key(own),
        batch_key(batch_of(&frame, RenderPhase::Opaque, PLANE_A)),
        "an unresolved paint cannot share a draw with a resolved one"
    );

    // Three unresolved paints in a row are still three draws: two unresolved
    // paints may be different, so merging them would invent a match.
    let mut all_unbound = fixture();
    for instance in [PLANE_A, PLANE_B, PLANE_C] {
        all_unbound.visuals.insert(InstanceVisual::unbound(instance));
    }
    // `a`'s wing is intact here, so the three wings are consecutive items with
    // one geometry, one state and one image between them.
    all_unbound.visuals.insert(
        InstanceVisual::unbound(PLANE_A)
            .with_damage(&AirframeDamageState::new()),
    );
    let frame = all_unbound.batch(&RenderProfile::faithful());
    let wings = batches_of(&frame, RenderPhase::Masked);
    assert_eq!(wings.len(), 3, "three unresolved paints, three draws");
    assert!(
        wings.iter().all(|batch| batch.len() == 1 && !batch.mergeable()),
        "no two unresolved paints may share a draw"
    );
    assert_eq!(frame.instance_count(), 6, "nothing was withheld or dropped");

    // An unresolved part identity means the damage could not be checked, so the
    // item is drawn and the gap is named — an unestablished destruction is not
    // a destruction.
    let mut without_parts = fixture();
    without_parts.choices[0] = PartChoice::Unresolved; // b.wing
    let frame = without_parts.batch(&RenderProfile::faithful());
    assert_eq!(
        frame.limitations(),
        [BatchingLimitation::UnresolvedPartIdentity {
            item: DrawItemKey::new("b.wing").expect("valid"),
            reason: "mesh_group_has_no_part_identity",
        }]
    );
    assert_eq!(frame.instance_count(), 5, "nothing was dropped");
    assert_eq!(
        frame.withheld().len(),
        1,
        "only the recorded damage withholds"
    );
    assert_eq!(frame.withheld()[0].item().as_str(), "a.wing");
}

/// The batcher refuses a scene that does not match the plan, in both
/// directions, so a stale list cannot be batched into a frame that looks
/// complete.
#[test]
fn accept_f17_c_batching_refuses_a_scene_that_does_not_match_the_plan() {
    let fixture = fixture();
    let submitted = fixture.submitted();
    let mut visuals = fixture.visuals.clone();
    visuals.insert(InstanceVisual::unbound(PLANE_C));
    let profile = RenderProfile::faithful();

    // A short list: a plan entry with no submitted draw.
    let short = &submitted[..submitted.len() - 1];
    let error = batch_frame(short, &fixture.plan, &visuals, &profile, TICK)
        .expect_err("the plan reaches a draw that is not there");
    assert_eq!(error.code(), "plan_entry_without_scene_outcome");

    // A long list: a submitted draw no plan entry reaches.
    let mut long = submitted.clone();
    long.push(SubmittedDraw {
        item: &fixture.items[0],
        outcome: &fixture.outcomes[0],
        instance: PLANE_A,
        part: PartRef::Known(&fixture.parts.body),
    });
    let error =
        batch_frame(&long, &fixture.plan, &visuals, &profile, TICK).expect_err("an extra draw");
    assert_eq!(error.code(), "submitted_draw_without_plan_entry");

    // A key that does not match the plan's entry at that index.
    let mut wrong = fixture.items.clone();
    wrong[0] = DrawItem::new(
        DrawItemKey::new("b.other").expect("valid"),
        classify_item(&specs()[0]),
        [0.0, 0.0, 0.0],
        None,
    )
    .expect("finite");
    let mut mismatched = fixture.submitted();
    mismatched[0] = SubmittedDraw {
        item: &wrong[0],
        outcome: &fixture.outcomes[0],
        instance: PLANE_B,
        part: PartRef::Known(&fixture.parts.wing),
    };
    let error = batch_frame(&mismatched, &fixture.plan, &visuals, &profile, TICK)
        .expect_err("the key does not match");
    assert_eq!(error.code(), "submitted_draw_key_mismatch");

    // The same list batches, so the three refusals above are the list's fault.
    batch_frame(&submitted, &fixture.plan, &visuals, &profile, TICK)
        .expect("the fixture list batches");
}

/// A Bevy world with the asset stores the consumer binds into, and nothing
/// else.
fn render_world() -> World {
    let mut world = World::new();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<StandardMaterial>::default());
    world
}

/// A camera with an antialiasing setting and a tone curve, a directional light
/// and a window: the three entities that own a presentation decision.
fn presentation_targets(world: &mut World) -> (Entity, Entity, Entity) {
    let camera = world
        .spawn((
            bevy::transform::prelude::Transform::default(),
            Msaa::Sample4,
            BevyTonemapping::TonyMcMapface,
        ))
        .id();
    let light = world.spawn(DirectionalLight::default()).id();
    let window = world.spawn(Window::default()).id();
    (camera, light, window)
}

/// How many entities draw a batch.
fn batch_entities(world: &World) -> usize {
    world
        .iter_entities()
        .filter(|entity| entity.contains::<BatchDraw>())
        .count()
}

/// The entity that draws the batch `instance` appears in.
fn batch_entity(
    world: &World,
    frame: &BatchedFrame,
    phase: RenderPhase,
    instance: ModelInstanceId,
) -> Entity {
    let key = batch_key(batch_of(frame, phase, instance));
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
