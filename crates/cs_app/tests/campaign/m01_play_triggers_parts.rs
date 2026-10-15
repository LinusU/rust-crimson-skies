//! Acceptance suite M01-PLAY-TRIGGERS-PARTS (#1481): M01's zeppelin
//! part-state chains are **observable in the live world**, so the `INACTIVE`
//! ladders that count them advance when a part really leaves play.
//!
//! M01-PLAY-TRIGGERS-OBSERVE (#1480) wrote the world observation from the live
//! composition but could not see the 39 part-state chains
//! (`["workersvoyagezep", "reng1", "healthy"]`, …): the world import makes an
//! object only for an `object3d` record the world node owns directly, so an
//! engine's `healthy` state node had no entity anywhere and every ladder over it
//! stayed `false` forever.
//!
//! This suite pins the parts binding (`mission_session::parts`, installed by the
//! spawn host) and the observation rows it produces:
//!
//! * **the rows** — one live part-state entity per spelled chain, reported by
//!   [`mission_world_observation`] at the owning actor's pose while the session's
//!   damage authority says the part is intact, and reported *out of play* the
//!   moment that authority says `Destroyed` — which is exactly the `INACTIVE`
//!   read;
//! * **the ladder** — M01's block index 5 (`OBJECTIVE6`: at least 1 of
//!   `workersvoyagezep`'s 18 engine chains out of play) completes on the live
//!   world's own state, the higher-threshold ladder over the *same* chains
//!   (index 7, threshold 5) stalls exactly one short and then completes on the
//!   fifth part, and the other actor's ladder (index 35, `piratezep`, threshold
//!   2) is never satisfied by a `workersvoyagezep` part going out;
//! * **fail-closed** — with the parts provider removed the chains are unobserved
//!   again and the ladder stalls, exactly as it did before this stage; and a
//!   part whose pool nothing resolved is **not** reported in play.
//!
//! What makes a part destroyable at all is
//! [`mission_session::parts::DESIGNED_PART_INTEGRITY`]: a **labelled designed**
//! single-unit pool, because the installation states no engine integrity
//! anywhere (`docs/findings/2026-10-15-m01-play-triggers-part-state-presence.md`).
//! The destruction itself goes through the production damage authority — the
//! same `DamageResolver::resolve` seam the F27 gun step and the F28 ordnance step
//! publish into — never a despawn from the test.
//!
//! Nothing here is `verified_original`: no original executable ran, and every
//! measured number is read out of the production readers on this run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::{App, Entity};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionPartReport, MissionPartState, MissionStage, build_headless, part_in_play,
    stage_for, teardown,
};
use cs_app::weapons::SessionDamageResolver;
use cs_app::world_actors::LoweredWorldActors;
use cs_app::world_facts::{MemberObservation, MemberResolver};
use cs_script::ir::{
    Action, ActorId as WorldActorId, Condition, IR_VERSION, MemberName, MissionProgram, Objective,
    Outcome, SymbolId,
};
use cs_script::runtime::{MemberPresence, ObjectiveLifecycle};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageGraph, DamageNode, DamageNodeKey,
    DamageNodeKind, DamagePolicy, DamageResolver, HitEvent, HitEventId, PartState,
};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl};
use cs_sim::world_actors::runtime::{ActorMotion, WorldActorKind, WorldActorSpec};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::ClaimId;

use crate::common::{cid, label};
use crate::vs_m01_rt_window::{Scratch, synthetic_stage};

/// `OBJECTIVE6`: the 18 `workersvoyagezep` engine chains, threshold 1.
const ENGINE_LADDER_BLOCK: u32 = 5;
/// `OBJECTIVE8`: the same 18 chains, threshold 5.
const ENGINE_THRESHOLD_FIVE_BLOCK: u32 = 7;
/// `OBJECTIVE36`: `piratezep`'s 12 engine chains, threshold 2.
const PIRATE_LADDER_BLOCK: u32 = 35;
/// The synthetic chains the CI legs bind.
const SYNTHETIC_CHAINS: [&str; 2] = ["engine1", "engine2"];
/// The synthetic world actor's node name.
const SYNTHETIC_ACTOR: &str = "syntheticzep";

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The production stage for retail M01.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// One member chain out of its spellings.
fn member(parts: &[&str]) -> MemberName {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

/// The surviving `MissionHostReport` answer's completed blocks, when there is
/// one.
fn last_completed(app: &App) -> Vec<u32> {
    app.world()
        .get_resource::<cs_app::mission_session::MissionHostReport>()
        .and_then(|report| report.answer.as_ref().ok())
        .map(|answer| answer.lifecycles.completed.clone())
        .unwrap_or_default()
}

/// Every part the composition bound, with its own entity and damage actor.
fn bound_parts(app: &mut App) -> Vec<(MemberName, ActorId, Entity)> {
    let report = app
        .world()
        .get_resource::<MissionPartReport>()
        .cloned()
        .unwrap_or_default();
    report
        .parts
        .iter()
        .map(|part| (part.chain.clone(), part.actor, part.entity))
        .collect()
}

/// The part entity bound for one chain, or a panic naming the chain.
fn part_entity(app: &mut App, chain: &MemberName) -> (Entity, ActorId) {
    bound_parts(app)
        .into_iter()
        .find(|(bound, _, _)| bound == chain)
        .map(|(_, actor, entity)| (entity, actor))
        .unwrap_or_else(|| panic!("the composition binds a part for {chain:?}"))
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

/// Whether the host's lifecycle model projects `block` as completed (`Done` is
/// what the engine's own completion sets, `+0x5c8 = 3`).
fn completed(app: &App, block: u32) -> bool {
    app.world()
        .resource::<MissionHost>()
        .lifecycles()
        .state(block)
        == Some(ObjectiveLifecycle::Done)
}

/// Runs `count` composed frames.
fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
    }
}

/// Destroys one part through the **production damage authority** — the seam the
/// F27 gun step and the F28 ordnance step publish into — and answers the part
/// state the resolver now holds.
///
/// This is not a despawn from the test: the hit is a [`HitEvent`] carrying the
/// resolver's own session and tick, addressed to the part's damage node, applied
/// by [`DamageResolver::resolve`] exactly as the composed weapon step applies
/// the hits it routes.
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
        cs_app::mission_session::DESIGNED_PART_INTEGRITY * 2.0,
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

// --------------------------------------------------------------- retail ---

/// **M01 binds one live part-state entity per spelled chain, and the observer
/// reports all of them in play.**
///
/// The measured population: 27 `workersvoyagezep` chains (its 18 engine nodes
/// `reng1`…`leng9` plus the nine turret chains `utur1`…`utur4`/`ctur1`…`ctur4`
/// and `maingun9`) and 12 `piratezep` engine chains — 39 in all. The three
/// remaining operands are not part-state chains: `["player"]` resolves against
/// no node, and `["workersvoyagezep"]`/`["piratezep"]` name the actors
/// themselves, which are #1480's own actor rows.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_parts_m01_binds_one_live_part_per_spelled_chain() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let parts = bound_parts(&mut app);
    assert_eq!(
        parts.len(),
        39,
        "M01's 42 operands spell 39 part-state chains (27 workersvoyagezep + 12 piratezep)"
    );
    let workers = parts
        .iter()
        .filter(|(chain, _, _)| chain.first().map(String::as_str) == Some("workersvoyagezep"))
        .count();
    let pirates = parts
        .iter()
        .filter(|(chain, _, _)| chain.first().map(String::as_str) == Some("piratezep"))
        .count();
    assert_eq!(workers, 27, "workersvoyagezep carries 27 part chains");
    assert_eq!(pirates, 12, "piratezep carries 12 part chains");

    let report = app
        .world()
        .get_resource::<MissionPartReport>()
        .expect("the parts report is installed")
        .clone();
    assert!(
        report.refusals.is_empty(),
        "the binding refused nothing: {:?}",
        report.refusals
    );

    // Every chain is observed, in play, and carries an intact node on its
    // owner's graph.
    let rows = observed_members(&app);
    let host = app.world().resource::<MissionHost>();
    let facts = host.world_facts().facts(host.world_operands());
    assert_eq!(
        facts
            .members
            .values()
            .filter(|row| row.presence == MemberPresence::InPlay)
            .count(),
        42,
        "the fold carries the 39 part rows plus the player and the two actors: {:?}",
        facts.members.keys().collect::<Vec<_>>()
    );
    let resolver = app.world().resource::<SessionDamageResolver>().resolver();
    for part in &report.parts {
        let row = rows
            .get(&part.chain)
            .unwrap_or_else(|| panic!("{} is observed", part.chain.join("/")));
        assert!(row.in_play, "{} is in play", part.chain.join("/"));
        assert_eq!(
            resolver.part_state(&part.actor, &part.node),
            Some(PartState::Intact),
            "{} starts intact",
            part.chain.join("/")
        );
    }
    for (actor, nodes) in &report.nodes {
        let graph = resolver
            .graph(actor)
            .expect("the owning actor is registered");
        for node in nodes {
            assert!(
                graph.node(node).is_some(),
                "{node} is a node of the actor's own graph"
            );
        }
    }

    // The two actor-level operands are not parts: their nodes' ladders read the
    // actor rows #1480 writes, and this stage adds no part entity for them.
    for chain in [member(&["workersvoyagezep"]), member(&["piratezep"])] {
        assert!(
            !report.parts.iter().any(|part| part.chain == chain),
            "{} names the actor itself, not a part",
            chain.join("/")
        );
    }

    teardown(&mut app);
}

/// **A destroyed engine part leaves play, the ladder that counts it completes,
/// and the other ladders do not.**
///
/// M01's measured chain groups: block index 5 is `workersvoyagezep`'s 18 engine
/// chains at threshold 1, index 7 the same 18 at threshold 5 and index 35
/// `piratezep`'s 12 at threshold 2. One engine part destroyed therefore
/// completes index 5 and must not complete index 7 or index 35; four more
/// complete index 7 (whose block index 5's own completion wakes).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_parts_a_destroyed_engine_completes_its_ladder_and_not_the_others() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    frames(&mut app, 4);
    assert!(
        !completed(&app, ENGINE_LADDER_BLOCK)
            && !completed(&app, ENGINE_THRESHOLD_FIVE_BLOCK)
            && !completed(&app, PIRATE_LADDER_BLOCK),
        "every part is in play, so no engine ladder is armed yet"
    );

    let chain = member(&["workersvoyagezep", "reng1", "healthy"]);
    let (_, actor) = part_entity(&mut app, &chain);
    assert_eq!(destroy_part(&mut app, actor, "reng1"), PartState::Destroyed);
    frames(&mut app, 4);

    let rows = observed_members(&app);
    let row = rows
        .get(&chain)
        .unwrap_or_else(|| panic!("the destroyed part is still reported: {rows:?}"));
    assert!(
        !row.in_play,
        "the destroyed part is reported out of play — that is the INACTIVE read"
    );
    assert!(
        completed(&app, ENGINE_LADDER_BLOCK),
        "OBJECTIVE6 (>= 1 of the 18 engine chains out of play) advances from the live world; \
         completed blocks on the last tick: {:?}",
        last_completed(&app)
    );
    assert!(
        !completed(&app, ENGINE_THRESHOLD_FIVE_BLOCK),
        "OBJECTIVE8 needs five of the same 18, so one destroyed part must not satisfy it"
    );
    assert!(
        !completed(&app, PIRATE_LADDER_BLOCK),
        "OBJECTIVE36 counts piratezep's chains, and a workersvoyagezep part is the wrong chain"
    );

    for part in ["reng2", "reng3", "leng1", "leng2"] {
        let chain = member(&["workersvoyagezep", part, "healthy"]);
        let (_, actor) = part_entity(&mut app, &chain);
        destroy_part(&mut app, actor, part);
    }
    frames(&mut app, 6);
    assert!(
        completed(&app, ENGINE_THRESHOLD_FIVE_BLOCK),
        "the fifth engine to leave play completes OBJECTIVE8, the block OBJECTIVE6's own \
         completion wakes"
    );
    assert!(
        !completed(&app, PIRATE_LADDER_BLOCK),
        "no piratezep part left play, so OBJECTIVE36 still reads its own chains"
    );

    teardown(&mut app);
}

// ------------------------------------------------------------ synthetic ---

/// A synthetic stage whose host carries one bound world actor and two
/// part-state chains, so the production parts binder runs with no installation:
/// the world-actor program registers one held actor whose subject is
/// `scene_node/…syntheticzep` and the resolver pins `syntheticzep` →
/// `engine1`/`engine2` → `healthy`.
fn synthetic_parts_stage(scratch: &Path, program: MissionProgram) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    let mut blocks = BlockLifecycleTable::new();
    for index in 0..program.objectives.len() {
        blocks
            .declare(index as u32, LifecycleDecl::awake())
            .expect("each synthetic block is declared once");
    }
    stage.host.block_lifecycles = blocks;
    stage.host.control = program;
    stage.host.resolver = MemberResolver::from_hierarchy([
        (SYNTHETIC_ACTOR.to_owned(), None),
        (SYNTHETIC_CHAINS[0].to_owned(), Some(0)),
        (SYNTHETIC_CHAINS[1].to_owned(), Some(0)),
        ("healthy".to_owned(), Some(1)),
        ("healthy".to_owned(), Some(2)),
    ]);
    stage.host.world_actors = Some(LoweredWorldActors {
        ticks_per_second: 30,
        actors: vec![WorldActorSpec {
            actor: WorldActorId(1),
            kind: WorldActorKind::Kinematic,
            faction: cid(ContentKind::Faction, "synthetic.faction"),
            objective: None,
            motion: ActorMotion::Held {
                position_m: [1_000.0, 100.0, -2_000.0],
                orientation: cs_sim::world_actors::Quat::IDENTITY,
            },
        }],
        subjects: BTreeMap::from([(WorldActorId(1), synthetic_subject())]),
        sockets: BTreeMap::new(),
        support: Vec::new(),
        pickups: Vec::new(),
        transitions: Vec::new(),
    });
    stage
}

/// The synthetic actor's joined subject, spelled the way the composition's own
/// scene-node subjects are.
fn synthetic_subject() -> ContentId {
    ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.synthetic.window.{SYNTHETIC_ACTOR}"),
    )
    .expect("the synthetic subject id is valid")
}

/// A synthetic program whose one block is an `InactiveMembers` ladder over the
/// named synthetic chains: `threshold` cleared members complete it.
fn ladder_program(threshold: u32, chains: &[&str]) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-m01-parts"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(ContentKind::Objective, "synthetic-m01-parts.objective.1"),
            condition: Condition::InactiveMembers {
                members: chains
                    .iter()
                    .map(|part| member(&[SYNTHETIC_ACTOR, part, "healthy"]))
                    .collect(),
                threshold,
            },
            actions: vec![Action::Finish(Outcome::Succeeded)],
            span: None,
        }],
    }
}

/// A synthetic program whose ladder counts one chain while a second block reads
/// the *other* chain: both are operands (so both get a live part) and only the
/// first can complete — the wrong-chain leg.
fn wrong_chain_program() -> MissionProgram {
    let mut program = ladder_program(1, &[SYNTHETIC_CHAINS[1]]);
    program.objectives.push(Objective {
        id: SymbolId(1),
        content: cid(ContentKind::Objective, "synthetic-m01-parts.objective.2"),
        condition: Condition::InactiveMembers {
            members: vec![member(&[SYNTHETIC_ACTOR, SYNTHETIC_CHAINS[0], "healthy"])],
            threshold: 99,
        },
        actions: Vec::new(),
        span: None,
    });
    program
}

/// **The synthetic composed run binds and observes the parts, and a part the
/// damage authority destroys leaves play.**
#[test]
fn accept_m01_play_triggers_parts_the_synthetic_ladder_completes_on_the_live_worlds_own_state() {
    let scratch = Scratch::new("m01-play-parts-synthetic");
    let stage = synthetic_parts_stage(scratch.path(), ladder_program(1, &SYNTHETIC_CHAINS));
    let mut app = build_headless(&stage).expect("the synthetic parts stage composes");
    app.update();

    let chain = member(&[SYNTHETIC_ACTOR, SYNTHETIC_CHAINS[0], "healthy"]);
    let other = member(&[SYNTHETIC_ACTOR, SYNTHETIC_CHAINS[1], "healthy"]);
    assert_eq!(
        bound_parts(&mut app).len(),
        2,
        "the production binder gave both chains of the bound actor a live part"
    );
    let rows = observed_members(&app);
    assert_eq!(
        rows.get(&chain).map(|row| row.in_play),
        Some(true),
        "the live part is observed in play at its actor's pose: {rows:?}"
    );
    assert!(
        !completed(&app, 0),
        "both parts are in play, so a ladder needing one cleared member cannot hold"
    );

    let (entity, actor) = part_entity(&mut app, &chain);
    assert_eq!(
        destroy_part(&mut app, actor, SYNTHETIC_CHAINS[0]),
        PartState::Destroyed
    );
    app.update();
    let rows = observed_members(&app);
    assert_eq!(
        rows.get(&chain).map(|row| row.in_play),
        Some(false),
        "the destroyed chain is reported out of play"
    );
    assert!(
        completed(&app, 0),
        "the ladder completes on the row the production writer produced"
    );
    assert_eq!(
        rows.get(&other).map(|row| row.in_play),
        Some(true),
        "the untouched part is still in play"
    );
    assert!(
        app.world().get_entity(entity).is_ok(),
        "the part entity stays live: leaving play is a state, not a despawn"
    );

    teardown(&mut app);
}

/// **A ladder whose threshold is not met stalls exactly at it.**
#[test]
fn accept_m01_play_triggers_parts_a_ladder_whose_threshold_is_not_met_stalls_exactly_at_it() {
    let scratch = Scratch::new("m01-play-parts-threshold");
    let stage = synthetic_parts_stage(scratch.path(), ladder_program(2, &SYNTHETIC_CHAINS));
    let mut app = build_headless(&stage).expect("the synthetic parts stage composes");
    app.update();

    let first = member(&[SYNTHETIC_ACTOR, SYNTHETIC_CHAINS[0], "healthy"]);
    let second = member(&[SYNTHETIC_ACTOR, SYNTHETIC_CHAINS[1], "healthy"]);
    let (_, actor) = part_entity(&mut app, &first);
    destroy_part(&mut app, actor, SYNTHETIC_CHAINS[0]);
    frames(&mut app, 3);
    assert!(
        !completed(&app, 0),
        "one of two cleared members is exactly one short of the threshold"
    );
    let rows = observed_members(&app);
    assert_eq!(rows.get(&first).map(|row| row.in_play), Some(false));
    assert_eq!(rows.get(&second).map(|row| row.in_play), Some(true));

    let (_, actor) = part_entity(&mut app, &second);
    destroy_part(&mut app, actor, SYNTHETIC_CHAINS[1]);
    frames(&mut app, 3);
    assert!(
        completed(&app, 0),
        "the second part leaving play reaches the threshold"
    );

    teardown(&mut app);
}

/// **The wrong chain never satisfies a ladder, and removing the provider blocks
/// it.**
#[test]
fn accept_m01_play_triggers_parts_the_wrong_chain_and_a_missing_provider_never_satisfy_a_ladder() {
    let scratch = Scratch::new("m01-play-parts-wrong-chain");
    // The ladder counts only the second chain; the first is an operand of
    // another block, so it is bound and destroyable — but the wrong chain.
    let stage = synthetic_parts_stage(scratch.path(), wrong_chain_program());
    let mut app = build_headless(&stage).expect("the synthetic parts stage composes");
    app.update();

    let wrong = member(&[SYNTHETIC_ACTOR, SYNTHETIC_CHAINS[0], "healthy"]);
    let wanted = member(&[SYNTHETIC_ACTOR, SYNTHETIC_CHAINS[1], "healthy"]);
    let (_, actor) = part_entity(&mut app, &wrong);
    destroy_part(&mut app, actor, SYNTHETIC_CHAINS[0]);
    frames(&mut app, 3);
    assert_eq!(
        observed_members(&app).get(&wrong).map(|row| row.in_play),
        Some(false),
        "the wrong chain really did leave play"
    );
    assert!(
        !completed(&app, 0),
        "a ladder counting the second chain is never satisfied by the first leaving play"
    );

    let (_, actor) = part_entity(&mut app, &wanted);
    assert_eq!(
        destroy_part(&mut app, actor, SYNTHETIC_CHAINS[1]),
        PartState::Destroyed
    );
    frames(&mut app, 2);
    assert!(
        completed(&app, 0),
        "the wanted chain leaving play completes the ladder"
    );

    // Removing the provider: with no live part entity the chain is unobserved
    // again and the ladder cannot complete, whatever the damage authority holds.
    let strip = Scratch::new("m01-play-parts-no-provider");
    let stage = synthetic_parts_stage(strip.path(), ladder_program(1, &[SYNTHETIC_CHAINS[1]]));
    let mut app = build_headless(&stage).expect("the synthetic parts stage composes");
    app.update();
    let (entity, _) = part_entity(&mut app, &wanted);
    app.world_mut().despawn(entity);
    frames(&mut app, 3);
    let rows = observed_members(&app);
    assert!(
        !rows.contains_key(&wanted),
        "with the part entity gone the chain is unobserved: {:?}",
        rows.keys().collect::<Vec<_>>()
    );
    assert!(
        !completed(&app, 0),
        "an unobserved chain is not a cleared member — the ladder stalls"
    );

    teardown(&mut app);
}

/// **A part whose pool nothing resolved is not reported in play.**
///
/// The fail-closed read M01's own engine parts do **not** exercise (their pool
/// is the designed one), pinned so a later stage cannot silently turn "no
/// measured pool" into "in play": the row is *absent*, which is what the
/// `INACTIVE` evaluator reads as an uncleared member.
#[test]
fn accept_m01_play_triggers_parts_an_unresolved_pool_writes_no_row() {
    let scratch = Scratch::new("m01-play-parts-unresolved");
    let stage = synthetic_parts_stage(scratch.path(), ladder_program(1, &SYNTHETIC_CHAINS));
    let mut app = build_headless(&stage).expect("the synthetic parts stage composes");
    app.update();

    let chain = member(&[SYNTHETIC_ACTOR, SYNTHETIC_CHAINS[0], "healthy"]);
    let (_, actor) = part_entity(&mut app, &chain);
    // Replace the authority with one whose graph carries the same node under an
    // unresolved pool — the state a stage with no measured integrity is in.
    let session = app
        .world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .session();
    let node = DamageNode::new(
        DamageNodeKey::new(SYNTHETIC_CHAINS[0]).expect("the node key is valid"),
        DamageNodeKind::InternalStructure,
        Resolved::unknown(
            ClaimId::new("m01-play-triggers-parts.test-unresolved").expect("the claim id is valid"),
            "the test removes the pool to pin the fail-closed read",
        )
        .expect("the unknown pool is well formed"),
    );
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Blueprint, "damage-graph.zeppelin.unresolved")
            .expect("the graph subject is valid"),
        vec![node],
    )
    .expect("the graph is valid");
    let mut fresh = DamageResolver::new(session, 0);
    fresh
        .register_actor(
            actor,
            graph,
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the actor registers");
    app.world_mut()
        .insert_resource(SessionDamageResolver::new(fresh));
    app.update();

    let rows = observed_members(&app);
    assert!(
        !rows.contains_key(&chain),
        "an unresolved pool is not a statement about presence, so the chain stays \
         unobserved: {:?}",
        rows.keys().collect::<Vec<_>>()
    );
    assert!(
        !completed(&app, 0),
        "an unobserved member is not a cleared one"
    );
    assert_eq!(
        part_in_play(
            app.world().resource::<SessionDamageResolver>().resolver(),
            &MissionPartState {
                chain: chain.clone(),
                node: DamageNodeKey::new(SYNTHETIC_CHAINS[0]).expect("the node key is valid"),
                subject: synthetic_subject(),
                actor,
                generation: cs_app::scene::SceneGeneration(1),
            },
        ),
        None,
        "the reader itself answers None for an unresolved pool"
    );

    teardown(&mut app);
}
