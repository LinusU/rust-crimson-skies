//! Acceptance: the F17 render sync draws what the composed visibility verdict
//! says (task `#503`, the follow-up to `### F20-C.03`).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C` (non-negotiable behavior 3) and
//! `specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-C` (non-negotiable 4 and 5). Task test prefix:
//! `accept_f20_c_draw_`.
//!
//! Every test here drives the **production** path end to end: the declared
//! F20-C.03 breakable clip through `play_animation` and the wired fixed-tick
//! entry, F11-C's own `process_airframe_scene_request`, `apply_airframe_damage`
//! and `select_lod_presentation`, F17-C's `batch_frame` and `sync_frame`. None
//! of them writes `NodePresentation` or `NodeAnimatedVisibility` by hand to make
//! an assertion pass, and none of them asserts the composed verdict alone — the
//! assertion is on **which draws the sync placed**, because the consumer is the
//! thing whose output has to change.
//!
//! Every fixture value is newly authored synthetic data: a node array converted
//! through the production readers, a quad mesh and a stored `.bm` built here.
//! Nothing original, no `CS_GAME_DIR`, and no claim about the original renderer.

use std::sync::Arc;

use bevy::asset::Assets;
use bevy::ecs::prelude::{Entity, World};
use bevy::ecs::schedule::Schedule;
use bevy::image::Image;
use bevy::mesh::Mesh;
use bevy::pbr::StandardMaterial;
use cs_app::airframe_visual::AirframeVisual;
use cs_app::animation::{
    AnimatedNodeBinding, AnimationInstance, AnimationPlayback, CommittedSessionTick,
    advance_animation_on_session_tick, play_animation,
};
use cs_app::livery::{LiveryRuntime, LiverySession, ModelInstanceId, PaintChoice};
use cs_app::render::additive::AdditiveMaterial;
use cs_app::render::batch::{InstanceVisuals, PartRef, SubmittedDraw, batch_frame};
use cs_app::render::capture::{SceneSurface, upload_surface};
use cs_app::render::material::{
    AddressMode, Coverage, DeclaredClass, MaterialClass, MaterialFacts, TextureAddress, classify,
};
use cs_app::render::plan::{DrawItem, DrawItemKey, DrawPlan, SceneView};
use cs_app::render::profile::{Enhancement, RenderProfile};
use cs_app::render::sync::{
    BatchDraw, BatchInstancePlacement, FrameSync, RenderProfileRequest, RenderSession,
    process_render_profile_request, sync_frame,
};
use cs_app::render::visibility::VisibilityReport;
use cs_app::scene::{
    AirframeDamageState, AirframeSceneRequest, LiveAirframeScene, LodDistance,
    apply_airframe_damage, process_airframe_scene_request, select_lod_presentation,
};
use cs_content::animation::{
    SYNTHETIC_BREAKABLE_HIDDEN_TICK, SYNTHETIC_BREAKABLE_SHOWN_TICK,
    declared_synthetic_breakable_clip,
};
use cs_content::coordinates::SourceAdapter;
use cs_content::livery::LiveryPaint;
use cs_content::scene::{BindingMap, ParsedNode, ParsedNodeKind, SceneGraph, SceneNodeId};
use cs_formats::bm::PaintColor;
use cs_formats::io::AllocationBudget;
use cs_formats::{ParseContext, read_bm};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimStatus;
use cs_types::net::SessionId;
use cs_types::space::Meters;

use super::fixture::{QuadShape, quad_mesh};

// -------------------------------------------------------------- fixture ---

/// The tick the frame in this file is at.
const TICK: Tick = Tick(4_243);
/// The render session the consumer opens.
const SESSION: RenderSession = RenderSession(21);
/// The livery session the fixture's paint is bound in.
const LIVERY_SESSION: LiverySession = LiverySession(6);
/// The one aircraft the frame draws: one batch, four rows, four parts.
const PLANE: ModelInstanceId = ModelInstanceId(1);
/// The animation session the playback stamps its events with.
const ANIMATION_SESSION: u64 = 44;

/// The container the fixture's node array lives in. The key is what every node
/// id is derived from, so it is chosen to make the animated node's derived id
/// exactly the one the declared F20-C.03 clip drives.
const CONTAINER: &str = "synthetic";
/// The authored root name, so the airframe's root is `synthetic.plane`.
const ROOT: &str = "plane";
/// The animated node: derived as `synthetic.plane.hatch`, which is
/// `SYNTHETIC_BREAKABLE_NODE` — the clip's own target.
const HATCH: &str = "hatch";
/// The near band of the wing's LOD group, authored in centimetres.
const WING_NEAR: &str = "wing_lod0";
/// The far band of the same group: the same part at another distance.
const WING_FAR: &str = "wing_lod1";
/// The part damage destroys.
const TAIL: &str = "tail";

/// 10_000 cm is a 100 m near bound after the fixture adapter's conversion.
const NEAR_CM: f32 = 10_000.0;
/// 30_000 cm is a 300 m far bound.
const FAR_CM: f32 = 30_000.0;
/// A viewer distance inside the near band.
const NEAR_METRES: f64 = 50.0;
/// A viewer distance inside the far band.
const FAR_METRES: f64 = 200.0;

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("the fixture ids are valid")
}

fn node_id(path: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(cid(ContentKind::SceneNode, path)).expect("a scene node id")
}

fn budget() -> AllocationBudget {
    AllocationBudget::with_defaults("synthetic/f20-c-draw-consumer.bm")
}

/// A 2x2 `.bm` with a full mask on every plane, so a faction paint composes to
/// bytes that differ from the base image. Authored here; nothing original.
fn stored_bm() -> Vec<u8> {
    let base = [
        [12u8, 34, 56],
        [78, 90, 111],
        [133, 144, 155],
        [166, 177, 188],
    ];
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

/// The fixture's node array: one root with four parts — the animated hatch, a
/// two-band LOD group and a plain tail.
///
/// Authored through the production record and converted by
/// [`SceneGraph::build`], so the node ids the frame addresses are ids the
/// canonical conversion derived, never ids a test typed in.
fn fixture_graph() -> Arc<SceneGraph> {
    let mut root = ParsedNode::new(0, ROOT, ParsedNodeKind::World);
    root.children = vec![1, 2, 3, 4];

    let mut hatch = ParsedNode::new(1, HATCH, ParsedNodeKind::Object3d);
    hatch.parent = Some(0);

    let mut near = ParsedNode::new(
        2,
        WING_NEAR,
        ParsedNodeKind::Lod {
            level: false,
            range_min: 0.0,
            range_max: NEAR_CM,
        },
    );
    near.parent = Some(0);

    let mut far = ParsedNode::new(
        3,
        WING_FAR,
        ParsedNodeKind::Lod {
            level: true,
            range_min: NEAR_CM,
            range_max: FAR_CM,
        },
    );
    far.parent = Some(0);

    let mut tail = ParsedNode::new(4, TAIL, ParsedNodeKind::Object3d);
    tail.parent = Some(0);

    let adapter = SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "fixture.left-handed-z-up-centimeters-degrees")
        .expect("the F16-A registry declares the left-handed centimeters fixture");
    let graph = SceneGraph::build(
        &cid(ContentKind::InstallFile, CONTAINER),
        &[root, hatch, near, far, tail],
        &adapter,
        &BindingMap::default(),
    )
    .expect("the fixture node array converts");
    Arc::new(graph)
}

/// The airframe whose visual subtree the load imports.
fn fixture_airframe() -> AirframeVisual {
    AirframeVisual::new(
        cid(ContentKind::Airframe, "synthetic.kestrel"),
        cid(ContentKind::InstallFile, CONTAINER),
        node_id("synthetic.plane"),
    )
    .expect("the fixture airframe references its own container's root")
}

/// The parts the frame draws, in submission order, and the scene node each one
/// is.
fn drawn_parts() -> Vec<(&'static str, SceneNodeId)> {
    vec![
        ("a.hatch", node_id("synthetic.plane.hatch")),
        ("a.wing_near", node_id("synthetic.plane.wing_lod0")),
        ("a.wing_far", node_id("synthetic.plane.wing_lod1")),
        ("a.tail", node_id("synthetic.plane.tail")),
    ]
}

/// The synthetic frame: four items of one geometry and one material, submitted
/// so the batcher merges them into **one** draw with four rows.
struct Scene {
    items: Vec<DrawItem>,
    outcomes: Vec<cs_app::render::capture::SceneOutcome>,
    parts: Vec<SceneNodeId>,
    plan: DrawPlan,
    visuals: InstanceVisuals,
    runtime: LiveryRuntime,
}

impl Scene {
    /// The frame's submitted draws, in submission order.
    fn submitted(&self) -> Vec<SubmittedDraw<'_>> {
        self.items
            .iter()
            .enumerate()
            .map(|(index, item)| SubmittedDraw {
                item,
                outcome: &self.outcomes[index],
                instance: PLANE,
                part: PartRef::Known(&self.parts[index]),
            })
            .collect()
    }
}

/// The material every fixture item is classified with: opaque, no texture, so
/// one upload serves all four and the batch key is decided by nothing but the
/// per-instance paint.
fn fixture_item(key: &str, center_m: [f32; 3]) -> DrawItem {
    let declared = DeclaredClass::new(MaterialClass::Opaque, ClaimStatus::Designed)
        .expect("Designed asserts its own contract");
    let facts = MaterialFacts {
        declared: Some(declared),
        coverage: Coverage::Opaque,
        alpha_test: cs_formats::texture::AlphaTest::Disabled,
        two_sided: Some(false),
        addressing: Some(TextureAddress {
            u: AddressMode::Repeat,
            v: AddressMode::Repeat,
        }),
        vertex_colors: false,
        unknown_flag_bits: 0,
    };
    let material = match classify(&facts) {
        cs_app::render::material::Classification::Classified(material) => material,
        cs_app::render::material::Classification::Unclassified { reasons } => {
            panic!("the fixture item {key} stopped classifying: {reasons:?}")
        }
    };
    DrawItem::new(
        DrawItemKey::new(key).expect("authored keys are valid"),
        material,
        center_m,
        None,
    )
    .expect("authored geometry is finite")
}

fn scene() -> Scene {
    let parts = drawn_parts()
        .into_iter()
        .map(|(_, part)| part)
        .collect::<Vec<_>>();
    let view = SceneView::new([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]).expect("a finite view");
    let mut items = Vec::new();
    let mut outcomes = Vec::new();
    for (index, (key, _)) in drawn_parts().into_iter().enumerate() {
        let item = fixture_item(key, [index as f32, 0.0, 0.0]);
        // The same quad for every item: the batch key can only be decided by
        // the per-instance paint, which every row shares, so all four land in
        // one batch and only the visibility verdict can keep one off screen.
        outcomes.push(upload_surface(&SceneSurface {
            item: &item,
            mesh: &quad_mesh(QuadShape::full(), 0),
            group: 0,
            image: None,
            unknowns: &[],
        }));
        items.push(item);
    }
    let plan = DrawPlan::build(&items, &view);

    let bytes = stored_bm();
    let mut context = ParseContext::with_defaults("synthetic/f20-c-draw-consumer.bm");
    let file = read_bm(&mut context, &bytes).expect("the synthetic image parses");
    let mut runtime = LiveryRuntime::new(LIVERY_SESSION);
    runtime
        .bind(
            LIVERY_SESSION,
            PLANE,
            &file,
            PaintChoice::faction(
                cid(ContentKind::Faction, "red"),
                LiveryPaint::new([
                    PaintColor::new(220, 30, 30),
                    PaintColor::WHITE,
                    PaintColor::WHITE,
                ]),
            ),
            &mut budget(),
        )
        .expect("the fixture paint fits");

    let mut visuals = InstanceVisuals::new();
    visuals
        .bind(&runtime, LIVERY_SESSION, PLANE, &AirframeDamageState::new())
        .expect("the livery session is this one");

    Scene {
        items,
        outcomes,
        parts,
        plan,
        visuals,
        runtime,
    }
}

/// A render world with the four asset stores `sync_frame` refuses to draw
/// without, and the profile the fixture's frames are built under.
fn render_world() -> World {
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

/// The world the consumer draws into: a live airframe scene loaded through
/// F11-C's own request, one part destroyed through its own damage pass, and the
/// LOD pass run at `metres`.
fn live_world(metres: f64) -> World {
    let mut world = render_world();
    let mut damage = AirframeDamageState::new();
    damage.destroy(node_id("synthetic.plane.tail"));
    world.insert_resource(damage);
    set_distance(&mut world, metres);
    world.insert_resource(AirframeSceneRequest::load(
        fixture_airframe(),
        fixture_graph(),
    ));
    process_airframe_scene_request(&mut world);
    // F11-C's own order: the damage pass writes the markers, the LOD pass folds
    // them and the distance into the presentation record the verdict reads.
    apply_airframe_damage(&mut world);
    run_lod_pass(&mut world);
    world
}

fn set_distance(world: &mut World, metres: f64) {
    world.insert_resource(
        LodDistance::new(Meters(metres)).expect("the fixture distances are usable"),
    );
}

/// The real F11-C LOD system, the only writer of the presentation record.
fn run_lod_pass(world: &mut World) {
    let mut schedule = Schedule::default();
    schedule.add_systems(select_lod_presentation);
    schedule.run(world);
}

/// Binds the animated hatch to one live instance of the declared clip and
/// starts it, then advances it to `tick` through the wired fixed-tick entry.
fn play_breakable_at(world: &mut World, tick: u64) {
    let generation = live_generation(world);
    let hatch = node_entity(world, "synthetic.plane.hatch");
    world.entity_mut(hatch).insert(AnimatedNodeBinding {
        clip: declared_synthetic_breakable_clip().id().clone(),
        node: node_id("synthetic.plane.hatch").as_content_id().clone(),
        instance: AnimationInstance::new(1).expect("a nonzero instance"),
        generation,
    });
    world.insert_resource(AnimationPlayback::new(
        SessionId::new(ANIMATION_SESSION).expect("a nonzero session"),
    ));
    play_animation(
        world,
        &declared_synthetic_breakable_clip(),
        AnimationInstance::new(1).expect("a nonzero instance"),
        generation,
        Tick(0),
    )
    .expect("the declared clip starts");
    world.insert_resource(CommittedSessionTick::new(Tick(tick)));
    advance_animation_on_session_tick(world);
}

fn live_generation(world: &World) -> cs_app::scene::SceneGeneration {
    world
        .get_resource::<LiveAirframeScene>()
        .expect("the fixture scene is live")
        .generation()
}

fn node_entity(world: &World, path: &str) -> Entity {
    live_generation(world);
    world
        .get_resource::<LiveAirframeScene>()
        .expect("the fixture scene is live")
        .entity(&node_id(path))
        .expect("the node is part of the imported subtree")
}

/// The frame the fixture batches, under the faithful profile.
fn batched(scene: &Scene) -> cs_app::render::batch::BatchedFrame {
    let submitted = scene.submitted();
    batch_frame(
        &submitted,
        &scene.plan,
        &scene.visuals,
        &RenderProfile::faithful(),
        TICK,
    )
    .expect("the fixture scene batches")
}

/// The draw items whose placement the sync holds, by item key.
fn placed_items(world: &World) -> Vec<String> {
    let mut keys = world
        .iter_entities()
        .filter(|entity| entity.contains::<BatchInstancePlacement>())
        .filter_map(|entity| {
            world
                .get::<BatchInstancePlacement>(entity.id())
                .map(|placement| placement.row().item().as_str().to_owned())
        })
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

/// How many batch entities the world holds.
fn batch_entities(world: &World) -> usize {
    world
        .iter_entities()
        .filter(|entity| entity.contains::<BatchDraw>())
        .count()
}

/// Every draw the sync placed, counted by what the verdict said about it.
fn sync(world: &mut World, scene: &Scene) -> FrameSync {
    let submitted = scene.submitted();
    let frame = batched(scene);
    sync_frame(world, &submitted, &frame, SESSION, &scene.runtime)
        .expect("the fixture frame syncs under the applied profile")
}

// ------------------------------------------------- 1. the four verdicts ---

/// The minimum scenario: one batch, four rows, and the sync places **exactly**
/// the rows the composed verdict draws.
///
/// The four parts are four different combinations of the two records, all
/// reached through their own production passes — the clip hides the hatch at its
/// authored tick, the LOD pass culls the far band at this distance, the damage
/// pass disables the tail, and the near band is simply drawn. Before this
/// consumer existed the frame's four rows were placed whatever any of those
/// records said.
#[test]
fn accept_f20_c_draw_the_sync_places_exactly_what_the_composed_verdict_draws() {
    let scene = scene();
    let mut world = live_world(NEAR_METRES);
    play_breakable_at(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);

    // The records really are four different combinations, so the placement
    // assertion below is about the decision and not about a fixture that would
    // come out the same either way.
    let hatch = node_entity(&world, "synthetic.plane.hatch");
    let near = node_entity(&world, "synthetic.plane.wing_lod0");
    let far = node_entity(&world, "synthetic.plane.wing_lod1");
    let tail = node_entity(&world, "synthetic.plane.tail");
    assert_eq!(
        cs_app::animation::visibility::composed_visibility_verdict(&world, hatch)
            .draw()
            .label(),
        "hidden by animation",
        "the clip hides the hatch at its authored tick"
    );
    assert_eq!(
        cs_app::animation::visibility::composed_visibility_verdict(&world, near)
            .draw()
            .label(),
        "drawn",
        "the near band is the one this distance selects"
    );
    assert_eq!(
        cs_app::animation::visibility::composed_visibility_verdict(&world, far)
            .draw()
            .label(),
        "lod culled",
        "the far band is not, and is never presented at the same time as the near one"
    );
    assert_eq!(
        cs_app::animation::visibility::composed_visibility_verdict(&world, tail)
            .draw()
            .label(),
        "disabled",
        "damage disabled the tail"
    );

    // One draw, four rows: only the per-instance state could separate them, and
    // it does not, so this is a single batch and the whole question is which of
    // its rows are placed.
    let frame = batched(&scene);
    assert_eq!(
        frame.batch_count(),
        1,
        "the four parts share one geometry, one state and one paint"
    );
    assert_eq!(frame.instance_count(), 4);

    let report = sync(&mut world, &scene);

    assert_eq!(
        report.visibility,
        VisibilityReport {
            drawn: 1,
            hidden_by_animation: 1,
            lod_culled: 1,
            disabled: 1,
            no_record: 0,
        },
        "every row is counted under the verdict that decided it"
    );
    assert_eq!(report.placed, 1);
    assert_eq!(
        placed_items(&world),
        ["a.wing_near"],
        "the drawn row is placed and the other three are not"
    );
    assert_eq!(
        batch_entities(&world),
        1,
        "the batch itself still draws one row"
    );
}

/// The draw decision follows the **animation record**, not a snapshot taken when
/// the frame was built: the same frame, synced twice around the clip's show
/// tick, places the hatch once the clip shows it again.
///
/// This is what fails if the consumer stops reading the world's animation record
/// (or reads it once per frame instead of per sync).
#[test]
fn accept_f20_c_draw_the_clip_showing_a_node_again_places_it_again() {
    let scene = scene();
    let mut world = live_world(NEAR_METRES);
    play_breakable_at(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);

    assert_eq!(
        placed_items(&world),
        Vec::<String>::new(),
        "nothing has been synced yet"
    );
    let hidden = sync(&mut world, &scene);
    assert_eq!(hidden.visibility.hidden_by_animation, 1);
    assert_eq!(placed_items(&world), ["a.wing_near"]);

    // The clip's authored show tick, through the same wired fixed-tick entry.
    world.insert_resource(CommittedSessionTick::new(Tick(
        SYNTHETIC_BREAKABLE_SHOWN_TICK,
    )));
    advance_animation_on_session_tick(&mut world);

    let shown = sync(&mut world, &scene);
    assert_eq!(
        shown.visibility,
        VisibilityReport {
            drawn: 2,
            hidden_by_animation: 0,
            lod_culled: 1,
            disabled: 1,
            no_record: 0,
        },
        "the hatch's own record is what changed, and nothing else"
    );
    assert_eq!(
        placed_items(&world),
        ["a.hatch", "a.wing_near"],
        "the hatch is placed again and the culled and disabled rows stay off screen"
    );
}

/// A withheld row leaves nothing behind: a batch whose every row the verdict
/// keeps off the screen is not spawned, and the entity a previous frame placed
/// for it is released.
#[test]
fn accept_f20_c_draw_a_wholly_withheld_batch_is_released_not_left_drawn() {
    let scene = scene();
    let mut world = live_world(NEAR_METRES);
    // Two of the four parts are presented at this distance: the hatch (no clip
    // is playing) and the near band.
    let first = sync(&mut world, &scene);
    assert_eq!(first.placed, 2);
    assert_eq!(batch_entities(&world), 1);

    // Damage now names every part of the airframe, so every row is `Disabled`.
    let mut damage = AirframeDamageState::new();
    for part in drawn_parts() {
        damage.destroy(part.1);
    }
    world.insert_resource(damage);
    apply_airframe_damage(&mut world);
    run_lod_pass(&mut world);

    let second = sync(&mut world, &scene);
    assert_eq!(
        second.visibility,
        VisibilityReport {
            drawn: 0,
            hidden_by_animation: 0,
            lod_culled: 0,
            disabled: 4,
            no_record: 0,
        },
        "damage outranks both other reasons for all four rows"
    );
    assert_eq!(second.placed, 0);
    assert_eq!(
        second.spawned, 0,
        "a batch with no drawn row is not spawned"
    );
    assert!(
        second.released >= 1,
        "the batch the previous frame placed is released, not left drawn"
    );
    assert_eq!(placed_items(&world), Vec::<String>::new());
    assert_eq!(
        batch_entities(&world),
        0,
        "no batch entity survives a frame that draws none of its rows"
    );
}

/// The distance is a presentation-only fact, so a fractional frame cannot invent
/// a draw state no fixed tick produced: interpolating between two committed
/// poses changes nothing about what is placed, and the LOD pass is what moves a
/// row between `Drawn` and `LodCulled`.
#[test]
fn accept_f20_c_draw_interpolation_between_ticks_never_re_decides_a_draw() {
    use cs_app::animation::presentation::interpolated_pose;
    use cs_sim::animated_object::PoseSample;
    use cs_types::space::Quaternion;

    let scene = scene();
    let mut world = live_world(NEAR_METRES);
    play_breakable_at(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    let before = sync(&mut world, &scene);
    let placed_before = placed_items(&world);

    // A presentation frame between the last committed tick and the next one.
    let previous = PoseSample::try_new(Quaternion::IDENTITY, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])
        .expect("an identity pose is representable");
    let next = PoseSample::try_new(Quaternion::IDENTITY, [4.0, 0.0, 0.0], [1.0, 1.0, 1.0])
        .expect("a translated pose is representable");
    let blended = interpolated_pose(&previous, &next, 0.5).expect("a fractional blend is valid");

    let after = sync(&mut world, &scene);
    assert_eq!(
        after.visibility, before.visibility,
        "a blended pose is presentation only: it names no draw state"
    );
    assert_eq!(
        placed_items(&world),
        placed_before,
        "and the draws on screen are the ones the fixed ticks decided"
    );
    assert_ne!(
        blended.translation_m()[0],
        previous.translation_m()[0],
        "the blend really did move the pose, so the sync above saw a changed frame"
    );

    // The distance pass is what moves a row: the far band is presented there and
    // the near band is not, and the animated and disabled rows do not move.
    set_distance(&mut world, FAR_METRES);
    run_lod_pass(&mut world);
    let far = sync(&mut world, &scene);
    assert_eq!(
        far.visibility,
        VisibilityReport {
            drawn: 1,
            hidden_by_animation: 1,
            lod_culled: 1,
            disabled: 1,
            no_record: 0,
        },
        "the LOD reason moved to the other band and nothing else moved"
    );
    assert_eq!(
        placed_items(&world),
        ["a.wing_far"],
        "exactly one band of the group is on screen, never both"
    );
}

/// Absence of a presentation record is not a cull: a world with no live scene
/// has no evidence about any part, so every row is placed and the gap is
/// counted rather than looking like a decision.
#[test]
fn accept_f20_c_draw_without_a_live_scene_every_row_is_placed_and_counted() {
    let scene = scene();
    // The asset stores and the applied profile, but no loaded airframe scene.
    let mut world = render_world();
    let report = sync(&mut world, &scene);

    assert_eq!(
        report.visibility,
        VisibilityReport {
            drawn: 0,
            hidden_by_animation: 0,
            lod_culled: 0,
            disabled: 0,
            no_record: 4,
        },
        "no record is a reportable state, not a draw decision"
    );
    assert_eq!(report.placed, 4);
    assert_eq!(placed_items(&world).len(), 4);

    // And with a live scene, a part the scene does not contain is the same case:
    // the producer named a node nothing owns, so nothing withholds that row.
    let mut live = live_world(NEAR_METRES);
    let unknown = node_id("synthetic.other.not_in_this_scene");
    let mut foreign = scene.submitted();
    foreign[0].part = PartRef::Known(&unknown);
    let frame = batched(&scene);
    let report = sync_frame(&mut live, &foreign, &frame, SESSION, &scene.runtime)
        .expect("a part the live scene does not contain is still syncable");
    assert_eq!(
        report.visibility,
        VisibilityReport {
            // The hatch row is the one whose part no live entity carries, so it
            // is placed on no evidence rather than withheld on none.
            drawn: 1,
            hidden_by_animation: 0,
            lod_culled: 1,
            disabled: 1,
            no_record: 1,
        },
        "the row whose part no live entity carries is counted, not culled"
    );
    assert_eq!(
        report.placed, 2,
        "it is placed, and so is the one band this distance selects"
    );
    assert_eq!(
        placed_items(&live),
        ["a.hatch", "a.wing_near"],
        "and it is on screen, with the culled and disabled rows still off"
    );
}

/// The render profile is an enhancement switch, and it reaches no visibility
/// rule (F17 non-negotiable 5): applying a different profile changes what the
/// camera, light and window are told, and not one placement.
#[test]
fn accept_f20_c_draw_a_profile_switch_never_reaches_the_visibility_rule() {
    let scene = scene();
    let mut world = live_world(NEAR_METRES);
    play_breakable_at(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);

    let faithful = sync(&mut world, &scene);
    let placed_before = placed_items(&world);

    let enhanced_profile = RenderProfile::faithful()
        .with(Enhancement::ShadowMapping)
        .expect("shadow mapping is an expressible enhancement");
    world.insert_resource(RenderProfileRequest::set(SESSION, enhanced_profile.clone()));
    process_render_profile_request(&mut world);

    let submitted = scene.submitted();
    let enhanced_frame = batch_frame(
        &submitted,
        &scene.plan,
        &scene.visuals,
        &enhanced_profile,
        TICK,
    )
    .expect("the fixture scene batches under the enhanced profile");
    let enhanced = sync_frame(
        &mut world,
        &submitted,
        &enhanced_frame,
        SESSION,
        &scene.runtime,
    )
    .expect("the enhanced frame syncs under the profile it was applied with");

    assert_eq!(
        enhanced.visibility, faithful.visibility,
        "an enhancement cannot reach a visibility rule"
    );
    assert_eq!(
        placed_items(&world),
        placed_before,
        "the same rows are on screen under the enhanced profile"
    );
}

/// The reason codes the report carries are the composed verdict's own, so a
/// report can never disagree with the verdict about why a row is missing.
#[test]
fn accept_f20_c_draw_a_withheld_row_reports_the_composed_verdicts_own_reason() {
    let mut world = live_world(NEAR_METRES);
    play_breakable_at(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);

    let reason = |entity: Entity| {
        let node = recover_node(&world, entity);
        cs_app::render::visibility::row_draw(&world, PartRef::Known(&node))
            .reason()
            .expect("a withheld row carries a reason")
    };
    assert_eq!(
        reason(node_entity(&world, "synthetic.plane.hatch")),
        "hidden by animation"
    );
    assert_eq!(
        reason(node_entity(&world, "synthetic.plane.wing_lod1")),
        "lod culled"
    );
    assert_eq!(
        reason(node_entity(&world, "synthetic.plane.tail")),
        "disabled"
    );

    // The drawn row carries none, and a row with no record is not "withheld
    // for an unknown reason" either: it is placed and it has no reason code.
    let near = recover_node(&world, node_entity(&world, "synthetic.plane.wing_lod0"));
    assert_eq!(
        cs_app::render::visibility::row_draw(&world, PartRef::Known(&near)).reason(),
        None
    );
    let unknown = node_id("synthetic.other.nowhere");
    assert_eq!(
        cs_app::render::visibility::row_draw(&world, PartRef::Known(&unknown)),
        cs_app::render::visibility::decide(None),
        "absence of a record is its own state, not a withheld draw"
    );
}

/// The scene node identity a fixture entity was imported under, recovered
/// through the live scene's own record.
fn recover_node(world: &World, entity: Entity) -> SceneNodeId {
    world
        .get_resource::<LiveAirframeScene>()
        .expect("the fixture scene is live")
        .import()
        .entities()
        .find_map(|(id, owned)| (owned == entity).then(|| id.clone()))
        .expect("every fixture entity is a node of the live scene")
}
