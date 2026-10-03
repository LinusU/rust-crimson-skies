//! F38-B acceptance: the measured host-binding families of a UI script program
//! and the normalized differential trace of a recreated scenario.
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
//! stage `### F38-B`. The minimum scenario is **AC02**: differential traces
//! compare normalized original and recreated event ordering for a measured
//! scenario.
//!
//! Every program here is authored synthetic text in the measured dialect
//! (`callback($$handler$$, <value>, <arg>…)`, `mail(<message>, <recipient>)`) —
//! no byte of the installation is committed, and the measured values are
//! fixtures written for the tests, not measured claims. The retail corpus is
//! measured in `accept_f38_b_retail_*`, which reads `$CS_GAME_DIR` read-only.
//!
//! The tests call production code end to end: `scan_ui_program` and
//! `measure_host_call_corpus` (the `cs_formats` measurement),
//! `ObservedBindingTable::measure` and `register_measured` (the engine's family
//! table), `MissionState::step` (the recreated runtime) and `compare_traces`
//! (the comparison). Nothing asserts a stub's success.

use cs_formats::script_raw::ui_host_calls::{
    ArgShape, CorpusMember, DispatchForm, HostCallCorpus, UiScriptLimits, measure_host_call_corpus,
    scan_ui_program,
};
use cs_script::bindings::differential::{
    DeclaredStep, ScenarioRef, TraceDivergence, TraceError, compare_traces, normalize_declared,
    normalize_emitted,
};
use cs_script::bindings::observed::{
    CoverageError, MeasuredCall, MeasuredCallRow, MeasuredForm, MeasuredShape,
    ObservedBindingTable, ObservedDisposition, RowError, UnimplementedReason, register_measured,
};
use cs_script::bindings::{
    ArgDomain, HostBindingRegistry, HostFamily, Lowering, Repeatability, lower_program,
};
use cs_script::ir::{
    Action, Condition, IR_VERSION, MissionProgram, Objective, Outcome, SymbolId, Value, Variable,
};
use cs_script::runtime::{EventKind, MissionFacts, MissionState, SessionGeneration};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

/// A small authored UI script in the measured dialect: two blocks, five host
/// calls, one of them an unevaluated dispatch expression and one of them a call
/// inside a string literal, which must not be measured at all.
const SCRIPT: &[u8] = b"main\r\n\
{\r\n\
gui_create\r\n\
{\r\n\
object AA = @ctl@BE\r\n\
initialize(AA)\r\n\
callback($$A$$, 100, 1)\r\n\
mail(200, this)\r\n\
}\r\n\
gui_focus\r\n\
{\r\n\
label = \"callback($$A$$, 999, 9)\"\r\n\
callback($$NB$$, 100, 1)\r\n\
callback($$A$$, (VJA-1), QW)\r\n\
initialize(AA)\r\n\
}\r\n\
}\r\n";

fn corpus() -> HostCallCorpus {
    measure_host_call_corpus(
        [CorpusMember {
            spelling: "synthetic.script",
            bytes: SCRIPT,
        }],
        UiScriptLimits::default(),
    )
    .expect("the authored program scans")
}

/// The measured rows a consumer hands the engine, field for field, as
/// `cs_script::bindings::measured` documents.
fn rows(corpus: &HostCallCorpus) -> Vec<MeasuredCallRow> {
    corpus
        .calls
        .iter()
        .map(|call| MeasuredCallRow {
            form: match call.form {
                DispatchForm::Callback => MeasuredForm::Callback,
                DispatchForm::Mail => MeasuredForm::Mail,
            },
            native_id: call.native_id,
            sites: call.sites,
            scripts: call.scripts,
            arities: call.arities.clone(),
            arg_shapes: call
                .arg_shapes
                .iter()
                .map(|counts| {
                    counts
                        .is_uniform()
                        .then(|| counts.dominant())
                        .flatten()
                        .and_then(|shape| MeasuredShape::from_code(shape.code()))
                })
                .collect(),
            evidence: call.evidence.note().to_owned(),
        })
        .collect()
}

// --- AC01, the F38-A criterion the measured table must preserve --------------

/// **AC01 (preserved).** A measured host call that no binding claims fails
/// validation before flight, with its source location, and never lowers to a
/// no-op. `ObservedBindingTable::measure` binds no family, so *every* measured
/// call is unknown to the registry it hands out.
#[test]
fn accept_f38_b_a_measured_call_no_binding_claims_fails_before_flight() {
    let measured = corpus();
    assert_eq!(
        measured.sites_with_native_id, 3,
        "three sites spell an integer dispatch value; the fourth does not"
    );
    assert_eq!(
        measured.sites_without_native_id, 1,
        "the site with a non-literal dispatch value is counted, not dropped"
    );
    let table = ObservedBindingTable::measure(&rows(&measured)).expect("the corpus measures");
    assert!(
        table.registry().is_empty(),
        "no measured family has a measured meaning, so the registry ships nothing"
    );

    let errors = lower_program(
        table.registry(),
        cs_script::bindings::RawProgram {
            mission: cid(ContentKind::Mission, "synthetic-f38b"),
            variables: vec![Variable {
                id: SymbolId(1),
                name: "roll".to_owned(),
                initial: Value::Int(0),
            }],
            objectives: vec![cs_script::bindings::RawObjective {
                id: SymbolId(2),
                content: cid(ContentKind::Objective, "synthetic-obj"),
                condition: Condition::Const(true),
                calls: vec![cs_script::bindings::RawCall {
                    name: "callback#100".to_owned(),
                    args: vec![Value::Int(1)],
                    span: Some(cs_script::ir::SourceSpan {
                        start: 120,
                        end: 143,
                    }),
                }],
                span: None,
            }],
        },
    )
    .expect_err("an unbound measured call must not produce a program");
    assert_eq!(errors.len(), 1);
    let text = errors[0].to_string();
    assert!(
        text.contains("unknown host call `callback#100`"),
        "the diagnostic names the call: {text}"
    );
    assert!(
        text.contains("0x78..0x8f"),
        "the diagnostic carries the source span: {text}"
    );
    assert_eq!(
        errors[0].site().span.unwrap().start,
        120,
        "the site is the one the adapter measured"
    );
}

/// The measured table never turns a call into a stub: with the whole registry
/// removed the table's coverage must be unchanged, and with the measurement
/// removed the table must not exist. A stub that reports success would make the
/// coverage non-zero; this pins that it is zero and says why.
#[test]
fn accept_f38_b_no_measured_family_is_bound_and_the_gate_refuses() {
    let measured = corpus();
    let table = ObservedBindingTable::measure(&rows(&measured)).expect("the corpus measures");
    let coverage = table.coverage();

    assert_eq!(
        coverage.families, 2,
        "two distinct dispatch values: callback#100 (two sites) and mail#200"
    );
    assert_eq!(
        coverage.bound_families, 0,
        "no original family has a measured meaning that an engine operation states"
    );
    assert_eq!(coverage.unimplemented_families, 2);
    assert_eq!(
        coverage.sites, 3,
        "the three sites that spell a dispatch value; the fourth names none"
    );
    assert_eq!(coverage.unimplemented_sites, 3);
    assert!(
        !coverage.complete(),
        "a corpus with an unimplemented family is never complete"
    );
    assert!(
        !coverage.campaign_ready(),
        "AC04's rule: the gate refuses while one measured family is unimplemented"
    );

    // Every family states why, and never a success.
    for family in table.unimplemented() {
        match &family.disposition {
            ObservedDisposition::Unimplemented { reason, evidence } => {
                assert!(!evidence.is_empty(), "every refusal keeps its provenance");
                match reason {
                    UnimplementedReason::ArgumentShapeHasNoDomain { position, shape } => {
                        // `mail#200`'s recipient is a named reference, which the
                        // engine has no value for; the refusal names the
                        // position and the shape rather than dropping it.
                        assert_eq!(family.call.spelling(), "mail#200");
                        assert_eq!(*position, 0);
                        assert_eq!(*shape, MeasuredShape::NameRef);
                    }
                    UnimplementedReason::MeaningNotMeasured { detail } => {
                        // `callback#100`'s arguments are measurable, but no
                        // original observation states what the call *does*.
                        assert_eq!(family.call.spelling(), "callback#100");
                        assert_eq!(family.call.sites, 2);
                        assert!(
                            family.call.arg_domains().is_ok(),
                            "this refusal is about meaning, not about the argument shapes"
                        );
                        assert!(!detail.is_empty());
                    }
                }
            }
            ObservedDisposition::Bound { .. } => {
                panic!("{} must not be bound", family.call.spelling())
            }
        }
    }

    // The gate also refuses a corpus that measured nothing: an empty family set
    // is not "nothing unimplemented", it is "nothing looked at", and answering
    // `campaign_ready` for it would fail open on the input most likely to be
    // wrong.
    let empty = ObservedBindingTable::new();
    assert_eq!(empty.coverage().families, 0);
    assert!(
        !empty.coverage().complete() && !empty.coverage().campaign_ready(),
        "an empty measurement is never campaign-ready"
    );
    let no_rows = ObservedBindingTable::measure(&[]).expect("an empty corpus measures");
    assert_eq!(no_rows.coverage(), empty.coverage());
    assert!(!no_rows.coverage().campaign_ready());
}

/// The refusal is per family and names the family, and the argument domain
/// derivation is total only over the shapes the engine has a value for.
#[test]
fn accept_f38_b_argument_domains_cover_only_shapes_with_a_value() {
    let call = MeasuredCallRow {
        form: MeasuredForm::Callback,
        native_id: 100,
        sites: 2,
        scripts: 1,
        arities: vec![1],
        arg_shapes: vec![Some(MeasuredShape::IntegerLiteral)],
        evidence: "authored for the test".to_owned(),
    };
    let call = MeasuredCall::from_row(&call).expect("a valid row");
    assert_eq!(
        call.arg_domains().expect("an integer literal has a domain"),
        vec![ArgDomain::IntRange {
            min: i32::MIN,
            max: i32::MAX
        }],
        "an integer literal of unknown meaning gets no invented narrower domain"
    );

    for shape in [
        MeasuredShape::FloatLiteral,
        MeasuredShape::NameRef,
        MeasuredShape::WidgetClassRef,
        MeasuredShape::MemberRef,
        MeasuredShape::IndexedRef,
        MeasuredShape::Unevaluated,
    ] {
        assert!(
            MeasuredCall::arg_domain(shape).is_none(),
            "{} has no engine value standing for it",
            shape.label()
        );
    }

    // A string literal is bounded, not unbounded, and the bound is the named
    // safety cap rather than an invented "measured" length.
    let string = MeasuredCall::arg_domain(MeasuredShape::StringLiteral).expect("a string");
    assert!(
        matches!(string, ArgDomain::Str { max_bytes } if max_bytes
            == cs_script::bindings::observed::MAX_MEASURED_STRING_BYTES
            && max_bytes > 0),
        "a measured string carries the named cap: {string:?}"
    );

    // The shape wire vocabulary is total in both directions: the two crates
    // publish the same codes, and each side's labels agree with the other's.
    for (index, shape) in MeasuredShape::ALL.iter().enumerate() {
        assert_eq!(shape.code() as usize, index, "{} code", shape.label());
        assert_eq!(MeasuredShape::from_code(shape.code()), Some(*shape));
        let other = ArgShape::from_code(shape.code()).unwrap_or_else(|| {
            panic!(
                "code {} is unknown to the measured vocabulary",
                shape.code()
            )
        });
        assert_eq!(
            other.code(),
            shape.code(),
            "the cs_formats and cs_script vocabularies agree on code {}",
            shape.code()
        );
        assert_eq!(
            other.label(),
            shape.label(),
            "and on what it is called: code {}",
            shape.code()
        );
    }
    assert_eq!(
        MeasuredShape::from_code(8),
        None,
        "an unknown code is refused"
    );
    assert_eq!(ArgShape::from_code(8), None);
}

/// The row refusals: an out-of-range dispatch value, too many arguments, sites
/// that disagree about arity or a position's shape, and a row with no evidence.
/// Each names the measured value, so a diagnostic points at the family.
#[test]
fn accept_f38_b_a_measured_row_is_validated_or_named() {
    let base = MeasuredCallRow {
        form: MeasuredForm::Callback,
        native_id: 100,
        sites: 1,
        scripts: 1,
        arities: vec![1],
        arg_shapes: vec![Some(MeasuredShape::IntegerLiteral)],
        evidence: "authored for the test".to_owned(),
    };

    let mut bad = base.clone();
    bad.native_id = -1;
    assert_eq!(
        MeasuredCall::from_row(&bad),
        Err(RowError::IdOutOfRange { native_id: -1 })
    );

    let mut bad = base.clone();
    bad.arg_shapes = vec![Some(MeasuredShape::IntegerLiteral); 9];
    assert_eq!(
        MeasuredCall::from_row(&bad),
        Err(RowError::TooManyArguments {
            native_id: 100,
            args: 9,
            limit: 8
        })
    );

    let mut bad = base.clone();
    bad.arities = vec![1, 2];
    assert_eq!(
        MeasuredCall::from_row(&bad),
        Err(RowError::DisagreeingArity { native_id: 100 })
    );

    let mut bad = base.clone();
    bad.arg_shapes = vec![None];
    assert_eq!(
        MeasuredCall::from_row(&bad),
        Err(RowError::DisagreeingArgumentShape {
            native_id: 100,
            position: 0
        })
    );

    let mut bad = base.clone();
    bad.evidence = "   ".to_owned();
    assert_eq!(
        MeasuredCall::from_row(&bad),
        Err(RowError::NoEvidence { native_id: 100 })
    );

    // An over-long summary is refused: the row crosses into a provenance record
    // and from there into diagnostics, so an unbounded string must not travel.
    let mut bad = base.clone();
    bad.evidence = "e".repeat(cs_script::bindings::observed::MAX_EVIDENCE_BYTES + 1);
    assert_eq!(
        MeasuredCall::from_row(&bad),
        Err(RowError::NoEvidence { native_id: 100 })
    );
    let mut ok = base.clone();
    ok.evidence = "e".repeat(cs_script::bindings::observed::MAX_EVIDENCE_BYTES);
    assert!(
        MeasuredCall::from_row(&ok).is_ok(),
        "a summary at the cap is still evidence"
    );

    // A duplicate dispatch value yields no table at all.
    let rows = vec![base.clone(), base.clone()];
    assert_eq!(
        ObservedBindingTable::measure(&rows),
        Err(CoverageError::DuplicateCall { native_id: 100 }),
        "a corpus that measures one value twice is refused whole"
    );
}

// --- the measurement itself --------------------------------------------------

/// The scanner measures the two dispatch forms and nothing else: a call spelled
/// inside a string literal is not a site, a call-shaped head outside the two
/// forms is counted rather than ignored, and an unbalanced program is refused.
#[test]
fn accept_f38_b_the_measurement_counts_forms_and_refuses_a_broken_program() {
    let scan = scan_ui_program("synthetic.script", SCRIPT, UiScriptLimits::default())
        .expect("the authored program scans");

    // callback: 100 (gui_create), 100 (gui_focus), an unevaluated dispatch
    // expression; mail: 200. The `callback(...)` inside the string literal is
    // not a site.
    assert_eq!(scan.sites.len(), 4, "four measured sites");
    assert_eq!(scan.sites_with_native_id().count(), 3);
    assert_eq!(scan.sites_without_native_id().count(), 1);

    let callback = scan
        .sites
        .iter()
        .filter(|s| s.form == DispatchForm::Callback)
        .count();
    let mail = scan
        .sites
        .iter()
        .filter(|s| s.form == DispatchForm::Mail)
        .count();
    assert_eq!((callback, mail), (3, 1));

    // The enclosing block label is measured, and the site inside `gui_focus`
    // carries it rather than the outer `main`.
    assert_eq!(
        scan.sites[0].block, "main",
        "the enclosing depth-1 block label is measured"
    );
    assert_eq!(
        scan.sites[0].span.offset, 61,
        "the first site's byte offset"
    );
    assert_eq!(
        scan.sites[0].native_id,
        Some(100),
        "the dispatch expression is the second one in a callback"
    );
    assert_eq!(
        scan.sites[0].target,
        Some(ArgShape::NameRef),
        "the callback's `$$…$$` target is measured as a shape, never as text"
    );
    assert_eq!(
        scan.sites[0].args,
        vec![ArgShape::IntegerLiteral],
        "an integer argument is measured as its shape, not its value"
    );
    assert_eq!(
        scan.sites[1].target, None,
        "a mail site has no target reference"
    );
    assert_eq!(scan.sites[1].native_id, Some(200));
    assert_eq!(
        scan.sites[2].args,
        vec![ArgShape::IntegerLiteral],
        "the target and the dispatch value are not counted as arguments"
    );
    assert_eq!(
        scan.sites[3].native_id, None,
        "a dispatch expression that is not a literal yields no id"
    );
    assert_eq!(
        scan.sites[3].args,
        vec![ArgShape::NameRef],
        "its own first expression becomes an argument, not a dispatch value"
    );

    // `initialize` and `label` are call-shaped heads outside the two forms.
    let heads: Vec<(&str, u32)> = scan
        .other_call_heads
        .iter()
        .map(|h| (h.head.as_str(), h.sites))
        .collect();
    assert_eq!(heads, vec![("initialize", 2)]);

    // Refusals.
    let unbalanced = b"main\n{\ncallback($$A$$, 1, 2)\n";
    assert!(
        scan_ui_program("synthetic/broken", unbalanced, UiScriptLimits::default()).is_err(),
        "unbalanced blocks are refused rather than guessed"
    );
    let unterminated = b"main\n{\ncallback($$A$$, 1, 2\n}\n";
    assert!(
        scan_ui_program("synthetic/broken", unterminated, UiScriptLimits::default()).is_err(),
        "an unterminated call is refused"
    );
    let tight = UiScriptLimits {
        max_sites: 2,
        ..UiScriptLimits::default()
    };
    assert!(
        scan_ui_program("synthetic.script", SCRIPT, tight).is_err(),
        "a site bound is refused, not silently truncated"
    );
}

/// Every scan bound is enforced on **every** argument, the last one included,
/// and the dialect's unmeasured `;` marker is measured rather than assumed to be
/// a comment.
#[test]
fn accept_f38_b_every_scan_bound_is_enforced_and_the_semicolon_is_measured() {
    // The expression count of one call: `mail(1, 2, 3)` spells three, so a bound
    // of two must refuse it — and the refusal has to see the *last* expression,
    // which a bound checked only at the commas would let through.
    let three = b"main\n{\nmail(1, 2, 3)\n}\n";
    let at_two = UiScriptLimits {
        max_args: 2,
        ..UiScriptLimits::default()
    };
    let refused = scan_ui_program("synthetic/three", three, at_two).expect_err("three is over two");
    assert_eq!(
        refused.code(),
        "too_many_arguments",
        "the count bound refuses: {refused}"
    );
    let at_three = UiScriptLimits {
        max_args: 3,
        ..UiScriptLimits::default()
    };
    let scan = scan_ui_program("synthetic/three", three, at_three).expect("three fits three");
    assert_eq!(
        scan.sites[0].args.len(),
        2,
        "at the bound every expression is still measured; the first is the dispatch"
    );

    // The expression length: an argument over the bound refuses the program
    // instead of being classified. The bound counts the bytes the scanner walks,
    // so the blank before `AA_BB_CC` counts too — deliberately, because a
    // narrower reading would make the bound depend on spelling.
    let long = b"main\n{\nmail(1, AA_BB_CC)\n}\n";
    let scan = scan_ui_program("synthetic/long", long, UiScriptLimits::default())
        .expect("a short argument scans");
    assert_eq!(scan.sites[0].args.len(), 1);
    let tight = UiScriptLimits {
        max_expr_bytes: 8,
        ..UiScriptLimits::default()
    };
    let refused = scan_ui_program("synthetic/long", long, tight).expect_err("AA_BB_CC is 8 bytes");
    assert_eq!(
        refused.code(),
        "expression_too_long",
        "the length bound refuses: {refused}"
    );
    let exact = UiScriptLimits {
        max_expr_bytes: 9,
        ..UiScriptLimits::default()
    };
    let scan = scan_ui_program("synthetic/long", long, exact).expect("nine bytes at the bound");
    assert_eq!(scan.sites[0].args.len(), 1);

    // The `;` marker: the scanner does not assume it comments, so it counts the
    // marker and counts everything spelled after it on the line, treating it as
    // text the way it treats everything else.
    let marked = b"main\n{\ncallback($$A$$, 7, 1) ; a note with initialize(3) and { brace\n\
                  caption = \"; inside a literal is text\"\n\
                  callback($$A$$, 8, 2)\n}\n}\n";
    let scan = scan_ui_program("synthetic/marked", marked, UiScriptLimits::default())
        .expect("the marked program scans");
    assert_eq!(
        scan.semicolon_bytes, 1,
        "the `;` inside the literal is text, not a marker"
    );
    assert_eq!(
        scan.heads_after_semicolon, 1,
        "the head in the tail is counted"
    );
    assert_eq!(
        scan.sites_after_semicolon, 0,
        "the tail spells no measured dispatch form"
    );
    assert_eq!(
        scan.braces_after_semicolon, 1,
        "the brace in the tail is counted as structure"
    );
    assert!(
        !scan.semicolon_exposure_free(),
        "a program whose tail spells a head or a brace has an open exposure"
    );
    assert_eq!(scan.sites.len(), 2, "both real sites are still measured");
    assert_eq!(
        scan.other_call_heads
            .iter()
            .find(|h| h.head == "initialize")
            .map(|h| h.sites),
        Some(1),
        "the tail's head is counted among the other heads, not dropped"
    );

    // A program whose `;` tails spell nothing callable or structural is
    // exposure-free: a comment rule would remove nothing from it, so its
    // measurement is the same under either reading.
    let clean = scan_ui_program("synthetic.script", SCRIPT, UiScriptLimits::default())
        .expect("the authored program scans");
    assert_eq!(clean.semicolon_bytes, 0);
    assert!(clean.semicolon_exposure_free());
    assert!(corpus().semicolon_exposure_free());
}

/// The corpus measurement carries provenance per family and counts the sites it
/// could not name, instead of folding them into an id.
#[test]
fn accept_f38_b_the_corpus_names_each_family_and_counts_the_rest() {
    let measured = corpus();
    assert_eq!(measured.members, 1);
    assert_eq!(measured.sites, 4);
    assert_eq!(measured.sites_with_native_id, 3);
    assert_eq!(
        measured.sites_without_native_id, 1,
        "a site with no integer dispatch value is counted, not dropped"
    );
    assert_eq!(measured.other_call_sites(), 2);

    let hundred = measured
        .call(DispatchForm::Callback, 100)
        .expect("the family was measured");
    assert_eq!(hundred.sites, 2, "two sites spell 100");
    assert_eq!(hundred.scripts, 1);
    assert_eq!(hundred.arities, vec![1]);
    assert_eq!(
        hundred.evidence.method().label(),
        "structural_decode",
        "the measurement names its method"
    );
    assert_eq!(
        hundred.evidence.confidence().label(),
        "observed_tool",
        "a measured shape is tool-observed, not verified original"
    );
    assert!(
        hundred.evidence.establishes_semantics(),
        "a structural decode at observed_tool is what the evidence schema calls \
         semantic-establishing; what it establishes is the *shape* and the span"
    );

    let mail = measured.call(DispatchForm::Mail, 200).expect("measured");
    assert_eq!(
        mail.arities,
        vec![1],
        "mail#200's recipient is one argument; the message id is the dispatch value"
    );
    assert!(
        mail.evidence.note().contains("no meaning is claimed"),
        "the evidence says so in words: {}",
        mail.evidence.note()
    );
}

/// Two sites of one value that disagree about an argument's shape make the
/// family unimplementable at that position — the disagreement is never resolved
/// by picking one shape.
#[test]
fn accept_f38_b_disagreeing_argument_shapes_are_refused_not_resolved() {
    let bytes = b"main\n{\ncallback($$A$$, 7, 1)\ncallback($$A$$, 7, AA)\n}\n";
    let measured = measure_host_call_corpus(
        [CorpusMember {
            spelling: "synthetic.script",
            bytes,
        }],
        UiScriptLimits::default(),
    )
    .expect("the authored program scans");
    let family = measured
        .call(DispatchForm::Callback, 7)
        .expect("one family");
    assert_eq!(family.sites, 2);
    let position = family.arg_shapes[0];
    assert_eq!(position.count(ArgShape::IntegerLiteral), 1);
    assert_eq!(position.count(ArgShape::NameRef), 1);
    assert!(
        !position.is_uniform(),
        "the two sites disagree, so the shape is not uniform"
    );
    assert_eq!(
        position.dominant(),
        Some(ArgShape::IntegerLiteral),
        "the reported dominant shape is the first in the fixed order on a tie"
    );

    // The consumer hands `None` for a position that is not uniform, and the
    // table refuses the row rather than binding one of the two shapes.
    let mut row = MeasuredCallRow {
        form: MeasuredForm::Callback,
        native_id: 7,
        sites: 2,
        scripts: 1,
        arities: family.arities.clone(),
        arg_shapes: vec![None],
        evidence: family.evidence.note().to_owned(),
    };
    assert_eq!(
        MeasuredCall::from_row(&row),
        Err(RowError::DisagreeingArgumentShape {
            native_id: 7,
            position: 0
        })
    );
    assert_eq!(
        ObservedBindingTable::measure(&[row.clone()]),
        Err(CoverageError::BadRow(RowError::DisagreeingArgumentShape {
            native_id: 7,
            position: 0
        }))
    );

    // With both sites agreeing the same row is valid, so the refusal is the
    // disagreement and not the shape.
    row.arg_shapes = vec![Some(MeasuredShape::IntegerLiteral)];
    assert!(MeasuredCall::from_row(&row).is_ok());
}

// --- AC02, the differential trace -------------------------------------------

/// The recreated program for one measured scenario: each declared step becomes
/// one objective whose action is that step's own `GrantReward`, so the runtime's
/// emitted `RewardGranted` events carry exactly the declared dispatch value.
fn recreated_program(declared: &[DeclaredStep]) -> MissionProgram {
    recreated_program_declared_in(declared, &(0..declared.len()).collect::<Vec<_>>())
}

/// The same program with its objectives **declared** in `declaration` order while
/// their symbols stay in original-program order.
///
/// F37-D measured that execution order is declaration order and observation
/// order is `EventKey` (source symbol), and that the two are not each other's
/// sort. Building the program this way separates them, so the differential can
/// only agree if it compares the runtime's **observation** order — a
/// normalization that used declaration order instead would produce a different
/// trace and fail.
fn recreated_program_declared_in(
    declared: &[DeclaredStep],
    declaration: &[usize],
) -> MissionProgram {
    let mut objectives = Vec::with_capacity(declaration.len());
    for &index in declaration {
        let step = &declared[index];
        let symbol = SymbolId(10_000 + index as u32);
        objectives.push(Objective {
            id: symbol,
            content: cid(
                ContentKind::Objective,
                &format!("synthetic-f38b-obj-{}-{index}", step.native_id),
            ),
            condition: Condition::Const(true),
            actions: vec![Action::GrantReward {
                reward: cid(
                    ContentKind::ScrapbookItem,
                    &format!("f38b-{}", step.native_id),
                ),
            }],
            span: None,
        });
    }
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f38b"),
        variables: vec![Variable {
            id: SymbolId(1),
            name: "roll".to_owned(),
            initial: Value::Int(0),
        }],
        objectives,
    }
}

/// Runs the recreated program and reports the events that **carry a call**.
///
/// The runtime emits two events per firing objective: the latch
/// (`ObjectiveCompleted`, sequence 0) and one per action (sequence 1 and up).
/// Only the action events are the ones a declared host call produced, so the
/// latch events are filtered here rather than in the normalization, which must
/// stay a pure reduction of what it is given.
fn run(program: &MissionProgram, ticks: u64) -> Vec<cs_script::runtime::MissionEvent> {
    let validated = program.clone().validate().expect("a valid program");
    let mut state = MissionState::new(&validated, SessionGeneration(1));
    let facts = MissionFacts::default();
    let mut events = Vec::new();
    for tick in 0..ticks {
        let result = state
            .step(&validated, &facts, Tick(tick))
            .expect("ticks advance");
        events.extend(
            result
                .events
                .into_iter()
                .filter(|event| matches!(event.kind, EventKind::RewardGranted(_))),
        );
    }
    events
}

fn declared_from_scan() -> Vec<DeclaredStep> {
    let scan = scan_ui_program("synthetic.script", SCRIPT, UiScriptLimits::default())
        .expect("the authored program scans");
    scan.sites
        .iter()
        .enumerate()
        .map(|(ordinal, site)| DeclaredStep {
            form: match site.form {
                DispatchForm::Callback => MeasuredForm::Callback,
                DispatchForm::Mail => MeasuredForm::Mail,
            },
            // The unevaluated dispatch expression has no id, so the recreated
            // program cannot declare one either; the differential then reports
            // the divergence instead of hiding it.
            native_id: site.native_id.unwrap_or(-1),
            ordinal: ordinal as u32,
            offset: site.span.offset,
        })
        .collect()
}

/// **AC02.** For a measured scenario the normalized original ordering (what the
/// original program declares, in source order) and the normalized recreated
/// ordering (what the recreated runtime emits, in `EventKey` order) are
/// compared, and the comparison is a real comparison: an engine that emitted
/// nothing, emitted something extra, or emitted a different call is reported at
/// a named index.
#[test]
fn accept_f38_b_the_differential_trace_compares_original_and_recreated_order() {
    let declared: Vec<DeclaredStep> = declared_from_scan()
        .into_iter()
        // The fourth site has no measured dispatch value; it is excluded here so
        // the two traces are comparable, and the next test covers the case where
        // it is not.
        .filter(|s| s.native_id >= 0)
        .collect();
    assert_eq!(declared.len(), 3, "three declared steps with an id");

    let scenario = ScenarioRef::new("synthetic/f38b-scenario-1");
    let original = normalize_declared(&scenario, &declared).expect("a named scenario");
    assert_eq!(original.len(), 3);

    let program = recreated_program(&declared);
    let events = run(&program, 2);
    let map: std::collections::HashMap<(u32, u32), (MeasuredForm, i64, u32)> = declared
        .iter()
        .enumerate()
        .map(|(index, step)| {
            (
                (10_000 + index as u32, 1),
                (step.form, step.native_id, step.ordinal),
            )
        })
        .collect();
    let recreated = normalize_emitted(&scenario, &events, &|event| {
        map.get(&(event.key.source.0, event.key.sequence)).copied()
    })
    .expect("a named scenario");
    assert_eq!(recreated.len(), 3);

    let comparison = compare_traces(&original, &recreated, &scenario, &scenario)
        .expect("both traces name the same scenario");
    assert!(
        comparison.agrees(),
        "the recreated ordering must match the declared one: {:?}",
        comparison.first_divergence
    );
    assert_eq!(comparison.matched, 3);
    assert_eq!((comparison.original_len, comparison.recreated_len), (3, 3));
    assert_eq!(
        original
            .kinds()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["callback#100", "mail#200", "callback#100"],
        "the normalized kinds are the measured dispatch values in source order"
    );
    assert_eq!(
        original.kinds(),
        recreated.kinds(),
        "the two normalized traces carry the same call sequence"
    );
    assert_eq!(
        comparison.divergences(&original, &recreated),
        Vec::new(),
        "an agreeing comparison reports no divergence"
    );
    // The original trace is in declaration order and the recreated one in
    // `EventKey` order, and the comparison says they are the same: that is
    // AC02's claim, produced by two independent orders.
    let offsets: Vec<u64> = original.steps().iter().map(|s| s.ordinal as u64).collect();
    assert_eq!(
        offsets,
        vec![0, 1, 2],
        "the declared ordinals are source order"
    );
    assert_eq!(
        recreated
            .steps()
            .iter()
            .map(|s| (s.source.unwrap(), s.sequence.unwrap()))
            .collect::<Vec<_>>(),
        vec![(10_000, 1), (10_001, 1), (10_002, 1)],
        "the recreated steps carry the runtime's own event keys"
    );

    // The two orders are genuinely different. Declaring the same objectives in
    // reverse order leaves the runtime's *execution* order reversed while its
    // observation order stays the symbol order — so the recreated trace still
    // agrees with the original, and a normalization that had followed
    // declaration order would not.
    let reversed = recreated_program_declared_in(&declared, &[2, 1, 0]);
    assert_eq!(
        reversed
            .objectives
            .iter()
            .map(|o| o.id.0)
            .collect::<Vec<_>>(),
        vec![10_002, 10_001, 10_000],
        "the objectives are declared in reverse"
    );
    let reversed_events = run(&reversed, 2);
    assert_ne!(
        reversed_events
            .iter()
            .map(|e| e.key.source.0)
            .collect::<Vec<_>>(),
        vec![10_002, 10_001, 10_000],
        "the runtime reports events in `EventKey` order, not in declaration order"
    );
    let reversed_trace = normalize_emitted(&scenario, &reversed_events, &|event| {
        map.get(&(event.key.source.0, event.key.sequence)).copied()
    })
    .expect("a named scenario");
    let reversed_comparison =
        compare_traces(&original, &reversed_trace, &scenario, &scenario).expect("same scenario");
    assert!(
        reversed_comparison.agrees(),
        "declaration order must not change the comparison: {:?}",
        reversed_comparison.first_divergence
    );
    assert_eq!(reversed_comparison.matched, 3);
}

/// The comparison is not decoration: each way the recreated side can differ from
/// the declared side is reported at its own index, and two traces of different
/// measured scenarios are refused rather than compared.
#[test]
fn accept_f38_b_every_divergence_is_reported_at_its_index() {
    let declared: Vec<DeclaredStep> = declared_from_scan()
        .into_iter()
        .filter(|s| s.native_id >= 0)
        .collect();
    let scenario = ScenarioRef::new("synthetic/f38b-scenario-1");
    let original = normalize_declared(&scenario, &declared).expect("named");

    // Recreated: reordered.
    let mut swapped = declared.clone();
    swapped.swap(0, 1);
    let reordered = normalize_declared(&scenario, &swapped).expect("named");
    let comparison = compare_traces(&original, &reordered, &scenario, &scenario).expect("same");
    // The two steps share one dispatch value, so swapping them is invisible to
    // the normalized comparison — which is correct: normalization is by call
    // identity, not by position.
    assert_eq!(comparison.original_len, comparison.recreated_len);

    // Recreated: a different call.
    let mut changed = declared.clone();
    changed[2].native_id = 201;
    assert_eq!(changed[2].form, MeasuredForm::Callback);
    let changed = normalize_declared(&scenario, &changed).expect("named");
    let comparison = compare_traces(&original, &changed, &scenario, &scenario).expect("same");
    assert!(!comparison.agrees());
    assert_eq!(
        comparison.first_divergence,
        Some(TraceDivergence::Mismatched {
            index: 2,
            declared: cs_script::bindings::differential::TraceStepKind::Measured {
                form: MeasuredForm::Callback,
                native_id: 100,
            },
            emitted: cs_script::bindings::differential::TraceStepKind::Measured {
                form: MeasuredForm::Callback,
                native_id: 201,
            },
        }),
        "the divergence names the index and both calls"
    );

    // Recreated: shorter.
    let short = normalize_declared(&scenario, &declared[..2]).expect("named");
    let comparison = compare_traces(&original, &short, &scenario, &scenario).expect("same");
    assert_eq!(
        comparison.first_divergence,
        Some(TraceDivergence::Missing {
            index: 2,
            declared: cs_script::bindings::differential::TraceStepKind::Measured {
                form: MeasuredForm::Callback,
                native_id: 100,
            },
        })
    );

    // Recreated: longer.
    let mut longer = declared.clone();
    longer.push(DeclaredStep {
        form: MeasuredForm::Callback,
        native_id: 300,
        ordinal: 3,
        offset: 400,
    });
    let longer = normalize_declared(&scenario, &longer).expect("named");
    let comparison = compare_traces(&original, &longer, &scenario, &scenario).expect("same");
    assert_eq!(
        comparison.first_divergence,
        Some(TraceDivergence::Unexpected {
            index: 3,
            emitted: cs_script::bindings::differential::TraceStepKind::Measured {
                form: MeasuredForm::Callback,
                native_id: 300,
            },
        })
    );

    // The full report lists every divergence, not only the first.
    let mut two = declared.clone();
    two[1].native_id = 999;
    two[2].native_id = 998;
    let two = normalize_declared(&scenario, &two).expect("named");
    let comparison = compare_traces(&original, &two, &scenario, &scenario).expect("same");
    assert_eq!(
        comparison.divergences(&original, &two).len(),
        2,
        "both differences are reported"
    );

    // Different scenarios are refused, and an unnamed one is refused too.
    let other = ScenarioRef::new("synthetic/f38b-scenario-2");
    assert_eq!(
        compare_traces(&original, &two, &scenario, &other),
        Err(TraceError::ScenarioMismatch {
            original: "synthetic/f38b-scenario-1".to_owned(),
            recreated: "synthetic/f38b-scenario-2".to_owned(),
        })
    );
    assert_eq!(
        ScenarioRef::from_digest("original", "  "),
        Err(TraceError::NoScenario { side: "original" })
    );
    let unnamed = ScenarioRef::new("");
    assert_eq!(
        normalize_declared(&unnamed, &declared),
        Err(TraceError::NoScenario { side: "original" })
    );
    assert_eq!(
        normalize_emitted(&unnamed, &[], &|_| None),
        Err(TraceError::NoScenario { side: "recreated" })
    );
}

/// A site whose dispatch value was never measured cannot be given one by the
/// recreated side: the engine emits an event, the normalization refuses to name
/// a call for it, and the comparison reports it as an extra step instead of
/// dropping it.
#[test]
fn accept_f38_b_an_unmeasured_dispatch_value_surfaces_as_an_extra_step() {
    let declared = vec![DeclaredStep {
        form: MeasuredForm::Callback,
        native_id: -1,
        ordinal: 0,
        offset: 0,
    }];
    let scenario = ScenarioRef::new("synthetic/f38b-scenario-3");
    let original = normalize_declared(&scenario, &declared).expect("named");
    let program = recreated_program(&declared);
    let events = run(&program, 1);
    assert_eq!(events.len(), 1, "the engine emitted its event");
    // No measured origin for the event: the dispatch value was never measured,
    // so nothing maps the engine's event back to a declared call.
    let recreated = normalize_emitted(&scenario, &events, &|_| None).expect("named");
    assert_eq!(recreated.len(), 1);
    assert_eq!(
        recreated.kinds(),
        vec![cs_script::bindings::differential::TraceStepKind::Unattributed],
        "an event with no measured origin gets the distinct unattributed identity"
    );
    let comparison = compare_traces(&original, &recreated, &scenario, &scenario).expect("same");
    assert!(
        !comparison.agrees(),
        "an event with no measured origin cannot agree with a declared step"
    );
    match comparison.first_divergence {
        Some(TraceDivergence::Mismatched {
            index,
            declared,
            emitted,
        }) => {
            assert_eq!(index, 0);
            assert_eq!(
                declared,
                cs_script::bindings::differential::TraceStepKind::Measured {
                    form: MeasuredForm::Callback,
                    native_id: -1,
                },
                "the declared step keeps its own identity, even an unmeasured one"
            );
            assert_eq!(
                emitted,
                cs_script::bindings::differential::TraceStepKind::Unattributed,
                "the emitted event is unattributed, and the two kinds are distinct"
            );
            assert_ne!(
                declared, emitted,
                "a declared step and an unattributed event can never compare equal"
            );
        }
        other => panic!("a divergence must be reported: {other:?}"),
    }
}

// --- the registry path a bound family takes ----------------------------------

/// A measured family a caller has an engine operation for registers under its
/// derived spelling with `Observed` provenance and binds a raw call through the
/// production registry, so the bound path is production code rather than a test
/// shortcut.
#[test]
fn accept_f38_b_a_measured_family_with_an_operation_registers_and_binds() {
    let row = MeasuredCallRow {
        form: MeasuredForm::Callback,
        native_id: 100,
        sites: 2,
        scripts: 1,
        arities: vec![2],
        arg_shapes: vec![
            Some(MeasuredShape::IntegerLiteral),
            Some(MeasuredShape::IntegerLiteral),
        ],
        evidence: "measured in the authored fixture".to_owned(),
    };
    let call = MeasuredCall::from_row(&row).expect("valid");
    assert_eq!(call.spelling(), "callback#100");

    let mut registry = HostBindingRegistry::new();
    let spec = register_measured(
        &mut registry,
        &call,
        HostFamily::MissionState,
        Lowering::SetVariable,
        Repeatability::Once,
    )
    .expect("the registry accepts the derived spelling");
    assert_eq!(spec.name, "callback#100");
    assert_eq!(registry.len(), 1);
    match &spec.provenance {
        cs_script::bindings::BindingProvenance::Observed { evidence } => {
            assert_eq!(evidence, "measured in the authored fixture");
        }
        other => panic!("a measured binding keeps its provenance: {other:?}"),
    }

    // The registered binding lowers a raw call through the production path.
    let program = lower_program(
        &registry,
        cs_script::bindings::RawProgram {
            mission: cid(ContentKind::Mission, "synthetic-f38b"),
            variables: vec![],
            objectives: vec![cs_script::bindings::RawObjective {
                id: SymbolId(2),
                content: cid(ContentKind::Objective, "synthetic-obj"),
                condition: Condition::Const(true),
                calls: vec![cs_script::bindings::RawCall {
                    name: "callback#100".to_owned(),
                    args: vec![Value::Int(1), Value::Int(7)],
                    span: None,
                }],
                span: None,
            }],
        },
    )
    .expect("the measured call binds");
    assert_eq!(
        program.objectives[0].actions,
        vec![Action::SetVariable {
            variable: SymbolId(1),
            value: Value::Int(7),
        }],
        "the measured call lowers to the registered operation"
    );

    // An argument outside the declared domain is still refused, with its site.
    let errors = lower_program(
        &registry,
        cs_script::bindings::RawProgram {
            mission: cid(ContentKind::Mission, "synthetic-f38b"),
            variables: vec![],
            objectives: vec![cs_script::bindings::RawObjective {
                id: SymbolId(2),
                content: cid(ContentKind::Objective, "synthetic-obj"),
                condition: Condition::Const(true),
                calls: vec![cs_script::bindings::RawCall {
                    name: "callback#100".to_owned(),
                    args: vec![Value::Str("nope".to_owned())],
                    span: Some(cs_script::ir::SourceSpan { start: 8, end: 16 }),
                }],
                span: None,
            }],
        },
    )
    .expect_err("a wrong argument type is refused");
    assert!(
        errors[0].to_string().contains("0x8..0x10"),
        "the refusal keeps the source span: {}",
        errors[0]
    );

    // A family with no domain for one of its measured shapes cannot be
    // registered at all: the registry is not the place to drop an argument.
    let member_row = MeasuredCallRow {
        form: MeasuredForm::Callback,
        native_id: 101,
        sites: 1,
        scripts: 1,
        arities: vec![1],
        arg_shapes: vec![Some(MeasuredShape::MemberRef)],
        evidence: "measured in the authored fixture".to_owned(),
    };
    let member = MeasuredCall::from_row(&member_row).expect("valid");
    assert!(
        register_measured(
            &mut registry,
            &member,
            HostFamily::Presentation,
            Lowering::Finish(Outcome::Failed),
            Repeatability::Once,
        )
        .is_err(),
        "an argument the engine has no value for cannot be declared"
    );
}
