//! Acceptance scenarios F20-C.03: the visibility channel applied to the ECS,
//! the ownership decision against LOD and damage, and the authored
//! destruction transition it has to survive.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C` (non-negotiable behavior 3). Task test prefix:
//! `accept_f20_c_03_`.
//!
//! Every test here drives the **production path** — the declared
//! [`AnimationClip`] through `play_animation` and the fixed-tick entry
//! ([`advance_animation_on_session_tick`], the system
//! `AnimationSchedulePlugin` installs) — and reads the **real** verdict through
//! [`composed_visibility_verdict`]. None of them asserts the evaluator's
//! `Visibility` alone (that is F20-A, already covered) and none of them writes
//! the public `NodePresentation` (this slice's design is that nothing outside
//! F11-C's LOD system does).
//!
//! The three discriminating failures this stage fixes, one per test group:
//!
//! * **A hide that a distance change erases.** The clip hides the node at an
//!   authored tick, and then the **real** `select_lod_presentation` runs and
//!   rewrites `NodePresentation` at a far and then a near distance. The
//!   composed verdict must stay not drawn with no collider across every
//!   rewrite. A design that stored the verdict, or wrote the shared field, or
//!   ordered against the LOD pass by hand, fails here.
//! * **A destroyed node that comes back.** The clip **loops** and re-shows the
//!   node on every pass, so a looping track is constantly trying to restore it.
//!   With the damage marker set, the verdict must be `Disabled` and never drawn
//!   at every tick of four passes, at two distances, and after the instance is
//!   torn down — while the same teardown on an undamaged node does release the
//!   record and hand the node back (F20 non-negotiable behavior 3). It must also
//!   hold in the frame the damage lands, before the LOD pass has folded the
//!   marker: F11-C's chained order leaves that window, and a verdict read inside
//!   it is a wrong answer rather than a late one.
//! * **A write that never verified.** Of four entities — the bound one, one
//!   stamped by a superseded scene load, one with no binding and one bound to a
//!   node the clip does not drive — exactly the bound one receives the record
//!   and the verdict.
//!
//! Damage's marker is inserted directly instead of being driven through
//! F11-C's `apply_airframe_damage`: that pass needs the `LiveAirframeScene` its
//! own load path publishes, and `crates/cs_app/src/scene.rs` is read-only for
//! this task. The marker it writes is the interface this composition reads, and
//! the LOD pass — which *is* run here, in the order the real schedule runs the
//! two — is what folds the marker, and its ancestors, into the presentation
//! record the composition reads.
//!
//! Every value here is newly authored fixture data, not measured original game
//! data: the original animation containers are still undecoded (F13), and
//! F20-D keeps the original-family validation gate.

use bevy::ecs::lifecycle::Insert;
use bevy::ecs::observer::On;
use bevy::ecs::schedule::Schedule;
use bevy::ecs::world::World;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, ResMut, Resource};
use cs_app::animation::lower::lower_clip;
use cs_app::animation::{
    AnimatedNodeBinding, AnimationInstance, AnimationLog, AnimationPlayback, ColliderVerdict,
    CommittedSessionTick, DrawVerdict, NodeAnimatedVisibility, VisibilityVerdict,
    advance_animation, advance_animation_on_session_tick, composed_visibility_verdict,
    play_animation, stop_animation,
};
use cs_app::scene::{
    LodDistance, NodeDisabled, NodeLodVariant, NodePresentation, NodeVisualTransform,
    PresentationState, SceneGeneration, SceneNodeBinding, select_lod_presentation,
};
use cs_content::animation::{
    AnimationChannel, AnimationClip, LoopMode, SYNTHETIC_BREAKABLE_BREAK_TICK,
    SYNTHETIC_BREAKABLE_DURATION, SYNTHETIC_BREAKABLE_HIDDEN_TICK, SYNTHETIC_BREAKABLE_MARKER,
    SYNTHETIC_BREAKABLE_NODE, SYNTHETIC_BREAKABLE_SHOWN_TICK, VisibilityChannel, VisibilityKey,
    declared_synthetic_breakable_clip,
};
use cs_content::scene::{LodInfo, NodeVisibility};
use cs_sim::animated_object::{AnimatedObject, NodeChannel, Visibility};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Resolved};
use cs_types::net::SessionId;
use cs_types::space::Meters;

// -------------------------------------------------------------- helpers ---

/// The session the playback stamps its events with.
const SESSION: u64 = 31;
/// The near band of the synthetic hatch's LOD group.
const NEAR_BAND: (f64, f64) = (0.0, 100.0);
/// The far band of that group, so the hatch is the culled variant there.
const FAR_BAND: (f64, f64) = (100.0, 500.0);
/// A viewer distance inside the near band.
const NEAR_METRES: f64 = 50.0;
/// A viewer distance inside the far band.
const FAR_METRES: f64 = 200.0;
/// The far band's node: the same part at another distance.
const FAR_NODE: &str = "synthetic.plane.hatch.lod1";
/// A mesh under the hatch, which no clip ever names.
const CHILD_NODE: &str = "synthetic.plane.hatch.frame";

fn node(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::SceneNode, key).expect("valid content id")
}

fn track(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::AnimationTrack, key).expect("valid content id")
}

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

fn generation() -> SceneGeneration {
    SceneGeneration::default().next()
}

fn band(min: f64, max: f64) -> NodeLodVariant {
    NodeLodVariant::new(LodInfo {
        level: false,
        range_min: Meters(min),
        range_max: Meters(max),
    })
    .expect("the band range is usable")
}

/// A world with the playback and the viewer distance the LOD system reads.
fn world_at(distance: f64) -> World {
    let mut world = World::new();
    world.insert_resource(AnimationPlayback::new(session(SESSION)));
    world.insert_resource(VisibilityWrites::default());
    set_distance(&mut world, distance);
    world
}

/// Counts how many times the production path actually **inserted** the applied
/// visibility record.
///
/// A value comparison cannot tell "written again with the same value" from
/// "not written", which is the whole content of the idempotence rule; Bevy's
/// `Insert` trigger fires on every insert and on nothing else, so this counter
/// is the direct observation of it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
struct VisibilityWrites(u32);

/// Registers the counter. Called before anything plays, so the very first
/// insert is counted too.
fn count_visibility_writes(world: &mut World) {
    world.add_observer(
        |_insert: On<Insert, NodeAnimatedVisibility>, mut writes: ResMut<VisibilityWrites>| {
            writes.0 += 1;
        },
    );
}

fn writes(world: &World) -> u32 {
    world.resource::<VisibilityWrites>().0
}

fn set_distance(world: &mut World, distance: f64) {
    world.insert_resource(LodDistance::new(Meters(distance)).expect("a usable distance"));
}

/// Runs the real F11-C LOD pass — the only writer of the presentation record in
/// the crate.
fn run_lod_pass(world: &mut World) {
    let mut schedule = Schedule::default();
    schedule.add_systems(select_lod_presentation);
    schedule.run(world);
}

fn presentation(world: &World, entity: Entity) -> Option<PresentationState> {
    world
        .get::<NodePresentation>(entity)
        .map(|presentation| presentation.0)
}

/// Spawns one LOD band: the stable scene binding, the initial presentation
/// record the loader gives it, its composed world pose and the band itself.
fn spawn_band(world: &mut World, key: &str, at: SceneGeneration) -> Entity {
    world
        .spawn((
            SceneNodeBinding {
                node: node(key),
                generation: at,
            },
            NodePresentation(PresentationState::Drawn),
            NodeVisualTransform(GlobalTransform::IDENTITY),
        ))
        .id()
}

/// Spawns the hatch's two LOD bands: the animated node itself and the far
/// variant of the same part.
fn spawn_hatch_group(world: &mut World, at: SceneGeneration) -> Entity {
    let hatch = spawn_band(world, SYNTHETIC_BREAKABLE_NODE, at);
    let far = spawn_band(world, FAR_NODE, at);
    world
        .entity_mut(hatch)
        .insert(band(NEAR_BAND.0, NEAR_BAND.1));
    world.entity_mut(far).insert(band(FAR_BAND.0, FAR_BAND.1));
    hatch
}

/// Spawns a mesh under `parent`: the ancestor case `select_lod_presentation`
/// folds, and which no clip ever names.
fn spawn_child(world: &mut World, key: &str, parent: Entity, at: SceneGeneration) -> Entity {
    world
        .spawn((
            SceneNodeBinding {
                node: node(key),
                generation: at,
            },
            NodePresentation(PresentationState::Drawn),
            NodeVisualTransform(GlobalTransform::IDENTITY),
            ChildOf(parent),
        ))
        .id()
}

/// Binds an entity to one node of one instance of the breakable clip.
fn bind(world: &mut World, bound: &ContentId, at: SceneGeneration) -> Entity {
    world
        .spawn(AnimatedNodeBinding {
            clip: track("synthetic.breakable"),
            node: bound.clone(),
            instance: instance(1),
            generation: at,
        })
        .id()
}

/// Binds the scene node entity itself to one node of one instance of the
/// breakable clip — the shape the spawn wiring produces, where the animated
/// node and its scene node are the same entity.
fn bind_node(world: &mut World, entity: Entity, bound: &ContentId, at: SceneGeneration) {
    world.entity_mut(entity).insert(AnimatedNodeBinding {
        clip: track("synthetic.breakable"),
        node: bound.clone(),
        instance: instance(1),
        generation: at,
    });
}

/// Starts the declared breakable clip on the hatch's node, at tick 0.
fn play_breakable(world: &mut World, at: SceneGeneration) {
    play_animation(
        world,
        &declared_synthetic_breakable_clip(),
        instance(1),
        at,
        Tick(0),
    )
    .expect("the declared breakable clip starts");
}

/// Commits a session tick and runs the wired fixed-tick entry, the way the
/// session driver and `AnimationSchedulePlugin` do.
fn commit(world: &mut World, tick: u64) {
    world.insert_resource(CommittedSessionTick::new(Tick(tick)));
    advance_animation_on_session_tick(world);
}

fn applied_visibility(world: &World, entity: Entity) -> Option<Visibility> {
    world
        .get::<NodeAnimatedVisibility>(entity)
        .map(NodeAnimatedVisibility::visibility)
}

fn verdict(world: &World, entity: Entity) -> VisibilityVerdict {
    composed_visibility_verdict(world, entity)
}

fn drain(world: &mut World) -> AnimationLog {
    match world.get_resource_mut::<AnimationLog>() {
        Some(mut log) => log.drain(),
        None => AnimationLog::new(),
    }
}

// ------------------------------------------------ 1. the declared fixture ---

/// The fixture is the production input: a declared clip with a visibility
/// channel, one gameplay marker, and the evaluator's own verdict at the break
/// tick. Asserting the evaluator here is what makes the ECS assertions below
/// meaningful — the value the ECS carries is the one the fixed-tick evaluator
/// produced, not one a test invented.
#[test]
fn accept_f20_c_03_the_breakable_fixture_drives_the_production_lowering() {
    let declared: AnimationClip = declared_synthetic_breakable_clip();

    assert_eq!(declared.id(), &track("synthetic.breakable"));
    assert_eq!(
        declared.origin(),
        &Origin::SyntheticFixture,
        "the fixture can never be mistaken for retail content"
    );
    assert_eq!(declared.duration_ticks(), SYNTHETIC_BREAKABLE_DURATION);
    assert_eq!(
        declared.loop_mode(),
        LoopMode::Loop,
        "the clip loops, so every pass re-shows the node: the destruction stress case"
    );

    let [AnimationChannel::Visibility(VisibilityChannel { target, keys })] = declared.channels()
    else {
        panic!("the breakable fixture carries exactly one visibility channel");
    };
    assert_eq!(target.as_content_id(), &node(SYNTHETIC_BREAKABLE_NODE));
    assert_eq!(
        *keys,
        vec![
            VisibilityKey {
                tick: 0,
                visibility: NodeVisibility::Visible,
            },
            VisibilityKey {
                tick: SYNTHETIC_BREAKABLE_HIDDEN_TICK,
                visibility: NodeVisibility::Hidden,
            },
            VisibilityKey {
                tick: SYNTHETIC_BREAKABLE_SHOWN_TICK,
                visibility: NodeVisibility::Visible,
            },
        ]
    );
    let [marker] = declared.markers() else {
        panic!("the fixture carries exactly one marker");
    };
    assert_eq!(marker.tick, SYNTHETIC_BREAKABLE_BREAK_TICK);
    assert_eq!(marker.key, SYNTHETIC_BREAKABLE_MARKER);
    let Resolved::Known(effect) = &marker.effect else {
        panic!("the fixture's marker effect is a known gameplay cue");
    };
    assert!(
        effect.value.is_gameplay(),
        "the break is a gameplay cue, so it fires once per activation however often the \
         visibility cycles"
    );

    // The production lowering carries the channel into the runtime record, and
    // the fixed-tick evaluator is what produces the value the ECS applies.
    let runtime = lower_clip(&declared).expect("the declared clip lowers");
    assert_eq!(runtime.id(), declared.id());
    assert!(
        runtime.channels().iter().any(
            |channel| matches!(channel, NodeChannel::Visibility { target, .. }
                if target == &node(SYNTHETIC_BREAKABLE_NODE))
        ),
        "the lowered clip still drives the hatch's visibility"
    );

    let mut object = AnimatedObject::new(runtime, session(SESSION), 1);
    let tick = SYNTHETIC_BREAKABLE_HIDDEN_TICK;
    object
        .advance_to(tick, Tick(tick))
        .expect("the break tick moves forward");
    let at_break = object.states();
    let hidden = at_break
        .get(&node(SYNTHETIC_BREAKABLE_NODE))
        .expect("the clip drives the hatch");
    assert_eq!(hidden.visibility(), Some(Visibility::Hidden));
    assert!(
        !hidden.collider_enabled(),
        "F20-A's designed rule: a hidden node carries no collider"
    );
    drop(at_break);

    let tick = SYNTHETIC_BREAKABLE_SHOWN_TICK;
    object
        .advance_to(tick, Tick(tick))
        .expect("the show tick moves forward");
    let at_show = object.states();
    assert_eq!(
        at_show
            .get(&node(SYNTHETIC_BREAKABLE_NODE))
            .and_then(|state| state.visibility()),
        Some(Visibility::Visible)
    );
}

// ------------------------------------------- 2. the hide survives a LOD pass ---

/// The minimum scenario: hiding a node at its authored tick reaches the
/// presentation consumers, and **stays** hidden when the real LOD system
/// rewrites the presentation record underneath it.
///
/// This is the failure the ownership decision exists to prevent. The assertions
/// that `NodePresentation` itself changed (`LodCulled` at the far distance,
/// `Drawn` again at the near one) are what make the test discriminating: the
/// shared field really was rewritten between the two verdicts, and the verdict
/// followed the animation's fact through it.
#[test]
fn accept_f20_c_03_a_hidden_node_stays_hidden_across_a_lod_selection_pass() {
    let at = generation();
    let mut world = world_at(NEAR_METRES);
    let hatch = spawn_hatch_group(&mut world, at);
    bind_node(&mut world, hatch, &node(SYNTHETIC_BREAKABLE_NODE), at);
    play_breakable(&mut world, at);
    run_lod_pass(&mut world);
    assert_eq!(
        presentation(&world, hatch),
        Some(PresentationState::Drawn),
        "the near distance presents this band"
    );

    // The authored break tick, through the wired fixed-tick entry.
    commit(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        applied_visibility(&world, hatch),
        Some(Visibility::Hidden),
        "the visibility channel applied the clip's value to the verified binding"
    );
    let hidden = verdict(&world, hatch);
    assert_eq!(hidden.draw(), DrawVerdict::HiddenByAnimation);
    assert_eq!(hidden.collider(), ColliderVerdict::NoCollider);
    assert!(!hidden.drawn());
    assert_eq!(
        drain(&mut world)
            .events()
            .iter()
            .map(|event| event.marker.as_str())
            .collect::<Vec<_>>(),
        vec![SYNTHETIC_BREAKABLE_MARKER],
        "the gameplay cue fired once, in the tick that broke it"
    );

    // The real LOD pass, at a distance that culls this band: the shared field
    // is rewritten, and the animation's verdict is not lost.
    set_distance(&mut world, FAR_METRES);
    run_lod_pass(&mut world);
    assert_eq!(
        presentation(&world, hatch),
        Some(PresentationState::LodCulled),
        "the far distance presents the other band, so the LOD pass really did rewrite the field"
    );
    let culled = verdict(&world, hatch);
    assert_eq!(
        culled.draw(),
        DrawVerdict::LodCulled,
        "LOD's own reason outranks the clip's, because this is not the band the group chose"
    );
    assert_eq!(
        culled.collider(),
        ColliderVerdict::NoCollider,
        "the clip's hidden fact reaches collision whatever the draw reason is"
    );
    assert!(!culled.drawn());

    // And back to the near distance, where nothing in LOD or damage opposes the
    // node: the clip's own reason is the answer again.
    set_distance(&mut world, NEAR_METRES);
    run_lod_pass(&mut world);
    assert_eq!(presentation(&world, hatch), Some(PresentationState::Drawn));
    let drawn = verdict(&world, hatch);
    assert_eq!(drawn.draw(), DrawVerdict::HiddenByAnimation);
    assert_eq!(drawn.collider(), ColliderVerdict::NoCollider);
    assert!(!drawn.drawn());
}

/// Showing the node again is the same decision read the other way: the record
/// follows the clip, and the clip stops deciding collision the moment it stops
/// hiding — at both distances, so a culled band is not a second cull.
#[test]
fn accept_f20_c_03_showing_the_node_again_is_the_symmetric_verdict() {
    let at = generation();
    let mut world = world_at(NEAR_METRES);
    let hatch = spawn_hatch_group(&mut world, at);
    bind_node(&mut world, hatch, &node(SYNTHETIC_BREAKABLE_NODE), at);
    play_breakable(&mut world, at);

    commit(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert!(!verdict(&world, hatch).drawn());
    let _ = drain(&mut world);

    commit(&mut world, SYNTHETIC_BREAKABLE_SHOWN_TICK);
    assert_eq!(
        applied_visibility(&world, hatch),
        Some(Visibility::Visible),
        "the re-show key of the loop is applied like any other"
    );
    let shown = verdict(&world, hatch);
    assert_eq!(shown.draw(), DrawVerdict::Drawn);
    assert_eq!(
        shown.collider(),
        ColliderVerdict::Undecided,
        "a node the clip does not hide is nobody's guess: the authored collision role decides"
    );
    assert!(shown.drawn());
    assert!(
        drain(&mut world).is_empty(),
        "the loop's second pass does not re-fire the one-shot gameplay cue"
    );

    // A distance change culls it again, and still says nothing about collision.
    set_distance(&mut world, FAR_METRES);
    run_lod_pass(&mut world);
    let culled = verdict(&world, hatch);
    assert_eq!(culled.draw(), DrawVerdict::LodCulled);
    assert_eq!(culled.collider(), ColliderVerdict::Undecided);
    assert!(!culled.drawn());
}

// ------------------------------------------------- 3. a destroyed node stays ---

/// Non-negotiable behavior 3: a destroyed node is never re-drawn or re-enabled
/// by an animation loop or an LOD/distance change.
///
/// The clip re-shows the node on **every** pass, so this walks four passes
/// tick by tick and checks the verdict against the clip's own value: at the
/// re-show ticks the clip says `Visible` and the verdict still says `Disabled`.
/// The mesh under the destroyed hatch is checked too — `select_lod_presentation`
/// folds the marker through the hierarchy, so the composition needs no walk of
/// its own.
#[test]
fn accept_f20_c_03_a_destroyed_node_is_never_restored_by_a_loop_pass_or_an_lod_pass() {
    let at = generation();
    let mut world = world_at(NEAR_METRES);
    let hatch = spawn_hatch_group(&mut world, at);
    let frame = spawn_child(&mut world, CHILD_NODE, hatch, at);
    bind_node(&mut world, hatch, &node(SYNTHETIC_BREAKABLE_NODE), at);
    play_breakable(&mut world, at);

    // Damage's marker, then the LOD pass that folds it (the order the real
    // schedule runs the two in).
    world.entity_mut(hatch).insert(NodeDisabled);
    run_lod_pass(&mut world);
    assert_eq!(
        presentation(&world, hatch),
        Some(PresentationState::Disabled)
    );
    assert_eq!(
        presentation(&world, frame),
        Some(PresentationState::Disabled),
        "the marker reaches the mesh mounted under the destroyed part"
    );

    let mut re_shown = 0;
    for tick in 1..=SYNTHETIC_BREAKABLE_DURATION * 4 {
        commit(&mut world, tick);
        let clip = applied_visibility(&world, hatch);
        if clip == Some(Visibility::Visible) {
            re_shown += 1;
        }
        let now = verdict(&world, hatch);
        assert_eq!(
            now.draw(),
            DrawVerdict::Disabled,
            "session tick {tick}: damage outranks the clip, whatever the clip evaluates (it says \
             {clip:?})"
        );
        assert!(
            !now.drawn(),
            "session tick {tick}: a destroyed node is not drawn"
        );
        assert!(
            world.get::<NodeDisabled>(hatch).is_some(),
            "session tick {tick}: the animation path never touches the damage marker"
        );
    }
    assert!(
        re_shown >= 4,
        "the loop really did try to re-show the node on every pass ({re_shown} times), and the \
         verdict never followed it"
    );
    assert!(
        !verdict(&world, frame).drawn(),
        "the mesh under the destroyed part is not drawn either"
    );
    assert_eq!(
        drain(&mut world)
            .events()
            .iter()
            .filter(|event| event.marker == SYNTHETIC_BREAKABLE_MARKER)
            .count(),
        1,
        "four loop passes still fire the one-shot gameplay cue exactly once"
    );

    // A distance change cannot bring it back.
    for distance in [FAR_METRES, NEAR_METRES] {
        set_distance(&mut world, distance);
        run_lod_pass(&mut world);
        assert_eq!(
            verdict(&world, hatch).draw(),
            DrawVerdict::Disabled,
            "at {distance} m a destroyed node stays destroyed"
        );
    }

    // And the teardown does not either: releasing what the instance applied
    // leaves a destroyed node alone (its sibling test hands a live one back).
    assert!(stop_animation(
        &mut world,
        &track("synthetic.breakable"),
        instance(1)
    ));
    assert!(
        world.get::<NodeAnimatedVisibility>(hatch).is_none(),
        "the teardown released the applied record"
    );
    assert_eq!(
        verdict(&world, hatch).draw(),
        DrawVerdict::Disabled,
        "a destroyed node stays destroyed after the clip is gone"
    );
}

/// The destruction half of the verdict does not wait for the LOD pass.
///
/// F11-C's own systems are chained damage-then-LOD in one frame, and it calls
/// the consequence of any other order "a late update, never a wrong one" — true
/// of the presentation record, which its own pass recomputes every frame, but
/// not of a verdict a consumer **reads** in the frame the damage landed. Between
/// `apply_airframe_damage` writing the marker and that frame's
/// `select_lod_presentation` folding it, the record still says `Drawn`, so a
/// composition that read only the record would report a node destroyed earlier
/// in the same frame as drawn. This is the frame in which a looping clip's
/// re-show tick does most of its damage, and it is asserted here **without**
/// running the LOD pass in between.
#[test]
fn accept_f20_c_03_a_destroyed_node_is_never_drawn_before_the_lod_pass_folds_it() {
    let at = generation();
    let mut world = world_at(NEAR_METRES);
    let hatch = spawn_hatch_group(&mut world, at);
    bind_node(&mut world, hatch, &node(SYNTHETIC_BREAKABLE_NODE), at);
    play_breakable(&mut world, at);
    run_lod_pass(&mut world);

    // The clip shows the node (tick 0's key), so without damage the verdict is
    // "drawn" — the state a stale record would keep reporting.
    commit(&mut world, 0);
    assert_eq!(verdict(&world, hatch).draw(), DrawVerdict::Drawn);
    let _ = drain(&mut world);

    // Damage lands in this frame; the LOD pass has not run since.
    world.entity_mut(hatch).insert(NodeDisabled);
    assert_eq!(
        presentation(&world, hatch),
        Some(PresentationState::Drawn),
        "the presentation record is still last frame's, which is the window under test"
    );
    assert_eq!(
        verdict(&world, hatch).draw(),
        DrawVerdict::Disabled,
        "a node destroyed in this frame is not drawn in this frame"
    );

    // And the same window with the clip hiding it: damage still outranks the
    // clip's own reason, and the clip's hidden fact still reaches collision.
    commit(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    let hidden_and_destroyed = verdict(&world, hatch);
    assert_eq!(applied_visibility(&world, hatch), Some(Visibility::Hidden));
    assert_eq!(hidden_and_destroyed.draw(), DrawVerdict::Disabled);
    assert_eq!(hidden_and_destroyed.collider(), ColliderVerdict::NoCollider);
    let _ = drain(&mut world);

    // The loop's re-show tick, still inside the window: the clip asks for the
    // node back and the verdict refuses.
    commit(&mut world, SYNTHETIC_BREAKABLE_SHOWN_TICK);
    assert_eq!(applied_visibility(&world, hatch), Some(Visibility::Visible));
    assert_eq!(
        verdict(&world, hatch).draw(),
        DrawVerdict::Disabled,
        "the re-show key is applied to the record and refused by the verdict"
    );

    // Once the pass does run, the fold agrees with what the marker already said,
    // and the mesh under the part is disabled with it.
    let frame = spawn_child(&mut world, CHILD_NODE, hatch, at);
    run_lod_pass(&mut world);
    assert_eq!(
        presentation(&world, hatch),
        Some(PresentationState::Disabled)
    );
    assert_eq!(
        presentation(&world, frame),
        Some(PresentationState::Disabled),
        "the ancestor fold is F11-C's, and it reaches the same verdict"
    );
    assert_eq!(verdict(&world, hatch).draw(), DrawVerdict::Disabled);

    // The repair direction is deliberately not the same, and the difference is
    // the point: a marker removal is F11-C's own convergent recompute, so the
    // presentation record still says `Disabled` until that pass runs again. A
    // destruction may never wait for a pass (a destroyed node that is drawn for
    // a frame is a wrong frame), while a repair one frame late is the late
    // update F11-C already accepts for its own field. The composition reports
    // what the records say and invents neither.
    world.entity_mut(hatch).remove::<NodeDisabled>();
    assert_eq!(
        verdict(&world, hatch).draw(),
        DrawVerdict::Disabled,
        "the record has not been recomputed since the repair, and this composition does not \\
         recompute another stage's record"
    );
    assert!(
        world.get::<NodeDisabled>(hatch).is_none(),
        "the repair is the damage pass's to make; the animation never removes the marker"
    );
    run_lod_pass(&mut world);
    let repaired = verdict(&world, hatch);
    assert_eq!(presentation(&world, hatch), Some(PresentationState::Drawn));
    assert_eq!(
        applied_visibility(&world, hatch),
        Some(Visibility::Visible),
        "the clip's last reached key is the re-show, so it no longer hides the node"
    );
    assert_eq!(
        repaired.draw(),
        DrawVerdict::Drawn,
        "a repaired node is presented again once its record is recomputed"
    );
    assert_eq!(repaired.collider(), ColliderVerdict::Undecided);

    // One more commit into the next pass's hidden key with the marker gone:
    // the clip's own hide is what decides, which is the whole ownership
    // decision in one line.
    commit(
        &mut world,
        SYNTHETIC_BREAKABLE_HIDDEN_TICK + SYNTHETIC_BREAKABLE_DURATION,
    );
    assert_eq!(
        verdict(&world, hatch).draw(),
        DrawVerdict::HiddenByAnimation,
        "with no damage and no cull, the clip's own verdict is the answer"
    );
}

/// The mirror of the teardown: a node nothing has destroyed returns to LOD's
/// own verdict when the instance that hid it is released. Without this half the
/// destruction test could pass with a teardown that never released anything.
#[test]
fn accept_f20_c_03_a_released_instance_hands_a_live_node_back_to_lod() {
    let at = generation();
    let mut world = world_at(NEAR_METRES);
    let hatch = spawn_hatch_group(&mut world, at);
    bind_node(&mut world, hatch, &node(SYNTHETIC_BREAKABLE_NODE), at);
    play_breakable(&mut world, at);
    run_lod_pass(&mut world);

    commit(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    assert_eq!(
        verdict(&world, hatch).draw(),
        DrawVerdict::HiddenByAnimation
    );

    assert!(stop_animation(
        &mut world,
        &track("synthetic.breakable"),
        instance(1)
    ));
    assert!(world.get::<NodeAnimatedVisibility>(hatch).is_none());
    let released = verdict(&world, hatch);
    assert_eq!(released.draw(), DrawVerdict::Drawn);
    assert_eq!(released.collider(), ColliderVerdict::Undecided);
    assert!(released.drawn());

    // A later advance of the same track drives nothing: the instance is gone.
    advance_animation(&mut world, Tick(SYNTHETIC_BREAKABLE_HIDDEN_TICK + 1));
    assert!(world.get::<NodeAnimatedVisibility>(hatch).is_none());
    assert!(verdict(&world, hatch).drawn());
}

// ------------------------------------------------------ 4. the rules hold ---

/// The application is idempotent: a second advance of the same tick writes the
/// component in no observable way at all, publishes nothing, and an LOD pass in
/// between does not restate the record. The last assertion is that a *changed*
/// verdict is written.
#[test]
fn accept_f20_c_03_the_visibility_verdict_is_written_only_when_it_changes() {
    let at = generation();
    let mut world = world_at(NEAR_METRES);
    count_visibility_writes(&mut world);
    let hatch = spawn_hatch_group(&mut world, at);
    bind_node(&mut world, hatch, &node(SYNTHETIC_BREAKABLE_NODE), at);
    play_breakable(&mut world, at);

    commit(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);
    let hidden = verdict(&world, hatch);
    assert_eq!(writes(&world), 1, "the break tick wrote the record once");
    let _ = drain(&mut world);

    // A second advance of the same tick must not write it again — the same
    // value, so nothing is inserted.
    advance_animation(&mut world, Tick(SYNTHETIC_BREAKABLE_HIDDEN_TICK));
    assert_eq!(
        writes(&world),
        1,
        "applying the same visibility twice inserts the record again in no observable way"
    );
    assert_eq!(verdict(&world, hatch), hidden);
    assert!(
        drain(&mut world).is_empty(),
        "a repeat publishes no marker and no refusal"
    );

    // A real LOD pass does not write it either: the composition reads the
    // record, it does not restate it.
    set_distance(&mut world, FAR_METRES);
    run_lod_pass(&mut world);
    assert_eq!(
        writes(&world),
        1,
        "the LOD pass does not touch the animation's record"
    );
    assert_eq!(
        verdict(&world, hatch).collider(),
        ColliderVerdict::NoCollider
    );

    // The next authored change is written, and only then.
    commit(&mut world, SYNTHETIC_BREAKABLE_SHOWN_TICK);
    assert_eq!(applied_visibility(&world, hatch), Some(Visibility::Visible));
    assert_eq!(writes(&world), 2, "a changed verdict is written");
}

/// Only a verified binding is written: of four entities — the bound one, one
/// stamped by a superseded scene load, one with no binding and one bound to a
/// node the clip does not drive — exactly the bound one receives the record and
/// the verdict.
#[test]
fn accept_f20_c_03_an_unbound_or_stale_generation_entity_is_never_written() {
    let at = generation();
    let superseded = SceneGeneration::default(); // a load before the live one
    let mut world = world_at(NEAR_METRES);
    let _hatch = spawn_hatch_group(&mut world, at);

    let bound = bind(&mut world, &node(SYNTHETIC_BREAKABLE_NODE), at);
    let stale = bind(&mut world, &node(SYNTHETIC_BREAKABLE_NODE), superseded);
    let unbound = spawn_band(&mut world, SYNTHETIC_BREAKABLE_NODE, at);
    let undriven = bind(&mut world, &node(FAR_NODE), at);

    play_breakable(&mut world, at);
    commit(&mut world, SYNTHETIC_BREAKABLE_HIDDEN_TICK);

    assert_eq!(
        applied_visibility(&world, bound),
        Some(Visibility::Hidden),
        "the verified binding receives the clip's value"
    );
    for (what, entity) in [
        ("a superseded generation", stale),
        ("an entity with no binding", unbound),
        ("a node the clip does not drive", undriven),
    ] {
        assert_eq!(
            applied_visibility(&world, entity),
            None,
            "{what} is never written"
        );
        assert!(
            verdict(&world, entity).drawn(),
            "{what} keeps the verdict its own records give it"
        );
    }
}
