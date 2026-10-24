//! Census of `ZBD/C3/M01`'s control program (#2086): the objective blocks,
//! targets and order that win "The Lost Treasure". Task test prefix:
//! `accept_m01_c3_win_route_census_`.
//!
//! `crates/cs_app/tests/campaign/m01_play_triggers_route.rs` was measured on
//! `ZBD/C1C/M01` (work order M06 "The Red Menace"); this census is the same
//! question asked of the archive the owner's M01 actually is (see
//! `docs/findings/2026-10-17-m01-archive-decision.md`). Every number here is
//! read out of the retail program through the production lowering, never
//! typed in from a previous mission.
//!
//! Block numbers are the record's `OBJECTIVE<n>` numbers. The lowered program
//! indexes them from zero (`OBJECTIVE<n>` is `objectives[n - 1]`); the wake,
//! nap and kill lists spell the `OBJECTIVE<n>` number itself (the executor
//! decodes `n - 1`, `cs_sim::objectives::directives`). The findings write
//! `OBJECTIVE<n>`; the code below says so when it means the index.
//!
//! Needs `CS_GAME_DIR`. See `docs/findings/2026-10-24-m01-c3-win-route-census.md`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionStage, build_headless, read_landings, stage_for, teardown,
};
use cs_app::world_facts::{WorldOperands, member_chain};
use cs_script::ir::{
    Action, AnimationState, Condition, DirectiveOperation, MissionProgram, Outcome, Value,
};

use crate::campaign::common::label;
use crate::campaign::m01_play_triggers_anims_land::pump;
use crate::campaign::vs_m01_rt_window::game_dir;

fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads")
}

fn numbers(value: &Value, out: &mut Vec<u32>) {
    match value {
        Value::Int(n) => out.push(u32::try_from(*n).expect("an objective number is positive")),
        Value::List(items) => items.iter().for_each(|item| numbers(item, out)),
        _ => {}
    }
}

fn text(value: &Value) -> &str {
    match value {
        Value::Str(text) => text,
        other => panic!("expected a text argument, got {other:?}"),
    }
}

/// One block's census row: everything the win route reads from it.
#[derive(Debug, Default)]
struct Block {
    /// `Some(seconds)` for `BEGIN_DORMANT`; `None` when the block starts awake.
    begin_dormant: Option<f64>,
    /// The condition without its own awake gate.
    condition: String,
    wakes: Vec<u32>,
    /// `NAP_OBJECTIVE_WHEN_I_COMPLETE` targets with their seconds.
    naps: Vec<(u32, f64)>,
    kills: Vec<u32>,
    wake_anims: Vec<String>,
    wake_enemies: Vec<String>,
    /// `Some(n)` for the block's `DependencyGate` (the spelled objective number).
    gate: Option<u32>,
    finishes: bool,
}

fn census(program: &MissionProgram) -> BTreeMap<u32, Block> {
    let mut table = BTreeMap::new();
    for (index, objective) in program.objectives.iter().enumerate() {
        let number = u32::try_from(index).expect("few blocks") + 1;
        let mut block = Block {
            condition: match &objective.condition {
                Condition::All(parts) => format!("{:?}", &parts[1..]),
                other => format!("{other:?}"),
            },
            ..Block::default()
        };
        for action in &objective.actions {
            match action {
                Action::Finish(Outcome::Succeeded) => block.finishes = true,
                Action::Finish(other) => panic!("OBJECTIVE{number} finishes {other:?}"),
                Action::Directive { operation, args } => match operation {
                    DirectiveOperation::DormantStart => {
                        let Some(Value::Float(seconds)) = args.first() else {
                            panic!("OBJECTIVE{number}: BEGIN_DORMANT without seconds");
                        };
                        block.begin_dormant = Some(*seconds);
                    }
                    DirectiveOperation::WakeObjectives => {
                        args.iter().for_each(|arg| numbers(arg, &mut block.wakes));
                    }
                    DirectiveOperation::KillObjectives => {
                        args.iter().for_each(|arg| numbers(arg, &mut block.kills));
                    }
                    DirectiveOperation::NapObjective => {
                        let (Some(Value::Int(target)), Some(Value::Float(seconds))) =
                            (args.first(), args.get(1))
                        else {
                            panic!("OBJECTIVE{number}: a nap without [target, seconds]");
                        };
                        block.naps.push((u32::try_from(*target).unwrap(), *seconds));
                    }
                    DirectiveOperation::WakeAnimation => {
                        block.wake_anims.push(text(&args[0]).to_owned());
                    }
                    DirectiveOperation::WakeEnemies => {
                        block
                            .wake_enemies
                            .extend(args.iter().map(|a| text(a).to_owned()));
                    }
                    DirectiveOperation::DependencyGate => {
                        let Some(Value::Int(gate)) = args.first() else {
                            panic!("OBJECTIVE{number}: a gate without a number");
                        };
                        block.gate = Some(u32::try_from(*gate).unwrap());
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        table.insert(number, block);
    }
    table
}

/// The blocks awake from the first tick: no `BEGIN_DORMANT`, or one that
/// names a mission-clock second (`>= 0`; `-1` is the measured "never").
fn initially_scheduled(table: &BTreeMap<u32, Block>) -> BTreeSet<u32> {
    table
        .iter()
        .filter(|(_, block)| block.begin_dormant.is_none_or(|seconds| seconds >= 0.0))
        .map(|(number, _)| *number)
        .collect()
}

/// The shortest wake/nap path from the initially scheduled blocks to `goal`.
fn shortest_chain(table: &BTreeMap<u32, Block>, goal: u32) -> Vec<u32> {
    let start = initially_scheduled(table);
    let mut parent: BTreeMap<u32, u32> = BTreeMap::new();
    let mut seen = start.clone();
    let mut queue: VecDeque<u32> = start.into_iter().collect();
    while let Some(from) = queue.pop_front() {
        let block = &table[&from];
        let targets = block
            .wakes
            .iter()
            .copied()
            .chain(block.naps.iter().map(|n| n.0));
        for to in targets {
            if seen.insert(to) {
                parent.insert(to, from);
                queue.push_back(to);
            }
        }
    }
    let mut chain = vec![goal];
    while let Some(&from) = parent.get(chain.last().unwrap()) {
        chain.push(from);
    }
    chain.reverse();
    chain
}

/// Every block that wakes or naps `target`.
fn wakers_of(table: &BTreeMap<u32, Block>, target: u32) -> Vec<u32> {
    table
        .iter()
        .filter(|(_, b)| b.wakes.contains(&target) || b.naps.iter().any(|n| n.0 == target))
        .map(|(number, _)| *number)
        .collect()
}

/// **The block table and the win chain of `ZBD/C3/M01`.**
///
/// The single `INSTANTWIN` is `OBJECTIVE15`; it fires the tick it wakes.
/// Everything before it is pinned: the proximity site that starts the chain,
/// the naps between, the landing-gated animations, the Medusa group and the
/// hydrogen-tank gate.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_c3_win_route_census_pins_the_block_table_and_the_win_chain() {
    let stage = retail_stage();
    let program = &stage.host.control;
    let table = census(program);
    assert_eq!(table.len(), 39, "C3/M01 carries 39 OBJECTIVE blocks");

    // The mission has exactly one success: OBJECTIVE15's own Finish.
    let finishing: Vec<u32> = table
        .iter()
        .filter(|(_, b)| b.finishes)
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(
        finishing,
        [15],
        "only OBJECTIVE15 finishes, and it succeeds"
    );
    assert_eq!(table[&15].begin_dormant, Some(-1.0));
    assert!(
        table[&15]
            .condition
            .contains("ObjectiveAwake { index: 14 }"),
        "OBJECTIVE15 has no condition beyond being awake: {}",
        table[&15].condition
    );
    // Only OBJECTIVE14 wakes it.
    assert_eq!(wakers_of(&table, 15), [14]);

    // Awake from the first tick (plus OBJECTIVE1's 2.0 s self-wake).
    assert_eq!(
        initially_scheduled(&table),
        BTreeSet::from([1, 2, 7, 9, 10, 11, 12, 19])
    );
    assert_eq!(table[&1].begin_dormant, Some(2.0));

    // The chain, link by link.
    let shipwreck = &table[&12];
    assert!(
        shipwreck.condition.contains("Object([\"shipwreck\"])"),
        "{}",
        shipwreck.condition
    );
    assert!(shipwreck.condition.contains("radius: 200.0"));
    assert_eq!(shipwreck.naps, [(27, 2.0)]);
    assert_eq!(table[&27].naps, [(21, 3.0)]);
    let texdrop = &table[&21];
    assert!(
        texdrop.condition.contains("(\"texdrop\", Executed)"),
        "{}",
        texdrop.condition
    );
    assert_eq!(texdrop.wake_anims, ["enable_dropoff"]);
    assert_eq!(texdrop.wakes, [22]);
    assert_eq!(table[&22].naps, [(23, 10.0)]);
    let medusas = &table[&23];
    assert_eq!(
        medusas.wake_enemies,
        ["medkestrel_1", "medkestrel_2", "medkestrel_3"]
    );
    assert_eq!(medusas.wakes, [29, 28, 25, 13]);
    assert_eq!(medusas.naps, [(39, 2.0)]);
    let cargo = &table[&39];
    assert_eq!(cargo.wake_enemies, ["cargozep1"]);
    assert_eq!(cargo.wake_anims, ["fadein_cg1zep"]);
    assert_eq!(cargo.naps, [(24, 13.0)]);
    assert_eq!(table[&24].naps, [(38, 31.0)]);

    // OBJECTIVE13: the Medusa group is gone and OBJECTIVE26 is awake.
    let depletion = &table[&13];
    assert!(
        depletion
            .condition
            .contains("EnemyGroupDepletion { group: 1, remaining: 0")
    );
    assert_eq!(depletion.gate, Some(26));
    assert_eq!(depletion.naps, [(14, 5.0)]);
    assert_eq!(wakers_of(&table, 13), [23]);
    // OBJECTIVE26 is woken only by OBJECTIVE25, which needs hydrogen_fire running.
    assert_eq!(wakers_of(&table, 26), [25]);
    assert!(
        table[&25]
            .condition
            .contains("(\"hydrogen_fire\", Running)")
    );
    assert_eq!(wakers_of(&table, 25), [23]);
    // OBJECTIVE14: the docking, then the win.
    let dock = &table[&14];
    assert!(
        dock.condition
            .contains("(\"hooked_to_klondike\", Executed)")
    );
    assert_eq!(dock.wake_anims, ["pzhomebase"]);
    assert_eq!(dock.wakes, [15]);
    assert_eq!(wakers_of(&table, 14), [13]);

    // The shortest wake/nap path ignores the OBJECTIVE13 conjunction (it is a
    // chain of wakes, not of conditions); the second branch is OBJECTIVE25/26.
    assert_eq!(shortest_chain(&table, 15), [12, 27, 21, 22, 23, 13, 14, 15]);
    assert_eq!(shortest_chain(&table, 26), [12, 27, 21, 22, 23, 25, 26]);

    // No required block is killed by a required block. The cargo-zeppelin
    // blocks (39, 24, 38: wake the zeppelin, then mark its hydrogen tank)
    // support the route; OBJECTIVE25's completion kills 24 and 38, which only
    // retires the tank marker.
    let route: BTreeSet<u32> = [12, 27, 21, 22, 23, 25, 26, 13, 14, 15].into();
    for number in &route {
        for killed in &table[number].kills {
            assert!(
                !route.contains(killed),
                "OBJECTIVE{number} kills OBJECTIVE{killed}"
            );
        }
    }
    assert_eq!(table[&25].kills, [24, 38]);
    // The convoy blocks (OBJECTIVE5/6/7) are off the route: the win chain
    // never wakes them, and OBJECTIVE21 kills them when it completes.
    for convoy in [5, 6, 7] {
        assert!(!shortest_chain(&table, 15).contains(&convoy));
        assert!(table[&21].kills.contains(&convoy));
    }
    // The other two examine sites are off the route as well.
    for off in [2, 9, 19] {
        assert!(!route.contains(&off));
    }

    // The animation names the program reads, the groups, the zones.
    let operands = WorldOperands::of(program);
    assert_eq!(operands.groups, BTreeSet::from([1]));
    let names: BTreeSet<&str> = operands.animations.iter().map(String::as_str).collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "hooked_to_klondike",
            "hydrogen_fire",
            "rope1burn",
            "texdrop",
            "volcano1"
        ])
    );
    // Each ANIM_STATE gate on the route and the state it waits for.
    for (number, name, state) in [
        (21, "texdrop", AnimationState::Executed),
        (25, "hydrogen_fire", AnimationState::Running),
        (14, "hooked_to_klondike", AnimationState::Executed),
    ] {
        let wanted = format!("(\"{name}\", {state:?})");
        assert!(
            table[&number].condition.contains(&wanted),
            "OBJECTIVE{number}"
        );
    }
}

/// **What the composed windowed mission offers the route.**
///
/// The landing table of `zbd/c3` carries the six `do_approach` rows (the
/// shipwreck drop-off) and the two `pz_*` rows (the docking), all positioned
/// by the composition; the composed observation reports a Medusa group, and an idle run changes none of the
/// route's animation states.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_c3_win_route_census_the_composed_mission_carries_the_route_inputs() {
    let stage = retail_stage();
    let landings = read_landings(&game_dir(), &stage.mount.directory).expect("C3's landings read");
    assert!(landings.refusals.is_empty(), "{:?}", landings.refusals);
    let rows: Vec<(&str, &str, bool)> = landings
        .rows
        .iter()
        .map(|row| (row.anim.as_str(), row.node.as_str(), row.auto))
        .collect();
    for n in 1..=6 {
        let name = format!("do_approach{n}");
        assert!(
            rows.contains(&(name.as_str(), name.as_str(), false)),
            "{name}"
        );
    }
    assert!(rows.contains(&("hooked_to_klondike", "pz_manual_land", false)));
    assert!(rows.contains(&("hooked_to_klondike", "pz_auto_land", true)));

    let mut app = build_headless(&stage).expect("M01 composes");
    pump(&mut app, 120, None);
    let host = app.world().resource::<MissionHost>();
    let observation = host.world_facts().observation();

    // The Medusa group (OBJECTIVE13's DEDG group 1): the three Kestrels.
    assert_eq!(observation.groups().get(&1), Some(&3));

    // The docking nodes and the six drop-off nodes compose a position. The
    // drop-off nodes hang from `treasure_approach`, a static container node
    // (no bound actor), so the container's own position is theirs.
    let positioned = |node: &str| observation.members().contains_key(&member_chain([node]));
    assert!(positioned("pz_manual_land") && positioned("pz_auto_land"));
    for n in 1..=6 {
        assert!(
            positioned(&format!("do_approach{n}")),
            "do_approach{n} has no position"
        );
    }

    // Unaided, none of the route's gated animations has started.
    for name in ["texdrop", "hydrogen_fire", "hooked_to_klondike"] {
        assert_eq!(
            observation.animations().get(name),
            None,
            "{name} moved unaided"
        );
    }
    teardown(&mut app);
}

/// **The drop-off nodes sit where the container states `treasure_approach`.**
///
/// `do_approach1..6` anchor on `treasure_approach`, a static node of the
/// world container that no bound actor spells. The composition reports each
/// node at the container's own position for it (anchor plus offset), while a
/// zeppelin-anchored docking node still waits for its live actor row.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_do_approach_position_nodes_sit_at_the_static_anchor() {
    let stage = retail_stage();
    let landings = read_landings(&game_dir(), &stage.mount.directory).expect("C3's landings read");
    let mut app = build_headless(&stage).expect("M01 composes");
    pump(&mut app, 120, None);
    let host = app.world().resource::<MissionHost>();
    let observation = host.world_facts().observation();
    for n in 1..=6 {
        let name = format!("do_approach{n}");
        let entry = landings
            .nodes
            .iter()
            .find(|entry| entry.node == name)
            .unwrap_or_else(|| panic!("{name} has a node entry"));
        assert_eq!(entry.anchor, "treasure_approach", "{name}");
        let member = observation
            .members()
            .get(&member_chain([name.as_str()]))
            .unwrap_or_else(|| panic!("{name} has no position"));
        for axis in 0..3 {
            let want = entry.anchor_rest[axis] + entry.offset[axis];
            assert!(
                (member.position[axis] - want).abs() < 1e-6,
                "{name} axis {axis}: {} vs {want}",
                member.position[axis]
            );
        }
        assert!(member.in_play, "{name}");
    }
    // The static fallback never applies to an actor anchor: the docking
    // nodes sit at the live piratezep, not at the container's origin.
    let pz = observation
        .members()
        .get(&member_chain(["pz_manual_land"]))
        .expect("pz_manual_land is positioned");
    let zep = observation
        .members()
        .get(&member_chain(["piratezep"]))
        .expect("piratezep is observed");
    let entry = landings
        .nodes
        .iter()
        .find(|entry| entry.node == "pz_manual_land")
        .expect("pz_manual_land has a node entry");
    for axis in 0..3 {
        assert!((pz.position[axis] - zep.position[axis] - entry.offset[axis]).abs() < 1e-3);
    }
    teardown(&mut app);
}
