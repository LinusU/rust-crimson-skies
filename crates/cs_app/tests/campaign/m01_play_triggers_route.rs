//! Acceptance suite M01-PLAY-TRIGGERS (#1439): the **headless scripted route**
//! reaches M01's own `INSTANTWIN`.
//!
//! The parent's acceptance carries two clauses. The windowed half — "each
//! trigger fires from the player's actual position or actions in the windowed
//! mission" — stays with `M01-PLAY-TRIGGERS-WINDOWED` (#1486), which owns the
//! driven windowed run. This suite is the **headless** half: one scripted
//! flight over the composed retail M01, driven through the production
//! schedule only, settles the run on the record's own `Finish(Succeeded)`.
//!
//! The four slices the parent was split into each made one world fact
//! observable in the live composition:
//!
//! * M01-PLAY-TRIGGERS-OBSERVE (#1480) — the player row, the three bound
//!   zeppelins and the live group counts;
//! * M01-PLAY-TRIGGERS-PARTS (#1481) — the 39 part-state chains;
//! * M01-PLAY-TRIGGERS-ANIMS (#1482) — the `WAKE_ANIM` → record →
//!   `ANIM_STATE` gate join, with its arm (#1497) and land (#1498) halves;
//! * M01-PLAY-TRIGGERS-DEPLETION (#1483) — the `DEDG` groups 2 and 4 the
//!   original's spawn loop skips.
//!
//! Nothing here is `verified_original`; no original executable ran. Every
//! number and every state the assertions read is read out of the production
//! readers on this run.
//!
//! # The route
//!
//! The scripted flight is the player's own body, placed through the very
//! components the physics step commits and the stage-4 observer reads
//! ([`place_player`]), so every trigger it fires reads the *observed* player:
//!
//! 1. **the proximity trigger** — the body flies the leg from its composed
//!    start to `workersvoyagezep`; `OBJECTIVE3` (block 2) completes on the
//!    first observed tick inside the record's 700 m radius;
//! 2. **the fuel-tank / engine beat** — the eighteen `workersvoyagezep`
//!    engine parts leave play through the production damage authority (the
//!    seam the F27 gun step publishes into), completing the `INACTIVE`
//!    ladders `OBJECTIVE6` (block 5, one part) and `OBJECTIVE8` (block 7,
//!    five) and `OBJECTIVE9` (block 8, nine);
//! 3. **the drop-off beat** — the record's own `WAKE_ANIM` arms
//!    `wv_manual_land`; the body flies the low pass to that composed node and
//!    lands inside the row's window, running `wv_initiate_hookup`, which
//!    publishes the `wv_drop_copilot`/`wv_pickup_copilot` calls the gates
//!    read. The original plays a cutscene here; the task licenses this
//!    designed landing placeholder for it (`m01-dropoff.designed-placeholder`);
//! 4. **the Medusa wave** — the four group-1 `rusdevastator_*` aircraft leave
//!    the world, so `OBJECTIVE16` (block 15, group 1 == 0) completes; group 2
//!    carries only the measured `enabled == 0` skip, so `OBJECTIVE17` (block
//!    16) reads its measured 0;
//! 5. **the docking beat** — `OBJECTIVE18`'s own `WAKE_ANIM pzhomebase` arms
//!    the `piratezep` landing node; the body lands there, running
//!    `hooked_to_klondike`, which closes the block's own gate;
//! 6. **the win** — `OBJECTIVE18` wakes `OBJECTIVE24`, whose `INSTANTWIN`
//!    `Finish(Succeeded)` settles the run.
//!
//! The clock is compressed (one host step covers [`STEP`] of mission clock):
//! the win chain's own `NAP` re-wakes spell 123 s of mission clock, and the
//! measured M01-B-WAKE drive already establishes that nothing in the lowered
//! program reads the tick-to-second ratio — the compressed clock changes only
//! *how fast* the measured states arrive, never which ones do.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::prelude::{
    App, Entity, FixedPostUpdate, IntoScheduleConfigs, Quat, Resource, Vec3, With, World,
};
use bevy::time::{Fixed, Time, TimeUpdateStrategy};
use cs_app::mission_actors::ScriptedAircraftActor;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    DESIGNED_PART_INTEGRITY, MissionHost, MissionHostReport, MissionPartReport, MissionStage,
    MissionTerminal, build_headless, stage_for, subject_node_name, teardown,
};
use cs_app::weapons::SessionDamageResolver;
use cs_app::world_actors::WorldActorBinding;
use cs_script::ir::SymbolId;
use cs_sim::damage::{ActorId, DamageChannel, DamageNodeKey, HitEvent, HitEventId, PartState};
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_types::Tick;

use crate::common::label;
use crate::m01_play_triggers_anims_land::{Body, composed_node_position, place_player, pump};
use crate::vs_m01_rt_window::game_dir;

/// The mission clock one composed host step covers. The baseline composition
/// ticks at `BASELINE_FIXED_HZ`; the win chain's own `NAP` re-wakes spell
/// 123 s, which at the baseline is 7 380 host steps. A compressed step is the
/// M01-B-WAKE drive's own device (see the module doc).
const STEP: Duration = Duration::from_millis(500);

/// `OBJECTIVE3` — the one `TRAVELERS` proximity site.
const PROXIMITY_BLOCK: u32 = 2;
/// `OBJECTIVE6` / `OBJECTIVE8` / `OBJECTIVE9` — the three engine ladders.
const ENGINE_LADDER_BLOCKS: [u32; 3] = [5, 7, 8];
/// `OBJECTIVE11` — the `wv_drop_copilot RUNNING` gate the drop-off closes.
const DROP_COPILOT_BLOCK: u32 = 10;
/// `OBJECTIVE15` — the `wv_pickup_copilot EXECUTED` gate.
const PICKUP_COPILOT_BLOCK: u32 = 14;
/// `OBJECTIVE16` / `OBJECTIVE17` — the `DEDG` group-1 and group-2 blocks.
const GROUP_ONE_BLOCK: u32 = 15;
const GROUP_TWO_BLOCK: u32 = 16;
/// `OBJECTIVE18` — the `hooked_to_klondike EXECUTED` gate.
const HOOKED_BLOCK: u32 = 17;
/// `OBJECTIVE24` — the `INSTANTWIN` block.
const WIN_BLOCK: u32 = 23;

/// The measured subject name M01's `TRAVELERS` anchor spells.
const ANCHOR: &str = "workersvoyagezep";
/// The measured landing node the record's own drop-off `WAKE_ANIM` arms.
const DROPOFF_NODE: &str = "wv_manual_land";
/// A measured `piratezep` landing node the docking beat lands at.
const HOOKUP_NODE: &str = "pz_manual_land";
/// Its measured `auto` sibling: the `pzhomebase` wake's own vocabulary decides
/// which of the two this route may use.
const HOOKUP_AUTO_NODE: &str = "pz_auto_land";

/// The cruise speed of the scripted flight, in metres per second — inside the
/// landing rows' measured `[50, 220]` window, so only the node distance keeps
/// a cruising body from landing.
const CRUISE_MPS: f32 = 180.0;

/// How far above the anchor the body cruises while a beat is waited for: far
/// outside every landing radius, so no wait can satisfy a row by accident.
const CRUISE_CLEARANCE_M: f32 = 6_000.0;

/// The production stage for retail M01, with the recovered engine lifecycle
/// model in place — the same seed M01-B-WAKE drives.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// Every block that completed, across all host ticks.
#[derive(Resource, Default)]
struct Completed(Vec<u32>);

impl Completed {
    fn has(&self, block: u32) -> bool {
        self.0.contains(&block)
    }
}

fn record_completions(world: &mut World) {
    let Some(report) = world.get_resource::<MissionHostReport>() else {
        return;
    };
    let Ok(answer) = &report.answer else {
        return;
    };
    let done = answer.lifecycles.completed.clone();
    world.resource_mut::<Completed>().0.extend(done);
}

/// Installs the completion recorder after the host's own tick.
fn arm(app: &mut App) {
    app.insert_resource(Completed::default());
    app.add_systems(
        FixedPostUpdate,
        record_completions.after(cs_app::mission_session::mission_host_tick),
    );
}

fn completed(app: &App, block: u32) -> bool {
    app.world().resource::<Completed>().has(block)
}

/// The completed blocks, for a failure message.
fn completed_blocks(app: &App) -> Vec<u32> {
    let mut done = app.world().resource::<Completed>().0.clone();
    done.sort_unstable();
    done.dedup();
    done
}

/// Every lifecycle row the host projects, as `(block, state)` pairs, for a
/// failure message.
fn lifecycle_rows(app: &App) -> Vec<(u32, String)> {
    let host = app.world().resource::<MissionHost>();
    (0..host.lifecycles().len() as u32)
        .filter_map(|index| {
            host.lifecycles()
                .state(index)
                .map(|state| (index, format!("{state:?}")))
        })
        .collect()
}

/// One composed step with the compressed clock: exactly one host tick, and
/// [`STEP`] of mission clock.
fn step(app: &mut App) {
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(STEP);
    app.update();
}

/// The player body's entity.
fn player_entity(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<cs_app::mission_session::MissionPlayerBody>>()
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

/// The live position of an entity, from the source the observer reads.
fn live_position(app: &mut App, entity: Entity) -> [f64; 3] {
    let world = app.world();
    let [x, y, z] = world
        .get::<avian3d::prelude::Position>(entity)
        .map_or_else(
            || {
                world
                    .get::<bevy::prelude::Transform>(entity)
                    .expect("the entity carries a pose the observer can read")
                    .translation
            },
            |position| position.0,
        )
        .to_array();
    [f64::from(x), f64::from(y), f64::from(z)]
}

fn vec3(position: [f64; 3]) -> Vec3 {
    Vec3::new(position[0] as f32, position[1] as f32, position[2] as f32)
}

/// Flies the player's body along the straight leg from its current pose to
/// `to`, at `CRUISE_MPS`, committing one composed step per [`STEP`] of flight
/// — so the observed player really moves along the route rather than jumping
/// between waypoints. `player` is the body the composition spawns; the
/// placement goes through [`place_player`]'s own query, exactly as the
/// composed physics step reads it.
fn fly_to(app: &mut App, player: Entity, from: Vec3, to: Vec3) {
    let _ = player;
    let leg = to - from;
    let distance = leg.length();
    let per_step = CRUISE_MPS * STEP.as_secs_f32();
    let steps = (distance / per_step).ceil().max(1.0) as usize;
    let heading = leg.normalize_or_zero();
    for index in 1..=steps {
        let factor = index as f32 / steps as f32;
        let waypoint = from + leg * factor;
        place_player(app, Some(waypoint), heading * CRUISE_MPS, Quat::IDENTITY);
        step(app);
    }
}

/// The body that cruises clear of every landing node.
fn cruise(clear_of: Vec3) -> Body {
    Body {
        position: Some(clear_of + Vec3::new(0.0, CRUISE_CLEARANCE_M, 0.0)),
        velocity: Vec3::new(CRUISE_MPS, 0.0, 0.0),
        rotation: Quat::IDENTITY,
    }
}

/// Stops the body at `position` inside the row's measured window — a landing.
fn landing(position: Vec3) -> Body {
    Body {
        position: Some(position),
        velocity: Vec3::new(60.0, -2.0, 0.0),
        rotation: Quat::IDENTITY,
    }
}

/// Whether the host has folded an `OBJECT_ACTIVE_STATE` statement naming
/// `node`.
fn armed_has(app: &App, node: &str) -> bool {
    app.world()
        .resource::<MissionHost>()
        .armed_nodes()
        .iter()
        .any(|armed| armed.nodes().iter().any(|name| name == node))
}

/// Whether the host's lifecycle model currently projects `block` as `Awake`.
fn awake(app: &App, block: u32) -> bool {
    app.world()
        .resource::<MissionHost>()
        .lifecycles()
        .state(block)
        == Some(cs_script::runtime::ObjectiveLifecycle::Awake)
}

/// Steps the cruise until `block`'s own lifecycle row reads `Awake` — the
/// moment the record's chain admits the block, which is when its own wake
/// effects (and its own gate) run.
fn cruise_until_awake(app: &mut App, block: u32, clear_of: Vec3, limit: usize) {
    for _ in 0..limit {
        pump(app, 1, Some(&cruise(clear_of)));
        if awake(app, block) || completed(app, block) {
            return;
        }
    }
    panic!(
        "block {block} never read Awake within {limit} steps; completed blocks: {:?}",
        completed_blocks(app)
    );
}

/// Steps the cruise until `node` is armed, or panics naming what was wanted.
fn cruise_until_armed(app: &mut App, node: &str, clear_of: Vec3, limit: usize) {
    for _ in 0..limit {
        pump(app, 1, Some(&cruise(clear_of)));
        if armed_has(app, node) {
            return;
        }
    }
    panic!(
        "no `OBJECT_ACTIVE_STATE` statement armed {node} within {limit} steps; completed blocks: {:?}",
        completed_blocks(app)
    );
}

/// The run's own state, for a failure message: what completed, what the
/// lifecycle model projects, the observed animation rows, and what the
/// `CALL_ANIMATION` ledger started or refused.
fn route_diagnostics(app: &App) -> String {
    let host = app.world().resource::<MissionHost>();
    let animations = host.world_facts().observation().animations();
    let calls: Vec<String> = host
        .call_animations()
        .activations()
        .iter()
        .map(|call| format!("{}({})->{:?}", call.caller(), call.name(), call.outcome()))
        .collect();
    let refused: Vec<String> = host
        .call_animations()
        .refused()
        .iter()
        .map(|refusal| {
            format!(
                "{}({})={}",
                refusal.caller(),
                refusal.name(),
                refusal.reason()
            )
        })
        .collect();
    format!(
        "completed: {:?}; lifecycles: {:?}; animations: {:?}; calls: {calls:?}; refused: {refused:?}",
        completed_blocks(app),
        lifecycle_rows(app),
        animations
    )
}

/// Steps the cruise until one of `nodes` is armed, answering the first one
/// that is — the `pzhomebase` wake's own `OBJECT_ACTIVE_STATE` vocabulary
/// decides which `piratezep` landing node this route may use.
fn cruise_until_any_armed(app: &mut App, nodes: &[&str], clear_of: Vec3, limit: usize) -> String {
    for _ in 0..limit {
        pump(app, 1, Some(&cruise(clear_of)));
        if let Some(node) = nodes.iter().find(|node| armed_has(app, node)) {
            return (*node).to_owned();
        }
    }
    panic!(
        "none of {nodes:?} was armed within {limit} steps; {}",
        route_diagnostics(app)
    );
}

/// Destroys one part through the production damage authority, exactly as the
/// composed weapon step applies the hits it routes.
fn destroy_part(app: &mut App, actor: ActorId, node: &str) -> PartState {
    let tick = Tick(0);
    let key = DamageNodeKey::new(node).expect("the node key is valid");
    let hit = HitEvent::try_new(
        HitEventId {
            session: actor.session,
            tick,
            producer: u32::MAX,
            sequence: 0,
        },
        None,
        actor,
        key.clone(),
        DamageChannel::Internal,
        DESIGNED_PART_INTEGRITY * 2.0,
    )
    .expect("the hit is well formed");
    let mut shared = app.world_mut().resource_mut::<SessionDamageResolver>();
    let resolver = shared.resolver_mut();
    resolver
        .resolve(tick, &[hit])
        .expect("the damage authority resolves the hit");
    resolver
        .part_state(&actor, &key)
        .expect("the part node is registered")
}

/// Takes `workersvoyagezep`'s eighteen engine chains out of play through the
/// production damage authority, in the record's own `reng<n>`/`leng<n>`
/// spelling.
fn destroy_workers_engines(app: &mut App) {
    let parts: Vec<(String, ActorId)> = {
        let report = app
            .world()
            .get_resource::<MissionPartReport>()
            .expect("the parts report is installed")
            .clone();
        report
            .parts
            .iter()
            .filter(|part| {
                part.chain.first().map(String::as_str) == Some(ANCHOR)
                    && part.chain.get(2).map(String::as_str) == Some("healthy")
                    && part
                        .chain
                        .get(1)
                        .is_some_and(|node| node.starts_with("reng") || node.starts_with("leng"))
            })
            .filter_map(|part| Some((part.chain.get(1)?.clone(), part.actor)))
            .collect()
    };
    assert_eq!(
        parts.len(),
        18,
        "workersvoyagezep carries the measured eighteen engine chains"
    );
    for (node, actor) in parts {
        assert_eq!(
            destroy_part(app, actor, &node),
            PartState::Destroyed,
            "the engine part {node} is destroyed"
        );
    }
    // The three ladders are woken in turn (`OBJECTIVE6` wakes `OBJECTIVE8`,
    // which wakes `OBJECTIVE9`), so each needs a tick of its own to fold the
    // observed part rows and complete.
    for _ in 0..600 {
        step(app);
        if ENGINE_LADDER_BLOCKS
            .iter()
            .all(|block| completed(app, *block))
        {
            return;
        }
    }
    panic!(
        "the engine ladders did not all complete on the live world's own state; completed: {:?}",
        completed_blocks(app)
    );
}

/// Empties group 1 — the four `rusdevastator_*` the roster spawned.
fn deplete_group_one(app: &mut App) {
    let group_one: Vec<Entity> = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(Entity, &ScriptedAircraftActor), ()>();
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
        "the measured roster puts four rusdevastators in group 1"
    );
    for entity in group_one {
        app.world_mut().despawn(entity);
    }
}

/// The terminal the run settled on, or a panic with the run's own state.
fn expect_win(app: &App) -> MissionTerminal {
    let host = app.world().resource::<MissionHost>();
    host.terminal().cloned().unwrap_or_else(|| {
        panic!(
            "the scripted route did not settle; {}",
            route_diagnostics(app)
        )
    })
}

/// **A headless scripted run flies M01's route and reaches `INSTANTWIN`.**
///
/// One composed retail run, one scripted flight, every beat through the
/// production path: the proximity trigger from the observed player's position,
/// the engine ladders from the damage authority's out-of-play parts, the
/// `ANIM_STATE` gates from real landings at armed nodes, the `DEDG` counts
/// from the live entities, and the terminal from the control program's own
/// `Finish(Succeeded)`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_route_a_scripted_flight_reaches_instantwin() {
    let stage = retail_stage();
    assert!(
        stage.host.engine_objectives.is_some(),
        "the route drives the recovered engine lifecycle model, not a declared table"
    );
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    arm(&mut app);
    app.world_mut()
        .resource_mut::<Time<Fixed>>()
        .set_timestep(STEP);
    pump(&mut app, 1, None);

    let player = player_entity(&mut app);
    let anchor = bound_actor(&mut app, ANCHOR);
    let start = vec3(live_position(&mut app, player));
    let anchor_position = vec3(live_position(&mut app, anchor));

    // --- 1. the proximity trigger, from the player's own flown position -----
    //      The leg approaches the anchor **from above**: the drop-off node
    //      hangs ~165 m astern and below it, and the route may not come inside
    //      the landing radius before the designed drop-off beat, or an
    //      accidental landing would run the record before its own gate block
    //      is awake (the node is armed from the mission's own intro records).
    let approach = anchor_position + Vec3::new(0.0, 500.0, 0.0);
    fly_to(&mut app, player, start, approach);
    assert!(
        completed(&app, PROXIMITY_BLOCK),
        "OBJECTIVE3 completes when the observed player enters the observed anchor's 700 m \
         radius; completed: {:?}",
        completed_blocks(&app)
    );
    eprintln!("route: proximity trigger fired");

    // --- 2. the fuel-tank/engine beat: the eighteen engine parts leave play -
    destroy_workers_engines(&mut app);
    eprintln!("route: engine ladders completed");

    // --- 4. the Medusa wave: group 1's four aircraft leave the world --------
    //      (done before the drop-off so both `DEDG` blocks are ready when the
    //      chain reaches them)
    deplete_group_one(&mut app);

    // --- 3. the drop-off beat: land at the armed `wv_manual_land` -----------
    //      The chain arms the node (block 48's own wake) *before* it wakes the
    //      gate block `OBJECTIVE11` (block 49's 5 s nap away), and the gate
    //      reads `wv_drop_copilot RUNNING` — a state the landing's record
    //      publishes for the single tick its call runs. The designed drop-off
    //      placeholder therefore waits for the gate block to be awake before
    //      the low pass, so the landing's own output is what closes it.
    cruise_until_awake(&mut app, DROP_COPILOT_BLOCK, anchor_position, 8_000);
    cruise_until_armed(&mut app, DROPOFF_NODE, anchor_position, 4_000);
    assert!(
        app.world()
            .resource::<MissionHost>()
            .landings()
            .activated()
            .is_empty(),
        "the route must not have landed before the drop-off beat: {:?}",
        app.world().resource::<MissionHost>().landings().activated()
    );
    eprintln!("route: {DROPOFF_NODE} armed and block {DROP_COPILOT_BLOCK} awake");
    let dropoff = composed_node_position(&app, DROPOFF_NODE);
    pump(&mut app, 4, Some(&landing(dropoff)));
    {
        let host = app.world().resource::<MissionHost>();
        let activated = host.landings().activated();
        assert!(
            activated
                .iter()
                .any(|row| row.node == DROPOFF_NODE && row.anim == "wv_initiate_hookup"),
            "the low pass at the armed node runs the record's own row: {activated:?}; {}",
            route_diagnostics(&app)
        );
    }

    // --- 5. the docking beat: land at the armed `pz_manual_land` -------------
    //      `OBJECTIVE18`'s own wake arms the node; the chain reaches it once
    //      the `wv_pickup_copilot` gate, both group depletions and the `NAP`
    //      re-wakes have advanced. Which `piratezep` node the wake's own
    //      vocabulary arms is the record's answer, not this test's.
    let hookup = cruise_until_any_armed(
        &mut app,
        &[HOOKUP_NODE, HOOKUP_AUTO_NODE],
        anchor_position,
        8_000,
    );
    eprintln!("route: {hookup} armed");
    let hookup_position = composed_node_position(&app, &hookup);
    pump(&mut app, 4, Some(&landing(hookup_position)));
    {
        let host = app.world().resource::<MissionHost>();
        let activated = host.landings().activated();
        assert!(
            activated
                .iter()
                .any(|row| row.node == hookup && row.anim == "hooked_to_klondike"),
            "the docking pass at the armed node runs the record's own row: {activated:?}"
        );
    }

    // --- 6. the win ---------------------------------------------------------
    //      The terminal's wake and the animations' own measured durations
    //      need a bounded number of further steps; the naps are already
    //      behind us at the compressed clock.
    for _ in 0..4_000 {
        pump(&mut app, 1, Some(&cruise(anchor_position)));
        if app.world().resource::<MissionHost>().terminal().is_some() {
            break;
        }
    }
    let terminal = expect_win(&app);
    assert_eq!(
        terminal.outcome,
        TerminalOutcome::Success,
        "the scripted route ends on the record's own INSTANTWIN"
    );
    assert_eq!(
        terminal.source,
        cs_app::mission_session::MissionTerminalSource::ControlProgram,
        "the control program's own terminal is this run's source"
    );
    assert_eq!(
        terminal.requested_by,
        Some(SymbolId(WIN_BLOCK)),
        "the block whose Finish asked is OBJECTIVE24"
    );

    // Every gate the route had to close really closed on the live world's own
    // state, so the terminal is attributable to the route and not to a
    // shortcut.
    for block in [
        GROUP_ONE_BLOCK,
        GROUP_TWO_BLOCK,
        DROP_COPILOT_BLOCK,
        PICKUP_COPILOT_BLOCK,
        HOOKED_BLOCK,
    ] {
        assert!(
            completed(&app, block),
            "the route closed the gate at block {block}; completed: {:?}",
            completed_blocks(&app)
        );
    }

    // No run where the world did not report these facts could have settled:
    // the observed part rows really left play and the group counts really
    // reached zero.
    let host = app.world().resource::<MissionHost>();
    let facts = host.world_facts().facts(host.world_operands());
    let workers_out: usize = facts
        .members
        .iter()
        .filter(|(chain, row)| {
            chain.first().map(String::as_str) == Some(ANCHOR)
                && chain.get(2).map(String::as_str) == Some("healthy")
                && row.presence == cs_script::runtime::MemberPresence::OutOfPlay
        })
        .count();
    assert!(
        workers_out >= 9,
        "at least nine engine chains are observed out of play: {workers_out}"
    );
    assert_eq!(
        facts.groups.get(&1),
        Some(&0),
        "group 1 is observed empty: {:?}",
        facts.groups
    );
    assert_eq!(
        facts.groups.get(&2),
        Some(&0),
        "group 2 reads its measured zero: {:?}",
        facts.groups
    );

    // The route's own landings: the drop-off node, and **both** `piratezep`
    // rows — `pzhomebase`'s own `OBJECT_ACTIVE_STATE` vocabulary arms both
    // `pz_manual_land` and `pz_auto_land`, whose composed positions sit close
    // enough that the docking pass satisfies the windowed row and its `auto`
    // sibling in the same placement. Nothing else landed.
    let landed: BTreeSet<String> = host
        .landings()
        .activated()
        .iter()
        .map(|row| row.node.clone())
        .collect();
    assert_eq!(
        landed,
        BTreeSet::from([
            DROPOFF_NODE.to_owned(),
            HOOKUP_NODE.to_owned(),
            HOOKUP_AUTO_NODE.to_owned(),
        ]),
        "exactly the route's three landings ran"
    );

    teardown(&mut app);
}
