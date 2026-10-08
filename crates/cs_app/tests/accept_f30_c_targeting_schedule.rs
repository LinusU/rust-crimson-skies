//! Acceptance scenario F30-C (schedule): the session schedule drives the
//! whole targeting path — roster sync, command edges, damage tick, consumer
//! pass — once per rendered frame, in the order the consumer contract
//! requires.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-C`. Task test prefix: `accept_f30_c_`.
//!
//! These tests drive production code only: the production
//! [`TargetingSchedulePlugin`] over a real `bevy::app::App`, a session
//! resource, bound entities, and the [`TargetingFrameInputs`] and
//! [`PendingTargetDamage`] a session publishes per frame. **No test here
//! calls a targeting entry** — not `sync_targetable_roster`,
//! `apply_selection_edges`, `apply_target_damage` or
//! `apply_target_consumers`: every observation is read from the resources the
//! schedule itself publishes ([`TargetConsumers`],
//! [`TargetingScheduleReport`]), exactly as a HUD, a spyglass or a weapon path
//! would. The destruction that clears a selection mid-tick is the **real**
//! [`DamageResolver`]'s.
//!
//! The minimum scenarios this slice adds to AC03:
//!
//! * the schedule's consumer pass runs after the same frame's damage tick, so
//!   a target destroyed mid-tick is not in the views at the end of that tick;
//! * the published views follow the session resource: removing or replacing
//!   [`TargetingSession`] tears the views down.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use bevy::app::App;
use cs_app::scene::SceneGeneration;
use cs_app::targeting::{
    ClearedTarget, ConsumerBinding, PendingTargetDamage, TargetConsumers, TargetableBinding,
    TargetableState, TargetingFrameInputs, TargetingSchedulePlugin, TargetingScheduleReport,
    TargetingSession, lower_rules, lower_selection_actions,
};
use cs_content::target_rules::{
    declared_synthetic_selection_actions, declared_synthetic_target_rules,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageNodeKey, DamagePolicy, DamageResolver, HitEvent,
    HitEventId, LifecycleKind, SYNTHETIC_HULL_INTEGRITY, SYNTHETIC_HULL_NODE,
    synthetic_airframe_graph,
};
use cs_sim::targeting::{SelectionClearReason, TargetClass};
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

/// The session generation the roster, the damage resolver and the observers all
/// share.
const SESSION: u64 = 7;
/// The resolver's producer serial.
const PRODUCER: u32 = 3;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn pos(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

/// The fixture's actors as targeting records: the player at the origin, the
/// three equal-distance raiders, the wingman and the objective trader — the
/// same roster the F30-C consumer fixtures select through, so `TargetNext`
/// walks to `actor(2)` deterministically.
fn roster() -> Vec<(u64, ContentId, TargetClass, bool, [f64; 3])> {
    let player = cs_sim::targeting::synthetic_player_faction();
    let raiders = cs_sim::targeting::synthetic_raider_faction();
    let traders = cs_sim::targeting::synthetic_trader_faction();
    vec![
        (
            1,
            player.clone(),
            TargetClass::Aircraft,
            false,
            [0.0, 0.0, 0.0],
        ),
        (
            9,
            raiders.clone(),
            TargetClass::Aircraft,
            false,
            [100.0, 0.0, 0.0],
        ),
        (
            2,
            raiders.clone(),
            TargetClass::Aircraft,
            false,
            [0.0, 100.0, 0.0],
        ),
        (5, raiders, TargetClass::Aircraft, false, [0.0, 0.0, -100.0]),
        (3, player, TargetClass::Aircraft, false, [50.0, 0.0, 0.0]),
        (4, traders, TargetClass::WorldObject, true, [0.0, 60.0, 0.0]),
    ]
}

/// An app with the production schedule plugin, the session resource installed
/// and every fixture actor bound to an entity under one scene generation —
/// the wiring a mission that opens targeting does.
fn fixture_app(generation: SceneGeneration) -> App {
    let mut app = App::new();
    app.add_plugins(TargetingSchedulePlugin);
    insert_session(&mut app, session_id(), generation);
    for (serial, faction, class, objective, position) in roster() {
        let mut state = TargetableState::aircraft(faction, pos(position));
        state.class = class;
        state.objective = objective;
        app.world_mut().spawn((
            TargetableBinding {
                actor: actor(serial),
                rules: declared_synthetic_target_rules().subject().clone(),
                generation,
            },
            state,
        ));
    }
    app
}

fn insert_session(app: &mut App, session: SessionId, generation: SceneGeneration) {
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let bindings = lower_selection_actions(&declared_synthetic_selection_actions())
        .expect("the fixture actions lower");
    app.insert_resource(TargetingSession::new(
        session,
        lowered,
        bindings,
        declared.subject().clone(),
        generation,
    ));
}

fn edges(commands: &[FlightCommand]) -> Vec<Action> {
    commands.iter().copied().map(Action::Flight).collect()
}

/// The real damage resolver for the fixture session: the actors the fixture
/// can destroy, with the designed airframe graph.
fn resolver() -> DamageResolver {
    let mut resolver = DamageResolver::new(session_id(), PRODUCER);
    for serial in [1_u64, 2, 5, 9] {
        resolver
            .register_actor(
                actor(serial),
                synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("the actor registers in the resolver's session");
    }
    resolver
}

/// A hit for the real resolver, stamped in the fixture's session.
fn hit(
    sequence: u32,
    node: &DamageNodeKey,
    attacker: ActorId,
    victim: ActorId,
    damage: f64,
    tick: u64,
) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: session_id(),
            tick: Tick(tick),
            producer: PRODUCER,
            sequence,
        },
        Some(attacker),
        victim,
        node.clone(),
        DamageChannel::Internal,
        damage,
    )
    .expect("the fixture hit is well-formed")
}

/// The tick's batch after the **real** resolver resolved it: one lethal hull
/// hit from `actor(9)` that destroys `victim`.
fn lethal_batch(resolver: &mut DamageResolver, victim: ActorId, tick: u64) -> PendingTargetDamage {
    let hull = DamageNodeKey::new(SYNTHETIC_HULL_NODE).expect("a valid node key");
    let hits = vec![hit(
        0,
        &hull,
        actor(9),
        victim,
        SYNTHETIC_HULL_INTEGRITY + 1.0,
        tick,
    )];
    let resolution = resolver
        .resolve(Tick(tick), &hits)
        .expect("the resolver accepts the batch");
    PendingTargetDamage::new(hits, resolution.events)
}

/// Publishes the frame's inputs (and any pending damage) and runs one
/// rendered frame of the schedule — the whole loop a session's frame does.
fn run_frame(app: &mut App, inputs: TargetingFrameInputs) {
    app.insert_resource(inputs);
    app.update();
}

/// The schedule drives the whole tick over many frames: the roster registers,
/// a command edge selects through the real bound table, the real resolver's
/// destruction reaches the published views, and the views follow the world
/// frame by frame — all without the test calling any targeting entry itself.
#[test]
fn accept_f30_c_schedule_drives_the_whole_tick_over_many_frames() {
    let generation = SceneGeneration::default().next();
    let mut app = fixture_app(generation);
    let mut damage_resolver = resolver();

    // Frame 1: an ordinary frame, no key press. The roster registers and the
    // consumer pass publishes an empty readout bound to the observer.
    run_frame(&mut app, TargetingFrameInputs::new(actor(1), Tick(10)));
    {
        let consumers = app.world().resource::<TargetConsumers>();
        assert_eq!(
            consumers.bound(),
            Some(ConsumerBinding {
                session: session_id(),
                observer: actor(1),
            }),
            "the schedule published the views under the session's own binding"
        );
        assert_eq!(consumers.hud().expect("a hud view").reticle, None);
        assert!(!consumers.spyglass().expect("a spyglass view").has_target());
    }
    {
        let report = app.world().resource::<TargetingScheduleReport>();
        assert_eq!(report.frames, 1);
        assert_eq!(report.session, Some(session_id()));
        assert_eq!(
            report.roster.clone().expect("the roster ran").registered,
            roster().len(),
            "every bound entity reached the store through the schedule"
        );
        let selection = report.selection.clone().expect("the edge pass ran");
        assert!(selection.acted.is_empty(), "no key was pressed");
        assert!(selection.phase.is_some(), "the phase record was derived");
        assert!(report.consumer_error.is_none());
    }

    // Frame 2: one real key press. The published views name the selected
    // raider, the tie-broken `actor(2)`.
    run_frame(
        &mut app,
        TargetingFrameInputs::new(actor(1), Tick(11))
            .with_edges(edges(&[FlightCommand::TargetNext])),
    );
    {
        let consumers = app.world().resource::<TargetConsumers>();
        let reticle = consumers
            .hud()
            .expect("a hud view")
            .reticle
            .clone()
            .expect("a target is selected");
        assert_eq!(reticle.target, actor(2));
        assert!(consumers.spyglass().expect("a spyglass view").has_target());
        assert!(consumers.guidance().expect("a guidance view").has_aid());
    }

    // Frame 3: the real damage system destroys the selected actor. The batch
    // is the resolver's own output, published like any session would.
    let batch = lethal_batch(&mut damage_resolver, actor(2), 12);
    app.insert_resource(batch);
    run_frame(&mut app, TargetingFrameInputs::new(actor(1), Tick(12)));
    let cleared = ClearedTarget {
        actor: actor(2),
        reason: SelectionClearReason::Ended(LifecycleKind::Destroyed),
    };
    {
        let consumers = app.world().resource::<TargetConsumers>();
        let hud = consumers.hud().expect("a hud view");
        assert_eq!(hud.reticle, None, "a wreck is not framed");
        assert_eq!(hud.cleared, Some(cleared), "the view says who went and why");
        assert!(!consumers.spyglass().expect("a spyglass view").has_target());
        assert!(!consumers.guidance().expect("a guidance view").has_aid());
    }
    {
        let report = app.world().resource::<TargetingScheduleReport>();
        assert_eq!(
            report.damage.clone().expect("the damage ran").lifecycle,
            1,
            "the resolver's own destruction was recorded by the schedule"
        );
    }
    assert_eq!(
        app.world()
            .resource::<TargetingSession>()
            .store()
            .gone(&actor(2)),
        Some(LifecycleKind::Destroyed),
        "the destruction is the damage system's own transition"
    );
    assert!(
        app.world().get_resource::<PendingTargetDamage>().is_none(),
        "the batch is consumed exactly once, not replayed next frame"
    );
    assert!(
        app.world().get_resource::<TargetingFrameInputs>().is_none(),
        "the frame inputs are consumed exactly once"
    );

    // Frame 4: nothing new is published, and the views still describe the
    // world as it stands — the destroyed actor stays gone and nothing is
    // re-cleared.
    run_frame(&mut app, TargetingFrameInputs::new(actor(1), Tick(13)));
    {
        let consumers = app.world().resource::<TargetConsumers>();
        let hud = consumers.hud().expect("a hud view");
        assert_eq!(hud.reticle, None);
        assert_eq!(hud.cleared, None, "nothing was cleared this frame");
    }
    {
        let report = app.world().resource::<TargetingScheduleReport>();
        assert_eq!(report.frames, 4);
        assert!(
            report.damage.is_none(),
            "the drained batch is not replayed as a second damage tick"
        );
    }
}

/// The ordering criterion, at the schedule level: one frame carries both the
/// selection edge and the damage that destroys the actor it selects. The edge
/// pass really selected the target and the damage pass really destroyed it in
/// that frame, and the consumer views at the end of the frame name neither —
/// because the consumer pass derives its phase after the tick's damage, a
/// schedule that ran it before could render a target the same tick destroyed.
#[test]
fn accept_f30_c_target_destroyed_mid_tick_is_not_in_the_views() {
    let generation = SceneGeneration::default().next();
    let mut app = fixture_app(generation);
    let mut damage_resolver = resolver();

    app.insert_resource(lethal_batch(&mut damage_resolver, actor(2), 20));
    run_frame(
        &mut app,
        TargetingFrameInputs::new(actor(1), Tick(20))
            .with_edges(edges(&[FlightCommand::TargetNext])),
    );

    {
        let consumers = app.world().resource::<TargetConsumers>();
        let hud = consumers.hud().expect("a hud view");
        assert_eq!(
            hud.reticle, None,
            "the destroyed actor is never described by this frame's views"
        );
        assert_eq!(
            hud.cleared,
            Some(ClearedTarget {
                actor: actor(2),
                reason: SelectionClearReason::Ended(LifecycleKind::Destroyed),
            }),
            "the clear names the actor and the damage system's own reason"
        );
        let spyglass = consumers.spyglass().expect("a spyglass view");
        assert_eq!(spyglass.target, None);
        assert_eq!(spyglass.cleared, hud.cleared);
        assert_eq!(consumers.guidance().expect("a guidance view").aid, None);
    }
    {
        let report = app.world().resource::<TargetingScheduleReport>();
        let selection = report.selection.clone().expect("the edge pass ran");
        assert_eq!(selection.acted, vec![FlightCommand::TargetNext]);
        assert_eq!(
            selection.selection,
            Some(actor(2)),
            "the edge did select the target earlier in the same frame, so the \
             views clear it only because the damage ran before the consumers"
        );
        assert_eq!(report.damage.clone().expect("the damage ran").lifecycle, 1);
        assert_eq!(report.frames, 1);
    }
}

/// The teardown criterion: the consumer views do not outlive the session
/// resource they were derived from. Removing `TargetingSession` — a
/// disconnect, a restart between generations — drops the views on the next
/// frame through the documented `teardown_target_consumers` path.
#[test]
fn accept_f30_c_removing_the_session_tears_down_the_views() {
    let generation = SceneGeneration::default().next();
    let mut app = fixture_app(generation);

    run_frame(
        &mut app,
        TargetingFrameInputs::new(actor(1), Tick(30))
            .with_edges(edges(&[FlightCommand::TargetNext])),
    );
    assert!(
        app.world()
            .resource::<TargetConsumers>()
            .spyglass()
            .expect("bound")
            .has_target(),
        "a target is framed before the session ends"
    );

    app.world_mut().remove_resource::<TargetingSession>();
    app.update();

    {
        let consumers = app.world().resource::<TargetConsumers>();
        assert_eq!(consumers.bound(), None, "no session, so no binding");
        assert_eq!(consumers.hud(), None);
        assert_eq!(consumers.spyglass(), None);
        assert_eq!(consumers.guidance(), None);
    }
    {
        let report = app.world().resource::<TargetingScheduleReport>();
        assert_eq!(report.tore_down, Some(session_id()));
        assert_eq!(report.session, None);
        assert_eq!(report.frames, 2, "the session-less frame still ran");
        assert!(report.consumers.is_none(), "and published nothing");
    }
}

/// The replacement half of the teardown criterion: a restart installs a new
/// session under a new generation, and the old generation's target box does
/// not survive the swap. The new session's first frame cannot describe the old
/// generation's actors, so it publishes nothing rather than inheriting views.
#[test]
fn accept_f30_c_replacing_the_session_drops_the_old_generation_views() {
    let generation = SceneGeneration::default().next();
    let mut app = fixture_app(generation);

    run_frame(
        &mut app,
        TargetingFrameInputs::new(actor(1), Tick(40))
            .with_edges(edges(&[FlightCommand::TargetNext])),
    );
    assert!(
        app.world()
            .resource::<TargetConsumers>()
            .hud()
            .expect("bound")
            .reticle
            .is_some(),
        "a target is framed before the restart"
    );

    let replacement = SessionId::new(SESSION + 1).expect("a nonzero session generation");
    insert_session(&mut app, replacement, generation);
    run_frame(&mut app, TargetingFrameInputs::new(actor(1), Tick(41)));

    {
        let consumers = app.world().resource::<TargetConsumers>();
        assert_eq!(
            consumers.bound(),
            None,
            "the old generation's binding did not survive the replacement"
        );
        assert_eq!(consumers.hud(), None);
        assert_eq!(consumers.spyglass(), None);
    }
    {
        let report = app.world().resource::<TargetingScheduleReport>();
        assert_eq!(
            report.tore_down,
            Some(session_id()),
            "the replaced session's views were torn down by name"
        );
        assert_eq!(report.session, Some(replacement));
        assert!(
            report.consumer_error.is_some(),
            "the new session reports that it cannot describe the old actors, \
             rather than silently publishing nothing"
        );
    }
}

/// A session installed but publishing no frame inputs leaves the frame with no
/// observer to re-derive the views from, so the previous frame's views are
/// dropped — the documented equivalent of a failed pass — and the report names
/// the miswired frame.
#[test]
fn accept_f30_c_frame_without_inputs_drops_the_previous_views() {
    let generation = SceneGeneration::default().next();
    let mut app = fixture_app(generation);

    run_frame(
        &mut app,
        TargetingFrameInputs::new(actor(1), Tick(50))
            .with_edges(edges(&[FlightCommand::TargetNext])),
    );
    assert_eq!(
        app.world()
            .resource::<TargetConsumers>()
            .hud()
            .expect("bound")
            .reticle
            .as_ref()
            .map(|reticle| reticle.target),
        Some(actor(2)),
        "a target is framed before the miswired frame"
    );

    // No inputs published: the schedule drains them, so this frame has none.
    app.update();

    {
        let consumers = app.world().resource::<TargetConsumers>();
        assert_eq!(
            consumers.bound(),
            None,
            "a frame with no observer publishes no stale target box"
        );
        assert_eq!(consumers.spyglass(), None);
    }
    {
        let report = app.world().resource::<TargetingScheduleReport>();
        assert!(report.missing_inputs);
        assert_eq!(report.session, Some(session_id()));
        assert!(report.selection.is_none());
        assert!(report.consumers.is_none());
    }
}
