//! Acceptance suite M01-PLAY-TRIGGERS-ACTOR-BIT (#1490): M01's bound
//! zeppelins have an **out-of-play state**, so the two actor-level `INACTIVE`
//! ladders (block indexes 53 and 55, `OBJECTIVE54`/`OBJECTIVE56`, threshold 1
//! over the one-element chain `["piratezep"]`) can advance.
//!
//! M01-PLAY-TRIGGERS-OBSERVE (#1480) wrote the actor row `held_in_play` from
//! the binding alone, and M01-PLAY-TRIGGERS-PARTS (#1481) gave the 39
//! *part-state* chains their own rows — but nothing could ever take the
//! zeppelin's **own** node out of play, so both actor-level ladders stalled
//! forever. This suite pins the measured read that replaced the default
//! (`mission_session::world_actor_in_play`):
//!
//! * **the rows** — a bound world actor is in play while the world's own
//!   state holds it (the world-actor session's lifecycle presence, crossed
//!   with the session damage authority's destruction ledger), and out of
//!   play the moment that ledger records the actor's destruction;
//! * **the ladder** — a one-element ladder over the actor's own chain
//!   completes on that row, and only on it: the wrong actor's destruction
//!   never satisfies it, and the binding entity stays live (leaving play is
//!   a state, not a despawn);
//! * **fail-closed** — a binding the world-actor session does not hold gets
//!   **no row at all**, so its ladder can never be satisfied by a state
//!   nobody reported;
//! * **retail** — M01's three zeppelins are observed in play, and a
//!   scripted destruction of `piratezep` takes exactly its row out and
//!   advances `OBJECTIVE54` (block 53) once its measured wake from block 1
//!   has armed it; `OBJECTIVE56` (block 55) stays behind its own measured
//!   wake (`OBJECTIVE22`, which this suite does not drive) and says so.
//!
//! The destruction in every leg goes through the **production damage
//! authority's own scripted-destruction port**
//! (`DamageResolver::record_lifecycle`, the ledger the F36-C.1 publication
//! `publish_destroyed_actors` reads a scripted destruction from) — never a
//! despawn from the test, never a poke at the observer. What the *original*
//! does to clear the zeppelin's in-play bit stays unmeasured
//! (`docs/findings/2026-10-15-m01-play-triggers-actor-bit.md`); nothing here
//! is `verified_original`, and every number the assertions pin is read out of
//! the production readers on this run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::{App, Entity, Transform};
use cs_app::mission_actors::MissionActorSpawnReport;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionStage, build_headless, stage_for, subject_node_name, teardown,
    world_actor_in_play,
};
use cs_app::weapons::SessionDamageResolver;
use cs_app::world_actors::{LoweredWorldActors, WorldActorBinding};
use cs_app::world_facts::{MemberObservation, MemberResolver};
use cs_script::ir::{
    Action, ActorId as WorldActorId, Condition, IR_VERSION, MemberName, MissionProgram, Objective,
    Outcome, SymbolId,
};
use cs_script::runtime::ObjectiveLifecycle;
use cs_sim::damage::{ActorId, LifecycleKind};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl};
use cs_sim::world_actors::runtime::{ActorMotion, WorldActorKind, WorldActorSpec};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

use crate::campaign::common::{cid, label};
use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

/// `OBJECTIVE54`: threshold 1 over the one-element chain `["piratezep"]`.
/// Starts Dormant: woken by block 1 (`OBJECTIVE2`), measured.
const PIRATE_LADDER_BLOCK: u32 = 53;
/// `OBJECTIVE56`: the same chain, threshold 1. Starts Dormant: woken by
/// block 21 (`OBJECTIVE22`), which itself starts Dormant, measured.
const PIRATE_LADDER_TWO_BLOCK: u32 = 55;
/// `OBJECTIVE55`: `workersvoyagezep`'s 18 engine chains, threshold 2 — the
/// part ladder that must not move when the *actor* leaves play. Awake.
const WORKERS_ENGINES_BLOCK: u32 = 54;
/// `OBJECTIVE2`: the `DEDG` group-1 block that starts Awake — the measured
/// waker of block 53.
const GROUP_ONE_BLOCK: u32 = 1;
/// The measured subject name M01's `piratezep` zeppelin binds under.
const PIRATEZEP: &str = "piratezep";
/// The measured subject name M01's `workersvoyagezep` zeppelin binds under.
const WORKERSVOYAGEZEP: &str = "workersvoyagezep";
/// The measured subject name M01's `blackswanzep` zeppelin binds under.
const BLACKSWANZEP: &str = "blackswanzep";
/// The synthetic ladder actor this suite binds.
const SYNTHETIC_ACTOR: &str = "syntheticzep";
/// The synthetic decoy the wrong-actor leg destroys first.
const DECOY_ACTOR: &str = "decoyzep";

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The production stage for retail M01.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// One member chain out of its spellings.
fn member(parts: &[&str]) -> MemberName {
    parts.iter().map(|part| (*part).to_owned()).collect()
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
    lifecycle_state(app, block) == Some(ObjectiveLifecycle::Done)
}

/// The host lifecycle model's projected state for `block`.
fn lifecycle_state(app: &App, block: u32) -> Option<ObjectiveLifecycle> {
    app.world()
        .resource::<MissionHost>()
        .lifecycles()
        .state(block)
}

/// Runs `count` composed frames.
fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
    }
}

/// The session damage actor and binding entity the composition registered for
/// the world actor whose measured subject spells `name`.
fn bound_actor(app: &mut App, name: &str) -> (ActorId, Entity) {
    app.world()
        .resource::<MissionActorSpawnReport>()
        .zeppelins
        .iter()
        .find(|row| subject_node_name(&row.subject) == Some(name))
        .map(|row| (row.actor, row.entity))
        .unwrap_or_else(|| panic!("the composition bound the world actor `{name}`"))
}

/// Drives one bound world actor out of play through the **production damage
/// authority's own scripted-destruction port**.
///
/// `DamageResolver::record_lifecycle` is the resolver's documented way an
/// actor's destruction that *no hit produced* becomes the ledger fact every
/// consumer reads — the ledger the F36-C.1 publication
/// (`publish_destroyed_actors` → `DamageActorDestroyed`) forwards, and the
/// same once-per-kind ledger a lethal hit records into. This is not a
/// despawn from the test and not a poke at the observer: it is the measured
/// port a scripted destruction enters the composition by.
fn destroy_actor(app: &mut App, actor: ActorId) {
    let mut shared = app.world_mut().resource_mut::<SessionDamageResolver>();
    let resolver = shared.resolver_mut();
    resolver
        .record_lifecycle(actor, LifecycleKind::Destroyed, Tick(0))
        .expect("the damage authority records the scripted destruction");
    assert!(
        resolver.is_destroyed(&actor),
        "the ledger now carries the actor's destruction"
    );
}

// ------------------------------------------------------------ synthetic ---

/// One held synthetic world actor, registered the way the lowering registers
/// M01's zeppelins: kinematic, held at its authored pose. The actor's *name*
/// reaches the composition through the program's subject map, not the spec.
fn held_actor(id: u32, at: [f64; 3]) -> WorldActorSpec {
    WorldActorSpec {
        actor: WorldActorId(id),
        kind: WorldActorKind::Kinematic,
        faction: cid(ContentKind::Faction, "synthetic.faction"),
        objective: None,
        motion: ActorMotion::Held {
            position_m: at,
            orientation: cs_sim::world_actors::Quat::IDENTITY,
        },
    }
}

/// The synthetic actor's joined subject, spelled the way the composition's own
/// scene-node subjects are.
fn synthetic_subject(name: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.synthetic.window.{name}"),
    )
    .expect("the synthetic subject id is valid")
}

/// A synthetic stage whose host carries two bound world actors and one
/// one-element ladder, so the production binder and the production observer
/// run with no installation — the same shape M01's actor-level ladders have.
fn synthetic_actor_stage(scratch: &Path, program: MissionProgram) -> MissionStage {
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
        (DECOY_ACTOR.to_owned(), None),
    ]);
    stage.host.world_actors = Some(LoweredWorldActors {
        ticks_per_second: 30,
        actors: vec![
            held_actor(1, [1_000.0, 100.0, -2_000.0]),
            held_actor(2, [-3_000.0, 250.0, 500.0]),
        ],
        subjects: BTreeMap::from([
            (WorldActorId(1), synthetic_subject(SYNTHETIC_ACTOR)),
            (WorldActorId(2), synthetic_subject(DECOY_ACTOR)),
        ]),
        sockets: BTreeMap::new(),
        support: Vec::new(),
        pickups: Vec::new(),
        transitions: Vec::new(),
    });
    stage
}

/// A synthetic program whose one block is an `InactiveMembers` ladder over
/// the **one-element** chain naming `actor` — the exact shape M01's block
/// indexes 53 and 55 have over `["piratezep"]`.
fn actor_ladder_program(actor: &str) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-m01-actor-bit"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(
                ContentKind::Objective,
                "synthetic-m01-actor-bit.objective.1",
            ),
            condition: Condition::InactiveMembers {
                members: vec![member(&[actor])],
                threshold: 1,
            },
            actions: vec![Action::Finish(Outcome::Succeeded)],
            span: None,
        }],
    }
}

/// **A scripted destruction takes the actor row out of play and completes the
/// one-element ladder.**
///
/// The row starts in play — the world-actor session holds the actor — and
/// the ladder cannot hold. The moment the damage authority's ledger records
/// the actor's destruction through its own scripted port, the observer
/// reports the row out of play (that is the `INACTIVE` read) and the ladder
/// completes; the binding entity stays live, because leaving play is a state
/// of the world's own authorities, not a despawn from the test.
#[test]
fn accept_m01_play_triggers_actor_bit_a_scripted_destruction_takes_the_actor_out_of_play() {
    let scratch = Scratch::new("m01-play-actor-bit-ladder");
    let stage = synthetic_actor_stage(scratch.path(), actor_ladder_program(SYNTHETIC_ACTOR));
    let mut app = build_headless(&stage).expect("the synthetic actor stage composes");
    app.update();

    let chain = member(&[SYNTHETIC_ACTOR]);
    let (damage_actor, entity) = bound_actor(&mut app, SYNTHETIC_ACTOR);
    let rows = observed_members(&app);
    assert_eq!(
        rows.get(&chain).map(|row| row.in_play),
        Some(true),
        "the session holds the actor, so its row is in play: {rows:?}"
    );
    assert!(
        !completed(&app, 0),
        "the actor is in play, so a ladder needing one cleared member cannot hold"
    );

    destroy_actor(&mut app, damage_actor);
    app.update();

    let rows = observed_members(&app);
    assert_eq!(
        rows.get(&chain).map(|row| row.in_play),
        Some(false),
        "the destruction ledger takes the actor row out of play — that is the INACTIVE read"
    );
    assert!(
        completed(&app, 0),
        "the one-element ladder completes on the world's own state"
    );
    assert!(
        app.world().get_entity(entity).is_ok(),
        "the binding entity stays live: leaving play is a state, not a despawn"
    );

    teardown(&mut app);
}

/// **The wrong actor's destruction never satisfies the ladder.**
///
/// Both bound actors are addressable and destroyable; only the ladder's own
/// actor can clear it. The decoy leaving play is reported — its row goes out
/// — but the ladder counts the other chain and stays where it is.
#[test]
fn accept_m01_play_triggers_actor_bit_the_wrong_actor_never_satisfies_the_ladder() {
    let scratch = Scratch::new("m01-play-actor-bit-wrong-actor");
    let stage = synthetic_actor_stage(scratch.path(), actor_ladder_program(SYNTHETIC_ACTOR));
    let mut app = build_headless(&stage).expect("the synthetic actor stage composes");
    app.update();

    let chain = member(&[SYNTHETIC_ACTOR]);
    let decoy_chain = member(&[DECOY_ACTOR]);
    let (decoy, _) = bound_actor(&mut app, DECOY_ACTOR);
    destroy_actor(&mut app, decoy);
    frames(&mut app, 2);

    let rows = observed_members(&app);
    assert_eq!(
        rows.get(&decoy_chain).map(|row| row.in_play),
        Some(false),
        "the decoy really did leave play"
    );
    assert_eq!(
        rows.get(&chain).map(|row| row.in_play),
        Some(true),
        "the ladder's own actor is untouched"
    );
    assert!(
        !completed(&app, 0),
        "a ladder counting one actor's chain is never satisfied by the other leaving play"
    );

    let (actor, _) = bound_actor(&mut app, SYNTHETIC_ACTOR);
    destroy_actor(&mut app, actor);
    frames(&mut app, 2);
    assert!(
        completed(&app, 0),
        "the wanted actor leaving play completes the ladder"
    );

    teardown(&mut app);
}

/// **A binding the world-actor session does not hold gets no row at all.**
///
/// The fail-closed half: the observer may not report a presence the world's
/// own state never stated. A bare binding entity whose actor id the session
/// never registered is therefore unobserved — not "in play", not "out of
/// play" — and its ladder can never be satisfied, exactly like a part whose
/// pool nothing resolved (#1481's fail-closed read).
#[test]
fn accept_m01_play_triggers_actor_bit_an_actor_the_world_does_not_hold_gets_no_row() {
    let scratch = Scratch::new("m01-play-actor-bit-no-row");
    // The ladder names an actor nothing launched: the stage registers only
    // the two synthetic actors above.
    let stage = synthetic_actor_stage(scratch.path(), actor_ladder_program("ghostzep"));
    let mut app = build_headless(&stage).expect("the synthetic actor stage composes");
    app.update();

    // A bare binding, exactly the shape #1480 wrote rows from: an entity the
    // composition spawned whose subject spells the chain, but whose actor id
    // the world-actor session never registered.
    app.world_mut().spawn((
        Transform::from_translation(bevy::prelude::Vec3::new(0.0, 0.0, 0.0)),
        WorldActorBinding {
            actor: WorldActorId(99),
            subject: synthetic_subject("ghostzep"),
            generation: cs_app::scene::SceneGeneration(1),
        },
    ));
    frames(&mut app, 3);

    let chain = member(&["ghostzep"]);
    let rows = observed_members(&app);
    assert!(
        !rows.contains_key(&chain),
        "a binding the world's own state does not hold stays unobserved: {:?}",
        rows.keys().collect::<Vec<_>>()
    );
    assert!(
        !completed(&app, 0),
        "an unobserved member is not a cleared one — the ladder stalls"
    );

    // The reader itself answers None: no session, and a session that does
    // not hold the actor, are both "the world says nothing".
    let session = app
        .world()
        .resource::<MissionHost>()
        .world_actors()
        .expect("the synthetic stage launched its world-actor session");
    assert_eq!(
        world_actor_in_play(None, WorldActorId(1), None, None),
        None,
        "without the world-actor session the composition holds no lifecycle state at all"
    );
    assert_eq!(
        world_actor_in_play(Some(session), WorldActorId(99), None, None),
        None,
        "a session that never registered the actor says nothing about it"
    );
    assert_eq!(
        world_actor_in_play(Some(session), WorldActorId(1), None, None),
        Some(true),
        "a held actor with a silent ledger is in play"
    );

    teardown(&mut app);
}

// ---------------------------------------------------------------- retail ---

/// **M01's actor-level ladders read the `piratezep` row, and a scripted
/// destruction of the actor advances the ladder the engine model lets run.**
///
/// Measured lifecycle gating first: block 53 (`OBJECTIVE54`) starts
/// **Dormant** and is woken by block 1 (`OBJECTIVE2`, the `DEDG` group-1
/// block, which starts Awake); block 55 (`OBJECTIVE56`) starts Dormant and
/// is woken by block 21 (`OBJECTIVE22`), which itself starts Dormant — this
/// suite does not drive that chain and says so. Block 54 (`OBJECTIVE55`,
/// `workersvoyagezep`'s engines at threshold 2) starts Awake and must not
/// move when the *actor* leaves play.
///
/// The legs, all through the composed production schedule:
///
/// 1. three bound zeppelins, all observed in play; the measured lifecycle
///    states above; group 1 observed at its live count of four;
/// 2. group 1 leaves the world (the depletion mechanism #1480's own suite
///    pins) → block 1 completes and wakes block 53, whose ladder still
///    cannot hold while `piratezep` is in play;
/// 3. the damage authority records `piratezep`'s destruction through its
///    scripted port → exactly its row goes out of play, block 53 completes,
///    `workersvoyagezep` stays in play and block 54 does not move.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_triggers_actor_bit_m01_s_actor_level_ladders_read_the_piratezep_row() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    frames(&mut app, 3);

    // --- 1. the measured baseline ------------------------------------------
    for name in [PIRATEZEP, WORKERSVOYAGEZEP, BLACKSWANZEP] {
        let row = observed_members(&app)
            .get(&member(&[name]))
            .cloned()
            .unwrap_or_else(|| panic!("the bound world actor `{name}` is observed"));
        assert!(
            row.in_play,
            "`{name}` is bound and the world holds it in play"
        );
    }
    assert_eq!(
        lifecycle_state(&app, PIRATE_LADDER_BLOCK),
        Some(ObjectiveLifecycle::Dormant),
        "OBJECTIVE54 starts Dormant — the engine model, measured, gates it behind block 1"
    );
    assert_eq!(
        lifecycle_state(&app, PIRATE_LADDER_TWO_BLOCK),
        Some(ObjectiveLifecycle::Dormant),
        "OBJECTIVE56 starts Dormant, woken only by OBJECTIVE22 (block 21, itself Dormant)"
    );
    assert!(
        !completed(&app, PIRATE_LADDER_BLOCK) && !completed(&app, PIRATE_LADDER_TWO_BLOCK),
        "both actor-level ladders are incomplete while the actor is in play"
    );

    // --- 2. wake block 53 through the engine model's own effects -----------
    //     Group 1's four `rusdevastator_*` entities leave the world the way
    //     #1480's own suite pins (nothing else declares their depletion);
    //     `OBJECTIVE2` (block 1, Awake) completes and its record's wake list
    //     arms `OBJECTIVE54`.
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
    assert_eq!(group_one.len(), 4, "M01 spawns four group-1 aircraft");
    for entity in group_one {
        app.world_mut().despawn(entity);
    }
    frames(&mut app, 10);
    assert!(
        completed(&app, GROUP_ONE_BLOCK),
        "the group-1 depletion completes OBJECTIVE2; completed blocks: {:?}",
        last_completed(&app)
    );
    assert_eq!(
        lifecycle_state(&app, PIRATE_LADDER_BLOCK),
        Some(ObjectiveLifecycle::Awake),
        "OBJECTIVE2's own wake list armed OBJECTIVE54"
    );
    assert!(
        !completed(&app, PIRATE_LADDER_BLOCK),
        "waking is not completing: the ladder cannot hold while `piratezep` is in play"
    );

    // --- 3. the scripted destruction the ladder reads -----------------------
    let (actor, _) = bound_actor(&mut app, PIRATEZEP);
    destroy_actor(&mut app, actor);
    frames(&mut app, 3);

    let rows = observed_members(&app);
    assert_eq!(
        rows.get(&member(&[PIRATEZEP])).map(|row| row.in_play),
        Some(false),
        "the destroyed zeppelin's own row is out of play — the INACTIVE read"
    );
    assert_eq!(
        rows.get(&member(&[WORKERSVOYAGEZEP]))
            .map(|row| row.in_play),
        Some(true),
        "the other zeppelin's row is untouched"
    );
    assert!(
        completed(&app, PIRATE_LADDER_BLOCK),
        "the woken block 53 (threshold 1 over [`piratezep`]) advances from the live world's own \
         state; completed blocks: {:?}",
        last_completed(&app)
    );
    assert!(
        !completed(&app, PIRATE_LADDER_TWO_BLOCK),
        "block 55 stays behind its own measured wake (OBJECTIVE22): this suite drives no \
         OBJECTIVE22 chain, and nothing here may invent one"
    );
    assert!(
        !completed(&app, WORKERS_ENGINES_BLOCK),
        "OBJECTIVE55 counts `workersvoyagezep`'s engine parts, not the actor's own row"
    );

    teardown(&mut app);
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
