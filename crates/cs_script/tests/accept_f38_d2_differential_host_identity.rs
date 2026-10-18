//! F38-D.2: the differential trace identifies a declared step by
//! (host, dispatch value), never by the dispatch value alone.
//!
//! The retail corpus spells the value 6 under two natives (`$$A$$`
//! goscallback and `$$UB$$` uicontrol), which are unrelated protocols. A trace
//! that declares them in one order and a recreation that emits them in the
//! other must therefore diverge. Static code evidence, never `verified_original`.

use cs_script::bindings::differential::{
    DeclaredStep, ScenarioRef, TraceDivergence, TraceStepKind, compare_traces, normalize_declared,
    normalize_emitted,
};
use cs_script::bindings::measured::{MeasuredHost, MeasuredNative};
use cs_script::ir::{
    Action, Condition, IR_VERSION, MissionProgram, Objective, SymbolId, Value, Variable,
};
use cs_script::runtime::{EventKind, MissionFacts, MissionState, SessionGeneration};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

const GOS_CALLBACK: MeasuredHost = MeasuredHost::Native(MeasuredNative::GosCallback);
const UI_CONTROL: MeasuredHost = MeasuredHost::Native(MeasuredNative::UiControl);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn step(host: MeasuredHost, native_id: i64, ordinal: u32) -> DeclaredStep {
    DeclaredStep {
        host,
        native_id,
        ordinal,
        offset: u64::from(ordinal) * 16,
    }
}

/// The shared-value scenario: goscallback#6 then uicontrol#6.
fn declared() -> Vec<DeclaredStep> {
    vec![step(GOS_CALLBACK, 6, 0), step(UI_CONTROL, 6, 1)]
}

#[test]
fn accept_f38_d2_declared_swap_of_two_natives_sharing_a_value_diverges() {
    let scenario = ScenarioRef::new("synthetic/f38d2-scenario");
    let original = normalize_declared(&scenario, &declared()).expect("named");
    let mut swapped = declared();
    swapped.swap(0, 1);
    let recreated = normalize_declared(&scenario, &swapped).expect("named");

    let comparison = compare_traces(&original, &recreated, &scenario, &scenario).expect("same");
    assert!(!comparison.agrees(), "the host is part of the identity");
    assert_eq!(
        comparison.first_divergence,
        Some(TraceDivergence::Mismatched {
            index: 0,
            declared: TraceStepKind::Measured {
                host: GOS_CALLBACK,
                native_id: 6
            },
            emitted: TraceStepKind::Measured {
                host: UI_CONTROL,
                native_id: 6
            },
        })
    );
    assert_eq!(
        original.kinds()[0].to_string(),
        "goscallback#6",
        "a report names the host, not the form"
    );
    assert_eq!(original.kinds()[1].to_string(), "uicontrol#6");
}

/// The engine side: a program whose objectives fire in the opposite order of the
/// declaration emits uicontrol#6 first, and the divergence is reported.
#[test]
fn accept_f38_d2_emitted_order_of_two_natives_sharing_a_value_diverges() {
    let scenario = ScenarioRef::new("synthetic/f38d2-scenario");
    let declared = declared();
    let original = normalize_declared(&scenario, &declared).expect("named");

    let objective = |index: usize| Objective {
        id: SymbolId(10_000 + index as u32),
        content: cid(
            ContentKind::Objective,
            &format!("synthetic-f38d2-obj-{index}"),
        ),
        condition: Condition::Const(true),
        actions: vec![Action::GrantReward {
            reward: cid(ContentKind::ScrapbookItem, &format!("f38d2-{index}")),
        }],
        span: None,
    };
    let run = |declaration: [usize; 2]| {
        let program = MissionProgram {
            version: IR_VERSION,
            mission: cid(ContentKind::Mission, "synthetic-f38d2"),
            variables: vec![Variable {
                id: SymbolId(1),
                name: "roll".to_owned(),
                initial: Value::Int(0),
            }],
            objectives: declaration.iter().map(|&i| objective(i)).collect(),
        };
        let validated = program.validate().expect("a valid program");
        let mut state = MissionState::new(&validated, SessionGeneration(1), 1);
        let mut events = Vec::new();
        for tick in 0..3 {
            let result = state
                .step(&validated, &MissionFacts::default(), Tick(tick))
                .expect("ticks advance");
            events.extend(
                result
                    .events
                    .into_iter()
                    .filter(|e| matches!(e.kind, EventKind::RewardGranted(_))),
            );
        }
        events
    };
    let origin = |event: &cs_script::runtime::MissionEvent| {
        let index = event.key.source.0.checked_sub(10_000)? as usize;
        let s = declared.get(index)?;
        Some((s.host, s.native_id, s.ordinal))
    };

    // Same order: agrees.
    let same = normalize_emitted(&scenario, &run([0, 1]), &origin).expect("named");
    let comparison = compare_traces(&original, &same, &scenario, &scenario).expect("same");
    assert!(comparison.agrees(), "{:?}", comparison.first_divergence);

    // Other order: the dispatch values are equal, the hosts are not.
    let flipped = normalize_emitted(&scenario, &run([1, 0]), &origin).expect("named");
    let comparison = compare_traces(&original, &flipped, &scenario, &scenario).expect("same");
    assert!(!comparison.agrees());
    assert!(matches!(
        comparison.first_divergence,
        Some(TraceDivergence::Mismatched { index: 0, .. })
    ));
}
