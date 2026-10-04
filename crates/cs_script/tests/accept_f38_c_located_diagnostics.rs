//! F38-C acceptance: source-located diagnostics and the per-site coverage audit.
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
//! stage `### F38-C`. The minimum scenario is **AC03**: bad argument types and
//! ranges report source location without a Rust panic.
//!
//! Every program is authored synthetic text in the measured dialect; no byte of
//! the installation is committed. The producer is
//! `cs_formats::script_raw::{ui_host_calls, source_map}`; the consumer is
//! `cs_script::bindings::{located, observed}`. The test performs the crossing
//! field for field, as `cs_script::bindings::measured` documents, because
//! `cs_script` may not name `cs_formats`' types.

use cs_formats::script_raw::source_map::{SourceMapError, line_column, map_sites};
use cs_formats::script_raw::ui_host_calls::{
    CorpusMember, DispatchForm, UiScriptLimits, measure_host_call_corpus, scan_ui_program,
};
use cs_script::bindings::located::{
    LocatedError, SiteOrigin, SiteRow, SiteVerdict, SourceMap, audit_sites, lower_program_located,
};
use cs_script::bindings::observed::{
    MeasuredCallRow, MeasuredForm, MeasuredShape, ObservedBindingTable, register_measured,
};
use cs_script::bindings::{
    BindingError, HostBindingRegistry, HostFamily, Lowering, RawCall, RawObjective, RawProgram,
    Repeatability,
};
use cs_script::ir::{Condition, SymbolId, Value};
use cs_types::content::{ContentId, ContentKind};

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

/// The measured program: `callback#100` spells two integer literals twice.
const MEASURED: &[u8] =
    b"main\r\n{\r\ncallback($$A$$, 100, 1, 2)\r\ncallback($$B$$, 100, 3, 4)\r\n}\r\n";

/// A program audited against the table built from [`MEASURED`]: a site that
/// fits, one with the wrong shape, one with the wrong arity, an id nobody
/// measured, a dispatch expression that is not an id and a `mail` form.
const AUDITED: &[u8] = b"main\n{\ncallback($$A$$, 100, 1, 2)\n  callback($$A$$, 100, \"s\", 2)\ncallback($$A$$, 100, 1)\ncallback($$A$$, 999, 1)\ncallback($$A$$, (X-1), 1)\nmail(100, this)\n}\n";

fn rows_of(corpus: &cs_formats::script_raw::ui_host_calls::HostCallCorpus) -> Vec<MeasuredCallRow> {
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

fn table_of(bytes: &'static [u8]) -> ObservedBindingTable {
    let corpus = measure_host_call_corpus(
        [CorpusMember {
            spelling: "measured.script",
            bytes,
        }],
        UiScriptLimits::default(),
    )
    .expect("scans");
    ObservedBindingTable::measure(&rows_of(&corpus)).expect("measured rows are valid")
}

/// The site rows a consumer reads from the producer's source map.
fn site_rows(spelling: &str, bytes: &[u8]) -> Vec<SiteRow> {
    let scan = scan_ui_program(spelling, bytes, UiScriptLimits::default()).expect("scans");
    map_sites(&scan, bytes)
        .expect("maps")
        .into_iter()
        .map(|mapped| SiteRow {
            form: match mapped.site.form {
                DispatchForm::Callback => MeasuredForm::Callback,
                DispatchForm::Mail => MeasuredForm::Mail,
            },
            native_id: mapped.site.native_id,
            arg_shape_codes: mapped.site.args.iter().map(|s| s.code()).collect(),
            origin: SiteOrigin {
                member: mapped.origin.member,
                offset: mapped.origin.offset,
                len: mapped.origin.len,
                line: mapped.origin.line,
                column: mapped.origin.column,
            },
        })
        .collect()
}

// --- the producer's source map ----------------------------------------------

#[test]
fn accept_f38_c_source_map_names_member_offset_line_and_column() {
    let scan = scan_ui_program("A.SCRIPT", AUDITED, UiScriptLimits::default()).unwrap();
    let mapped = map_sites(&scan, AUDITED).unwrap();
    assert_eq!(mapped.len(), scan.sites.len());
    // Line 3 is the first call (`\n` ends a line), column 1.
    let first = &mapped[0].origin;
    assert_eq!(
        (first.member.as_str(), first.line, first.column),
        ("A.SCRIPT", 3, 1)
    );
    assert_eq!(
        &AUDITED[first.offset as usize..(first.offset + first.len) as usize],
        b"callback($$A$$, 100, 1, 2)"
    );
    // The second call is indented two bytes: the column counts them.
    let second = &mapped[1].origin;
    assert_eq!((second.line, second.column), (4, 3));
    assert!(second.to_string().starts_with("A.SCRIPT:4:3 (0x"));

    // The pure helper never indexes out of range.
    assert_eq!(line_column(b"a\nbc", 0), Some((1, 1)));
    assert_eq!(line_column(b"a\nbc", 3), Some((2, 2)));
    assert_eq!(line_column(b"a\nbc", 4), Some((2, 3)));
    assert_eq!(line_column(b"a\nbc", 5), None);
    assert_eq!(line_column(b"", u64::MAX), None);
}

#[test]
fn accept_f38_c_a_scan_that_disagrees_with_its_bytes_is_refused_not_indexed() {
    let scan = scan_ui_program("A.SCRIPT", AUDITED, UiScriptLimits::default()).unwrap();
    // The same scan against a truncated copy of its bytes: the spans no longer
    // lie inside the program, so no location is claimed and nothing panics.
    let error = map_sites(&scan, &AUDITED[..20]).expect_err("spans outside the program");
    assert!(matches!(error, SourceMapError::SpanOutsideProgram { .. }));
    assert_eq!(error.code(), "span_outside_program");
    assert!(map_sites(&scan, b"").is_err());
}

// --- AC03: bad argument types and ranges, located ---------------------------

fn registry_with_callback_100() -> HostBindingRegistry {
    let table = table_of(MEASURED);
    let family = table
        .family(MeasuredForm::Callback, 100)
        .expect("measured family");
    let mut registry = HostBindingRegistry::new();
    register_measured(
        &mut registry,
        &family.call,
        HostFamily::MissionState,
        Lowering::SetVariable,
        Repeatability::Once,
    )
    .expect("two integer-literal arguments register as SetVariable");
    registry
}

fn raw_program(calls: Vec<(Vec<Value>, u32)>) -> RawProgram {
    RawProgram {
        mission: cid(ContentKind::Mission, "synthetic-f38c"),
        variables: vec![],
        objectives: vec![RawObjective {
            id: SymbolId(2),
            content: cid(ContentKind::Objective, "synthetic-obj"),
            condition: Condition::Const(true),
            calls: calls
                .into_iter()
                .map(|(args, start)| RawCall {
                    name: "callback#100".to_owned(),
                    args,
                    span: Some(cs_script::ir::SourceSpan {
                        start,
                        end: start + 26,
                    }),
                })
                .collect(),
            span: None,
        }],
    }
}

#[test]
fn accept_f38_c_bad_argument_types_and_ranges_report_source_location() {
    let registry = registry_with_callback_100();
    // The origins come from the producer's own map of the audited program.
    let origins: Vec<SiteOrigin> = site_rows("A.SCRIPT", AUDITED)
        .into_iter()
        .take(3)
        .map(|row| row.origin)
        .collect();
    let mut map = SourceMap::new();
    for (call, origin) in origins.iter().cloned().enumerate() {
        map.insert(SymbolId(2), call, origin).unwrap();
    }

    let raw = raw_program(vec![
        (vec![Value::Int(1), Value::Int(7)], 0), // fine
        (vec![Value::Str("nope".to_owned()), Value::Int(7)], 40), // wrong type
        (vec![Value::Int(-1), Value::Int(7)], 80), // symbol out of range
    ]);
    let errors = lower_program_located(&registry, raw, &map).expect_err("two bad calls");
    assert_eq!(errors.len(), 2, "every bad call is reported, none skipped");

    assert!(matches!(
        errors[0].error,
        BindingError::ArgumentType { index: 0, .. }
    ));
    assert_eq!(errors[0].origin.as_ref(), Some(&origins[1]));
    let text = errors[0].to_string();
    assert!(text.starts_with("A.SCRIPT:4:3 (0x"), "{text}");

    assert!(matches!(
        errors[1].error,
        BindingError::ArgumentRange { index: 0, .. }
    ));
    assert_eq!(errors[1].origin.as_ref(), Some(&origins[2]));
    assert!(
        errors[1].to_string().contains("A.SCRIPT:5:1"),
        "{}",
        errors[1]
    );

    // A good program still lowers.
    let ok = lower_program_located(
        &registry,
        raw_program(vec![(vec![Value::Int(1), Value::Int(7)], 0)]),
        &map,
    )
    .expect("a good call lowers");
    assert_eq!(ok.objectives[0].actions.len(), 1);
}

#[test]
fn accept_f38_c_a_call_without_a_mapped_origin_is_reported_without_an_invented_one() {
    let registry = registry_with_callback_100();
    let errors = lower_program_located(
        &registry,
        raw_program(vec![(vec![Value::Str("x".to_owned()), Value::Int(1)], 0)]),
        &SourceMap::new(),
    )
    .expect_err("refused");
    assert_eq!(errors[0].origin, None);
    assert!(!errors[0].to_string().contains("A.SCRIPT"));
    // Wrong arity and an unknown call are located the same way, not panics.
    let mut unknown = raw_program(vec![(vec![], 0)]);
    unknown.objectives[0].calls[0].name = "callback#5".to_owned();
    let errors = lower_program_located(&registry, unknown, &SourceMap::new()).unwrap_err();
    assert!(matches!(
        errors[0].error,
        BindingError::UnknownHostCall { .. }
    ));
    let errors =
        lower_program_located(&registry, raw_program(vec![(vec![], 0)]), &SourceMap::new())
            .unwrap_err();
    assert!(matches!(
        errors[0].error,
        BindingError::ArityMismatch { .. }
    ));
}

#[test]
fn accept_f38_c_the_source_map_is_bounded_and_refuses_a_key_mapped_twice() {
    let origin = SiteOrigin {
        member: "A.SCRIPT".to_owned(),
        offset: 0,
        len: 1,
        line: 1,
        column: 1,
    };
    let mut map = SourceMap::new();
    map.insert(SymbolId(1), 0, origin.clone()).unwrap();
    let error = map
        .insert(SymbolId(1), 0, origin.clone())
        .expect_err("a key mapped twice");
    assert_eq!(
        error,
        LocatedError::DuplicateKey {
            objective: SymbolId(1),
            call: 0
        }
    );
    assert_eq!(error.code(), "duplicate_key");
    let long = SiteOrigin {
        member: "x".repeat(10_000),
        ..origin
    };
    let error = map
        .insert(SymbolId(1), 1, long)
        .expect_err("over the member bound");
    assert!(matches!(error, LocatedError::MemberTooLong { .. }));
    assert_eq!(error.code(), "member_too_long");
    assert_eq!(map.len(), 1);
}

// --- the per-site coverage audit ---------------------------------------------

#[test]
fn accept_f38_c_the_audit_judges_every_site_and_refuses_campaign_ready() {
    let table = table_of(MEASURED);
    let rows = site_rows("A.SCRIPT", AUDITED);
    assert_eq!(rows.len(), 6);
    // The synthetic program spells no other call-shaped head, so nothing is
    // left unjudged here.
    let audit = audit_sites(&table, &rows, 0).expect("audits");
    assert_eq!(audit.unjudged_heads, 0);
    let codes: Vec<&str> = audit.sites.iter().map(|s| s.verdict.code()).collect();
    assert_eq!(
        codes,
        [
            "refused",        // fits callback#100, whose meaning is not measured
            "shape_mismatch", // a string where the family measured an integer
            "arity_mismatch", // one argument where the family measured two
            "no_family",      // 999 was never measured
            "no_native_id",   // (X-1) names no integer
            "no_family",      // `mail` 100 is not `callback` 100
        ]
    );
    assert!(matches!(
        audit.sites[1].verdict,
        SiteVerdict::ShapeMismatch {
            position: 0,
            expected: MeasuredShape::IntegerLiteral,
            found: MeasuredShape::StringLiteral
        }
    ));
    assert_eq!(
        (audit.bound(), audit.refused(), audit.malformed()),
        (0, 1, 5)
    );
    assert!(!audit.campaign_ready());
    // Each verdict is located.
    assert_eq!(audit.sites[1].origin.line, 4);
    let line = audit.sites[1].to_string();
    assert!(line.starts_with("A.SCRIPT:4:3"), "{line}");
    assert!(line.contains("site spells string_literal"), "{line}");
    assert_eq!(audit.with_code("no_family").count(), 2);
}

#[test]
fn accept_f38_c_an_empty_audit_and_an_unbound_table_are_never_ready() {
    let bytes: &'static [u8] = b"main\n{\ncallback($$A$$, 100, 1, 2)\n}\n";
    let table = table_of(MEASURED);
    // The measured table binds nothing, so a fitting site is refused, not bound.
    // (`SiteVerdict::Bound` is reachable only through a family `classify` binds;
    // none is, until a meaning is measured. The finding says so.)
    assert!(table.registry().is_empty());
    let audit = audit_sites(&table, &site_rows("B.SCRIPT", bytes), 0).unwrap();
    assert_eq!((audit.bound(), audit.refused()), (0, 1));
    assert!(!audit.campaign_ready());
    // An empty audit is not ready: it would be ready only because nothing was
    // looked at.
    assert!(!audit_sites(&table, &[], 0).unwrap().campaign_ready());
    // A call-shaped head the audit cannot judge is reported and keeps the gate
    // closed, so a corpus that spells a call nothing judged is never "covered".
    let with_unjudged = audit_sites(&table, &site_rows("B.SCRIPT", bytes), 3).unwrap();
    assert_eq!(with_unjudged.unjudged_heads, 3);
    assert_eq!(with_unjudged.sites.len(), audit.sites.len());
    assert!(!with_unjudged.campaign_ready());
}

#[test]
fn accept_f38_c_an_unknown_shape_code_and_an_oversized_audit_are_refused_not_panics() {
    let table = table_of(MEASURED);
    let mut rows = site_rows("A.SCRIPT", b"main\n{\ncallback($$A$$, 100, 1, 2)\n}\n");
    rows[0].arg_shape_codes[1] = 250;
    let audit = audit_sites(&table, &rows, 0).unwrap();
    assert_eq!(
        audit.sites[0].verdict,
        SiteVerdict::UnknownShapeCode {
            position: 1,
            code: 250
        }
    );
    assert!(!audit.campaign_ready());

    let one = rows.remove(0);
    let many = vec![one.clone(); cs_script::bindings::located::MAX_AUDIT_SITES + 1];
    let error = audit_sites(&table, &many, 0).expect_err("over the bound");
    assert!(matches!(error, LocatedError::TooMany { .. }));
    assert_eq!(error.code(), "too_many");
    let mut long = one;
    long.origin.member = "m".repeat(cs_script::bindings::located::MAX_MEMBER_BYTES + 1);
    let error = audit_sites(&table, &[long], 0).expect_err("over the member bound");
    assert!(matches!(error, LocatedError::MemberTooLong { .. }));
    assert_eq!(error.code(), "member_too_long");
}
