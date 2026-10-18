//! T583 acceptance (task `CS-TYPES-RNG-STATE`): the F37-C save record carries
//! the mission RNG stream's own `u64` state (`MissionStateSnapshot::rng_state`)
//! instead of a draw count a restore had to replay, and a record carrying an
//! out-of-domain state is refused rather than trusted.
//!
//! Synthetic fixture only; no original data and no original semantics claimed.

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::random::SplitMix64;

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

/// The run's root seed every session below is launched with.
const ROOT_SEED: u64 = 1;

/// One objective that draws once per tick into `SymbolId(100)`, so the
/// session's stream advances one draw on every tick.
fn drawing_program() -> ValidatedProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-t583"),
        variables: vec![
            Variable {
                id: SymbolId(100),
                name: "roll".into(),
                initial: Value::Int(0),
            },
            Variable {
                id: SymbolId(101),
                name: "marks".into(),
                initial: Value::Int(0),
            },
        ],
        objectives: vec![
            objective(1, Condition::Const(true), vec![mark(1), draw()]),
            marks(1),
            marks(2),
            marks(3),
        ],
    }
    .validate()
    .unwrap()
}

fn objective(id: u32, condition: Condition, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("synthetic-obj-{id}")),
        condition,
        actions,
        span: None,
    }
}

fn draw() -> Action {
    Action::Draw {
        variable: SymbolId(100),
        min: -1_000_000,
        max: 1_000_000,
    }
}

fn mark(n: i32) -> Action {
    Action::SetVariable {
        variable: SymbolId(101),
        value: Value::Int(n),
    }
}

fn marks(n: i32) -> Objective {
    objective(
        (200 + n) as u32,
        Condition::Compare {
            variable: SymbolId(101),
            op: CompareOp::Eq,
            value: Value::Int(n),
        },
        vec![mark(n + 1), draw()],
    )
}

/// The stream a save record claims: `MissionState::new` seeds it from the
/// recorded evaluator seed ([`MissionStateSnapshot::seed`]).
fn recorded_stream(record: &MissionStateSnapshot) -> SplitMix64 {
    SplitMix64::for_domain(record.seed, MISSION_EVALUATOR_DOMAIN)
}

/// The record carries the stream's own state — `SplitMix64::state` after the
/// draws taken — and a restore resumes that stream with no replay loop, so
/// every later draw is the uninterrupted session's, bit-for-bit.
#[test]
fn accept_t583_save_record_stores_and_continues_the_stream_state() {
    let p = drawing_program();
    let mut live = MissionState::new(&p, SessionGeneration(9), ROOT_SEED);

    // Uninterrupted reference: the drawn value after each of four ticks.
    let mut reference = MissionState::new(&p, SessionGeneration(9), ROOT_SEED);
    let mut expected = Vec::new();
    for tick in 1..=4 {
        reference
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        expected.push(reference.variable(SymbolId(100)).cloned().unwrap());
    }

    // Save after tick 2: two draws taken, so the record's state is the
    // recorded seed's stream start advanced twice — the state itself, not a
    // count to replay.
    for tick in 1..=2 {
        live.step(&p, &MissionFacts::default(), Tick(tick)).unwrap();
    }
    let record = live.snapshot(&p);
    let mut stream = recorded_stream(&record);
    stream.next_u64();
    stream.next_u64();
    assert_eq!(
        record.rng_state,
        stream.state(),
        "the record stores the stream's state after two draws"
    );
    assert_eq!(record.version, SNAPSHOT_VERSION);

    // Restored, the stream resumes at that state: ticks 3 and 4 draw the
    // uninterrupted session's third and fourth values.
    let mut restored = MissionState::restore(&p, record).unwrap();
    let mut actual = Vec::new();
    for tick in 3..=4 {
        restored
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        actual.push(restored.variable(SymbolId(100)).cloned().unwrap());
    }
    assert_eq!(actual, expected[2..], "the restored stream diverged");
}

/// A record whose `rng_state` is outside the recorded stream's domain —
/// further than `MAX_RNG_STREAM_DRAWS` from the recorded seed's stream start —
/// is refused whole, never installed: another stream's state, a state one
/// draw past the bound, and a `u64` no stream wrote.
#[test]
fn accept_t583_restore_refuses_an_out_of_domain_rng_state() {
    let p = drawing_program();
    let mut live = MissionState::new(&p, SessionGeneration(9), ROOT_SEED);
    live.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    let start = recorded_stream(&live.snapshot(&p));

    // Another stream's start state is out of the recorded stream's domain.
    let foreign = SplitMix64::for_domain(10, MISSION_EVALUATOR_DOMAIN).state();
    let foreign_draws = SplitMix64::new(foreign).draws_since(start);
    assert!(
        foreign_draws > MAX_RNG_STREAM_DRAWS,
        "fixture foreign state must index past the bound: {foreign_draws}"
    );
    let mut record = live.snapshot(&p);
    record.rng_state = foreign;
    assert_eq!(
        MissionState::restore(&p, record),
        Err(RestoreError::RngStateOutOfDomain {
            state: foreign,
            draws: foreign_draws
        })
    );

    // The state one draw past the bound is refused; the bound's own is not.
    let mut stream = start;
    for _ in 0..=MAX_RNG_STREAM_DRAWS {
        stream.next_u64();
    }
    let past = stream.state();
    let mut record = live.snapshot(&p);
    record.rng_state = past;
    assert_eq!(
        MissionState::restore(&p, record),
        Err(RestoreError::RngStateOutOfDomain {
            state: past,
            draws: MAX_RNG_STREAM_DRAWS + 1
        })
    );

    // An arbitrary u64 is refused for what it is: an index past the bound.
    let mut record = live.snapshot(&p);
    record.rng_state = 0;
    let draws = SplitMix64::new(0).draws_since(start);
    assert!(draws > MAX_RNG_STREAM_DRAWS);
    assert_eq!(
        MissionState::restore(&p, record),
        Err(RestoreError::RngStateOutOfDomain { state: 0, draws })
    );

    // The intact record still restores, so the refusals are the domain check
    // and not a fixture that never restores.
    assert!(MissionState::restore(&p, live.snapshot(&p)).is_ok());
}

/// `SNAPSHOT_VERSION` is 6: records carrying an older label — version 3's
/// draw count, version 4's missing running totals, version 5's missing wake ledger or anything earlier — are refused rather than read as a state
/// record.
#[test]
fn accept_t583_older_versioned_records_are_refused() {
    let p = drawing_program();
    let live = MissionState::new(&p, SessionGeneration(9), ROOT_SEED);
    for found in [2, 3, 4, 5] {
        let mut record = live.snapshot(&p);
        record.version = found;
        assert_eq!(
            MissionState::restore(&p, record),
            Err(RestoreError::SnapshotVersion { found })
        );
    }
    assert_eq!(SNAPSHOT_VERSION, 6);
}
