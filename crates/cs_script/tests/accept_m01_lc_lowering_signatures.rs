//! M01-LC lowering acceptance: every measured directive signature is
//! represented, a nested directive argument is carried as structure, and every
//! measured `DirectiveOperation` has a real `Lowering` that validates and
//! executes (`specs/` task `M01-LC-DIRECTIVE-LOWERING.01`; shared contract
//! `docs/contracts/SCRIPT-MISSION.md`).
//!
//! The per-key measured shapes below are the census's own labels from
//! `docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md`,
//! pinned against the retail record by
//! `cs_app`'s `accept_m01_lc_directive_a_each_mapped_keys_argument_shapes_match_
//! the_record`. The `cs_content::mission_control::DirectiveOperation` cross-
//! check is a test-only edge (`cs_script` may not link `cs_content`).

use std::collections::BTreeSet;

use cs_content::mission_control::DirectiveOperation as MeasuredOperation;
use cs_script::bindings::observed::MAX_MEASURED_STRING_BYTES;
use cs_script::bindings::{
    ArgDomain, BindingError, BindingProvenance, BindingSpec, CallSite, HostBindingRegistry,
    HostFamily, Lowering, RawCall, RawObjective, RawProgram, RegistryError, Repeatability,
    lower_program,
};
use cs_script::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MAX_VALUE_DEPTH, MAX_VALUE_ITEMS,
    MissionProgram, Objective, Outcome, Phase, SourceSpan, SymbolId, ValidationError, Value,
};
use cs_script::runtime::{
    DirectiveEmission, EventKey, MissionFacts, MissionState, MissionStateSnapshot, RestoreDefect,
    RestoreError, SNAPSHOT_VERSION, SessionGeneration, TerminalState, WorkLimits,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

// --- measured-shape builders ----------------------------------------------
//
// The mechanical crossing a record adapter performs
// (`cs_content::mission_control::MeasuredArg` → `ArgDomain`), spelled out here
// because the row lives outside this crate: `int` is an unrestricted integer
// literal of unknown meaning, `float` any finite float literal, `text` a
// string literal under the corpus's safety cap, `[]` the empty list, and
// `[a,b,…]` a `List` domain over its children in order.

fn t() -> ArgDomain {
    ArgDomain::Str {
        max_bytes: MAX_MEASURED_STRING_BYTES,
    }
}

fn i() -> ArgDomain {
    ArgDomain::IntRange {
        min: i32::MIN,
        max: i32::MAX,
    }
}

fn f() -> ArgDomain {
    ArgDomain::FloatRange {
        min: -f64::MAX,
        max: f64::MAX,
    }
}

fn l(children: Vec<ArgDomain>) -> ArgDomain {
    ArgDomain::List(children)
}

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

fn ls(items: Vec<Value>) -> Value {
    Value::List(items)
}

/// The measured signatures of one directive key: `(name, operation, shapes)`,
/// each shape one accepted argument domain list. Every shape M01 spells is
/// carried; none is chosen over another.
type MeasuredKey = (&'static str, DirectiveOperation, Vec<Vec<ArgDomain>>);

/// The nine M01 keys whose sites disagree about their argument shape, with
/// every shape the census records for them.
fn disagreeing_keys() -> Vec<MeasuredKey> {
    vec![
        // `[text]` ×2, `[[text,text]]` ×2.
        (
            "ADD_OBJECTIVE_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: true,
            },
            vec![vec![t()], vec![l(vec![t(), t()])]],
        ),
        // `[text]` ×2, `[[text,text]]` ×1.
        (
            "ADD_OTHER_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: false,
                set: true,
            },
            vec![vec![t()], vec![l(vec![t(), t()])]],
        ),
        // `[text,int]` ×1, `[text,int,text]` ×4.
        (
            "IDENTITY",
            DirectiveOperation::PresentationIdentity,
            vec![vec![t(), i()], vec![t(), i(), t()]],
        ),
        // `[text]` ×2, `[text,text,text]` ×10.
        (
            "INACTIVE1",
            DirectiveOperation::InactiveMembers,
            vec![vec![t()], vec![t(), t(), t()]],
        ),
        // `[int]` ×5, `[int,int]` ×2, `[int,int,int]` ×2.
        (
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            DirectiveOperation::KillObjectives,
            vec![vec![i()], vec![i(), i()], vec![i(), i(), i()]],
        ),
        // `[text]` ×1, `[[text,text]]` ×3.
        (
            "REMOVE_OBJECTIVE_TARGET",
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: false,
            },
            vec![vec![t()], vec![l(vec![t(), t()])]],
        ),
        // `[[text,text]]` ×2, `[[text,text],[text,text]]` ×1.
        (
            "SET_AI_NET",
            DirectiveOperation::AssignNet,
            vec![
                vec![l(vec![t(), t()])],
                vec![l(vec![t(), t()]), l(vec![t(), t()])],
            ],
        ),
        // `[text,text]` ×1, `[[text,text],text]` ×1.
        (
            "SET_HELP_LABEL",
            DirectiveOperation::SetHelpLabel,
            vec![vec![t(), t()], vec![l(vec![t(), t()]), t()]],
        ),
        // `[int]` ×16, `[int,int]` ×2, `[int,int,int]` ×1, `[int,int,int,int]` ×1.
        (
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
            DirectiveOperation::WakeObjectives,
            vec![
                vec![i()],
                vec![i(), i()],
                vec![i(), i(), i()],
                vec![i(), i(), i(), i()],
            ],
        ),
    ]
}

/// Argument values that satisfy a measured signature, one per domain —
/// `List` domains get `List` values with matching children, so the value is
/// the structure the domain describes.
fn arguments_of(domains: &[ArgDomain]) -> Vec<Value> {
    domains
        .iter()
        .map(|domain| match domain {
            ArgDomain::Bool => Value::Bool(true),
            ArgDomain::IntRange { min, .. } => Value::Int(*min),
            ArgDomain::FloatRange { min, .. } => Value::Float(*min),
            ArgDomain::Str { .. } => s("measured"),
            ArgDomain::Content(kind) => Value::Content(cid(*kind, "measured")),
            ArgDomain::Actor => Value::Actor(cs_script::ir::ActorId(7)),
            ArgDomain::Vector => Value::Vector([1.0, 2.0, 3.0]),
            ArgDomain::OptActor => Value::OptActor(None),
            ArgDomain::List(children) => ls(arguments_of(children)),
        })
        .collect()
}

fn directive_spec(
    name: &str,
    operation: DirectiveOperation,
    signatures: Vec<Vec<ArgDomain>>,
) -> BindingSpec {
    BindingSpec {
        name: name.to_owned(),
        family: HostFamily::MissionState,
        signatures,
        lowering: Lowering::Directive(operation),
        repeatability: Repeatability::Once,
        provenance: BindingProvenance::Observed {
            evidence: "M01 control-record census, findings A–D".to_owned(),
        },
    }
}

fn site(call_index: usize) -> CallSite {
    CallSite {
        mission: "zbd/c1c/m01".to_owned(),
        objective: SymbolId(2),
        call: call_index,
        span: Some(SourceSpan {
            start: 0x100,
            end: 0x104,
        }),
    }
}

fn call(name: &str, args: Vec<Value>) -> RawCall {
    RawCall {
        name: name.to_owned(),
        args,
        span: Some(SourceSpan {
            start: 0x100,
            end: 0x104,
        }),
    }
}

/// AC: every measured shape for one host key is represented and accepted; no
/// majority shape is chosen. For each of the nine disagreeing M01 keys, every
/// measured shape binds to the same declared operation with the call's own
/// arguments — the minority shape is not rejected for disagreeing.
#[test]
fn accept_m01_lc_lowering_signatures_every_measured_shape_binds() {
    for (name, operation, signatures) in disagreeing_keys() {
        let mut registry = HostBindingRegistry::new();
        registry
            .register(directive_spec(name, operation, signatures.clone()))
            .unwrap_or_else(|e| panic!("{name} must register its measured shapes: {e}"));
        // The registration kept the measured provenance with its evidence.
        assert_eq!(
            registry.get(name).unwrap().provenance,
            BindingProvenance::Observed {
                evidence: "M01 control-record census, findings A–D".to_owned()
            }
        );
        assert_eq!(registry.get(name).unwrap().signatures, signatures);
        for (n, signature) in signatures.iter().enumerate() {
            let args = arguments_of(signature);
            let action = registry
                .bind(&call(name, args.clone()), &site(n))
                .unwrap_or_else(|e| panic!("{name} measured shape {n} must bind: {e}"));
            assert_eq!(
                action,
                Action::Directive { operation, args },
                "{name} shape {n} binds to its measured operation with its own arguments"
            );
        }
        // And the literal witness: `lower_program` binds one objective
        // spelling the key once per measured shape, and every bound action
        // keeps its own site's arguments.
        let calls: Vec<RawCall> = signatures
            .iter()
            .map(|signature| call(name, arguments_of(signature)))
            .collect();
        let expected: Vec<Action> = signatures
            .iter()
            .map(|signature| Action::Directive {
                operation,
                args: arguments_of(signature),
            })
            .collect();
        let program = lower_program(
            &registry,
            RawProgram {
                mission: cid(ContentKind::Mission, "m01"),
                variables: vec![],
                objectives: vec![RawObjective {
                    id: SymbolId(2),
                    content: cid(ContentKind::Objective, "block"),
                    condition: Condition::Const(true),
                    calls,
                    span: None,
                }],
            },
        )
        .unwrap_or_else(|e| panic!("{name} sites must lower: {e:?}"));
        assert_eq!(program.objectives[0].actions, expected);
    }
}

/// AC: a nested argument structure round-trips as structure into the bound
/// call — the two census witnesses.
#[test]
fn accept_m01_lc_lowering_signatures_nested_arguments_stay_nested() {
    // `ANIM_STATE`: `[text,[text,[text],text,[text]]]` ×3 — arg1 is one list
    // node whose own children include two nested one-text lists.
    let anim_signature = vec![t(), l(vec![t(), l(vec![t()]), t(), l(vec![t()])])];
    let anim_args = vec![
        s("ANIM"),
        ls(vec![
            s("NAME"),
            ls(vec![s("engine_start")]),
            s("STATE"),
            ls(vec![s("RUNNING")]),
        ]),
    ];
    // `COMPLETED_STOPPOINT`: `[[text,int,int]]` ×1 — arg0 is one list node of
    // a `{name, int, int}` record.
    let stoppoint_signature = vec![l(vec![t(), i(), i()])];
    let stoppoint_args = vec![ls(vec![s("rail_junction"), Value::Int(2), Value::Int(1)])];

    let mut registry = HostBindingRegistry::new();
    registry
        .register(directive_spec(
            "ANIM_STATE",
            DirectiveOperation::AnimationStates,
            vec![anim_signature],
        ))
        .unwrap();
    registry
        .register(directive_spec(
            "COMPLETED_STOPPOINT",
            DirectiveOperation::AdvanceStopPoint,
            vec![stoppoint_signature],
        ))
        .unwrap();

    let program = lower_program(
        &registry,
        RawProgram {
            mission: cid(ContentKind::Mission, "m01"),
            variables: vec![],
            objectives: vec![RawObjective {
                id: SymbolId(2),
                content: cid(ContentKind::Objective, "block"),
                condition: Condition::Const(true),
                calls: vec![
                    call("ANIM_STATE", anim_args.clone()),
                    call("COMPLETED_STOPPOINT", stoppoint_args.clone()),
                ],
                span: None,
            }],
        },
    )
    .unwrap();
    let lowered = program.validate().unwrap();
    // The actions carry the nested arguments field for field — no flattening
    // into positional scalars, no reordering.
    assert_eq!(
        lowered.program().objectives[0].actions,
        vec![
            Action::Directive {
                operation: DirectiveOperation::AnimationStates,
                args: anim_args.clone(),
            },
            Action::Directive {
                operation: DirectiveOperation::AdvanceStopPoint,
                args: stoppoint_args.clone(),
            },
        ]
    );
    // And the runtime's host-effect record keeps the structure intact.
    let mut state = MissionState::new(&lowered, SessionGeneration(7));
    state
        .step(&lowered, &MissionFacts::default(), Tick(1))
        .unwrap();
    let directives = state.directives();
    assert_eq!(directives.len(), 2);
    assert_eq!(directives[0].operation, DirectiveOperation::AnimationStates);
    assert_eq!(directives[0].args, anim_args);
    assert_eq!(
        directives[1].operation,
        DirectiveOperation::AdvanceStopPoint
    );
    assert_eq!(directives[1].args, stoppoint_args);
}

/// AC: every operation code `cs_content::mission_control::DirectiveOperation`
/// publishes has a `cs_script::ir::DirectiveOperation` — the same codes, one
/// for one — so the two copies of the measured vocabulary cannot drift apart.
#[test]
fn accept_m01_lc_lowering_signatures_every_measured_operation_maps() {
    // The measured vocabulary, as `cs_content` declares it — 39 variants,
    // 43 code values (`SetTargetFlag` ×4, `OutcomeClass` ×2). Each pairs the
    // measured operation with the mirror variant it must map to.
    let pairs: [(MeasuredOperation, DirectiveOperation); 43] = [
        (
            MeasuredOperation::InactiveMembers,
            DirectiveOperation::InactiveMembers,
        ),
        (
            MeasuredOperation::InactiveThreshold,
            DirectiveOperation::InactiveThreshold,
        ),
        (
            MeasuredOperation::DangerZoneFlags,
            DirectiveOperation::DangerZoneFlags,
        ),
        (
            MeasuredOperation::DangerZoneThreshold,
            DirectiveOperation::DangerZoneThreshold,
        ),
        (
            MeasuredOperation::EnemyGroupDepletion,
            DirectiveOperation::EnemyGroupDepletion,
        ),
        (
            MeasuredOperation::AnimationStates,
            DirectiveOperation::AnimationStates,
        ),
        (MeasuredOperation::Travelers, DirectiveOperation::Travelers),
        (
            MeasuredOperation::NamedCounters,
            DirectiveOperation::NamedCounters,
        ),
        (
            MeasuredOperation::DormantStart,
            DirectiveOperation::DormantStart,
        ),
        (
            MeasuredOperation::DependencyGate,
            DirectiveOperation::DependencyGate,
        ),
        (
            MeasuredOperation::PresentationIdentity,
            DirectiveOperation::PresentationIdentity,
        ),
        (
            MeasuredOperation::WakeObjectives,
            DirectiveOperation::WakeObjectives,
        ),
        (
            MeasuredOperation::SleepObjectives,
            DirectiveOperation::SleepObjectives,
        ),
        (
            MeasuredOperation::KillObjectives,
            DirectiveOperation::KillObjectives,
        ),
        (
            MeasuredOperation::NapObjective,
            DirectiveOperation::NapObjective,
        ),
        (
            MeasuredOperation::SetTargetFlag {
                objective: true,
                set: true,
            },
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: true,
            },
        ),
        (
            MeasuredOperation::SetTargetFlag {
                objective: true,
                set: false,
            },
            DirectiveOperation::SetTargetFlag {
                objective: true,
                set: false,
            },
        ),
        (
            MeasuredOperation::SetTargetFlag {
                objective: false,
                set: true,
            },
            DirectiveOperation::SetTargetFlag {
                objective: false,
                set: true,
            },
        ),
        (
            MeasuredOperation::SetTargetFlag {
                objective: false,
                set: false,
            },
            DirectiveOperation::SetTargetFlag {
                objective: false,
                set: false,
            },
        ),
        (
            MeasuredOperation::AdvanceStopPoint,
            DirectiveOperation::AdvanceStopPoint,
        ),
        (
            MeasuredOperation::ZeppelinCannons,
            DirectiveOperation::ZeppelinCannons,
        ),
        (MeasuredOperation::AssignNet, DirectiveOperation::AssignNet),
        (
            MeasuredOperation::AssignTeam,
            DirectiveOperation::AssignTeam,
        ),
        (
            MeasuredOperation::SetAttackRadius,
            DirectiveOperation::SetAttackRadius,
        ),
        (
            MeasuredOperation::ReleaseTaxi,
            DirectiveOperation::ReleaseTaxi,
        ),
        (
            MeasuredOperation::SetHelpLabel,
            DirectiveOperation::SetHelpLabel,
        ),
        (
            MeasuredOperation::StopQueuedSounds,
            DirectiveOperation::StopQueuedSounds,
        ),
        (
            MeasuredOperation::CompletedSoundGroup,
            DirectiveOperation::CompletedSoundGroup,
        ),
        (
            MeasuredOperation::AdjustMissionTimer,
            DirectiveOperation::AdjustMissionTimer,
        ),
        (
            MeasuredOperation::EndMissionTimer,
            DirectiveOperation::EndMissionTimer,
        ),
        (
            MeasuredOperation::WarpVehicle,
            DirectiveOperation::WarpVehicle,
        ),
        (
            MeasuredOperation::WakeEnemies,
            DirectiveOperation::WakeEnemies,
        ),
        (
            MeasuredOperation::WakeTurrets,
            DirectiveOperation::WakeTurrets,
        ),
        (
            MeasuredOperation::WakeZeppelinTurrets,
            DirectiveOperation::WakeZeppelinTurrets,
        ),
        (
            MeasuredOperation::FeedGenerator,
            DirectiveOperation::FeedGenerator,
        ),
        (
            MeasuredOperation::WakeAnimation,
            DirectiveOperation::WakeAnimation,
        ),
        (
            MeasuredOperation::WakeSoundGroup,
            DirectiveOperation::WakeSoundGroup,
        ),
        (
            MeasuredOperation::ResetMissionTimer,
            DirectiveOperation::ResetMissionTimer,
        ),
        (
            MeasuredOperation::HideObjective,
            DirectiveOperation::HideObjective,
        ),
        (
            MeasuredOperation::TransitionAnimation,
            DirectiveOperation::TransitionAnimation,
        ),
        (
            MeasuredOperation::WakeObjectivesOnTransition,
            DirectiveOperation::WakeObjectivesOnTransition,
        ),
        (
            MeasuredOperation::OutcomeClass { won: true },
            DirectiveOperation::OutcomeClass { won: true },
        ),
        (
            MeasuredOperation::OutcomeClass { won: false },
            DirectiveOperation::OutcomeClass { won: false },
        ),
    ];
    let mut codes = BTreeSet::new();
    for (measured, mirror) in &pairs {
        let code = measured.code();
        assert!(codes.insert(code), "duplicate measured code {code}");
        // The wire code maps to exactly the paired mirror variant.
        assert_eq!(
            DirectiveOperation::from_code(code),
            Some(*mirror),
            "measured code {code} must resolve to its mirror"
        );
        // And the mirror publishes the identical code.
        assert_eq!(mirror.code(), code);
    }
    // `ALL` is exactly the measured set: same variants, same codes, no
    // remainder — a code `from_code` cannot name does not exist.
    let all: BTreeSet<DirectiveOperation> = DirectiveOperation::ALL.iter().copied().collect();
    assert_eq!(all.len(), pairs.len(), "ALL holds every measured code once");
    assert_eq!(
        all,
        pairs.iter().map(|(_, mirror)| *mirror).collect(),
        "ALL is exactly the mirrored vocabulary"
    );
}

/// AC: every operation binds to an `Action` `MissionProgram::validate`
/// accepts, its binding's `phase()` is derived and total, and the runtime
/// executes it as the documented host effect — one `DirectiveEmission` per
/// execution key on the session's directive log.
#[test]
fn accept_m01_lc_lowering_signatures_every_operation_executes() {
    let mut registry = HostBindingRegistry::new();
    let mut raw_calls = Vec::new();
    for operation in DirectiveOperation::ALL {
        // `NAP_OBJECTIVE_WHEN_I_COMPLETE`'s measured shape is `[int,float]`
        // (the census's `[int,float]` ×27); every other operation here binds
        // a single text argument.
        let signature = if operation == DirectiveOperation::NapObjective {
            vec![i(), f()]
        } else {
            vec![t()]
        };
        let spec = directive_spec(
            &format!("K_{}", operation.code()),
            operation,
            vec![signature],
        );
        assert_eq!(spec.phase(), Phase::Host, "a directive is a host effect");
        registry.register(spec).unwrap();
        let args = if operation == DirectiveOperation::NapObjective {
            vec![Value::Int(4), Value::Float(0.3)]
        } else {
            vec![s(&format!("arg-{}", operation.code()))]
        };
        raw_calls.push(call(&format!("K_{}", operation.code()), args));
    }
    assert_eq!(registry.len(), 43);
    let lowered = lower_program(
        &registry,
        RawProgram {
            mission: cid(ContentKind::Mission, "m01"),
            variables: vec![],
            objectives: vec![RawObjective {
                id: SymbolId(2),
                content: cid(ContentKind::Objective, "block"),
                condition: Condition::Const(true),
                calls: raw_calls,
                span: None,
            }],
        },
    )
    .unwrap();
    let program = lowered.validate().unwrap();
    let mut state = MissionState::new(&program, SessionGeneration(1));
    state
        .step(&program, &MissionFacts::default(), Tick(1))
        .unwrap();
    let directives = state.directives();
    assert_eq!(directives.len(), 43, "every bound operation executed");
    for (index, (emission, operation)) in directives
        .iter()
        .zip(DirectiveOperation::ALL.iter())
        .enumerate()
    {
        assert_eq!(emission.operation, *operation);
        let expected_args = if *operation == DirectiveOperation::NapObjective {
            vec![Value::Int(4), Value::Float(0.3)]
        } else {
            vec![s(&format!("arg-{}", operation.code()))]
        };
        assert_eq!(emission.args, expected_args);
        assert_eq!(
            emission.key,
            EventKey {
                session: SessionGeneration(1),
                tick: Tick(1),
                source: SymbolId(2),
                sequence: index as u32 + 1,
            }
        );
    }
}

/// The directive log's documented effect, under the runtime's own rules:
/// emissions carry their execution key exactly once, are kept in `EventKey`
/// order however execution interleaved them, survive a save/restore, and a
/// replay of the same execution does not emit twice.
#[test]
fn accept_m01_lc_lowering_signatures_directive_log_is_ordered_once_and_restorable() {
    // Two objectives firing in one tick: declared in reverse id order so
    // execution order and key order disagree.
    let program = MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "m01"),
        variables: vec![],
        objectives: vec![
            Objective {
                id: SymbolId(9),
                content: cid(ContentKind::Objective, "later"),
                condition: Condition::Const(true),
                actions: vec![Action::Directive {
                    operation: DirectiveOperation::WakeObjectives,
                    args: vec![Value::Int(1)],
                }],
                span: None,
            },
            Objective {
                id: SymbolId(3),
                content: cid(ContentKind::Objective, "earlier"),
                condition: Condition::Const(true),
                actions: vec![Action::Directive {
                    operation: DirectiveOperation::KillObjectives,
                    args: vec![ls(vec![s("x")])],
                }],
                span: None,
            },
        ],
    }
    .validate()
    .unwrap();
    let mut state = MissionState::new(&program, SessionGeneration(4));
    state
        .step(&program, &MissionFacts::default(), Tick(1))
        .unwrap();
    let directives = state.directives();
    assert_eq!(directives.len(), 2);
    // Key order wins over execution order: source 3 before source 9.
    assert_eq!(directives[0].key.source, SymbolId(3));
    assert_eq!(directives[0].operation, DirectiveOperation::KillObjectives);
    assert_eq!(directives[1].key.source, SymbolId(9));
    assert_eq!(directives[1].operation, DirectiveOperation::WakeObjectives);

    // Save/restore keeps the log, and the consumed keys with it.
    let record = state.snapshot(&program);
    let mut restored = MissionState::restore(&program, record).unwrap();
    assert_eq!(restored.directives(), directives);
    let t2 = restored
        .step(&program, &MissionFacts::default(), Tick(2))
        .unwrap();
    assert!(
        t2.events.is_empty() && restored.directives().len() == 2,
        "a replayed tick emits nothing twice"
    );
}

/// The restore defect checks a forged record trips on the directive log.
#[test]
fn accept_m01_lc_lowering_signatures_restore_checks_the_directive_log() {
    let program = MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "m01"),
        variables: vec![],
        objectives: vec![],
    }
    .validate()
    .unwrap();
    let base = MissionStateSnapshot {
        version: SNAPSHOT_VERSION,
        mission: program.program().mission.clone(),
        session: SessionGeneration(4),
        variables: vec![],
        completed: vec![],
        consumed: vec![],
        directives: vec![],
        terminal: TerminalState::Running,
        last_tick: None,
        policy: cs_script::runtime::PrecedencePolicy::SyntheticConservative,
        pending: vec![],
        next_item_ordinal: 0,
        limits: WorkLimits::default(),
        rng_draws: 0,
    };
    let key = |sequence| EventKey {
        session: SessionGeneration(4),
        tick: Tick(1),
        source: SymbolId(2),
        sequence,
    };
    let emission = |key| DirectiveEmission {
        key,
        operation: DirectiveOperation::WakeObjectives,
        args: vec![],
    };

    // An emission whose execution key was never consumed cannot come from a
    // live session.
    let mut forged = base.clone();
    forged.directives = vec![emission(key(1))];
    assert_eq!(
        MissionState::restore(&program, forged),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::DirectiveNotConsumed {
                key: key(1).execution_key()
            }
        })
    );

    // Out-of-order emissions: no live session produced them that way.
    let mut forged = base.clone();
    forged.consumed = vec![key(1).execution_key(), key(2).execution_key()];
    forged.directives = vec![emission(key(2)), emission(key(1))];
    assert_eq!(
        MissionState::restore(&program, forged),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::DirectiveOrder
        })
    );

    // An emission belonging to another session is foreign.
    let mut forged = base;
    forged.consumed = vec![key(1).execution_key()];
    forged.directives = vec![DirectiveEmission {
        key: EventKey {
            session: SessionGeneration(9),
            ..key(1)
        },
        operation: DirectiveOperation::WakeObjectives,
        args: vec![],
    }];
    assert_eq!(
        MissionState::restore(&program, forged),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::ForeignExecutionKey {
                session: SessionGeneration(9)
            }
        })
    );
}

/// AC (refusals): an unregistered directive key fails `UnknownHostCall`; an
/// unmeasured shape is refused; the registry's own checks still fire.
#[test]
fn accept_m01_lc_lowering_signatures_refusals_hold() {
    let mut registry = HostBindingRegistry::new();
    let help = directive_spec(
        "SET_HELP_LABEL",
        DirectiveOperation::SetHelpLabel,
        vec![vec![t(), t()], vec![l(vec![t(), t()]), t()]],
    );
    let wake = directive_spec(
        "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        DirectiveOperation::WakeObjectives,
        vec![
            vec![i()],
            vec![i(), i()],
            vec![i(), i(), i()],
            vec![i(), i(), i(), i()],
        ],
    );
    let stoppoint = directive_spec(
        "COMPLETED_STOPPOINT",
        DirectiveOperation::AdvanceStopPoint,
        vec![vec![l(vec![t(), i(), i()])]],
    );
    for spec in [help, wake, stoppoint] {
        registry.register(spec).unwrap();
    }

    // A key no binding was registered for is unknown, location and all.
    let e = registry
        .bind(&call("UNMEASURED_KEY", vec![]), &site(0))
        .unwrap_err();
    assert!(matches!(e, BindingError::UnknownHostCall { .. }));

    // An arity no measured shape spells is refused with every accepted count.
    let e = registry
        .bind(&call("SET_HELP_LABEL", vec![s("x")]), &site(0))
        .unwrap_err();
    assert!(matches!(
        e,
        BindingError::ArityMismatch { expected, found: 1, .. } if expected == [2]
    ));
    let e = registry
        .bind(
            &call("WAKE_OBJECTIVE_WHEN_I_COMPLETE", vec![Value::Int(0); 5]),
            &site(0),
        )
        .unwrap_err();
    assert!(matches!(
        e,
        BindingError::ArityMismatch { expected, found: 5, .. } if expected == [1, 2, 3, 4]
    ));

    // A shape no site spells — right count, wrong argument kind — is refused
    // by the arity-matching signature's own error.
    let e = registry
        .bind(
            &call("SET_HELP_LABEL", vec![s("x"), Value::Int(1)]),
            &site(0),
        )
        .unwrap_err();
    assert!(matches!(
        e,
        BindingError::ArgumentType {
            index: 1,
            expected: cs_script::ir::ValueType::Str,
            ..
        }
    ));

    // A nested structure that is not the measured one is refused inside the
    // list, not flattened into acceptance.
    let e = registry
        .bind(
            &call("COMPLETED_STOPPOINT", vec![ls(vec![s("x"), Value::Int(1)])]),
            &site(0),
        )
        .unwrap_err();
    assert!(matches!(e, BindingError::ArgumentRange { index: 0, .. }));
    let e = registry
        .bind(
            &call(
                "COMPLETED_STOPPOINT",
                vec![ls(vec![s("x"), Value::Int(1), s("not-an-int")])],
            ),
            &site(0),
        )
        .unwrap_err();
    assert!(matches!(e, BindingError::ArgumentRange { index: 0, .. }));

    // A scalar where the site spells a list is a type error at the argument.
    let e = registry
        .bind(&call("COMPLETED_STOPPOINT", vec![s("x")]), &site(0))
        .unwrap_err();
    assert!(matches!(
        e,
        BindingError::ArgumentType {
            index: 0,
            found: cs_script::ir::ValueType::Str,
            ..
        }
    ));
}

/// AC (registration): the registry's checks still fire — bad name, duplicate,
/// signature that does not fit, too many arguments — plus the directive's own
/// unfit domain: a `List` deeper or wider than a `Value` can be.
#[test]
fn accept_m01_lc_lowering_signatures_registration_checks_hold() {
    let mut registry = HostBindingRegistry::new();
    registry
        .register(directive_spec(
            "END_TIMER",
            DirectiveOperation::EndMissionTimer,
            vec![vec![]],
        ))
        .unwrap();

    assert!(matches!(
        registry.register(directive_spec(
            "END_TIMER",
            DirectiveOperation::EndMissionTimer,
            vec![vec![]],
        )),
        Err(RegistryError::Duplicate { .. })
    ));
    assert!(matches!(
        registry.register(directive_spec(
            "bad name",
            DirectiveOperation::EndMissionTimer,
            vec![vec![]],
        )),
        Err(RegistryError::BadName { .. })
    ));
    // A `Directive` spec with no signatures can never bind a call.
    assert!(matches!(
        registry.register(directive_spec(
            "NO_SHAPE",
            DirectiveOperation::EndMissionTimer,
            vec![],
        )),
        Err(RegistryError::SignatureMismatch { .. })
    ));
    // A signature over the call-argument bound.
    assert!(matches!(
        registry.register(directive_spec(
            "TOO_WIDE",
            DirectiveOperation::WakeObjectives,
            vec![vec![i(); 9]],
        )),
        Err(RegistryError::TooManyArgs { .. })
    ));
    // A `List` domain deeper than a `Value` can be is no domain at all.
    let mut deep = t();
    for _ in 0..=MAX_VALUE_DEPTH {
        deep = l(vec![deep]);
    }
    assert!(matches!(
        registry.register(directive_spec(
            "TOO_DEEP",
            DirectiveOperation::AnimationStates,
            vec![vec![deep]],
        )),
        Err(RegistryError::SignatureMismatch { .. })
    ));
    // Wider than the item bound, likewise.
    assert!(matches!(
        registry.register(directive_spec(
            "TOO_MANY_ITEMS",
            DirectiveOperation::WakeObjectives,
            vec![vec![l(vec![t(); MAX_VALUE_ITEMS + 1])]],
        )),
        Err(RegistryError::SignatureMismatch { .. })
    ));
    // And a non-directive lowering still enforces its own signature.
    let mut bad = directive_spec(
        "WRONG_LOWERING",
        DirectiveOperation::WakeObjectives,
        vec![vec![t()]],
    );
    bad.lowering = Lowering::Finish(Outcome::Succeeded);
    assert!(matches!(
        registry.register(bad),
        Err(RegistryError::SignatureMismatch { .. })
    ));
}

/// The IR's own bounds apply to a directive action's arguments: a list nested
/// past the depth bound or carrying past the item bound fails validation
/// before flight, as does a directive spelling more arguments than a list may
/// hold.
#[test]
fn accept_m01_lc_lowering_signatures_ir_bounds_hold() {
    let program = |actions: Vec<Action>| MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "m01"),
        variables: vec![],
        objectives: vec![Objective {
            id: SymbolId(2),
            content: cid(ContentKind::Objective, "block"),
            condition: Condition::Const(true),
            actions,
            span: None,
        }],
    };

    // A list nested one level past the bound.
    let mut deep = s("leaf");
    for _ in 0..=MAX_VALUE_DEPTH {
        deep = ls(vec![deep]);
    }
    assert!(matches!(
        program(vec![Action::Directive {
            operation: DirectiveOperation::AnimationStates,
            args: vec![deep],
        }])
        .validate(),
        Err(ValidationError::ValueTooDeep { .. })
    ));

    // A list carrying one item over the bound.
    assert!(matches!(
        program(vec![Action::Directive {
            operation: DirectiveOperation::WakeObjectives,
            args: vec![ls(vec![Value::Int(0); MAX_VALUE_ITEMS + 1])],
        }])
        .validate(),
        Err(ValidationError::TooManyValueItems { .. })
    ));

    // And the directive's own argument list is the same bound.
    assert!(matches!(
        program(vec![Action::Directive {
            operation: DirectiveOperation::WakeObjectives,
            args: vec![Value::Int(0); MAX_VALUE_ITEMS + 1],
        }])
        .validate(),
        Err(ValidationError::TooManyValueItems { .. })
    ));
}
