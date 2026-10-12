//! F37-C-FU1 acceptance: the mission evaluator's stream comes from the run's
//! root seed plus the mission's content id, and a reward outside the declared
//! catalog is refused rather than granted.
//!
//! Synthetic fixture only; the seed recipe is an engine design, not a measured
//! original one.

use cs_script::ir::*;
use cs_script::runtime::{
    MISSION_EVALUATOR_DOMAIN, MissionFacts, SessionGeneration, evaluator_seed,
};
use cs_sim::mission::{HostFault, MissionSession};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::random::SplitMix64;

const ROLL: SymbolId = SymbolId(100);
const MARKS: SymbolId = SymbolId(101);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn draw() -> Action {
    Action::Draw {
        variable: ROLL,
        min: -1_000_000,
        max: 1_000_000,
    }
}

fn mark(n: i32) -> Action {
    Action::SetVariable {
        variable: MARKS,
        value: Value::Int(n),
    }
}

fn objective(id: u32, condition: Condition, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("fu1-obj-{id}")),
        condition,
        actions,
        span: None,
    }
}

/// One draw per tick for three ticks, then a reward on the third.
fn program_for(mission: &str, reward: &str) -> MissionProgram {
    let at = |n: i32, mut actions: Vec<Action>| {
        let mut all = vec![mark(n + 1), draw()];
        all.append(&mut actions);
        objective(
            200 + n as u32,
            Condition::Compare {
                variable: MARKS,
                op: CompareOp::Eq,
                value: Value::Int(n),
            },
            all,
        )
    };
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, mission),
        variables: vec![
            Variable {
                id: ROLL,
                name: "roll".into(),
                initial: Value::Int(0),
            },
            Variable {
                id: MARKS,
                name: "marks".into(),
                initial: Value::Int(0),
            },
        ],
        objectives: vec![
            objective(1, Condition::Const(true), vec![mark(1), draw()]),
            at(1, vec![]),
            at(
                2,
                vec![Action::GrantReward {
                    reward: cid(ContentKind::Blueprint, reward),
                }],
            ),
        ],
    }
}

/// Every event key and the drawn value after each of four ticks.
fn run(session: &mut MissionSession) -> Vec<(Vec<String>, Value)> {
    (1..=4)
        .map(|tick| {
            let result = session
                .advance(&MissionFacts::default(), Tick(tick))
                .unwrap();
            let keys = result
                .events
                .iter()
                .map(|e| format!("{:?}", e.key))
                .collect();
            (keys, session.state().variable(ROLL).cloned().unwrap())
        })
        .collect()
}

fn launch(root: u64, mission: &str, rewards: Vec<ContentId>) -> MissionSession {
    MissionSession::launch_seeded(
        program_for(mission, "r"),
        SessionGeneration(7),
        root,
        rewards,
    )
    .unwrap()
}

fn rolls(trace: &[(Vec<String>, Value)]) -> Vec<Value> {
    trace.iter().map(|(_, v)| v.clone()).collect()
}

#[test]
fn accept_f37_c_fu1_one_root_seed_reproduces_and_another_diverges() {
    let reward = vec![cid(ContentKind::Blueprint, "r")];
    let a = run(&mut launch(42, "fu1-m", reward.clone()));
    let b = run(&mut launch(42, "fu1-m", reward.clone()));
    assert_eq!(a, b, "one root seed must reproduce keys and draws");
    let c = run(&mut launch(43, "fu1-m", reward.clone()));
    assert_eq!(
        a.iter().map(|t| &t.0).collect::<Vec<_>>(),
        c.iter().map(|t| &t.0).collect::<Vec<_>>(),
        "event keys are program-determined"
    );
    assert_ne!(rolls(&a), rolls(&c), "a different root seed must diverge");
    // The session counter is not the seed: another generation, same stream.
    let mut other =
        MissionSession::launch_seeded(program_for("fu1-m", "r"), SessionGeneration(99), 42, reward)
            .unwrap();
    assert_eq!(rolls(&a), rolls(&run(&mut other)));
}

#[test]
fn accept_f37_c_fu1_the_mission_id_separates_streams_of_one_root_seed() {
    let one = cid(ContentKind::Mission, "fu1-a");
    let two = cid(ContentKind::Mission, "fu1-b");
    assert_ne!(evaluator_seed(5, &one), evaluator_seed(5, &two));
    let a = run(&mut launch(5, "fu1-a", vec![]));
    let b = run(&mut launch(5, "fu1-b", vec![]));
    assert_ne!(rolls(&a), rolls(&b));
}

#[test]
fn accept_f37_c_fu1_save_restores_the_exact_stream_position() {
    let reward = vec![cid(ContentKind::Blueprint, "r")];
    let reference = run(&mut launch(77, "fu1-m", reward.clone()));
    let mut live = launch(77, "fu1-m", reward);
    for tick in 1..=2 {
        live.advance(&MissionFacts::default(), Tick(tick)).unwrap();
    }
    let record = live.snapshot();
    assert_eq!(
        record.state.seed,
        evaluator_seed(77, &cid(ContentKind::Mission, "fu1-m"))
    );
    // The record stores the stream's own state: the recorded seed's stream
    // start advanced once per tick's draw.
    let mut stream = SplitMix64::for_domain(record.state.seed, MISSION_EVALUATOR_DOMAIN);
    for _ in 0..2 {
        stream.next_u64();
    }
    assert_eq!(record.state.rng_state, stream.state());
    let mut restored = MissionSession::restore(program_for("fu1-m", "r"), record).unwrap();
    for tick in 3..=4 {
        restored
            .advance(&MissionFacts::default(), Tick(tick))
            .unwrap();
    }
    assert_eq!(
        restored.state().variable(ROLL),
        Some(&reference[3].1),
        "the restored stream diverged"
    );
}

#[test]
fn accept_f37_c_fu1_a_reward_outside_the_catalog_is_refused() {
    let declared = cid(ContentKind::Blueprint, "r");
    let mut granted = launch(1, "fu1-m", vec![declared.clone()]);
    let mut refused = launch(1, "fu1-m", vec![]);
    let mut grants = Vec::new();
    let mut faults = Vec::new();
    for tick in 1..=4 {
        let g = granted
            .advance(&MissionFacts::default(), Tick(tick))
            .unwrap();
        grants.extend(g.host.rewards_granted);
        let r = refused
            .advance(&MissionFacts::default(), Tick(tick))
            .unwrap();
        assert!(r.host.rewards_granted.is_empty());
        faults.extend(r.host.faults);
    }
    assert_eq!(grants, std::slice::from_ref(&declared));
    assert_eq!(
        faults,
        [HostFault::UnknownReward {
            reward: declared.clone()
        }]
    );
    assert_eq!(refused.host().granted_rewards(), 0);
}
