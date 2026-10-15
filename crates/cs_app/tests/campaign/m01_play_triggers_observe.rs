//! Acceptance suite M01-PLAY-TRIGGERS-OBSERVE (#1480): the world observation
//! the mission host folds in stage 4 comes from the **live composition**, so
//! M01's one proximity trigger fires from the player's real position.
//!
//! The port has existed since VS-M01-RT-MISSION-HOST
//! (`MissionWorldObservation`, consumed in `MissionHost::step` stage 4), but
//! no stage ever wrote one, so every real run folded only the record player's
//! animation rows. This suite pins the writer
//! `mission_session::observation::mission_world_observation`, which
//! `install_mission_host` installs after the physics step and before the
//! composed entry:
//!
//! * **the rows it can see** — `["player"]` at the `MissionPlayerBody`'s
//!   live `Position`, each bound world actor under the node name its measured
//!   `scene_node/…world1.<name>` subject spells, and a living count per group
//!   the composition really spawned members for (measured: `1`, `3`, `5`);
//! * **the rows it refuses to invent** — M01's 39 part-state chains have no
//!   entity, groups `2`/`4` have no spawned member (their only `aiv.zrd`
//!   records are the measured `enabled == 0` skips) and an unobserved group
//!   stays unobserved rather than becoming `0`;
//! * **the trigger itself** — block index 2 (`OBJECTIVE3`) completes only
//!   when the *observed* player row comes within 700 m of the *observed*
//!   `workersvoyagezep` row, and then applies exactly the record's own
//!   effects: `NAP_OBJECTIVE 4` (index 3, 20 s), `WAKE_OBJECTIVE_WHEN_I_COMPLETE`
//!   `27`/`51` (indexes 26/50) and `KILL_OBJECTIVE_WHEN_I_COMPLETE` `41`/`48`
//!   (indexes 40/47).
//!
//! The two synthetic members run in CI without an installation; the retail
//! members are `#[ignore = "requires CS_GAME_DIR"]` and run with
//! `--include-ignored`. Nothing here is `verified_original`: no original
//! executable ran, and every number the assertions pin is read out of the
//! production readers on this run.

use std::collections::BTreeMap;

use avian3d::prelude::Position;
use bevy::prelude::{
    App, Entity, FixedPostUpdate, IntoScheduleConfigs, Resource, Transform, Vec3, With, World,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionHostReport, MissionPlayerBody, MissionStage, MissionWorldObservation,
    PLAYER_MEMBER, build_headless, mission_host_tick, mission_world_observation, stage_for,
    subject_node_name, teardown,
};
use cs_app::world_actors::WorldActorBinding;
use cs_app::world_facts::{MemberObservation, member_chain};
use cs_content::objectives_engine::{EngineState, NapDeclaration};
use cs_script::ir::MemberName;
use cs_script::runtime::ObjectiveLifecycle;

use crate::common::{cid, label};
use crate::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// `OBJECTIVE3`'s zero-based block index — the one `TRAVELERS` site.
const PROXIMITY_BLOCK: u32 = 2;
/// `OBJECTIVE2`, the `DEDG` group-1 block that starts awake.
const DEDG_GROUP_ONE_BLOCK: u32 = 1;
/// The record's proximity radius, in metres.
const RADIUS_M: f64 = 700.0;
/// The measured subject name M01's `TRAVELERS` anchor spells.
const ANCHOR: &str = "workersvoyagezep";
/// The targets `OBJECTIVE3`'s completion effects touch, in the order the
/// assertions read them: the `NAP` target, the two `WAKE` targets and the
/// two `KILL` targets (record `4`, `27`/`51`, `41`/`48` → indexes 3, 26/50,
/// 40/47).
const EFFECT_TARGETS: [u32; 5] = [3, 26, 50, 40, 47];

/// The member chain the record anchors `TRAVELERS` on.
fn anchor_chain() -> MemberName {
    member_chain([ANCHOR])
}

/// The production stage for retail M01.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// One composed tick's lifecycle answer, copied out of the report the host
/// left standing for it.
#[derive(Clone, Debug, PartialEq)]
struct TickSnapshot {
    /// The fixed tick the answer belongs to.
    tick: cs_types::Tick,
    /// Blocks the engine's timer pass woke on that tick.
    woken: Vec<u32>,
    /// Blocks the completion pass completed on that tick.
    completed: Vec<u32>,
    /// The projected lifecycle of each [`EFFECT_TARGETS`] block **at the end
    /// of this tick** — the model the host runs, read right after the
    /// composed entry has applied the completion effects.
    effects: [Option<ObjectiveLifecycle>; 5],
}

/// Resource: every host tick this run answered, in order.
///
/// `MissionHostReport` holds only the **last** attempt, and one 1/60 s frame
/// runs two fixed ticks — so a completion landing on the frame's first tick
/// would be overwritten before a test could read it. This log is the
/// test-side recorder: a system installed after [`mission_host_tick`] copies
/// each tick's own answer out as it stands.
#[derive(Resource, Default)]
struct TickLog(Vec<TickSnapshot>);

impl TickLog {
    /// Whether any tick completed `block`.
    fn completed(&self, block: u32) -> bool {
        self.0.iter().any(|entry| entry.completed.contains(&block))
    }

    /// The first tick that completed `block`.
    fn first_completion(&self, block: u32) -> &TickSnapshot {
        self.0
            .iter()
            .find(|entry| entry.completed.contains(&block))
            .unwrap_or_else(|| panic!("no tick completed block {block}"))
    }
}

/// The test-side recorder (see [`TickLog`]).
fn log_tick(world: &mut World) {
    let Some(report) = world.get_resource::<MissionHostReport>() else {
        return;
    };
    let Ok(answer) = &report.answer else {
        return;
    };
    let snapshot = TickSnapshot {
        tick: report.tick,
        woken: answer.lifecycles.woken.clone(),
        completed: answer.lifecycles.completed.clone(),
        effects: EFFECT_TARGETS.map(|index| {
            world
                .get_resource::<MissionHost>()
                .and_then(|host| host.lifecycles().state(index))
        }),
    };
    let mut log = world.resource_mut::<TickLog>();
    if log.0.last().map(|entry| entry.tick) != Some(snapshot.tick) {
        log.0.push(snapshot);
    }
}

/// Resource: whether the world report is taken away before the host consumes
/// it — the "the observer never wrote one" leg, driven from the test side.
///
/// The production writer still runs; this system, ordered between it and
/// [`mission_host_tick`], removes what it wrote. With the flag off nothing is
/// removed and the run behaves exactly like an ordinary composition.
#[derive(Resource)]
struct StripObservation(bool);

/// The test-side remover (see [`StripObservation`]).
fn strip_observation(world: &mut World) {
    if world.resource::<StripObservation>().0 {
        world.remove_resource::<MissionWorldObservation>();
    }
}

/// Installs the two test-side systems around the production writer, before
/// the first frame runs.
fn arm(app: &mut App) {
    app.insert_resource(TickLog::default());
    app.insert_resource(StripObservation(false));
    app.add_systems(FixedPostUpdate, log_tick.after(mission_host_tick));
    app.add_systems(
        FixedPostUpdate,
        strip_observation
            .after(mission_world_observation)
            .before(mission_host_tick),
    );
}

/// Runs `count` composed frames.
fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
    }
}

/// This run's tick log.
fn log(app: &App) -> &TickLog {
    app.world().resource::<TickLog>()
}

/// The player body's entity.
fn player_entity(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .iter(world)
        .next()
        .expect("the composition spawns exactly one player body")
}

/// The bound world actor whose measured subject spells `name`.
fn bound_actor(app: &mut App, name: &str) -> Entity {
    let world = app.world_mut();
    let mut query = world.query_filtered::<(Entity, &WorldActorBinding), ()>();
    query
        .iter(world)
        .find(|(_, binding)| subject_node_name(&binding.subject) == Some(name))
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("the composition binds the world actor `{name}`"))
}

/// Moves a body by writing **both** poses, the way the composition's own
/// overlay shift does: Avian copies `Transform` into `Position` for any body
/// whose `Position` did not change that tick, so writing only one of the two
/// would be overwritten or ignored.
fn teleport(app: &mut App, entity: Entity, to: [f64; 3]) {
    let target = Vec3::new(to[0] as f32, to[1] as f32, to[2] as f32);
    let world = app.world_mut();
    if let Some(mut position) = world.get_mut::<Position>(entity) {
        position.0 = target;
    }
    if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.translation = target;
    }
}

/// The member rows the host folded on its last step.
fn observed_members(app: &App) -> BTreeMap<MemberName, MemberObservation> {
    app.world()
        .resource::<MissionHost>()
        .world_facts()
        .observation()
        .members()
        .clone()
}

/// The group rows the host folded on its last step.
fn observed_groups(app: &App) -> BTreeMap<i32, u32> {
    app.world()
        .resource::<MissionHost>()
        .world_facts()
        .observation()
        .groups()
        .clone()
}

/// The live position of an entity, from the same source the observer reads:
/// the physics `Position` when the entity is a body, otherwise its
/// `Transform` — the binding entity's own pose.
fn live_position(app: &mut App, entity: Entity) -> [f64; 3] {
    let world = app.world();
    if let Some(position) = world.get::<Position>(entity) {
        let [x, y, z] = position.0.to_array();
        return [f64::from(x), f64::from(y), f64::from(z)];
    }
    let transform = world
        .get::<Transform>(entity)
        .expect("the entity carries a pose the observer can read");
    let [x, y, z] = transform.translation.to_array();
    [f64::from(x), f64::from(y), f64::from(z)]
}

/// The straight-line distance between two positions.
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(here, there)| (here - there) * (here - there))
        .sum::<f64>()
        .sqrt()
}

/// A synthetic program whose only world-shaped arm is M01's own `TRAVELERS`
/// spelling: block 0 completes the moment the observed player row comes
/// within 700 m of the observed `harbor` row, and its only action is the
/// record's own `Finish(Succeeded)`.
///
/// M01's block wraps the same arm in `ObjectiveAwake{2}`; the synthetic stage
/// carries no lifecycle table, so that gate would answer `false` forever
/// here — the retail member below runs the record's real `All([...])`.
fn proximity_program() -> cs_script::ir::MissionProgram {
    use cs_script::ir::{
        Action, Condition, IR_VERSION, Objective, Outcome, SymbolId, TravelersAnchor,
    };

    cs_script::ir::MissionProgram {
        version: IR_VERSION,
        mission: cid(
            cs_types::content::ContentKind::Mission,
            "synthetic-m01-observe",
        ),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(
                cs_types::content::ContentKind::Objective,
                "synthetic-m01-observe.objective.1",
            ),
            condition: Condition::Travelers {
                subject: member_chain([PLAYER_MEMBER]),
                anchor: TravelersAnchor::Object(member_chain(["harbor"])),
                radius: RADIUS_M,
                approaching: true,
            },
            actions: vec![Action::Finish(Outcome::Succeeded)],
            span: None,
        }],
    }
}

/// Binds one world actor in a synthetic run, the way the composition binds
/// M01's zeppelins: an entity carrying the production [`WorldActorBinding`]
/// whose subject is a `scene_node` id under the fixture container.
fn spawn_binding(app: &mut App, node: &str, at: [f64; 3]) -> Entity {
    use cs_app::scene::SceneGeneration;
    use cs_script::ir::ActorId;
    use cs_types::content::{ContentId, ContentKind};

    let subject = ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.synthetic.window.{node}"),
    )
    .expect("the synthetic subject id is valid");
    app.world_mut()
        .spawn((
            Transform::from_translation(Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32)),
            WorldActorBinding {
                actor: ActorId(1),
                subject,
                generation: SceneGeneration(1),
            },
        ))
        .id()
}

/// **The composed run observes the player's live pose in the very tick the
/// host folds it — CI leg, no installation.**
///
/// The synthetic stage spawns no `aiv.zrd` record and no world-actor
/// program, so `["player"]` is the only row this observer can write: after
/// one frame the host's own fact table must hold it at the body's current
/// position (proving the writer ran *before* the composed entry on that
/// tick, not one frame late), no group row may exist (nothing spawned) and no
/// other chain may be invented.
#[test]
fn accept_m01_play_triggers_observe_the_synthetic_run_observes_the_players_live_pose() {
    let scratch = Scratch::new("m01-play-observe-pose");
    let mut stage = synthetic_stage(scratch.path());
    stage.host.control = proximity_program();
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    arm(&mut app);
    app.update();

    let player = player_entity(&mut app);
    let members = observed_members(&app);
    let row = members
        .get(&member_chain([PLAYER_MEMBER]))
        .expect("the composed run reports the player row on its first tick");
    assert!(row.in_play, "the live player body is in play");
    let live = live_position(&mut app, player);
    assert!(
        distance(row.position, live) < 1.0e-3,
        "the reported player position must be the body's own pose: observed {:?}, live {live:?}",
        row.position
    );
    assert_eq!(
        members.len(),
        1,
        "the synthetic stage binds no world actor, so `player` is the only row: {members:?}"
    );
    let groups = observed_groups(&app);
    assert!(
        groups.is_empty(),
        "nothing spawned, so no group is observed — an unobserved group is never 0: {groups:?}"
    );

    // The row follows the body, every tick, read from the live world.
    teleport(&mut app, player, [1_000.0, 2_000.0, -3_000.0]);
    app.update();
    let row = observed_members(&app)
        .get(&member_chain([PLAYER_MEMBER]))
        .cloned()
        .expect("the player row is reported after the move");
    assert!(
        distance(row.position, [1_000.0, 2_000.0, -3_000.0]) < 2.0,
        "the reported position follows the moved body, {:?}",
        row.position
    );
    assert!(
        !log(&app).completed(0),
        "the anchor is nowhere near the player, so the arm cannot hold"
    );

    teardown(&mut app);
}

/// **A `TRAVELERS` arm fires only when the observed player row is inside the
/// radius — CI leg.**
///
/// Three legs over one composed run, all through the production schedule:
/// the bound anchor is far (no completion), the report is taken away with the
/// player parked inside the radius (no completion), and with the report back
/// the very next tick completes the block on the record's own
/// `Finish(Succeeded)`.
#[test]
fn accept_m01_play_triggers_observe_the_synthetic_trigger_fires_only_when_observed() {
    let scratch = Scratch::new("m01-play-observe-trigger");
    let mut stage = synthetic_stage(scratch.path());
    stage.host.control = proximity_program();
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    arm(&mut app);
    app.update();

    let player = player_entity(&mut app);
    let start = live_position(&mut app, player);
    let anchor = spawn_binding(&mut app, "harbor", [start[0] + 5_000.0, start[1], start[2]]);
    frames(&mut app, 4);
    let row = observed_members(&app)
        .get(&member_chain(["harbor"]))
        .cloned()
        .expect("the bound world actor is observed under its subject's node name");
    assert!(
        row.in_play,
        "a bound world actor the composition holds is in play"
    );
    assert!(
        distance(row.position, live_position(&mut app, anchor)) < 1.0e-3,
        "the anchor row is the binding entity's own live pose"
    );
    assert!(
        !log(&app).completed(0),
        "5 km apart: the arm cannot hold on an unobserved or out-of-range anchor"
    );

    // The report is taken away before the host consumes it: parked inside
    // the radius, the block still never completes.
    let anchor_position = live_position(&mut app, anchor);
    app.world_mut().resource_mut::<StripObservation>().0 = true;
    teleport(&mut app, player, anchor_position);
    frames(&mut app, 8);
    assert!(
        !log(&app).completed(0),
        "with the world report stripped the host folds no member row, so nothing can hold"
    );
    assert!(
        observed_members(&app).is_empty(),
        "a stripped run folds no member row at all: {:?}",
        observed_members(&app)
    );

    // The report is back: the first observed tick inside the radius fires.
    app.world_mut().resource_mut::<StripObservation>().0 = false;
    frames(&mut app, 8);
    assert!(
        log(&app).completed(0),
        "the observed player inside the observed anchor's radius completes the block"
    );
    let host = app.world().resource::<MissionHost>();
    let terminal = host
        .terminal()
        .expect("the record's own Finish(Succeeded) settles the run");
    assert_eq!(
        terminal.requested_by,
        Some(cs_script::ir::SymbolId(0)),
        "the terminal is asked for by the block the arm completed"
    );

    teardown(&mut app);
}

/// **M01's proximity trigger fires from the player's real position — and
/// from nothing else.**
///
/// Five legs, one composed run, all through the production schedule:
///
/// 1. the rows the live composition observes: the player, the three
///    zeppelins under their measured subject names, and exactly the groups
///    the composition spawned members for;
/// 2. the player parked at the mission start (5.3 km away) never completes
///    the block;
/// 3. another actor inside the radius never completes it either — the row
///    the condition reads is the **player's**;
/// 4. a run whose world report never reaches the host never completes it,
///    even with the player parked inside the radius;
/// 5. flying the body along a route completes it on the first tick the
///    *observed* distance drops under 700 m, and applies exactly the
///    record's own `NAP`/`WAKE`/`KILL` effects — including the nap's
///    re-wake, which a later tick's lifecycle section reports.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_observe_the_trigger_fires_from_the_players_real_position() {
    let stage = retail_stage();

    // The record's own effects on `OBJECTIVE3`, read from the recovered
    // engine model rather than written down: `NAP_OBJECTIVE 4` lands on
    // index 3 for 20 s, the wake list `27, 51` on indexes 26 and 50 and the
    // kill list `41, 48` on indexes 40 and 47 (the lowering subtracts the
    // record's 1-based numbering exactly once).
    let engine = stage
        .host
        .engine_objectives
        .as_ref()
        .expect("M01's stage carries the recovered engine lifecycle model");
    let declaration = engine
        .get(PROXIMITY_BLOCK as usize)
        .expect("OBJECTIVE3 loads")
        .declaration
        .clone();
    assert_eq!(
        declaration.nap,
        Some(NapDeclaration {
            target: Some(3),
            time: 20.0
        }),
        "OBJECTIVE3 naps OBJECTIVE4 (record `NAP_OBJECTIVE 4`) for 20 s"
    );
    assert_eq!(
        declaration.wake.targets(),
        &[26, 50],
        "OBJECTIVE3 wakes OBJECTIVE27 and OBJECTIVE51 (record `27`/`51`)"
    );
    assert_eq!(
        declaration.kill.targets(),
        &[40, 47],
        "OBJECTIVE3 kills OBJECTIVE41 and OBJECTIVE48 (record `41`/`48`)"
    );
    for index in [3u32, 26, 50, 40, 47] {
        assert_eq!(
            engine.get(index as usize).map(|objective| objective.state),
            Some(EngineState::Dormant),
            "block {index} starts dormant, so every effect below is observable as a transition"
        );
    }

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    arm(&mut app);
    app.update();

    // --- 1. the rows the live composition observes -----------------------
    let player = player_entity(&mut app);
    let anchor = bound_actor(&mut app, ANCHOR);
    for name in ["piratezep", ANCHOR, "blackswanzep"] {
        let entity = bound_actor(&mut app, name);
        let row = observed_members(&app)
            .get(&member_chain([name]))
            .cloned()
            .unwrap_or_else(|| panic!("the bound world actor `{name}` is observed"));
        assert!(row.in_play, "`{name}` is bound and in play");
        let live = live_position(&mut app, entity);
        assert!(
            distance(row.position, live) < 1.0e-3,
            "`{name}`'s row is the binding entity's own live pose: {:?} vs {live:?}",
            row.position
        );
    }
    let groups = observed_groups(&app);
    assert_eq!(
        groups.keys().copied().collect::<Vec<i32>>(),
        [1, 3, 5],
        "exactly the groups the composition spawned members for are observed: {groups:?} \
         (M01's group 2 and 4 records are the measured `enabled == 0` skips, so nothing of \
         them ever spawned)"
    );
    let host = app.world().resource::<MissionHost>();
    let facts = host.world_facts().facts(host.world_operands());
    assert_eq!(
        facts.groups.keys().copied().collect::<Vec<i32>>(),
        [1, 3],
        "the program asks about groups 1..=4, so the fold carries the observed ones and no row \
         at all for the unobserved 2 and 4: {:?}",
        facts.groups
    );

    // --- 2. parked at the mission start: no completion --------------------
    let start = live_position(&mut app, player);
    let anchor_position = live_position(&mut app, anchor);
    frames(&mut app, 30);
    assert!(
        !log(&app).completed(PROXIMITY_BLOCK),
        "the player starts {:.0} m from the anchor, far outside the radius",
        distance(start, anchor_position)
    );
    let row = observed_members(&app)
        .get(&member_chain([PLAYER_MEMBER]))
        .cloned()
        .expect("the player row exists");
    assert!(
        distance(row.position, anchor_position) > RADIUS_M,
        "the observed distance is outside the radius"
    );

    // --- 3. another actor inside the radius never completes it ------------
    let intruder = {
        let world = app.world_mut();
        let mut query =
            world.query_filtered::<Entity, With<cs_app::mission_actors::ScriptedAircraftActor>>();
        query
            .iter(world)
            .next()
            .expect("a scripted aircraft spawned")
    };
    let inside = [
        anchor_position[0] + RADIUS_M * 0.5,
        anchor_position[1],
        anchor_position[2],
    ];
    teleport(&mut app, intruder, inside);
    frames(&mut app, 30);
    assert!(
        !log(&app).completed(PROXIMITY_BLOCK),
        "a scripted aircraft inside the radius must not complete the player's trigger"
    );
    assert!(
        distance(
            observed_members(&app)[&member_chain([PLAYER_MEMBER])].position,
            anchor_position
        ) > RADIUS_M,
        "the player is still outside the radius"
    );

    // --- 4. no report reaches the host: no completion ---------------------
    app.world_mut().resource_mut::<StripObservation>().0 = true;
    teleport(&mut app, player, inside);
    frames(&mut app, 30);
    assert!(
        !log(&app).completed(PROXIMITY_BLOCK),
        "the player is inside the radius but the world report never reaches the host"
    );
    assert!(
        observed_members(&app).is_empty(),
        "with the report taken away the host folds no member row at all: {:?}",
        observed_members(&app)
    );
    app.world_mut().resource_mut::<StripObservation>().0 = false;

    // --- 5. flying the body along the route --------------------------------
    teleport(&mut app, player, start);
    let mut fired = false;
    for step in 0..=10u32 {
        let factor = f64::from(step) / 10.0;
        let waypoint = [
            start[0] + (anchor_position[0] - start[0]) * factor,
            start[1] + (anchor_position[1] - start[1]) * factor,
            start[2] + (anchor_position[2] - start[2]) * factor,
        ];
        teleport(&mut app, player, waypoint);
        frames(&mut app, 4);
        fired |= log(&app).completed(PROXIMITY_BLOCK);
        let rows = observed_members(&app);
        let observed_distance = distance(
            rows[&member_chain([PLAYER_MEMBER])].position,
            rows[&anchor_chain()].position,
        );
        if observed_distance < RADIUS_M {
            assert!(
                fired,
                "the block must complete on the tick the observed distance drops under the \
                 radius ({observed_distance:.1} m at waypoint {step})"
            );
        } else {
            assert!(
                !fired,
                "the block must not complete outside the radius ({observed_distance:.1} m at \
                 waypoint {step})"
            );
        }
    }

    // --- the record's own effects, read off the trigger tick itself --------
    //     `MissionHostTick.lifecycles` names the completed block on that tick,
    //     and the same host step has already applied the record's
    //     `WAKE`/`NAP`/`KILL` effects — so the tick's own snapshot is where
    //     they are measured. Every target loaded `Dormant` (asserted above),
    //     so each of these is a transition, not a pre-existing state.
    let trigger = log(&app).first_completion(PROXIMITY_BLOCK);
    assert!(trigger.completed.contains(&PROXIMITY_BLOCK));
    assert_eq!(
        trigger.effects[0],
        Some(ObjectiveLifecycle::Napping),
        "NAP_OBJECTIVE 4 (index 3) put OBJECTIVE4 into its 20 s nap on the trigger tick"
    );
    assert_eq!(
        trigger.effects[1],
        Some(ObjectiveLifecycle::Awake),
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE 27 (index 26) woke OBJECTIVE27 on the trigger tick"
    );
    assert_eq!(
        trigger.effects[2],
        Some(ObjectiveLifecycle::Awake),
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE 51 (index 50) woke OBJECTIVE51 on the trigger tick"
    );
    assert_eq!(
        trigger.effects[3], None,
        "KILL_OBJECTIVE_WHEN_I_COMPLETE 41 (index 40) removed OBJECTIVE41: a killed objective \
         projects no row at all"
    );
    assert_eq!(
        trigger.effects[4], None,
        "KILL_OBJECTIVE_WHEN_I_COMPLETE 48 (index 47) removed OBJECTIVE48"
    );

    // …and the nap's own re-wake, carried by a later tick's lifecycle
    // section: `woken` holds only what the engine's timer pass woke, so a `3`
    // there is the 20 s expiry of the nap this trigger armed (2 400 fixed
    // ticks = 1 200 frames of mission clock at the composition's own rate).
    if !log(&app).0.iter().any(|entry| entry.woken.contains(&3)) {
        frames(&mut app, 1_400);
    }
    assert!(
        log(&app).0.iter().any(|entry| entry.woken.contains(&3)),
        "the nap this trigger armed re-wakes OBJECTIVE4 on the engine's own timer, and the \
         tick report carries it"
    );

    teardown(&mut app);
}

/// **Group counts come from the live entities, and a group nobody spawned
/// stays unobserved.**
///
/// M01's eight `DEDG` sites name groups 1..=4. The composition spawns four
/// `rusdevastator_*` (group 1), one `bsfury_1` (group 3) and four group-5
/// aircraft; groups 2 and 4 have exactly one `aiv.zrd` record each —
/// `rusdevastator_3` and `bsfury_2` — and both are the measured
/// `enabled == 0` skips, so **nothing of them ever spawned**. This test
/// despawns group 1's four entities and expects the measured count (`0`) and
/// the resulting completion of `OBJECTIVE2`; it never expects a row for 2 or
/// 4.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_observe_group_counts_come_from_the_live_entities() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    arm(&mut app);
    app.update();

    // The records the composition really spawned, from its own report.
    let spawned: Vec<String> = app
        .world()
        .resource::<cs_app::mission_actors::MissionActorSpawnReport>()
        .aircraft
        .iter()
        .map(|row| row.record.clone())
        .collect();
    assert_eq!(spawned.len(), 9, "M01 spawns nine scripted aircraft");
    assert!(
        !spawned
            .iter()
            .any(|name| name == "rusdevastator_3" || name == "bsfury_2"),
        "the measured `enabled == 0` skip drops the only members of groups 2 and 4"
    );

    let group_one: Vec<Entity> = {
        let world = app.world_mut();
        let mut query =
            world.query_filtered::<(Entity, &cs_app::mission_actors::ScriptedAircraftActor), ()>();
        query
            .iter(world)
            .filter(|(_, actor)| {
                actor.record.group.and_then(|id| i32::try_from(id).ok()) == Some(1)
            })
            .map(|(entity, _)| entity)
            .collect()
    };
    assert_eq!(
        group_one.len(),
        4,
        "the measured roster puts exactly four rusdevastators in group 1"
    );
    assert_eq!(
        observed_groups(&app).get(&1).copied(),
        Some(4),
        "group 1 is observed at its live count"
    );
    assert!(
        !log(&app).completed(DEDG_GROUP_ONE_BLOCK),
        "the block reads group 1's live count, so it must not complete while its four members \
         are still in play"
    );

    // Take them away — the count follows the world, and the `DEDG` block
    // that reads it completes.
    for entity in group_one {
        app.world_mut().despawn(entity);
    }
    frames(&mut app, 30);
    assert_eq!(
        observed_groups(&app).get(&1),
        Some(&0),
        "a declared group whose members are gone reports the measured 0, not an absence"
    );
    assert!(
        log(&app).completed(DEDG_GROUP_ONE_BLOCK),
        "OBJECTIVE2 completes only when its four members are really gone"
    );

    // The two groups nobody spawned answer exactly as the fail-closed read
    // documents: no row in the observation, no key in the fold, so
    // `EnemyGroupDepletion` over them answers false forever.
    let groups = observed_groups(&app);
    assert!(
        !groups.contains_key(&2) && !groups.contains_key(&4),
        "groups 2 and 4 have no spawned member, so they stay unobserved: {groups:?}"
    );
    let host = app.world().resource::<MissionHost>();
    let facts = host.world_facts().facts(host.world_operands());
    assert!(
        !facts.groups.contains_key(&2) && !facts.groups.contains_key(&4),
        "the fold carries no row for an unobserved group: {:?}",
        facts.groups
    );
    for block in [16u32, 41, 42, 20, 43] {
        assert!(
            !log(&app).completed(block),
            "no block reading group 2 or 4 completes on a count nobody observed"
        );
    }

    teardown(&mut app);
}

/// **M01's `zeppelins.zrd` rows carry no group field, so no zeppelin group
/// is ever reported.**
///
/// Measured, not assumed: `aiv.zrd`'s AI group is field 4 of an 81-field
/// *positional* record, while a `zeppelins.zrd` record is a *keyed* list whose
/// 26-key vocabulary `ZeppelinKey` enumerates — and the production decoder
/// refuses any key outside it (`ZeppelinsError::UnknownKey`), so a `group`
/// key would have refused M01's whole member before this test could read it.
/// The census below is that production read over every carrying member of the
/// installation.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_observe_zeppelin_rows_carry_no_group_field() {
    use cs_content::pilots::survey_retail_zeppelin_carrier;
    use cs_formats::zbd::zeppelins::ZeppelinKey;

    let census = survey_retail_zeppelin_carrier(&game_dir())
        .expect("the production carrier census reads the installation");
    assert!(
        census.record_count() > 0,
        "the census decoded records through the production reader"
    );
    let m01 = census
        .missions()
        .iter()
        .find(|row| {
            row.group().eq_ignore_ascii_case("c1c") && row.mission().eq_ignore_ascii_case("m01")
        })
        .expect("M01's reader archive carries zeppelins.zrd");
    let member = m01.member().expect("M01's member decodes");
    assert_eq!(member.records().len(), 3, "M01 places three zeppelins");

    // Every key M01's three records state is one of the decoder's measured
    // keys — the reader refuses anything else — and the measured vocabulary
    // itself has no group field: `aiv.zrd`'s AI group is field 4 of an
    // 81-field *positional* record, a grammar these keyed records do not
    // share.
    assert_eq!(ZeppelinKey::ALL.len(), 26, "the measured key vocabulary");
    assert!(
        !ZeppelinKey::ALL
            .iter()
            .any(|key| format!("{key:?}").to_lowercase().contains("group")),
        "no key of the decoder's exhaustive vocabulary names a group"
    );
    for record in member.records() {
        for key in record.keys() {
            assert!(
                ZeppelinKey::ALL.contains(key),
                "`{}` states a key outside the measured vocabulary, which the decoder would \
                 have refused",
                record.node()
            );
        }
    }
}
