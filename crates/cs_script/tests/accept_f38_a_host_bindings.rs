//! Acceptance stage F38-A: the host-binding registry and raw-call lowering
//! (`specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
//! AC01: "A program referencing an unknown host call fails validation before
//! flight"; AC03's source-location part). Every name and byte here is
//! authored for the test; nothing is an original call name.

use cs_script::bindings::{
    ArgDomain, BindingError, BindingProvenance, BindingSpec, HostBindingRegistry, HostFamily,
    Lowering, RawCall, RawObjective, RawProgram, RegistryError, Repeatability, lower_program,
};
use cs_script::ir::{
    Action, Condition, Outcome, SourceSpan, SymbolId, ValidationError, Value, Variable,
};
use cs_types::content::{ContentId, ContentKind};

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn spec(name: &str, args: Vec<ArgDomain>, lowering: Lowering) -> BindingSpec {
    BindingSpec {
        name: name.to_owned(),
        family: HostFamily::MissionState,
        args,
        lowering,
        repeatability: Repeatability::Once,
        provenance: BindingProvenance::Synthetic {
            note: "authored for F38-A tests".to_owned(),
        },
    }
}

fn registry() -> HostBindingRegistry {
    let mut r = HostBindingRegistry::new();
    r.register(spec(
        "synth_win",
        vec![],
        Lowering::Finish(Outcome::Succeeded),
    ))
    .unwrap();
    r.register(spec(
        "synth_reward",
        vec![ArgDomain::Content(ContentKind::ScrapbookItem)],
        Lowering::GrantReward,
    ))
    .unwrap();
    r.register(spec(
        "synth_set",
        vec![
            ArgDomain::IntRange { min: 0, max: 100 },
            ArgDomain::IntRange { min: 0, max: 9 },
        ],
        Lowering::SetVariable,
    ))
    .unwrap();
    r
}

fn call(name: &str, args: Vec<Value>, start: u32) -> RawCall {
    RawCall {
        name: name.to_owned(),
        args,
        span: Some(SourceSpan {
            start,
            end: start + 4,
        }),
    }
}

fn program(calls: Vec<RawCall>) -> RawProgram {
    RawProgram {
        mission: id(ContentKind::Mission, "synthetic_m"),
        variables: vec![Variable {
            id: SymbolId(1),
            name: "counter".to_owned(),
            initial: Value::Int(0),
        }],
        objectives: vec![RawObjective {
            id: SymbolId(2),
            content: id(ContentKind::Objective, "obj"),
            condition: Condition::Const(true),
            calls,
            span: None,
        }],
    }
}

#[test]
fn accept_f38_a_unknown_host_call_fails_before_flight() {
    let errors = lower_program(
        &registry(),
        program(vec![
            call("synth_win", vec![], 0x10),
            call("mystery_call", vec![], 0x20),
            call("another_mystery", vec![], 0x30),
        ]),
    )
    .unwrap_err();
    // Every unknown call is reported, with its location, in program order.
    assert_eq!(errors.len(), 2);
    match &errors[0] {
        BindingError::UnknownHostCall { at, name } => {
            assert_eq!(name, "mystery_call");
            assert_eq!(at.call, 1);
            assert_eq!(at.objective, SymbolId(2));
            assert_eq!(
                at.span,
                Some(SourceSpan {
                    start: 0x20,
                    end: 0x24
                })
            );
            assert!(at.to_string().contains("0x20"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(
        matches!(&errors[1], BindingError::UnknownHostCall { name, .. } if name == "another_mystery")
    );
}

#[test]
fn accept_f38_a_empty_registry_binds_nothing() {
    let errors = lower_program(
        &HostBindingRegistry::new(),
        program(vec![call("synth_win", vec![], 0)]),
    )
    .unwrap_err();
    assert!(matches!(errors[0], BindingError::UnknownHostCall { .. }));
}

#[test]
fn accept_f38_a_bound_program_lowers_and_validates() {
    let reward = id(ContentKind::ScrapbookItem, "card");
    let lowered = lower_program(
        &registry(),
        program(vec![
            call("synth_set", vec![Value::Int(1), Value::Int(3)], 0),
            call("synth_reward", vec![Value::Content(reward.clone())], 8),
            call("synth_win", vec![], 16),
        ]),
    )
    .unwrap();
    assert_eq!(
        lowered.objectives[0].actions,
        vec![
            Action::SetVariable {
                variable: SymbolId(1),
                value: Value::Int(3)
            },
            Action::GrantReward { reward },
            Action::Finish(Outcome::Succeeded),
        ]
    );
    lowered.validate().unwrap();
}

#[test]
fn accept_f38_a_bad_arguments_report_location_without_panic() {
    let r = registry();
    let one = |c: RawCall| lower_program(&r, program(vec![c])).unwrap_err().remove(0);

    assert!(matches!(
        one(call("synth_win", vec![Value::Int(1)], 0x40)),
        BindingError::ArityMismatch {
            expected: 0,
            found: 1,
            ..
        }
    ));
    let e = one(call(
        "synth_set",
        vec![Value::Bool(true), Value::Int(1)],
        0x44,
    ));
    assert!(matches!(e, BindingError::ArgumentType { index: 0, .. }));
    assert_eq!(e.site().span.unwrap().start, 0x44);
    assert!(matches!(
        one(call("synth_set", vec![Value::Int(1), Value::Int(10)], 0)),
        BindingError::ArgumentRange { index: 1, .. }
    ));
    assert!(matches!(
        one(call("synth_set", vec![Value::Int(-1), Value::Int(1)], 0)),
        BindingError::ArgumentRange { index: 0, .. }
    ));
    // A reward of the wrong content kind.
    assert!(matches!(
        one(call(
            "synth_reward",
            vec![Value::Content(id(ContentKind::Sound, "boom"))],
            0
        )),
        BindingError::ArgumentRange { index: 0, .. }
    ));
    // Untrusted oversized name.
    assert!(matches!(
        one(call(&"x".repeat(1000), vec![], 0)),
        BindingError::NameTooLong { len: 1000, .. }
    ));
}

#[test]
fn accept_f38_a_lowered_type_mismatch_still_fails_validation() {
    // The registry cannot know the variable's type; validation still does.
    let lowered = lower_program(
        &registry(),
        program(vec![call(
            "synth_set",
            vec![Value::Int(99), Value::Int(1)],
            0,
        )]),
    )
    .unwrap();
    assert!(matches!(
        lowered.validate(),
        Err(ValidationError::UnknownVariable { .. })
    ));
}

#[test]
fn accept_f38_a_registration_refuses_bad_specs() {
    let mut r = registry();
    assert!(matches!(
        r.register(spec("synth_win", vec![], Lowering::Finish(Outcome::Failed))),
        Err(RegistryError::Duplicate { .. })
    ));
    assert!(matches!(
        r.register(spec("bad name", vec![], Lowering::Finish(Outcome::Failed))),
        Err(RegistryError::BadName { .. })
    ));
    assert!(matches!(
        r.register(spec("no_content", vec![], Lowering::GrantReward)),
        Err(RegistryError::SignatureMismatch { .. })
    ));
    assert_eq!(r.len(), 3);
}
