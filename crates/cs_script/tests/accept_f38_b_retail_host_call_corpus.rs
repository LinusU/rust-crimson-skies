//! F38-B retail acceptance: the measured host-call corpus of the installed UI
//! script programs.
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`, stage
//! `### F38-B`; shared contract `docs/contracts/SCRIPT-MISSION.md`.
//!
//! Reads `$CS_GAME_DIR` **read-only** through production readers
//! (`cs_formats::rof::read_tree` / `read_member` over
//! `GOSDATA/ASSETS/crimson.rof`), measures the corpus with the production
//! scanner, and then asserts the measured numbers. Without `CS_GAME_DIR` this
//! test **fails loudly** rather than passing: it is marked
//! `#[ignore = "requires CS_GAME_DIR"]` so CI skips it, and the implementing and
//! reviewing agents run it with `--include-ignored`.
//!
//! Nothing from the installation is written into the repository. The test keeps
//! digests, counts, ids and spans — never a byte of a program and never a
//! statement from one.
//!
//! What this test does **not** claim: nothing here establishes what any
//! measured dispatch value means, that the corpus is the mission language, or
//! that any measured family is implemented. The coverage gate must refuse, and
//! the pinned counts are the measured denominator F38-C/D build on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cs_formats::ParseContext;
use cs_formats::rof::{RofLimits, read_member, read_tree};
use cs_formats::script_raw::ui_host_calls::{
    ArgShape, CorpusMember, DispatchForm, UiScriptLimits, measure_host_call_corpus, scan_ui_program,
};
use cs_script::bindings::observed::{
    MeasuredCall, MeasuredCallRow, MeasuredForm, MeasuredShape, ObservedBindingTable,
};

/// The container the shipped UI script programs live in.
const CRIMSON_ROF: &str = "GOSDATA/ASSETS/crimson.rof";
/// The member prefix the measured programs carry.
const SCRIPTS: &str = "ASSETS/SCRIPTS/";

fn game_dir() -> PathBuf {
    let root = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!("CS_GAME_DIR must be set: this test reads the owner's installation")
    });
    assert!(
        Path::new(&root).is_dir(),
        "CS_GAME_DIR is not a directory: {root}"
    );
    PathBuf::from(root)
}

/// The decoded UI script programs, in member-spelling order.
fn ui_scripts(context: &mut ParseContext, container: &[u8]) -> Vec<(String, Vec<u8>)> {
    let tree = read_tree(context, container).expect("the container decodes");
    let mut out = Vec::new();
    for member in tree.members() {
        let spelling = member
            .path
            .iter()
            .map(|segment| String::from_utf8_lossy(segment).into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if !spelling.starts_with(SCRIPTS) || !spelling.ends_with(".SCRIPT") {
            continue;
        }
        let read = read_member(context, container, member, &RofLimits::default())
            .unwrap_or_else(|error| panic!("{spelling} must read: {error}"));
        assert_eq!(
            read.trailing_len, 0,
            "{spelling}: no unconsumed stored bytes"
        );
        assert_eq!(
            read.decoded_len,
            member.declared_decoded_len(),
            "{spelling}: the decoded length is the declared one"
        );
        out.push((spelling, read.data));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn corpus_of(
    scripts: &[(String, Vec<u8>)],
) -> cs_formats::script_raw::ui_host_calls::HostCallCorpus {
    measure_host_call_corpus(
        scripts
            .iter()
            .map(|(spelling, bytes)| CorpusMember { spelling, bytes }),
        UiScriptLimits::default(),
    )
    .expect("every shipped UI script scans")
}

fn rows(corpus: &cs_formats::script_raw::ui_host_calls::HostCallCorpus) -> Vec<MeasuredCallRow> {
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

/// The measured corpus of the installation's UI script programs.
///
/// Fails loudly without `CS_GAME_DIR`. The pinned numbers below are the
/// measurement this task commits to; a change in the installation, in the
/// reader or in the scanner that moves any of them fails this test, and so does
/// a scanner that stops counting a form it used to count.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f38_b_retail_ui_script_host_calls_are_measured_with_provenance() {
    let root = game_dir();
    let container_path = root.join(CRIMSON_ROF);
    let container = std::fs::read(&container_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", container_path.display()));
    // A generous but bounded parse context: the container is ~60 MB with 846
    // members, so the limits are explicit inputs rather than hidden constants.
    let mut context = ParseContext::new(CRIMSON_ROF, 256 * 1024 * 1024, 8);
    let scripts = ui_scripts(&mut context, &container);
    assert_eq!(
        scripts.len(),
        61,
        "the installation ships 61 UI script programs under {}",
        SCRIPTS
    );

    let corpus = corpus_of(&scripts);

    // The corpus is measured, not assumed: every site is inside a program this
    // test just decoded, and both dispatch forms occur.
    assert!(
        corpus.members as usize == scripts.len(),
        "every program was scanned: {} of {}",
        corpus.members,
        scripts.len()
    );
    assert_eq!(
        corpus.sites,
        corpus.sites_with_native_id + corpus.sites_without_native_id,
        "every site either names a dispatch value or is counted as not naming one"
    );
    assert!(
        corpus.sites_without_native_id > 0,
        "the corpus contains dispatch expressions that are not integer literals; they \\
         must stay counted rather than folded into an id"
    );

    let callbacks = corpus
        .calls
        .iter()
        .filter(|c| c.form == DispatchForm::Callback)
        .count();
    let mails = corpus
        .calls
        .iter()
        .filter(|c| c.form == DispatchForm::Mail)
        .count();
    assert_eq!(callbacks, 384, "measured callback families");
    assert_eq!(mails, 90, "measured mail families");
    assert_eq!(
        corpus.call_count(),
        callbacks + mails,
        "a family belongs to exactly one dispatch form"
    );
    assert_eq!(
        corpus.other_call_sites(),
        3188,
        "call-shaped heads outside the two measured forms are counted, so the \
         batch's boundary is a measurement"
    );

    // The dialect's `;` marker is not established as a comment (F12-A observed
    // only `ScriptBlocks` for `UiScript`, whose grammar is `Unknown`), so the
    // scanner assumes no comment rule and **measures** what follows a `;`
    // instead. The corpus does contain `;` bytes, and this pins both halves of
    // that answer: the markers are there, and nothing the corpus spells after
    // one is a call head, a measured site or a brace. Every count above is
    // therefore the same whether or not `;` comments.
    assert_eq!(
        corpus.semicolon_bytes, 264,
        "`;` bytes outside a string literal, across the corpus"
    );
    let semicolon_programs = scripts
        .iter()
        .filter(|(spelling, bytes)| {
            scan_ui_program(spelling, bytes, UiScriptLimits::default())
                .expect("every shipped UI script scans")
                .semicolon_bytes
                > 0
        })
        .count();
    assert_eq!(
        semicolon_programs, 34,
        "the `;` markers are spread over 34 of the 61 programs"
    );
    assert_eq!(corpus.heads_after_semicolon, 0);
    assert_eq!(corpus.sites_after_semicolon, 0);
    assert_eq!(corpus.braces_after_semicolon, 0);
    assert!(
        corpus.semicolon_exposure_free(),
        "no measured site depends on what `;` means, so the corpus answers the \
         question on its own bytes"
    );

    // Every measured family keeps its provenance: a structural decode at
    // `observed_tool`, a container-span locator at the first site, and a note
    // that states no meaning is claimed. None of them is `verified_original`.
    for family in &corpus.calls {
        assert_eq!(family.evidence.method().label(), "structural_decode");
        assert_eq!(family.evidence.confidence().label(), "observed_tool");
        assert!(
            !family.evidence.note().is_empty(),
            "{}: evidence states what was observed",
            family.evidence.note()
        );
        assert!(
            family.evidence.locator().kind().label() == "container_span",
            "{}: the locator is a container span",
            family.evidence.note()
        );
        assert!(
            family.native_id >= 0,
            "{}: a measured dispatch value is not negative",
            family.evidence.note()
        );
        assert!(!family.arities.is_empty(), "every family has an arity");
    }

    // The per-program scan agrees with the corpus measurement: one program's
    // sites are a subset of the corpus's, with the same shapes.
    let first = &scripts[0];
    let scan = scan_ui_program(&first.0, &first.1, UiScriptLimits::default())
        .unwrap_or_else(|error| panic!("{} must scan: {error}", first.0));
    assert!(
        !scan.sites.is_empty(),
        "{}: the first measured program declares host calls",
        first.0
    );
    for site in &scan.sites {
        assert!(
            site.span.end() <= site.span.offset + first.1.len() as u64,
            "{}: every site span lies inside the program",
            first.0
        );
    }

    // The measured shape vocabulary is what the engine's wire codes carry, for
    // every shape the corpus actually produced.
    let produced: BTreeMap<&str, ArgShape> = [
        ("integer_literal", ArgShape::IntegerLiteral),
        ("float_literal", ArgShape::FloatLiteral),
        ("string_literal", ArgShape::StringLiteral),
        ("name_ref", ArgShape::NameRef),
        ("widget_class_ref", ArgShape::WidgetClassRef),
        ("member_ref", ArgShape::MemberRef),
        ("indexed_ref", ArgShape::IndexedRef),
        ("unevaluated", ArgShape::Unevaluated),
    ]
    .into_iter()
    .collect();
    for (label, shape) in produced {
        assert_eq!(shape.label(), label);
        assert_eq!(
            MeasuredShape::from_code(shape.code()),
            Some(match shape {
                ArgShape::IntegerLiteral => MeasuredShape::IntegerLiteral,
                ArgShape::FloatLiteral => MeasuredShape::FloatLiteral,
                ArgShape::StringLiteral => MeasuredShape::StringLiteral,
                ArgShape::NameRef => MeasuredShape::NameRef,
                ArgShape::WidgetClassRef => MeasuredShape::WidgetClassRef,
                ArgShape::MemberRef => MeasuredShape::MemberRef,
                ArgShape::IndexedRef => MeasuredShape::IndexedRef,
                ArgShape::Unevaluated => MeasuredShape::Unevaluated,
            }),
            "{}: both crates agree on the code",
            label
        );
    }

    // Every measured row the engine receives is either a valid measured call or
    // a disagreement the engine refuses by name. A row is never silently
    // dropped: the counts below must add up to the whole corpus.
    let rows = rows(&corpus);
    assert_eq!(rows.len(), corpus.calls.len());
    let mut valid = 0usize;
    let mut refused = 0usize;
    for row in &rows {
        match MeasuredCall::from_row(row) {
            Ok(_) => valid += 1,
            Err(_) => refused += 1,
        }
    }
    assert_eq!(
        valid + refused,
        corpus.calls.len(),
        "every measured family is either valid or refused by name"
    );

    // The coverage gate refuses: the corpus's families carry presentation,
    // dialogue and control semantics that no measured engine operation states,
    // and a stub is not a substitute.
    let measurable = rows
        .iter()
        .filter(|row| MeasuredCall::from_row(row).is_ok())
        .cloned()
        .collect::<Vec<_>>();
    let table = ObservedBindingTable::measure(&measurable).expect("the measurable rows");
    let coverage = table.coverage();
    assert_eq!(coverage.families, valid);
    assert_eq!(
        coverage.bound_families, 0,
        "no original family has a measured meaning; a stub would be fabrication"
    );
    assert_eq!(coverage.unimplemented_families, valid);
    assert!(
        !coverage.complete() && !coverage.campaign_ready(),
        "the gate refuses while a measured family is unimplemented"
    );
    assert!(
        table.registry().is_empty(),
        "the registry ships no measured binding"
    );
    assert!(
        table.unimplemented().count() == valid,
        "every family is counted as refused, none is dropped"
    );

    println!(
        "measured {} programs, {} sites ({} with a dispatch value, {} without), \
         {} callback families, {} mail families, {} other call-shaped sites",
        corpus.members,
        corpus.sites,
        corpus.sites_with_native_id,
        corpus.sites_without_native_id,
        callbacks,
        mails,
        corpus.other_call_sites(),
    );
}
