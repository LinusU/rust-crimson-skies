//! Integration acceptance for `M01-LC-WORLD-FACTS` (#751): the four world-side
//! [`MissionFacts`] maps get their production writers.
//!
//! `cs_app::mission_facts` supplies [`MissionWorldReads`] — the operand set a
//! lowered program's conditions actually read — and [`WorldFactTable`], the
//! mounted world plus the group, generator and animation registries the host
//! observes. This suite proves the compose: a lowered program's `members`,
//! `groups`, `generators` and `animations` are populated from observed state
//! and folded through [`MissionFacts::absorb`] before `MissionSession`
//! advances, so the conditions that could only evaluate `false` before can
//! now complete.
//!
//! What this suite does **not** claim: no original executable was run and no
//! mission was played, so nothing here is `verified_original`. Group
//! membership is a host declaration — which members the original assigns to
//! each AI group is unmeasured — and the `ANIM_STATE` records M01 spells are
//! started by directive-side launch paths this task does not build, so their
//! observed state stays `DORMANT` and the blocks stay honest rather than
//! completed.
//!
//! Test prefix `accept_m01_lc_world_facts_`; the retail cases need
//! `$CS_GAME_DIR` and are `#[ignore]`d so CI runs the synthetic case.

use std::path::PathBuf;

use cs_app::animation::carrier::survey_animation_bindings;
use cs_app::animation::mission::bind_mission_animation;
use cs_app::animation::survey::CarrierKind;
use cs_app::control_lowering::{LoweredControlRecord, lower_control_record};
use cs_app::mission_animations::MissionAnimationPlayer;
use cs_app::mission_control::survey_mission_control_programs;
use cs_app::mission_facts::{
    MemberObservation, MemberResolution, MissionWorldReads, WorldFactTable,
};
use cs_app::world::retail::RetailWorldContainer;
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::mission_control::measure_control_record;
use cs_content::scene::{BindingMap, SceneGraph};
use cs_content::stunts::ZrdValue;
use cs_content::world::world_scene_graph_from_gamez;
use cs_script::ir::SymbolId;
use cs_script::runtime::{MemberPresence, SessionGeneration, TerminalState};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl, MissionSession};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

/// The census row label of the mission the parent task is about.
const M01: &str = "zbd/c1c/m01";
/// The world group M01 plays in.
const M01_GROUP: &str = "c1c";
/// The mission animation carrier's logical key.
const M01_MISSION_CARRIER: &str = "zbd/c1c/m01/mis_anim.zbd";
/// The camera carrier's logical key.
const M01_CAMERA_CARRIER: &str = "zbd/c1c/cam_anim.zbd";

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

// ------------------------------------------------- the .zrd authoring helpers ---

fn zrd_int(value: u32) -> ZrdValue {
    ZrdValue::Int(value)
}

fn zrd_text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn zrd_list(children: Vec<ZrdValue>) -> ZrdValue {
    ZrdValue::List(children)
}

/// One authored directive of a block: the key, and — unless authored bare —
/// its argument list beside it, in the asymmetric grammar the census reads.
fn directive(key: &str, args: Vec<ZrdValue>) -> Vec<ZrdValue> {
    let mut children = vec![zrd_text(key)];
    if !args.is_empty() {
        children.push(zrd_list(args));
    }
    children
}

fn block(number: u32, directives: Vec<Vec<ZrdValue>>) -> (String, ZrdValue) {
    let mut children = Vec::new();
    for children_of_directive in directives {
        children.extend(children_of_directive);
    }
    (format!("OBJECTIVE{number}"), zrd_list(children))
}

fn control_record(fields: Vec<(String, ZrdValue)>) -> ZrdValue {
    let mut children = Vec::new();
    for (key, value) in fields {
        children.push(zrd_text(&key));
        children.push(value);
    }
    zrd_list(vec![zrd_list(children)])
}

/// Runs the production adapter over an authored record: measured, lowered and
/// bound through the record's own key dispositions, exactly as the census runs
/// it for a retail row.
fn lower(document: &ZrdValue) -> LoweredControlRecord {
    let record = measure_control_record(document);
    lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-mission")
            .map_err(|error| error.to_string()),
        "accept-mission",
        document,
        &record,
    )
}

/// M01's lowered program, straight from the census's own lowering attempt —
/// the record a production launch path reads.
fn m01_program() -> cs_script::ir::MissionProgram {
    let census =
        survey_mission_control_programs(&game_dir()).expect("the census runs on the installation");
    census
        .row(M01)
        .expect("M01 is measured")
        .lowering_attempt()
        .expect("a measured row carries the attempt")
        .program()
        .expect("every one of M01's sites bound")
        .clone()
}

/// The `SymbolId` `OBJECTIVE{number}` lowered to — the objective ids are the
/// program's declaration order, not the spelled numbers, so a test names the
/// block it means and lets the record's own ids speak.
fn objective_symbol(program: &cs_script::ir::MissionProgram, number: u32) -> SymbolId {
    program
        .objectives
        .iter()
        .find(|objective| {
            objective
                .content
                .key()
                .ends_with(&format!(".objective{number}"))
        })
        .map(|objective| objective.id)
        .unwrap_or_else(|| panic!("OBJECTIVE{number} is one of the program's objectives"))
}

/// The container the group's GameZ record decodes to — the measured node
/// array M01's member names live in.
fn c1c_container() -> RetailWorldContainer {
    cs_app::world::retail::read_world_containers(&game_dir())
        .expect("the world containers read")
        .container(M01_GROUP, &cs_content::textures::WorldTextureLoad::project_default())
        .expect("the c1c container is present")
}

/// The container's whole node array as a `SceneGraph` — the canonical mount
/// the member table is built over. The whole-container path reconciles the
/// world record's measured partial child list (the c1c array's own omitted
/// records) before the strict build runs. Mesh bindings stay
/// `Resolved::Unknown`: the member walk reads names, hierarchy and poses,
/// never a mesh.
fn c1c_scene_graph(container: &RetailWorldContainer) -> SceneGraph {
    let adapter =
        SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()));
    let id = ContentId::from_source(ContentKind::InstallFile, "c1c-gamez")
        .expect("the container id is valid");
    world_scene_graph_from_gamez(&id, container.nodes(), &[], &adapter, &BindingMap::default())
        .expect("the c1c node array reconciles and builds a scene graph")
        .graph()
        .clone()
}

/// The resolved [`MemberFact`] a chain carries — the test's own unwrapping,
/// so a `Missing`/`Ambiguous` answer fails with the verdict named.
fn resolved_member(
    world: &WorldFactTable,
    chain: &[String],
) -> cs_script::runtime::MemberFact {
    match world.member(chain) {
        MemberObservation::Resolved(fact) => fact,
        other => panic!("{} resolves: {other:?}", chain.join("/")),
    }
}

// ---------------------------------------------------------------------------
// The synthetic case: a declared member tree, no retail data
// ---------------------------------------------------------------------------

/// **A member chain's in-play bit is what `INACTIVE` counts — observed both
/// ways.**
///
/// An authored record lowers one `INACTIVE1` block over the chain
/// `["zep", "engine", "healthy"]` (the same member-of-member shape M01
/// spells). The table mounts nothing from a store — the host declares the
/// member and its parts, the way a spawned object enters the member table —
/// so the assertions below are about the writer's contract alone:
///
/// * while the member is in play the block cannot complete;
/// * once the host records it out of play the block completes on the next
///   advance;
/// * a chain that resolves to nothing stays `Missing` and is never counted,
///   and a name two members carry resolves `Ambiguous` and writes no fact.
#[test]
fn accept_m01_lc_world_facts_inactive_members_count_the_observed_in_play_bit() {
    let document = control_record(vec![block(
        1,
        vec![directive(
            "INACTIVE1",
            vec![
                zrd_text("zep"),
                zrd_text("engine"),
                zrd_text("healthy"),
            ],
        )],
    )]);
    let program = lower(&document)
        .program()
        .expect("the record lowers")
        .clone();
    let reads = MissionWorldReads::of(&program);
    assert_eq!(
        reads.members().collect::<Vec<_>>().len(),
        1,
        "the lowered program's one member chain is the whole member read set"
    );

    let chain = vec![
        "zep".to_owned(),
        "engine".to_owned(),
        "healthy".to_owned(),
    ];
    let mut world = WorldFactTable::new();
    // A member the static mount does not carry is declared into it — the
    // player craft, a spawned object — and its member parts mount under it
    // the way the store's tree carries them.
    world.declare_member("zep", [0.0; 3]);
    assert_eq!(
        world.declare_child_member(&["zep".to_owned()], "engine", [1.0, 0.0, 0.0]),
        MemberResolution::Resolved
    );
    assert_eq!(
        world.declare_child_member(&chain[..2].to_vec(), "healthy", [2.0, 0.0, 0.0]),
        MemberResolution::Resolved
    );
    assert_eq!(
        world.member(&chain),
        MemberObservation::Resolved(cs_script::runtime::MemberFact {
            presence: MemberPresence::InPlay,
            position: [2.0, 0.0, 0.0],
        }),
        "the declared chain resolves to its leaf member's row"
    );

    let objective1 = objective_symbol(&program, 1);
    let mut lifecycle = BlockLifecycleTable::new();
    lifecycle
        .declare(0, LifecycleDecl::awake())
        .expect("the one block is declared awake");
    let mut session = MissionSession::launch(program, SessionGeneration(9), [])
        .expect("the program launches");

    // While the member is in play the block cannot complete: the facts are
    // populated, the bit is set, and `INACTIVE` counts only cleared rows.
    let mut facts = lifecycle.facts();
    facts.absorb(world.facts(&reads));
    let running = session
        .advance(&facts, Tick(1))
        .expect("tick 1 advances");
    assert_eq!(running.terminal, TerminalState::Running);
    assert!(
        !session.state().is_completed(objective1),
        "a member still in play is not inactive"
    );

    // The host records the in-play bit clear: the leaf member went out of
    // play, and the next advance completes the block.
    assert_eq!(
        world.set_member_in_play(&chain, false),
        MemberResolution::Resolved,
        "the chain resolves so the bit lands on its member"
    );
    let mut facts = lifecycle.facts();
    facts.absorb(world.facts(&reads));
    session
        .advance(&facts, Tick(2))
        .expect("tick 2 advances");
    assert!(
        session.state().is_completed(objective1),
        "the member out of play completes the block"
    );

    // Fail-closed halves of the same writer: a chain nobody mounted stays
    // `Missing`, and a name two members carry writes no fact.
    let absent = vec!["zep".to_owned(), "engine".to_owned(), "gone".to_owned()];
    assert_eq!(
        world.member(&absent),
        MemberObservation::Missing,
        "a name nothing carries resolves missing"
    );
    world.declare_member("engine", [9.0; 3]);
    assert_eq!(
        world.resolve(&["engine".to_owned()]),
        MemberResolution::Ambiguous,
        "two members carrying one name resolve ambiguous, not picked"
    );
}

/// **Group rosters, generator pending counts and animation state bytes write
/// their maps — and unresolved names write nothing.**
///
/// The synthetic record arms all three readers: `DEDG` over group 1 with an
/// optional generator name, and `ANIM_STATE` for `engine_start`. The table
/// then shows the four fail-closed distinctions the evaluators turn on: a
/// declared empty roster counts 0 (the block completes), a group nobody
/// declared writes no key (a second `DEDG` block on group 2 cannot), a
/// generator name nobody registered contributes its measured zero, and an
/// animation name mounted dormant reads the measured `DORMANT` byte while a
/// name nobody mounted stays absent.
#[test]
fn accept_m01_lc_world_facts_groups_generators_and_animations_write_only_observed_state() {
    let document = control_record(vec![
        block(
            1,
            vec![directive(
                "DEDG",
                vec![zrd_int(1), zrd_int(0), zrd_text("gen_a")],
            )],
        ),
        block(2, vec![directive("DEDG", vec![zrd_int(2), zrd_int(0)])]),
        block(
            3,
            vec![directive(
                "ANIM_STATE",
                vec![
                    zrd_text("ANIM"),
                    zrd_list(vec![
                        zrd_text("NAME"),
                        zrd_list(vec![zrd_text("engine_start")]),
                        zrd_text("STATE"),
                        zrd_list(vec![zrd_text("EXECUTED")]),
                    ]),
                ],
            )],
        ),
    ]);
    let program = lower(&document)
        .program()
        .expect("the record lowers")
        .clone();
    let reads = MissionWorldReads::of(&program);
    assert_eq!(
        (
            reads.groups().count(),
            reads.generators().count(),
            reads.animations().count()
        ),
        (2, 1, 1),
        "the read set collects every world-side operand the record spells"
    );

    let mut world = WorldFactTable::new();
    // Group 1's roster exists and is empty: the host observed the group and
    // it holds no members, so its living count is the observed 0.
    world.declare_group(1, Vec::new());
    // The generator the DEDG site names was never observed — its measured
    // contribution is the zero the evaluator supplies.
    // The animation record exists in the mounted name space and has not run.
    world.mount_animation_records(CarrierKind::Mission, &[b"engine_start".to_vec()]);
    let player =
        MissionAnimationPlayer::new(SessionId::new(9).expect("nonzero session id"), 30).expect("a declared tick rate is nonzero");
    world.observe_animations(&player);

    let facts = world.facts(&reads);
    assert_eq!(
        facts.groups.get(&1),
        Some(&0),
        "a declared empty roster records its living count of zero"
    );
    assert_eq!(
        facts.groups.get(&2),
        None,
        "a group nobody declared is unknown, not empty"
    );
    assert_eq!(
        facts.generators.get("gen_a"),
        None,
        "a generator never observed writes no count"
    );
    assert_eq!(
        facts.animations.get("engine_start"),
        Some(&1),
        "the mounted record is dormant — the measured DORMANT byte"
    );
    assert_eq!(
        facts.animations.get("never_mounted"),
        None,
        "an animation nobody mounted writes no byte"
    );

    // Group 1's observed zero plus the generator's measured zero satisfy the
    // block; group 2 was never declared, so its block stays open — the two
    // halves of the same advance.
    let objective1 = objective_symbol(&program, 1);
    let objective2 = objective_symbol(&program, 2);
    let objective3 = objective_symbol(&program, 3);
    let mut lifecycle = BlockLifecycleTable::new();
    for index in 0..3 {
        lifecycle
            .declare(index, LifecycleDecl::awake())
            .expect("each block is declared once");
    }
    let mut session = MissionSession::launch(program, SessionGeneration(10), [])
        .expect("the program launches");
    let mut folded = lifecycle.facts();
    folded.absorb(facts);
    session
        .advance(&folded, Tick(1))
        .expect("the tick advances");
    assert!(
        session.state().is_completed(objective1),
        "the declared group's observed zero plus the unresolved generator's measured zero \
         completes the DEDG block"
    );
    assert!(
        !session.state().is_completed(objective2),
        "the undeclared group stays unknown, so its DEDG block cannot complete"
    );
    assert!(
        !session.state().is_completed(objective3),
        "a dormant animation is not EXECUTED, so its block stays open"
    );
}

// ---------------------------------------------------------------------------
// The retail cases: M01's own lowered program over the owner's installation
// ---------------------------------------------------------------------------

/// **M01's `OBJECTIVE53` — `INACTIVE` on `["piratezep"]` — completes once the
/// world records the member out of play.**
///
/// The census lowers M01's record; the member table mounts the c1c world's
/// own node array, so `piratezep` resolves to the node the store authored;
/// the host records it out of play; and the folded facts drive the lowered
/// block to completion through `MissionSession`. This is the half of
/// `MissionFacts` #717 left unwritten: the `INACTIVE` ladders now observe
/// real members.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_m01_inactive_block_completes_on_the_observed_world() {
    let program = m01_program();
    let reads = MissionWorldReads::of(&program);
    let container = c1c_container();
    let mut world = WorldFactTable::mount(&c1c_scene_graph(&container));

    // The authored member chain resolves to the store's own node, in play.
    let piratezep = vec!["piratezep".to_owned()];
    assert_eq!(
        resolved_member(&world, &piratezep).presence,
        MemberPresence::InPlay,
        "piratezep resolves in play on the mounted world"
    );

    // The read set the program spells: every `INACTIVE` member chain, every
    // `DEDG` group and every `ANIM_STATE` name — resolved or not, they are
    // the keys the evaluator reads.
    let facts = world.facts(&reads);
    assert!(
        facts
            .members
            .values()
            .all(|fact| fact.presence == MemberPresence::InPlay
                || fact.presence == MemberPresence::Missing),
        "the mounted world reports every spelled member in play or honestly missing"
    );
    assert_eq!(
        facts.members.get(&piratezep).map(|fact| fact.presence),
        Some(MemberPresence::InPlay),
        "piratezep's own row is in play"
    );

    let objective53 = objective_symbol(&program, 53);
    let mut lifecycle = BlockLifecycleTable::new();
    lifecycle
        .declare(53, LifecycleDecl::awake())
        .expect("block 53 is declared awake");
    let mut session = MissionSession::launch(program, SessionGeneration(11), [])
        .expect("M01's lowered program launches");

    // First the fail-closed half: the member is in play, so the ladder does
    // not count it and the block cannot complete.
    let mut folded = lifecycle.facts();
    folded.absorb(world.facts(&reads));
    session
        .advance(&folded, Tick(1))
        .expect("tick 1 advances");
    assert!(
        !session.state().is_completed(objective53),
        "piratezep in play: the block cannot complete"
    );

    // Then the observed transition: the host records the member out of play,
    // and the next advance completes it.
    assert_eq!(
        world.set_member_in_play(&piratezep, false),
        MemberResolution::Resolved
    );
    let mut folded = lifecycle.facts();
    folded.absorb(world.facts(&reads));
    session
        .advance(&folded, Tick(2))
        .expect("tick 2 advances");
    assert!(
        session.state().is_completed(objective53),
        "piratezep out of play completes OBJECTIVE53"
    );
}

/// **The same `OBJECTIVE53` stays open while `piratezep` remains in play —
/// and while its parts die without it.**
///
/// The non-completion half: the member's engine parts are recorded out of
/// play — `["piratezep", "reng11", "healthy"]` and kin resolve and read
/// `OutOfPlay`, because the in-play bit propagates down the mount — but the
/// member the block spells is still in play, so the ladder's count is zero.
/// Killing parts must not complete a block whose member still lives.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_m01_inactive_block_stays_open_while_the_member_lives() {
    let program = m01_program();
    let reads = MissionWorldReads::of(&program);
    let container = c1c_container();
    let mut world = WorldFactTable::mount(&c1c_scene_graph(&container));

    let piratezep = vec!["piratezep".to_owned()];
    let engine = vec![
        "piratezep".to_owned(),
        "reng11".to_owned(),
        "healthy".to_owned(),
    ];
    assert_eq!(
        world.set_member_in_play(&engine, false),
        MemberResolution::Resolved,
        "the engine chain resolves under the mounted piratezep subtree"
    );
    assert_eq!(
        resolved_member(&world, &engine).presence,
        MemberPresence::OutOfPlay,
        "the engine reads out of play while its zeppelin still lives"
    );

    let objective53 = objective_symbol(&program, 53);
    let mut lifecycle = BlockLifecycleTable::new();
    lifecycle
        .declare(53, LifecycleDecl::awake())
        .expect("block 53 is declared awake");
    let mut session = MissionSession::launch(program, SessionGeneration(12), [])
        .expect("M01's lowered program launches");
    let mut folded = lifecycle.facts();
    folded.absorb(world.facts(&reads));
    session
        .advance(&folded, Tick(1))
        .expect("the tick advances");
    assert!(
        !session.state().is_completed(objective53),
        "the spelled member is still in play: a dead part does not complete its block"
    );
    assert_eq!(
        resolved_member(&world, &piratezep).presence,
        MemberPresence::InPlay,
        "piratezep itself remains in play"
    );
}

/// **`OBJECTIVE3`'s `TRAVELERS` reads the declared player's position against
/// the mounted `workersvoyagezep`.**
///
/// `player` is not a store node — it is the member a spawned craft declares —
/// so the table mounts it by declaration at the host's observed position.
/// Inside 700 m the block completes; outside it does not, and before the
/// member exists at all the chain is `Missing`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_m01_travelers_reads_the_declared_subject_position() {
    let program = m01_program();
    let reads = MissionWorldReads::of(&program);
    let container = c1c_container();
    let mut world = WorldFactTable::mount(&c1c_scene_graph(&container));

    let anchor = vec!["workersvoyagezep".to_owned()];
    let anchor_position = resolved_member(&world, &anchor).position;

    let objective3 = objective_symbol(&program, 3);
    let mut lifecycle = BlockLifecycleTable::new();
    lifecycle
        .declare(2, LifecycleDecl::awake())
        .expect("block 2 is declared awake");
    let mut session = MissionSession::launch(program, SessionGeneration(13), [])
        .expect("M01's lowered program launches");

    // The player member does not exist yet: the subject chain resolves
    // `Missing` and TRAVELERS cannot fire — the original's own fail-closed
    // read of a subject nobody placed.
    let player = vec!["player".to_owned()];
    let mut folded = lifecycle.facts();
    folded.absorb(world.facts(&reads));
    session
        .advance(&folded, Tick(1))
        .expect("tick 1 advances");
    assert!(
        !session.state().is_completed(objective3),
        "no player member is placed, so TRAVELERS cannot fire"
    );

    // Far away it still cannot: 700 m is the spelled radius, strict.
    world.declare_member("player", [
        anchor_position[0] + 800.0,
        anchor_position[1],
        anchor_position[2],
    ]);
    let mut folded = lifecycle.facts();
    folded.absorb(world.facts(&reads));
    session
        .advance(&folded, Tick(2))
        .expect("tick 2 advances");
    assert!(
        !session.state().is_completed(objective3),
        "outside the spelled radius the traveler does not count"
    );

    // Inside it does — the same member, moved inside the radius.
    assert_eq!(
        world.move_member(&player, anchor_position),
        MemberResolution::Resolved,
        "the declared member moves on the host's observation"
    );
    let mut folded = lifecycle.facts();
    folded.absorb(world.facts(&reads));
    session
        .advance(&folded, Tick(3))
        .expect("tick 3 advances");
    assert!(
        session.state().is_completed(objective3),
        "inside the radius the declared subject completes OBJECTIVE3"
    );
}

/// **M01's `DEDG` group rosters count resolved members — a roster with an
/// ambiguous member stays unknown.**
///
/// `OBJECTIVE2` reads group 1 at `remaining = 0`. The roster is the host's
/// declaration (the original's group assignment is unmeasured), declared here
/// as the mission's pirate zeppelin plus its engines: while any of them is
/// in play the group reads its count; once the host records them all out of
/// play the count is the observed 0 and the block completes. And a roster
/// member that resolves ambiguously makes the whole count unknown — the
/// group writes no key rather than a wrong number.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_m01_dedg_counts_the_declared_roster() {
    let program = m01_program();
    let reads = MissionWorldReads::of(&program);
    let container = c1c_container();
    let mut world = WorldFactTable::mount(&c1c_scene_graph(&container));

    let roster = vec![
        vec!["piratezep".to_owned()],
        vec!["piratezep".to_owned(), "reng11".to_owned()],
        vec!["piratezep".to_owned(), "reng12".to_owned()],
    ];
    world.declare_group(1, roster.iter().cloned());
    assert_eq!(
        world.group_living(1),
        Some(3),
        "every declared member resolves in play on the mounted world"
    );

    let objective2 = objective_symbol(&program, 2);
    let mut lifecycle = BlockLifecycleTable::new();
    lifecycle
        .declare(1, LifecycleDecl::awake())
        .expect("block 1 is declared awake");
    let mut session = MissionSession::launch(program.clone(), SessionGeneration(14), [])
        .expect("M01's lowered program launches");

    // While a roster member lives the group is not depleted.
    let mut folded = lifecycle.facts();
    folded.absorb(world.facts(&reads));
    session
        .advance(&folded, Tick(1))
        .expect("tick 1 advances");
    assert!(
        !session.state().is_completed(objective2),
        "three living members do not satisfy remaining = 0"
    );

    // Out of play, one by one: the count follows the world, and the block
    // completes when it reaches zero.
    for member in &roster {
        assert_eq!(
            world.set_member_in_play(member, false),
            MemberResolution::Resolved
        );
    }
    assert_eq!(world.group_living(1), Some(0));
    let mut folded = lifecycle.facts();
    folded.absorb(world.facts(&reads));
    session
        .advance(&folded, Tick(2))
        .expect("tick 2 advances");
    assert!(
        session.state().is_completed(objective2),
        "the roster observed empty completes OBJECTIVE2's DEDG"
    );

    // The honest-unknown half: enroll a member the mount cannot disambiguate
    // — a bare `ctur1` names five world nodes — and the group count stops
    // existing rather than guessing at a number.
    world.add_to_group(1, vec!["ctur1".to_owned()]);
    assert_eq!(
        world.group_living(1),
        None,
        "an ambiguous roster member makes the count unknown"
    );
    let facts = world.facts(&reads);
    assert_eq!(
        facts.groups.get(&1),
        None,
        "an unknown group writes no key — never an undercount"
    );
}

/// **The `ANIM_STATE` names resolve to the carrier records' measured state
/// bytes — `DORMANT` before play, `RUNNING` and `EXECUTED` as the player
/// runs them.**
///
/// The name space is `mis_anim.zbd`'s and `cam_anim.zbd`'s record
/// `anim_name`s — the exact spellings `OBJECTIVE11`/`OBJECTIVE15`/
/// `OBJECTIVE18` carry (`wv_drop_copilot`, `wv_pickup_copilot`,
/// `hooked_to_klondike`). M01 starts none of them through its startup rows —
/// they belong to directive-side launch paths this task does not build — so
/// their observed byte is `DORMANT` and their blocks honestly stay open.
/// `pzep_engines_start` **is** a `NEW_GAME_START` row: driving it through
/// `MissionAnimationPlayer` exercises the `RUNNING` → `EXECUTED` transition
/// the same map carries.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_m01_animation_states_report_the_measured_bytes() {
    let program = m01_program();
    let reads = MissionWorldReads::of(&program);
    let mut world = WorldFactTable::new();

    let survey = survey_animation_bindings(&game_dir()).expect("the carrier survey runs");
    for (key, kind) in [
        (M01_MISSION_CARRIER, CarrierKind::Mission),
        (M01_CAMERA_CARRIER, CarrierKind::Camera),
    ] {
        let records = survey
            .carrier(key)
            .and_then(|carrier| carrier.payload.as_ref())
            .and_then(|payload| payload.records.as_ref())
            .unwrap_or_else(|| panic!("{key} walks its records"));
        world.mount_animation_records(kind, &records.anim_names);
    }

    // The three names the mission spells resolve — dormant, because M01's
    // startup rows never start them (they are directive-side records).
    let mut player =
        MissionAnimationPlayer::new(SessionId::new(15).expect("nonzero session id"), 30).expect("a declared tick rate is nonzero");
    world.observe_animations(&player);
    let facts = world.facts(&reads);
    for name in ["wv_drop_copilot", "wv_pickup_copilot", "hooked_to_klondike"] {
        assert_eq!(
            facts.animations.get(name),
            Some(&1),
            "{name} is mounted and has never run: the measured DORMANT byte"
        );
    }
    assert_eq!(
        facts.animations.get("reserved_anim_0"),
        None,
        "a name the program does not spell is never written"
    );

    // Now drive one startup record end to end and watch the byte move.
    let binding = bind_mission_animation(&game_dir(), M01).expect("M01's animation binding");
    let report = player.start("NEW_GAME_START", Tick(1), binding.startup());
    assert!(
        report
            .started()
            .iter()
            .any(|identity| identity == "pzep_engines_start"),
        "the startup row starts its record: {report:?}"
    );

    // The record is in the running ledger the moment `start` lands — observe
    // before the first advance, because this record's stored duration fits
    // inside one tick and the RUNNING byte is only reportable here.
    world.observe_animations(&player);
    assert_eq!(
        world.animation_state("pzep_engines_start"),
        Some(2),
        "started and not yet advanced: the measured RUNNING byte"
    );

    let mut observed_executed = false;
    for tick in 2..=600 {
        player.advance(Tick(tick)).expect("the tick advances");
        world.observe_animations(&player);
        match world.animation_state("pzep_engines_start") {
            Some(2) => {}
            Some(3) => {
                observed_executed = true;
                break;
            }
            other => panic!("pzep_engines_start reports a measured byte: {other:?}"),
        }
    }
    assert!(observed_executed, "the finished record reported EXECUTED");
}
