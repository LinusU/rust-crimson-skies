//! Acceptance scenarios F20-C: the mission/objective layer's consumer of the
//! animation playback's gameplay markers (task #507).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`, non-negotiable behaviors 1, 2 and 5. Task test prefix:
//! `accept_f20_c_`. Shared contracts: `docs/contracts/IDENTITY-CONTENT.md`
//! (session generations, `EventId`) and `docs/contracts/SCRIPT-MISSION.md`
//! ("Mission state", "Objective event ordering").
//!
//! # What these tests pin
//!
//! F20-C's integration slice made markers reach
//! [`AnimationLog`](cs_app::animation::AnimationLog) from a real session and
//! filed the missing consumer as #507. These tests drive the consumer that
//! closes it, end to end and through production code only:
//!
//! * the **real** [`PhysicsSession`](cs_app::physics::PhysicsSession) with the
//!   one-stop [`AnimationPlugin`](cs_app::animation::AnimationPlugin)
//!   installed through its `configure` seam, and the production spawn entry
//!   [`bind_animated_node`](cs_app::animation::bind_animated_node), so the
//!   marker is produced by the wired path rather than injected;
//! * [`MissionMarkerConsumer`], which takes the marker records through
//!   [`AnimationLog::take_markers`](cs_app::animation::AnimationLog::take_markers), and
//!   [`step_mission_with_markers`], which raises the delivered signals into a
//!   real [`ObjectiveSession`](cs_app::objectives::ObjectiveSession) launched
//!   from the F39-C lowering of
//!   [`declared_synthetic_objectives`](cs_content::objectives::declared_synthetic_objectives)
//!   — so "the gameplay effect happened" is an objective reveal in the mission
//!   runtime, not a counter in this test;
//! * the cue → signal table is **declared input**: a cue nobody bound is
//!   refused by name and no signal is invented;
//! * every refusal path is named: an unbound cue, a stale session, a marker
//!   blocked by an unknown effect, a repeated activation, a refused objective
//!   tick, and the ambiguous, empty and reserved rows the table itself refuses.
//!
//! Every value here is newly authored fixture data, never original game data:
//! the original animation containers are still undecoded (F13) and `MarkerEffect`
//! is a designed vocabulary, so nothing here claims an original cue label or an
//! original mission signal.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Mat4;
use bevy::prelude::{GlobalTransform, Vec3};
use cs_app::animation::{
    AnimationInstance, AnimationLog, AnimationPlayback, AnimationPlugin, advance_animation,
    bind_animated_node,
};
use cs_app::mission_markers::{
    MarkerAdmission, MarkerBindingError, MarkerRefusal, MarkerTeardownError, MissionMarkerBinding,
    MissionMarkerBindings, MissionMarkerConsumer, RESERVED_ACTOR_EVENT_SOURCE,
    step_mission_with_markers,
};
use cs_app::objectives::{ObjectiveSession, SessionTick, lower_program};
use cs_app::physics::PhysicsSession;
use cs_app::scene::{NodeVisualTransform, SceneGeneration, SceneNodeBinding};
use cs_content::animation::{
    AnimationChannel, AnimationClip, EventMarker, Interpolation, LoopMode, SYNTHETIC_DOOR_MARKER,
    SYNTHETIC_DOOR_OPEN_TICK, SYNTHETIC_PROPELLER_DURATION, SYNTHETIC_PROPELLER_GAMEPLAY_MARKER,
    MaterialChannel, MaterialKey, SYNTHETIC_PROPELLER_GAMEPLAY_TICK, SYNTHETIC_PROPELLER_NODE,
    SYNTHETIC_PROPELLER_PRESENTATION_MARKER, TransformChannel, TransformKey, TransformSample,
    declared_synthetic_door_clip, declared_synthetic_propeller_clip,
};
use cs_content::objectives::{
    SYNTHETIC_REACHED_WRECK, SYNTHETIC_SECONDARY, declared_synthetic_objectives,
};
use cs_script::ir::SymbolId;
use cs_script::runtime::SessionGeneration;
use cs_sim::animated_object::{AnimationEvent, AnimationEventId};
use cs_sim::objectives::runtime::{
    ObjectiveEventKind, RuntimeError, RuntimeLimits, StopReason, TickInput,
};
use cs_sim::objectives::state::ObjectiveState;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

/// The mission generation the objective session is launched under.
const GEN: SessionGeneration = SessionGeneration(1);

/// The fixture program's secondary objective, as the runtime sees it. Its
/// declared reveal rule is `OnSignal(SYNTHETIC_REACHED_WRECK)`, so a marker
/// that raises that signal performs a real gameplay transition.
const SECONDARY: SymbolId = SymbolId(SYNTHETIC_SECONDARY.0);
/// The signal the fixture program's reveal rule names.
const REACHED_WRECK: SymbolId = SymbolId(SYNTHETIC_REACHED_WRECK.0);
/// The fixture program's other objective: nothing in the program reacts to a
/// marker, so it must stay hidden however many signals are raised.
const PRIMARY: SymbolId = SymbolId(1);
/// A signal the fixture program declares no rule against: raised and reported,
/// and inert — which is what "the program decides what a signal means" means.
const UNDECLARED: SymbolId = SymbolId(61);

/// The gameplay cue labels the F20 synthetic fixtures author. Fixture strings,
/// read out of the declared clips rather than restated as engine vocabulary.
const DOOR_CUE: &str = "synthetic.hangar.door_opened";
const ROTOR_CUE: &str = "synthetic.plane.engine_started";
const BLADE_CUE: &str = "synthetic.plane.blade_pass";

// -------------------------------------------------------------- helpers ---

fn content_id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("valid content id")
}

fn node(key: &str) -> ContentId {
    content_id(ContentKind::SceneNode, key)
}

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn instance(value: u32) -> AnimationInstance {
    AnimationInstance::new(value).expect("a nonzero instance identity")
}

/// A mission host's declared table binding both fixture gameplay cues to
/// `target`.
fn bound_cues(target: SymbolId) -> MissionMarkerBindings {
    MissionMarkerBindings::new([
        MissionMarkerBinding::new(DOOR_CUE, target),
        MissionMarkerBinding::new(ROTOR_CUE, target),
    ])
    .expect("the fixture cues are distinct and bind a live signal")
}

/// The mission host's tick input: its own facts, and no signals of its own,
/// because this slice's claim is that the *marker* supplies them.
fn facts<'a>(tick: u64, committed: u64) -> TickInput<'a> {
    TickInput {
        tick: Tick(tick),
        committed_ticks: committed,
        lifecycles: &[],
        movements: &[],
        signals: &[],
        timer_requests: &[],
        objective_requests: &[],
        terminal_requests: &[],
    }
}

/// The real session the marker is produced by: the Avian fixed loop with the
/// one-stop animation plugin installed through the session's own `configure`
/// seam. The playback resource is inserted after the build, the way a session
/// driver owns it.
fn wired_session(session_value: u64) -> PhysicsSession {
    let mut physics = PhysicsSession::builder()
        .fixed_hz(64)
        .configure(|app| {
            app.add_plugins(AnimationPlugin);
        })
        .build();
    physics
        .world_mut()
        .expect("a fresh session owns a world")
        .insert_resource(AnimationPlayback::new(session(session_value)));
    physics
}

/// A fresh mission session: the F39-C lowering of the declared fixture program.
fn mission() -> ObjectiveSession {
    ObjectiveSession::launch(
        lower_program(&declared_synthetic_objectives()).expect("the fixture program lowers"),
        GEN,
    )
    .expect("the lowered program launches")
}

/// The same mission, launched under a runtime that admits no events at all.
///
/// A designed bound, not a measured original limit: it is the smallest value
/// that makes `ObjectiveRuntime`'s own pre-apply budget check stop every tick,
/// which is what pins what this layer does when the mission applied nothing.
fn bounded_mission() -> ObjectiveSession {
    ObjectiveSession::launch(
        lower_program(&declared_synthetic_objectives())
            .expect("the fixture program lowers")
            .with_limits(RuntimeLimits {
                max_events_per_tick: 0,
                ..RuntimeLimits::default()
            }),
        GEN,
    )
    .expect("the lowered program launches under a tighter bound")
}

/// Spawns one scene node: its stable binding and its composed world pose.
fn spawn_scene_node(world: &mut World, key: &str, generation: SceneGeneration) -> Entity {
    world
        .spawn((
            SceneNodeBinding {
                node: node(key),
                generation,
            },
            NodeVisualTransform(GlobalTransform::from(Mat4::from_translation(Vec3::ZERO))),
        ))
        .id()
}

/// Binds a declared clip to one scene node through the production spawn entry,
/// under the generation that clip will serve.
fn bind_clip(
    world: &mut World,
    declared: &AnimationClip,
    key: &str,
    identity: AnimationInstance,
    generation: SceneGeneration,
    at: Tick,
) -> Entity {
    let entity = spawn_scene_node(world, key, generation);
    bind_animated_node(
        world,
        declared,
        entity,
        &node(key),
        identity,
        generation,
        at,
    )
    .expect("the production spawn path binds the clip");
    entity
}

/// The batch the playback has published, read without draining it: a test reads
/// the log so it can offer one firing to the consumer a second time.
fn published(world: &World) -> Vec<AnimationEvent> {
    world
        .get_resource::<AnimationLog>()
        .map(AnimationLog::events)
        .unwrap_or_default()
        .to_vec()
}

/// How many times the objective runtime reported `objective` revealed.
fn reveals(tick: &SessionTick, objective: SymbolId) -> usize {
    tick.tick
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                ObjectiveEventKind::ObjectiveRevealed {
                    objective: revealed,
                    ..
                } if revealed == objective
            )
        })
        .count()
}

/// How many times the objective runtime reported `signal` raised.
fn signals_raised(tick: &SessionTick, signal: SymbolId) -> usize {
    tick.tick
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                ObjectiveEventKind::SignalRaised { signal: raised } if raised == signal
            )
        })
        .count()
}

/// A declared clip whose one marker's effect is an explicit unknown: the
/// fixture the "blocked by an unknown" case needs, because no declared fixture
/// ships one.
fn declared_unknown_effect_clip() -> AnimationClip {
    let designed =
        || Provenance::designed(ClaimId::new("f20c.marker-consumer").expect("a valid claim id"));
    AnimationClip::try_new(
        content_id(ContentKind::AnimationTrack, "synthetic.unknown_effect"),
        Origin::SyntheticFixture,
        8,
        LoopMode::Once,
        vec![AnimationChannel::Transform(TransformChannel {
            target: cs_content::scene::SceneNodeId::from_content_id(node("synthetic.hatch"))
                .expect("the fixture key names a scene node"),
            interpolation: Interpolation::Step,
            keys: vec![
                TransformKey {
                    tick: 0,
                    pose: TransformSample::IDENTITY,
                },
                TransformKey {
                    tick: 4,
                    pose: TransformSample::IDENTITY,
                },
            ],
        })],
        vec![EventMarker {
            tick: 2,
            key: "hatch_state_unknown".to_owned(),
            effect: Resolved::Unknown {
                claim_id: ClaimId::new("f20c.hatch-effect").expect("a valid claim id"),
                reason: "the marker effect is not decoded in the original".to_owned(),
            },
        }],
        designed(),
    )
    .expect("the declared clip is valid")
}

// ------------------------------------------------ the marker reaching a mission ---

/// The acceptance scenario: a real session's fired gameplay marker is drained
/// through the consumer, and its gameplay effect happens **exactly once** —
/// once for the skip that crossed the marker, once more for every later fixed
/// tick of the session, and never again when the very same firing is offered a
/// second time.
///
/// The effect is a mission reveal inside the F39 runtime: the marker's declared
/// cue raises the signal the fixture program's reveal rule names, and the
/// secondary objective that rule belongs to becomes visible.
#[test]
fn accept_f20_c_marker_consumer_a_fired_gameplay_marker_drives_the_mission_exactly_once() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let mut scene = wired_session(41);

    let door = {
        let world = scene.world_mut().expect("the session is active");
        bind_clip(
            world,
            &declared,
            "synthetic.hangar.door",
            instance(1),
            generation,
            Tick(0),
        )
    };
    assert!(
        !drives(scene.world().expect("the session is active"), door),
        "nothing is driven before the session's first committed tick"
    );

    // The wired path's own advance: one committed fixed tick at a time until the
    // marker's authored tick, which is how a real session crosses it.
    scene
        .step(SYNTHETIC_DOOR_OPEN_TICK)
        .expect("the session is active");
    assert!(
        drives(scene.world().expect("the session is active"), door),
        "the fixed ticks drove the bound node through the wired path"
    );
    let fired = published(scene.world().expect("the session is active"));
    assert_eq!(
        fired
            .iter()
            .filter(|event| event.effect.is_gameplay())
            .map(|event| event.marker.as_str())
            .collect::<Vec<_>>(),
        vec![SYNTHETIC_DOOR_MARKER],
        "the wired session published the door's gameplay marker exactly once"
    );

    let mut objectives = mission();
    let mut consumer = MissionMarkerConsumer::new(session(41), bound_cues(REACHED_WRECK));

    // The composed step: drain the log, raise the marker's declared signal,
    // step the mission with it.
    let step = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(1, 1))
            .expect("the objective tick is accepted")
    };

    assert_eq!(
        step.markers.raised().len(),
        1,
        "one gameplay marker became one mission signal: {:?}",
        step.markers
    );
    let raised = &step.markers.raised()[0];
    assert_eq!(raised.marker, SYNTHETIC_DOOR_MARKER);
    assert_eq!(raised.cue, DOOR_CUE);
    assert_eq!(raised.signal, REACHED_WRECK);
    assert_eq!(
        raised.at,
        Tick(SYNTHETIC_DOOR_OPEN_TICK),
        "the raise is stamped with the tick the marker fired on"
    );
    assert_eq!(step.markers.signals(), vec![REACHED_WRECK]);
    assert_eq!(
        step.markers.drained(),
        1,
        "the batch held exactly the one fired marker: {:?}",
        step.markers
    );
    assert!(
        step.markers.refusals().is_empty(),
        "a bound, live, first-presentation marker is not refused: {:?}",
        step.markers.refusals()
    );

    // The gameplay effect, in the mission runtime and not in this test: the
    // signal was raised, and the objective whose declared reveal rule names it
    // became visible.
    assert_eq!(signals_raised(&step.tick, REACHED_WRECK), 1);
    assert_eq!(reveals(&step.tick, SECONDARY), 1);
    let row = objectives
        .display()
        .row(SECONDARY)
        .expect("the secondary objective has a display row");
    assert!(row.revealed, "the marker revealed the objective: {row:?}");
    assert_eq!(row.state, ObjectiveState::Pending);
    let visible: Vec<SymbolId> = objectives
        .display()
        .visible()
        .iter()
        .map(|row| row.symbol)
        .collect();
    assert!(
        visible.contains(&SECONDARY),
        "the revealed objective is on the display the player reads: {visible:?}"
    );
    // The primary was born revealed and nothing the marker raised names a rule
    // against it, so the one signal moved one row and left the other alone.
    let primary = objectives
        .display()
        .row(PRIMARY)
        .expect("the primary objective has a display row");
    assert!(primary.revealed, "the primary was revealed at launch");
    assert_eq!(
        primary.state,
        ObjectiveState::Active,
        "the signal no declared rule names left the primary objective's state alone"
    );
    assert_eq!(
        reveals(&step.tick, PRIMARY),
        0,
        "the marker's tick reported no event about the primary objective"
    );

    // Later committed ticks of the same session: the one-shot clip is finished,
    // so nothing is published, nothing is raised and nothing re-reveals.
    scene.step(5).expect("the session is active");
    let later = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(6, 1))
            .expect("the objective tick is accepted")
    };
    assert_eq!(later.markers.drained(), 0);
    assert!(
        later.markers.raised().is_empty(),
        "a finished clip's later ticks raise nothing: {:?}",
        later.markers
    );
    assert_eq!(reveals(&later.tick, SECONDARY), 0);
    assert_eq!(later.markers.presentation_ignored(), 0);

    // The skip, after the fact: the head jumps past the clip's own end through
    // the same production entry the schedule calls. The marker is not offered
    // a second time, so the skip adds nothing to the mission.
    advance_animation(
        scene.world_mut().expect("the session is active"),
        Tick(SYNTHETIC_DOOR_OPEN_TICK + 40),
    );
    let skipped = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(7, 1))
            .expect("the objective tick is accepted")
    };
    assert_eq!(
        skipped.markers.raised().len(),
        0,
        "a skip over a marker that already fired raises nothing: {:?}",
        skipped.markers
    );
    assert_eq!(reveals(&skipped.tick, SECONDARY), 0);
    assert!(
        objectives
            .display()
            .row(SECONDARY)
            .expect("the secondary objective has a display row")
            .revealed,
        "the mission is where the first firing left it"
    );

    // The same firing offered a second time: refused as the repeat of an
    // activation already applied, and the mission stays as it was.
    let repeat = consumer.admit(&fired[0]);
    assert!(
        matches!(
            repeat,
            MarkerAdmission::Refused(MarkerRefusal::RepeatedActivation { .. })
        ),
        "one activation applies once, however often it is offered: {repeat:?}"
    );
    assert!(
        consumer.has_applied(&raised.activation),
        "the applied activation is on record: {}",
        raised.activation
    );
    assert!(
        objectives
            .display()
            .row(SECONDARY)
            .expect("the secondary objective has a display row")
            .revealed,
        "the refused repeat changed nothing in the mission"
    );

    // The activation identity is the live instance and the marker, deliberately
    // **not** the firing's stamp: the same activation presented under another
    // tick and another sequence is still that one activation, so it is refused
    // as the repeat too. A key built from the whole `EventId` would let every
    // re-stamped presentation through and duplicate the mission event.
    let restamped = AnimationEvent {
        id: AnimationEventId {
            tick: Tick(raised.at.0 + 999),
            sequence: raised.event.sequence.wrapping_add(7),
            ..fired[0].id
        },
        ..fired[0].clone()
    };
    let repeat = consumer.admit(&restamped);
    assert!(
        matches!(
            repeat,
            MarkerAdmission::Refused(MarkerRefusal::RepeatedActivation { .. })
        ),
        "the tick and the sequence are not part of the activation: {repeat:?}"
    );
    assert_eq!(
        consumer.applied().len(),
        1,
        "and the ledger still holds exactly the one activation"
    );
}

/// Behavior 1 and AC02 at this layer's scope: a looping clip keeps presenting
/// and keeps firing its presentation cue on every pass, while the gameplay
/// marker it fired once becomes one mission signal — and the presentation cues
/// are counted and never raised.
#[test]
fn accept_f20_c_marker_consumer_a_loop_pass_raises_one_signal_and_no_presentation_cue() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_propeller_clip();
    let mut scene = wired_session(42);

    {
        let world = scene.world_mut().expect("the session is active");
        bind_clip(
            world,
            &declared,
            SYNTHETIC_PROPELLER_NODE,
            instance(1),
            generation,
            Tick(0),
        );
    }
    let passes = SYNTHETIC_PROPELLER_DURATION * 3 + 1;
    scene.step(passes).expect("the session is active");
    let fired = published(scene.world().expect("the session is active"));
    assert_eq!(
        fired
            .iter()
            .filter(|event| event.effect.is_gameplay())
            .count(),
        1,
        "the rotor fired its one-shot gameplay marker once across every loop pass"
    );

    let mut objectives = mission();
    let mut consumer = MissionMarkerConsumer::new(session(42), bound_cues(UNDECLARED));
    let step = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(
            world,
            &mut consumer,
            &mut objectives,
            &facts(passes, passes),
        )
        .expect("the objective tick is accepted")
    };

    assert_eq!(
        step.markers.raised().len(),
        1,
        "a looping clip's gameplay marker became exactly one mission signal: {:?}",
        step.markers
    );
    let raised = &step.markers.raised()[0];
    assert_eq!(raised.marker, SYNTHETIC_PROPELLER_GAMEPLAY_MARKER);
    assert_eq!(raised.cue, ROTOR_CUE);
    assert_eq!(
        raised.pass, 0,
        "a one-shot gameplay marker fires on the first pass and never on a later one"
    );
    assert_eq!(
        raised.at,
        Tick(SYNTHETIC_PROPELLER_GAMEPLAY_TICK),
        "the raise is stamped with the tick the marker fired on"
    );
    assert_eq!(
        step.markers.presentation_ignored(),
        passes.div_ceil(SYNTHETIC_PROPELLER_DURATION) as usize,
        "every presentation cue the passes fired was seen and not applied"
    );
    assert_eq!(
        fired
            .iter()
            .filter(|event| event.marker == SYNTHETIC_PROPELLER_PRESENTATION_MARKER)
            .count(),
        passes.div_ceil(SYNTHETIC_PROPELLER_DURATION) as usize,
        "the presentation cue fired once per completed pass"
    );
    assert!(
        fired
            .iter()
            .all(|event| event.effect.cue() != BLADE_CUE || !event.effect.is_gameplay()),
        "the presentation cue never arrives as a gameplay effect"
    );
    assert_eq!(
        signals_raised(&step.tick, UNDECLARED),
        1,
        "the mission runtime saw the one raised signal and no other"
    );
    assert_eq!(
        reveals(&step.tick, SECONDARY),
        0,
        "a signal the program declares no rule for moves nothing: the program decides what a \
         signal means"
    );
    assert_eq!(
        objectives
            .display()
            .row(SECONDARY)
            .expect("the secondary objective has a display row")
            .state,
        ObjectiveState::Hidden,
        "the objective no rule names kept its state"
    );
}

/// AC04 at this layer's scope: the **skip is what crosses the marker**. The
/// head jumps from before the door's authored tick to past it in one advance,
/// the firing the skip published becomes exactly one mission signal, the
/// objective it reveals is revealed exactly once — and every later skip over
/// the same finished activation adds nothing.
#[test]
fn accept_f20_c_marker_consumer_a_skip_across_the_marker_reaches_the_mission_once() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let mut scene = wired_session(50);

    {
        let world = scene.world_mut().expect("the session is active");
        bind_clip(
            world,
            &declared,
            "synthetic.hangar.door",
            instance(1),
            generation,
            Tick(0),
        );
    }
    // One fixed tick, so the session is live and the head sits before the
    // marker's authored tick.
    scene.step(1).expect("the session is active");
    assert!(
        published(scene.world().expect("the session is active"))
            .iter()
            .all(|event| !event.effect.is_gameplay()),
        "nothing has crossed the marker yet"
    );

    // The skip: one advance from clip time 1 straight past the open tick.
    advance_animation(
        scene.world_mut().expect("the session is active"),
        Tick(SYNTHETIC_DOOR_OPEN_TICK + 20),
    );
    let mut objectives = mission();
    let mut consumer = MissionMarkerConsumer::new(session(50), bound_cues(REACHED_WRECK));
    let skipped = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(2, 1))
            .expect("the objective tick is accepted")
    };

    assert_eq!(
        skipped.markers.raised().len(),
        1,
        "the skip published the marker once and it became one signal: {:?}",
        skipped.markers
    );
    assert_eq!(
        skipped.markers.raised()[0].at,
        Tick(SYNTHETIC_DOOR_OPEN_TICK + 20),
        "the raise is stamped with the session tick the crossing was published \
         on — the skip's — because that is what the event id carries"
    );
    assert_eq!(signals_raised(&skipped.tick, REACHED_WRECK), 1);
    assert_eq!(reveals(&skipped.tick, SECONDARY), 1);
    assert!(
        objectives
            .display()
            .row(SECONDARY)
            .expect("the secondary objective has a display row")
            .revealed,
        "the skip's crossing reached the mission runtime's display"
    );

    // Further skips over the same activation, and a skip back before it: the
    // mission does not move again, and a reversal is named rather than silent.
    advance_animation(
        scene.world_mut().expect("the session is active"),
        Tick(SYNTHETIC_DOOR_OPEN_TICK * 3),
    );
    let again = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(3, 1))
            .expect("the objective tick is accepted")
    };
    assert!(
        again.markers.raised().is_empty(),
        "a second skip past the same activation raises nothing: {:?}",
        again.markers
    );
    assert_eq!(reveals(&again.tick, SECONDARY), 0);
    assert_eq!(again.markers.signals(), Vec::<SymbolId>::new());
}

/// The producer serial is what makes `(session, producer, marker)` the identity
/// of an **activation** rather than a conservative approximation: two instances
/// of one clip are two live producers, so two engines starting are two mission
/// events — the marker is not dedup'd across instances.
#[test]
fn accept_f20_c_marker_consumer_two_instances_of_one_clip_are_two_activations() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let mut scene = wired_session(51);

    {
        let world = scene.world_mut().expect("the session is active");
        // The same clip, the same node, two instance identities: the second
        // `bind_animated_node` starts a second live producer.
        bind_clip(
            world,
            &declared,
            "synthetic.hangar.door",
            instance(1),
            generation,
            Tick(0),
        );
        bind_clip(
            world,
            &declared,
            "synthetic.hangar.door",
            instance(2),
            generation,
            Tick(0),
        );
    }
    scene
        .step(SYNTHETIC_DOOR_OPEN_TICK)
        .expect("the session is active");

    let mut objectives = mission();
    let mut consumer = MissionMarkerConsumer::new(session(51), bound_cues(UNDECLARED));
    let step = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(1, 1))
            .expect("the objective tick is accepted")
    };

    assert_eq!(
        step.markers.raised().len(),
        2,
        "two instances of one clip are two activations, not one: {:?}",
        step.markers
    );
    let producers: Vec<u32> = step
        .markers
        .raised()
        .iter()
        .map(|raised| raised.activation.producer())
        .collect();
    assert_ne!(
        producers[0], producers[1],
        "the two activations are distinguished by their producer serial, {producers:?}"
    );
    assert_ne!(
        step.markers.raised()[0].activation,
        step.markers.raised()[1].activation,
        "and each is its own ledger key"
    );
    assert_eq!(consumer.applied().len(), 2, "both are on record");
    assert_eq!(
        signals_raised(&step.tick, UNDECLARED),
        2,
        "two engines starting are two mission events"
    );
}

/// The composition **adds** the marker's signals to the host's own facts; it
/// never substitutes them. A host that raises its own signal on the same tick
/// sees both, in the host's order first.
#[test]
fn accept_f20_c_marker_consumer_the_hosts_own_signals_survive_the_composition() {
    let generation = SceneGeneration::default().next();
    let (mut scene, _fired) = crossed_door(52, generation);

    let mut objectives = mission();
    let mut consumer = MissionMarkerConsumer::new(session(52), bound_cues(REACHED_WRECK));

    // The host's own fact this tick: a signal of its own, raised for its own
    // reasons (here the same declared signal, so the count is what shows it).
    let host_signals = [REACHED_WRECK];
    let mut host = facts(1, 1);
    host.signals = &host_signals;
    let step = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &host)
            .expect("the objective tick is accepted")
    };

    assert_eq!(
        step.markers.signals(),
        vec![REACHED_WRECK],
        "the delivery still reports only what the marker raised"
    );
    assert_eq!(
        signals_raised(&step.tick, REACHED_WRECK),
        2,
        "the host's own signal and the marker's are both applied: the composed \
         step added the marker rather than replacing the host's facts"
    );
    assert_eq!(
        reveals(&step.tick, SECONDARY),
        1,
        "and one declared reveal rule still reveals the objective once"
    );
}

/// A tick a runtime **bound** stopped is not a refusal: the session answers
/// `Ok` with a [`StopReason`] and applied nothing. The delivery therefore still
/// carries the marker's signal, the caller retries the next tick with it, and
/// the gameplay effect happens then — once. A layer that dropped the delivery
/// on a stopped tick would lose the marker for good.
#[test]
fn accept_f20_c_marker_consumer_a_stopped_objective_tick_still_hands_the_signal_back() {
    let generation = SceneGeneration::default().next();
    let (mut scene, _fired) = crossed_door(53, generation);

    let mut objectives = bounded_mission();
    let mut consumer = MissionMarkerConsumer::new(session(53), bound_cues(REACHED_WRECK));
    let stopped = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(1, 1))
            .expect("a bounded tick is a stop, not a refusal")
    };

    assert!(
        matches!(stopped.tick.stop, Some(StopReason::EventBudget { .. })),
        "the runtime's own bound stopped the tick: {:?}",
        stopped.tick.stop
    );
    assert_eq!(
        stopped.markers.signals(),
        vec![REACHED_WRECK],
        "a stopped tick still hands the marker's signal back, so the caller can \
         retry with it: {:?}",
        stopped.markers
    );
    assert_eq!(
        reveals(&stopped.tick, SECONDARY),
        0,
        "the stopped tick revealed nothing"
    );
    assert!(
        !objectives
            .display()
            .row(SECONDARY)
            .expect("the secondary objective has a display row")
            .revealed,
        "and the mission display did not move"
    );

    // The retry the delivery invites: the host carries the signals to a tick
    // this runtime accepts, and the gameplay effect happens there, once.
    let mut relaunched = mission();
    let carried = stopped.markers.signals();
    let mut retry = facts(2, 1);
    retry.signals = &carried;
    let applied = relaunched
        .step(&retry)
        .expect("an accepted tick applies the carried signals");
    assert_eq!(signals_raised(&applied, REACHED_WRECK), 1);
    assert_eq!(reveals(&applied, SECONDARY), 1);
    assert!(
        relaunched
            .display()
            .row(SECONDARY)
            .expect("the secondary objective has a display row")
            .revealed,
        "the marker's effect happened exactly once, on the tick that applied it"
    );
}

// -------------------------------------------------------------- refusals ---

/// Nothing is invented for a cue the mission program never bound: the marker
/// is refused by name, no signal is raised, the objective does not move — and
/// the activation is **not** consumed, so a later binding of the same firing
/// still applies it.
#[test]
fn accept_f20_c_marker_consumer_an_unbound_cue_is_refused_by_name() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let (mut scene, fired) = crossed_door(43, generation);

    // A table that binds nothing at all: the whole table is the mission host's
    // declaration, and this one declares no cue.
    let mut objectives = mission();
    let mut consumer = MissionMarkerConsumer::new(session(43), MissionMarkerBindings::default());
    let step = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(1, 1))
            .expect("the objective tick is accepted")
    };

    assert!(
        step.markers.raised().is_empty(),
        "no signal was invented for an unbound cue: {:?}",
        step.markers
    );
    assert_eq!(step.markers.signals(), Vec::<SymbolId>::new());
    assert_eq!(
        step.markers.refusals(),
        &[MarkerRefusal::UnboundCue {
            clip: declared.id().clone(),
            marker: SYNTHETIC_DOOR_MARKER.to_owned(),
            cue: DOOR_CUE.to_owned(),
            pass: 0,
        }],
        "the refusal names the clip, the marker, the cue and the pass"
    );
    assert_eq!(signals_raised(&step.tick, REACHED_WRECK), 0);
    assert_eq!(reveals(&step.tick, SECONDARY), 0);
    assert!(
        consumer.applied().is_empty(),
        "a refused marker consumes no activation, so a later binding can still apply it"
    );

    // The very same firing, offered to a consumer whose table does bind it.
    let mut binding = MissionMarkerConsumer::new(session(43), bound_cues(REACHED_WRECK));
    let admission = binding.admit(&fired[0]);
    assert!(
        matches!(
            &admission,
            MarkerAdmission::Raised(raised) if raised.cue == DOOR_CUE
        ),
        "the firing was refused for the program's sake, not lost: {admission:?}"
    );
}

/// A marker published by a session generation the consumer does not serve
/// belongs to a mission that is no longer live: it is refused before its cue is
/// even resolved, so a stale firing can neither raise a signal nor consume an
/// activation.
#[test]
fn accept_f20_c_marker_consumer_a_stale_session_marker_is_refused() {
    let generation = SceneGeneration::default().next();
    let (_scene, fired) = crossed_door(44, generation);

    // A consumer for the *next* generation of the same world.
    let mut consumer = MissionMarkerConsumer::new(session(45), bound_cues(REACHED_WRECK));
    let admission = consumer.admit(&fired[0]);
    assert!(
        matches!(
            &admission,
            MarkerAdmission::Refused(MarkerRefusal::StaleSession {
                session: fired_in,
                served,
                ..
            }) if *fired_in == session(44) && *served == session(45)
        ),
        "a firing from another session generation is refused by name: {admission:?}"
    );
    assert!(
        consumer.applied().is_empty(),
        "a stale firing consumes no activation"
    );
}

/// Behavior 2: a marker whose effect is an explicit unknown blocks the gameplay
/// transition it gates, and the block reaches the mission layer carrying the
/// unknown's own claim — never applied, never silently skipped.
#[test]
fn accept_f20_c_marker_consumer_a_blocked_marker_effect_is_reported_and_never_applied() {
    let generation = SceneGeneration::default().next();
    let declared = declared_unknown_effect_clip();
    let mut scene = wired_session(46);

    {
        let world = scene.world_mut().expect("the session is active");
        bind_clip(
            world,
            &declared,
            "synthetic.hatch",
            instance(1),
            generation,
            Tick(0),
        );
    }
    scene.step(4).expect("the session is active");

    let mut objectives = mission();
    let mut consumer = MissionMarkerConsumer::new(session(46), bound_cues(REACHED_WRECK));
    let step = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(4, 1))
            .expect("the objective tick is accepted")
    };

    assert!(
        step.markers.raised().is_empty(),
        "a marker with an unknown effect raises nothing: {:?}",
        step.markers
    );
    assert_eq!(
        step.markers.refusals().len(),
        1,
        "the block is reported once, not once per tick: {:?}",
        step.markers.refusals()
    );
    assert!(
        matches!(
            &step.markers.refusals()[0],
            MarkerRefusal::BlockedEffect { marker, claim_id, .. }
                if marker == "hatch_state_unknown"
                    && claim_id == &ClaimId::new("f20c.hatch-effect").expect("a valid claim id")
        ),
        "the refusal carries the unknown's claim: {:?}",
        step.markers.refusals()[0]
    );
    assert_eq!(signals_raised(&step.tick, REACHED_WRECK), 0);
    assert_eq!(reveals(&step.tick, SECONDARY), 0);
    assert!(
        consumer.applied().is_empty(),
        "a blocked marker consumes no activation"
    );
}

// ------------------------------------------------ the declared cue table ---

/// The table is input, and it refuses what it cannot decide: an empty cue
/// label, the same cue bound to two signals, and the reserved actor-event
/// source a raised signal must never carry.
#[test]
fn accept_f20_c_marker_consumer_the_declared_table_refuses_what_it_cannot_decide() {
    assert_eq!(
        MissionMarkerBindings::new([MissionMarkerBinding::new("", SymbolId(9))]),
        Err(MarkerBindingError::EmptyCue),
        "an empty cue label is an authoring defect, not a binding"
    );
    assert_eq!(
        MissionMarkerBindings::new([
            MissionMarkerBinding::new(DOOR_CUE, SymbolId(9)),
            MissionMarkerBinding::new(DOOR_CUE, SymbolId(10)),
        ]),
        Err(MarkerBindingError::DuplicateCue {
            cue: DOOR_CUE.to_owned(),
            first: SymbolId(9),
            second: SymbolId(10),
        }),
        "an undecided cue is refused rather than resolved by registration order"
    );
    assert_eq!(
        MissionMarkerBindings::new([MissionMarkerBinding::new(
            DOOR_CUE,
            RESERVED_ACTOR_EVENT_SOURCE
        )]),
        Err(MarkerBindingError::ReservedSignal {
            cue: DOOR_CUE.to_owned(),
            signal: RESERVED_ACTOR_EVENT_SOURCE,
        }),
        "a cue bound to the reserved source would alias every counted actor's own event"
    );

    // The declared table resolves by cue and nothing else.
    let table = bound_cues(REACHED_WRECK);
    assert_eq!(table.len(), 2);
    assert!(!table.is_empty());
    assert_eq!(table.signal_for(DOOR_CUE), Some(REACHED_WRECK));
    assert_eq!(table.signal_for(ROTOR_CUE), Some(REACHED_WRECK));
    assert_eq!(table.signal_for(BLADE_CUE), None);
    assert_eq!(table.signal_for("synthetic.door_opened"), None);
    assert_eq!(table.cues().collect::<Vec<_>>(), vec![DOOR_CUE, ROTOR_CUE]);
    assert!(
        MissionMarkerBindings::default().is_empty(),
        "a program that declares no cue binds nothing, and says so"
    );
}

// ----------------------------------------------- the refused objective tick ---

/// A refused objective tick changed nothing — and because the log was already
/// drained, the refusal hands its batch back so the caller retries with it
/// rather than losing the marker. The retry applies it exactly once, and a
/// blind re-drain applies nothing, because the activation was consumed.
#[test]
fn accept_f20_c_marker_consumer_a_refused_objective_tick_keeps_the_markers() {
    let generation = SceneGeneration::default().next();
    let (mut scene, _fired) = crossed_door(47, generation);

    let mut objectives = mission();
    let mut consumer = MissionMarkerConsumer::new(session(47), bound_cues(REACHED_WRECK));

    // The mission host steps its own tick first — no marker is pending yet, so
    // nothing is applied and nothing is drained.
    objectives
        .step(&facts(8, 1))
        .expect("the host's own tick is accepted");

    // Now the same tick again: the runtime refuses it, and the refusal hands
    // back the markers it could not apply instead of dropping them.
    let error = {
        let world = scene.world_mut().expect("the session is active");
        match step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(8, 1)) {
            Ok(step) => panic!(
                "the runtime accepted a non-advancing tick: {:?}",
                step.markers
            ),
            Err(error) => error,
        }
    };
    assert!(
        matches!(error.error, RuntimeError::NotAdvancing { .. }),
        "the refusal names the tick rule: {:?}",
        error.error
    );
    assert_eq!(
        error.markers.raised().len(),
        1,
        "the refused pass's markers are named, not dropped: {:?}",
        error.markers
    );
    assert_eq!(error.markers.signals(), vec![REACHED_WRECK]);
    assert!(
        error.to_string().contains("not applied"),
        "the refusal says what it did not apply: {}",
        error
    );
    assert!(
        !objectives
            .display()
            .row(SECONDARY)
            .expect("the secondary objective has a display row")
            .revealed,
        "a refused tick revealed nothing"
    );

    // The retry: the caller steps the next tick with the signals the refusal
    // carried. The gameplay effect happens then, once.
    let retry_signals = error.markers.signals();
    let mut retry_facts = facts(9, 1);
    retry_facts.signals = &retry_signals;
    let retried = objectives
        .step(&retry_facts)
        .expect("the retry is an advancing tick");
    assert_eq!(signals_raised(&retried, REACHED_WRECK), 1);
    assert_eq!(reveals(&retried, SECONDARY), 1);
    assert!(
        objectives
            .display()
            .row(SECONDARY)
            .expect("the secondary objective has a display row")
            .revealed,
        "the retried marker revealed the objective exactly once"
    );

    // And a blind re-drain of the now-empty log applies nothing: the activation
    // was consumed by the refused pass, which is what makes the layer idempotent.
    let blind = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(10, 1))
            .expect("the objective tick is accepted")
    };
    assert_eq!(blind.markers.drained(), 0);
    assert!(blind.markers.raised().is_empty());
    assert_eq!(reveals(&blind.tick, SECONDARY), 0);
}

/// Behavior 5's other half: a *reversed* cinematic. The head is held where it
/// is, no marker is offered — and the hold is named in the delivery rather than
/// drained away, so a mission can tell a rewind from a marker that never fired.
#[test]
fn accept_f20_c_marker_consumer_a_reversed_cinematic_is_named_and_raises_nothing() {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_door_clip();
    let mut scene = wired_session(49);

    {
        let world = scene.world_mut().expect("the session is active");
        bind_clip(
            world,
            &declared,
            "synthetic.hangar.door",
            instance(1),
            generation,
            Tick(0),
        );
    }
    scene
        .step(SYNTHETIC_DOOR_OPEN_TICK)
        .expect("the session is active");

    let mut objectives = mission();
    let mut consumer = MissionMarkerConsumer::new(session(49), bound_cues(REACHED_WRECK));
    let crossed = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(10, 1))
            .expect("the objective tick is accepted")
    };
    assert_eq!(reveals(&crossed.tick, SECONDARY), 1);
    assert_eq!(crossed.markers.raised().len(), 1);

    // The reversal: the session tick asks for a clip time behind the head.
    advance_animation(scene.world_mut().expect("the session is active"), Tick(0));
    let held = {
        let world = scene.world_mut().expect("the session is active");
        step_mission_with_markers(world, &mut consumer, &mut objectives, &facts(11, 1))
            .expect("the objective tick is accepted")
    };

    assert!(
        held.markers.raised().is_empty(),
        "a held advance offers no marker: {:?}",
        held.markers
    );
    assert_eq!(held.markers.signals(), Vec::<SymbolId>::new());
    assert_eq!(
        held.markers.refusals(),
        &[MarkerRefusal::HeldAdvance {
            clip: declared.id().clone(),
            instance: instance(1),
            from: SYNTHETIC_DOOR_OPEN_TICK,
            to: 0,
        }],
        "the hold is named with the clip, the instance and both clip times"
    );
    assert_eq!(reveals(&held.tick, SECONDARY), 0);
    assert!(
        objectives
            .display()
            .row(SECONDARY)
            .expect("the secondary objective has a display row")
            .revealed,
        "the mission still holds the one reveal the first firing produced"
    );
}

/// A generation change releases the applied activations, so the next generation
/// is not refused as the repeat of the last one's — and the release is reported
/// rather than silently dropped. A retry that names the generation **already
/// served** is refused: clearing the ledger of a live mission would re-arm every
/// activation in it and hand the same mission event out a second time.
#[test]
fn accept_f20_c_marker_consumer_a_generation_change_releases_the_ledger() {
    let generation = SceneGeneration::default().next();
    let (_scene, fired) = crossed_door(48, generation);

    let mut consumer = MissionMarkerConsumer::new(session(48), bound_cues(REACHED_WRECK));
    assert!(
        matches!(
            consumer.admit(&fired[0]),
            MarkerAdmission::Raised(raised) if raised.cue == DOOR_CUE
        ),
        "the firing applies under the generation that published it"
    );
    assert_eq!(consumer.applied().len(), 1);

    // The live generation is refused, and the refused retry changes nothing.
    let refused = consumer
        .retry(session(48))
        .expect_err("a retry for the live generation is not a generation change");
    assert_eq!(
        refused,
        MarkerTeardownError::SameSession {
            served: session(48)
        },
        "the refusal names the generation that is already served"
    );
    assert!(
        refused.to_string().contains("live mission"),
        "the refusal says what it protects: {refused}"
    );
    assert_eq!(
        consumer.applied().len(),
        1,
        "a refused retry re-armed nothing: the live mission keeps its ledger"
    );

    let teardown = consumer
        .retry(session(49))
        .expect("a new generation is a change");
    assert_eq!(teardown.served, session(49));
    assert_eq!(teardown.released, 1, "the release is reported, not dropped");
    assert_eq!(consumer.served(), session(49));
    assert!(
        consumer.applied().is_empty(),
        "the next generation starts with an empty ledger"
    );
    assert!(
        matches!(
            consumer.admit(&fired[0]),
            MarkerAdmission::Refused(MarkerRefusal::StaleSession { .. })
        ),
        "and the old generation's firing is stale to it"
    );
}

// --------------------------------------------------------------- helpers ---

/// Whether the bound node carries an animated pose, i.e. whether anything drove
/// it through the wired path.
fn drives(world: &World, entity: Entity) -> bool {
    world
        .get::<cs_app::animation::NodeAnimatedPose>(entity)
        .is_some()
}

/// The wired session with the door's gameplay marker already crossed by a skip,
/// and the batch that crossing published.
fn crossed_door(
    session_value: u64,
    generation: SceneGeneration,
) -> (PhysicsSession, Vec<AnimationEvent>) {
    let declared = declared_synthetic_door_clip();
    let mut scene = wired_session(session_value);
    {
        let world = scene.world_mut().expect("the session is active");
        bind_clip(
            world,
            &declared,
            "synthetic.hangar.door",
            instance(1),
            generation,
            Tick(0),
        );
    }
    scene.step(1).expect("the session is active");
    advance_animation(
        scene.world_mut().expect("the session is active"),
        Tick(SYNTHETIC_DOOR_OPEN_TICK + 20),
    );
    let fired = published(scene.world().expect("the session is active"));
    assert_eq!(
        fired
            .iter()
            .filter(|event| event.effect.is_gameplay())
            .count(),
        1,
        "the fixture crossed exactly one gameplay marker"
    );
    (scene, fired)
}

// ------------------------------------------- a second consumer of the same log ---

/// A clip whose material track names an unknown reference: the playback blocks
/// it once and publishes a `BlockedTrack`, which is the render consumer's.
fn declared_unknown_material_clip() -> AnimationClip {
    AnimationClip::try_new(
        content_id(ContentKind::AnimationTrack, "synthetic.unknown_material"),
        Origin::SyntheticFixture,
        8,
        LoopMode::Once,
        vec![AnimationChannel::Material(MaterialChannel {
            target: cs_content::scene::SceneNodeId::from_content_id(node("synthetic.panel"))
                .expect("the fixture key names a scene node"),
            keys: vec![MaterialKey {
                tick: 1,
                material: Resolved::unknown(
                    ClaimId::new("f20c.panel-material").expect("a valid claim id"),
                    "the material slot is undecoded",
                )
                .expect("a nonempty reason"),
            }],
        })],
        Vec::new(),
        Provenance::designed(ClaimId::new("f20c.log-seam").expect("a valid claim id")),
    )
    .expect("the declared clip is valid")
}

/// #671: the mission consumer takes the marker records and only those, so a
/// render consumer draining the same log on the same tick still finds its
/// blocked tracks, and nothing is published again by either one.
#[test]
fn accept_f20_c_marker_consumer_leaves_the_other_record_kinds_for_their_consumers() {
    let generation = SceneGeneration::default().next();
    let (mut scene, _) = crossed_door(61, generation);
    {
        let world = scene.world_mut().expect("the session is active");
        bind_clip(
            world,
            &declared_unknown_material_clip(),
            "synthetic.panel",
            instance(2),
            generation,
            Tick(0),
        );
    }
    advance_animation(
        scene.world_mut().expect("the session is active"),
        Tick(SYNTHETIC_DOOR_OPEN_TICK + 21),
    );
    let before = scene
        .world()
        .expect("the session is active")
        .resource::<AnimationLog>()
        .blocked_tracks()
        .to_vec();
    assert_eq!(before.len(), 1, "the unknown material is blocked once");

    let mut consumer = MissionMarkerConsumer::new(session(61), bound_cues(REACHED_WRECK));
    let delivery = consumer.drain(scene.world_mut().expect("the session is active"));
    assert_eq!(delivery.raised().len(), 1, "the marker reached the mission");
    assert_eq!(delivery.drained(), 1, "the mission took the marker record only");

    // The second consumer, later on the same tick, still has its records.
    let world = scene.world_mut().expect("the session is active");
    let log = world.resource::<AnimationLog>();
    assert!(log.events().is_empty(), "the markers were the mission's");
    assert_eq!(log.blocked_tracks(), before.as_slice());
    let taken = world.resource_mut::<AnimationLog>().take_blocked_tracks();
    assert_eq!(taken, before, "the render consumer got what it owns");
    assert!(world.resource::<AnimationLog>().is_empty());

    // Neither take publishes anything: later ticks add no record, so nothing is
    // recorded once per tick.
    advance_animation(world, Tick(SYNTHETIC_DOOR_OPEN_TICK + 22));
    assert!(world.resource::<AnimationLog>().is_empty());
}
