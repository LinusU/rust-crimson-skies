//! Acceptance for `M01-LC-WORLD-FACTS` (#751): the four **world-side**
//! [`MissionFacts`] maps get production writers, so the lowered conditions
//! `M01-LC-DIRECTIVE-LOWERING` emits can evaluate *and* complete.
//!
//! `MissionFacts::actors` (cs_sim's [`ActorFactTable`]) and `MissionFacts::
//! objectives` (cs_sim's [`BlockLifecycleTable`]) already had writers; `members`,
//! `groups`, `generators` and `animations` had none, so
//! [`MissionState::holds`] answered `false` for every key they read and the 24
//! of M01's 58 blocks carrying `Condition::InactiveMembers`,
//! `Condition::EnemyGroupDepletion`, `Condition::Travelers` or
//! `Condition::AnimationStates` could evaluate but never complete.
//!
//! The suite measures, in order:
//!
//! * that M01's own operands resolve against the owner's installation — its
//!   node hierarchy answers every chain `INACTIVE<n>` spells, and `player`,
//!   which names no node, answers none;
//! * that the observed world lands in **the right map** — chains in `members`,
//!   group ids in `groups`, animation names in `animations`, and M01's
//!   generator set (which the record never spells) stays empty;
//! * that one lowered block completes on observed world state;
//! * that the *same* block does not complete while the world still holds that
//!   member in play;
//! * that removing the resolver keeps every answer fail-closed rather than
//!   defaulting presence, position or count;
//! * and, without retail, that the resolution rule itself is fail-closed on a
//!   hierarchy where a flat name lookup would answer for the wrong object.
//!
//! Test prefix `accept_m01_lc_world_facts_`. The retail cases need
//! `$CS_GAME_DIR` and are `#[ignore]`d so CI runs the synthetic case.

use std::path::PathBuf;

use cs_app::mission_control::survey_mission_control_programs;
use cs_app::world::retail::read_world_containers;
use cs_app::world_facts::{
    MemberObservation, MemberResolver, WorldFactTable, WorldObservation, WorldOperands,
    compose_mission_facts, member_chain,
};
use cs_content::textures::WorldTextureLoad;
use cs_script::ir::{MissionProgram, SymbolId};
use cs_script::runtime::{
    EventKind, MemberPresence, MissionFacts, ObjectiveLifecycle, SessionGeneration,
};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl, MissionSession};
use cs_types::Tick;

/// The census row label of the mission this task is about.
const M01: &str = "zbd/c1c/m01";

/// The world group M01's own record lives beside: `zbd/c1c/m01` measures the
/// `C1C` container, and that is the hierarchy the record's chains name.
const M01_WORLD_GROUP: &str = "c1c";

/// The block this suite drives: `OBJECTIVE53` spells
/// `InactiveMembers { members: [["piratezep"]], threshold: 1 }`, the smallest
/// INACTIVE ladder in M01 — one chain, one required cleared member.
const DRIVEN_BLOCK: u32 = 53;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// M01's lowered program, measured over the owner's installation.
fn m01_program() -> MissionProgram {
    let census = survey_mission_control_programs(&game_dir()).expect("the census runs");
    let row = census.row(M01).expect("M01 is measured");
    row.lowering_attempt()
        .expect("a measured row carries the attempt")
        .program()
        .expect("every one of M01's sites bound")
        .clone()
}

/// The resolver over the installation's `C1C` node hierarchy.
fn m01_resolver() -> MemberResolver {
    let containers = read_world_containers(&game_dir()).expect("the world containers read");
    let container = containers
        .container(M01_WORLD_GROUP, &WorldTextureLoad::project_default())
        .expect("the C1C container reads");
    MemberResolver::from_container(&container)
}

/// The observation this suite calls "the world as M01 opens it": every chain
/// the hierarchy resolves is reported **in play**, the four groups M01 names
/// are reported with a living count above every `remaining` they spell, and the
/// three animations are reported in the states their blocks ask for.
///
/// The positions are the zeroed record [`MemberFact`] documents: this tree does
/// not decode a node's per-type transform, so a position is not invented here
/// — and no condition this suite drives reads one (see
/// [`accept_m01_lc_world_facts_m01_operands_resolve_against_the_retail_world`],
/// which pins that the only TRAVELERS subject in M01 never resolves).
fn world_as_m01_opens(operands: &WorldOperands, resolver: &MemberResolver) -> WorldObservation {
    let mut observation = WorldObservation::new();
    for chain in &operands.members {
        if resolver.resolve(chain).is_some() {
            observation =
                observation.member(chain.clone(), MemberObservation::held_in_play([0.0; 3]));
        }
    }
    observation
        .group(1, 5)
        .group(2, 5)
        .group(3, 5)
        .group(4, 5)
        .animation("wv_drop_copilot", 2)
        .animation("wv_pickup_copilot", 3)
        .animation("hooked_to_klondike", 3)
}

/// Launches M01 with exactly [`DRIVEN_BLOCK`] awake, so any completion this
/// suite observes belongs to the block it is driving and to nothing else.
fn session_with_block_awake(program: MissionProgram) -> (MissionSession, BlockLifecycleTable) {
    let mut blocks = BlockLifecycleTable::new();
    assert_eq!(
        blocks
            .declare(DRIVEN_BLOCK, LifecycleDecl::awake())
            .expect("the driven block is declared once"),
        ObjectiveLifecycle::Awake,
        "a block the record leaves awake starts awake"
    );
    let session = MissionSession::launch(program, SessionGeneration(1), [])
        .expect("the runtime accepts M01's lowered program");
    (session, blocks)
}

// ---------------------------------------------------------------------------
// The retail seam: M01's operands, on the owner's installation
// ---------------------------------------------------------------------------

/// **M01's world-side operands resolve against the observed world.**
///
/// The measurement the whole task rests on: every chain M01's `INACTIVE<n>`
/// sites spell, its `TRAVELERS` anchor, the four `DEDG` group ids, the three
/// `ANIM_STATE` animation names and the generator set (which M01 never spells)
/// are exactly what [`WorldOperands::of`] collects, and the installation's own
/// node hierarchy resolves every chain but one — `player`, which names no node
/// and is therefore recorded absent rather than guessed at.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_m01_operands_resolve_against_the_retail_world() {
    let program = m01_program();
    let operands = WorldOperands::of(&program);
    let resolver = m01_resolver();

    assert!(
        resolver.len() > 100,
        "the C1C hierarchy is really loaded, not an empty one: {} nodes",
        resolver.len()
    );
    assert_eq!(
        operands.groups.iter().copied().collect::<Vec<_>>(),
        vec![1, 2, 3, 4],
        "M01's eight DEDG sites name four group ids"
    );
    assert!(
        operands.generators.is_empty(),
        "M01 spells no generator name, so its DEDG sites read no pending spawns: {:?}",
        operands.generators
    );
    assert_eq!(
        operands.animations.iter().collect::<Vec<_>>(),
        vec![
            &"hooked_to_klondike".to_owned(),
            &"wv_drop_copilot".to_owned(),
            &"wv_pickup_copilot".to_owned(),
        ],
        "M01's three ANIM_STATE sites name three animations, in key order"
    );
    assert_eq!(
        operands.members.len(),
        42,
        "M01's INACTIVE ladders, TRAVELERS subject and TRAVELERS anchor spell 42 \
         distinct chains"
    );

    let mut unresolved = Vec::new();
    for chain in &operands.members {
        if resolver.resolve(chain).is_none() {
            unresolved.push(chain.clone());
        }
    }
    assert_eq!(
        unresolved,
        vec![member_chain(["player"])],
        "every chain M01 spells resolves against the C1C hierarchy except `player`, \
         which names no node — it is recorded absent, never resolved to something else"
    );
}

/// **The observed world lands in the right map.**
///
/// Chains go to `members`, group ids to `groups`, animation names to
/// `animations`, and the generator map stays empty because M01 spells no
/// generator. A chain the hierarchy could not resolve is carried as
/// [`MemberPresence::Missing`]; a chain the hierarchy resolved but nothing
/// observed would carry no row at all — here everything is observed, so every
/// spelled chain is present.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_the_observed_world_lands_in_the_right_map() {
    let program = m01_program();
    let operands = WorldOperands::of(&program);
    let resolver = m01_resolver();
    let observed = world_as_m01_opens(&operands, &resolver);

    let mut table = WorldFactTable::new(resolver);
    table.observe(observed);
    let facts: MissionFacts = table.facts(&operands);

    assert_eq!(
        facts.members.len(),
        operands.members.len(),
        "every chain M01 spells is carried: {} of {}",
        facts.members.len(),
        operands.members.len()
    );
    assert_eq!(
        facts.members[&member_chain(["player"])].presence,
        MemberPresence::Missing,
        "the chain the hierarchy cannot resolve is recorded absent"
    );
    assert_eq!(
        facts.members[&member_chain(["piratezep"])].presence,
        MemberPresence::InPlay,
        "a resolved, observed chain carries the world's own presence"
    );
    assert_eq!(
        facts.groups,
        [(1, 5), (2, 5), (3, 5), (4, 5)].into_iter().collect(),
        "the four group ids M01's DEDG sites name carry their observed living counts"
    );
    assert_eq!(
        facts.animations,
        [
            ("hooked_to_klondike".to_owned(), 3),
            ("wv_drop_copilot".to_owned(), 2),
            ("wv_pickup_copilot".to_owned(), 3),
        ]
        .into_iter()
        .collect(),
        "the three animation names M01's ANIM_STATE sites name carry their observed \
         state bytes"
    );
    assert!(
        facts.generators.is_empty(),
        "M01 spells no generator, so nothing is invented for the generator map: {:?}",
        facts.generators
    );
}

/// **One lowered M01 block completes on observed world state.**
///
/// The block is `OBJECTIVE53`: `All([ObjectiveAwake { 53 }, InactiveMembers {
/// members: [["piratezep"]], threshold: 1 }])`, declared awake by the record's
/// own lifecycle spelling. The world is observed holding `piratezep` **no
/// longer in play**, which is the one fact `Condition::InactiveMembers` counts;
/// the facts are folded through [`compose_mission_facts`] *before* the tick
/// that evaluates them, and the block completes as an ordered runtime event.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_a_lowered_block_completes_on_observed_world_state() {
    let program = m01_program();
    let operands = WorldOperands::of(&program);
    let resolver = m01_resolver();

    assert!(
        matches!(
            &program.objectives[DRIVEN_BLOCK as usize].condition,
            cs_script::ir::Condition::All(items)
                if items.iter().any(|item| matches!(
                    item,
                    cs_script::ir::Condition::InactiveMembers { members, threshold }
                        if members == &vec![member_chain(["piratezep"])] && *threshold == 1
                ))
        ),
        "the driven block is the single-chain INACTIVE ladder this suite drives"
    );

    let observed = world_as_m01_opens(&operands, &resolver)
        .member(member_chain(["piratezep"]), MemberObservation::no_longer_in_play([0.0; 3]));
    let mut table = WorldFactTable::new(resolver);
    table.observe(observed);

    let (mut session, blocks) = session_with_block_awake(program);
    let facts = compose_mission_facts(&session, &blocks, &table, &operands);
    let tick = session.advance(&facts, Tick(1)).expect("tick 1 advances");

    assert!(
        tick.stop.is_none(),
        "the tick ran inside the work budget: {:?}",
        tick.stop
    );
    assert!(
        session.state().is_completed(SymbolId(DRIVEN_BLOCK)),
        "the block completes once the world reports its member cleared; events: {:?}",
        tick.events
    );
    assert!(
        tick.events.iter().any(|event| {
            event.key.source == SymbolId(DRIVEN_BLOCK)
                && event.kind == EventKind::ObjectiveCompleted
        }),
        "the completion reaches the host as an ordered runtime event: {:?}",
        tick.events
    );
    assert_eq!(
        tick
            .events
            .iter()
            .filter(|event| event.kind == EventKind::ObjectiveCompleted)
            .count(),
        1,
        "only the driven block completes: every other block is undeclared, so its \
         wake gate is false whatever the world reports: {:?}",
        tick.events
    );
}

/// **The same block does not complete while the world holds the member in
/// play.**
///
/// The only difference from the case above is one observed presence: the world
/// still holds `piratezep` in play, so `Condition::InactiveMembers` counts zero
/// cleared members against its threshold of one. This is the half that proves
/// the completion above came from the observation rather than from a writer
/// that always answers "cleared".
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_the_same_block_holds_while_the_world_holds_the_member_in_play() {
    let program = m01_program();
    let operands = WorldOperands::of(&program);
    let resolver = m01_resolver();

    let observed = world_as_m01_opens(&operands, &resolver);
    let mut table = WorldFactTable::new(resolver);
    table.observe(observed);

    let (mut session, blocks) = session_with_block_awake(program);
    let facts = compose_mission_facts(&session, &blocks, &table, &operands);
    assert_eq!(
        facts.members[&member_chain(["piratezep"])].presence,
        MemberPresence::InPlay,
        "the world still holds the member in play"
    );

    let tick = session.advance(&facts, Tick(1)).expect("tick 1 advances");
    assert!(
        !session.state().is_completed(SymbolId(DRIVEN_BLOCK)),
        "a member still in play is not a cleared member, so the block holds: {:?}",
        tick.events
    );
    assert!(
        tick.events.is_empty(),
        "nothing completes while the world still holds the members in play: {:?}",
        tick.events
    );
    assert!(
        session.state().directives().is_empty(),
        "no block completes, so no measured directive reaches the host: {:?}",
        session.state().directives()
    );
}

/// **Removing the resolver keeps every answer fail-closed.**
///
/// The observation is untouched and still reports four groups, three animations
/// and every member in play. With the resolver gone, nothing can be tied to the
/// observed world, so every chain M01 spells is recorded
/// [`MemberPresence::Missing`], no count is reported at all, and the driven
/// block does not complete. Nothing is defaulted — no presence, no position,
/// no count.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_facts_removing_the_resolver_keeps_every_answer_fail_closed() {
    let program = m01_program();
    let operands = WorldOperands::of(&program);
    let resolver = m01_resolver();

    let observed = world_as_m01_opens(&operands, &resolver)
        .member(member_chain(["piratezep"]), MemberObservation::no_longer_in_play([0.0; 3]));
    let mut table = WorldFactTable::new(resolver);
    table.observe(observed);
    table.drop_resolver();
    assert!(!table.has_resolver(), "the resolver is gone");

    let facts = table.facts(&operands);
    assert_eq!(
        facts.members.len(),
        operands.members.len(),
        "every spelled chain is still recorded, as absent"
    );
    assert!(
        facts
            .members
            .values()
            .all(|row| row.presence == MemberPresence::Missing),
        "with no resolver no chain can be tied to the world: {:?}",
        facts.members
    );
    assert!(
        facts.members.values().all(|row| row.position == [0.0; 3]),
        "and no position is defaulted either"
    );
    assert!(
        facts.groups.is_empty() && facts.generators.is_empty()
            && facts.animations.is_empty(),
        "no count of any kind is reported without a resolver: groups={:?} \
         generators={:?} animations={:?}",
        facts.groups,
        facts.generators,
        facts.animations
    );

    let (mut session, blocks) = session_with_block_awake(program);
    let folded = compose_mission_facts(&session, &blocks, &table, &operands);
    let tick = session.advance(&folded, Tick(1)).expect("tick 1 advances");
    assert!(
        !session.state().is_completed(SymbolId(DRIVEN_BLOCK)),
        "and the block cannot complete: {:?}",
        tick.events
    );
}

// ---------------------------------------------------------------------------
// The resolution rule itself, without retail
// ---------------------------------------------------------------------------

/// A hierarchy shaped like the installation's: one zeppelin holds `reng1`, a
/// second holds a different `reng1`, and each holds a `healthy` beneath it.
fn two_zeppelin_hierarchy() -> MemberResolver {
    MemberResolver::from_hierarchy(vec![
        ("world1".to_owned(), None),
        ("zeppelin".to_owned(), Some(0)),
        ("gasbag1".to_owned(), Some(1)),
        ("reng1".to_owned(), Some(2)),
        ("healthy".to_owned(), Some(3)),
        ("otherzep".to_owned(), Some(0)),
        ("gasbag1".to_owned(), Some(5)),
        ("reng1".to_owned(), Some(6)),
        ("healthy".to_owned(), Some(7)),
    ])
}

/// **A chain the hierarchy cannot pin to one node is unresolved, not guessed.**
///
/// This is the rule the retail case measures against the installation, run on
/// the smallest hierarchy that can express the failure: `reng1` names two
/// nodes, so the flat name lookup the original's *first* element uses would
/// answer for the wrong ship, and only the hierarchy makes it unambiguous.
/// Without the resolver the table records the chain absent and reports no
/// count; with it, an unobserved chain carries no row at all.
#[test]
fn accept_m01_lc_world_facts_a_chain_that_cannot_be_pinned_stays_fail_closed() {
    let resolver = two_zeppelin_hierarchy();
    assert!(
        resolver
            .resolve(&member_chain(["zeppelin", "reng1", "healthy"]))
            .is_some(),
        "a chain pinned to one node resolves"
    );
    assert_eq!(
        resolver.resolve(&member_chain(["reng1", "healthy"])),
        None,
        "`reng1` names two nodes, so the chain cannot be pinned and must not be \
         answered with one of them"
    );
    assert_eq!(
        resolver.resolve(&member_chain(["missing"])),
        None,
        "a name nothing holds does not resolve"
    );
    assert_eq!(
        resolver.resolve(&member_chain(["zeppelin", "reng1", "absent"])),
        None,
        "a chain whose last element is absent does not resolve"
    );

    let operands = WorldOperands {
        members: [
            member_chain(["zeppelin", "reng1", "healthy"]),
            member_chain(["reng1", "healthy"]),
            member_chain(["missing"]),
        ]
        .into_iter()
        .collect(),
        groups: [7].into_iter().collect(),
        generators: ["owed".to_owned()].into_iter().collect(),
        animations: ["some_anim".to_owned()].into_iter().collect(),
    };

    // Observed: only the pinned chain, plus a group, a generator and an
    // animation the world did report.
    let observed = WorldObservation::new()
        .member(
            member_chain(["zeppelin", "reng1", "healthy"]),
            MemberObservation::no_longer_in_play([1.0, 2.0, 3.0]),
        )
        .group(7, 0)
        .generator("owed", 2)
        .animation("some_anim", 3);
    let mut table = WorldFactTable::new(resolver.clone());
    table.observe(observed.clone());

    let facts = table.facts(&operands);
    assert_eq!(
        facts.members[&member_chain(["zeppelin", "reng1", "healthy"])].presence,
        MemberPresence::OutOfPlay,
        "the observed, pinned chain carries the world's own presence and position"
    );
    assert_eq!(
        facts.members[&member_chain(["zeppelin", "reng1", "healthy"])].position,
        [1.0, 2.0, 3.0],
        "a position arrives with the observation, never from the hierarchy"
    );
    assert_eq!(
        facts.members[&member_chain(["reng1", "healthy"])].presence,
        MemberPresence::Missing,
        "the unpinnable chain is recorded absent rather than defaulted"
    );
    assert_eq!(
        facts.members[&member_chain(["missing"])].presence,
        MemberPresence::Missing,
        "so is the name nothing holds"
    );
    assert_eq!(
        facts.groups,
        [(7, 0)].into_iter().collect(),
        "a reported group count reaches the group map"
    );
    assert_eq!(
        facts.animations,
        [("some_anim".to_owned(), 3)].into_iter().collect(),
        "a reported animation state reaches the animation map"
    );
    assert_eq!(
        facts.generators,
        [("owed".to_owned(), 2)].into_iter().collect(),
        "a reported pending-spawn count reaches the generator map"
    );

    // Resolved but unobserved: no row at all, which is the same fail-closed
    // read `MissionState::holds` gives a key the map does not hold.
    let unobserved = WorldFactTable::new(resolver.clone());
    let facts = unobserved.facts(&operands);
    assert!(
        !facts.members.contains_key(&member_chain(["zeppelin", "reng1", "healthy"])),
        "a resolved chain nobody observed carries no row"
    );
    assert!(facts.groups.is_empty() && facts.generators.is_empty()
        && facts.animations.is_empty(),
        "and neither does a count nobody reported");

    // Resolver removed: every chain absent, no count, whatever was observed.
    let mut removed = WorldFactTable::new(resolver);
    removed.observe(observed);
    removed.drop_resolver();
    let facts = removed.facts(&operands);
    assert_eq!(
        facts.members.len(),
        operands.members.len(),
        "every spelled chain is recorded, as absent"
    );
    assert!(
        facts
            .members
            .values()
            .all(|row| row.presence == MemberPresence::Missing
                && row.position == [0.0; 3]),
        "with no resolver nothing is defaulted: {:?}",
        facts.members
    );
    assert!(
        facts.groups.is_empty() && facts.generators.is_empty()
            && facts.animations.is_empty(),
        "and no count at all is reported"
    );
}
