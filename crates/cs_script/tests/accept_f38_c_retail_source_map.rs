//! F38-C retail acceptance: every measured site of the installed UI script
//! programs carries a source location and a coverage verdict.
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`, stage
//! `### F38-C`; shared contract `docs/contracts/SCRIPT-MISSION.md`.
//!
//! Reads `$CS_GAME_DIR` **read-only** through production readers
//! (`cs_formats::rof::read_tree` / `read_member` over
//! `GOSDATA/ASSETS/crimson.rof`), scans and maps every program with the
//! production scanner and source map, crosses the rows into `cs_script` and
//! audits every site. Without `CS_GAME_DIR` it **fails loudly**; it is
//! `#[ignore = "requires CS_GAME_DIR"]` so CI skips it.
//!
//! Nothing from the installation is written to the repository: only counts,
//! offsets, lines and columns are asserted.
//!
//! What this does **not** claim: no dispatch value's meaning is established, so
//! no site is bound and the audit must refuse `campaign_ready`.

use std::path::{Path, PathBuf};

use cs_formats::ParseContext;
use cs_formats::rof::{RofLimits, read_member, read_tree};
use cs_formats::script_raw::source_map::map_sites;
use cs_formats::script_raw::ui_host_calls::{
    CorpusMember, DispatchForm, UiScriptLimits, measure_host_call_corpus, scan_ui_program,
};
use cs_script::bindings::located::{SiteOrigin, SiteRow, audit_sites};
use cs_script::bindings::observed::{
    MeasuredCall, MeasuredCallRow, MeasuredForm, MeasuredShape, ObservedBindingTable,
};

const CRIMSON_ROF: &str = "GOSDATA/ASSETS/crimson.rof";
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

fn family_rows(
    corpus: &cs_formats::script_raw::ui_host_calls::HostCallCorpus,
) -> Vec<MeasuredCallRow> {
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

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f38_c_retail_every_site_is_located_and_audited() {
    let root = game_dir();
    let container_path = root.join(CRIMSON_ROF);
    let container = std::fs::read(&container_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", container_path.display()));
    let mut context = ParseContext::new(CRIMSON_ROF, 256 * 1024 * 1024, 8);
    let scripts = ui_scripts(&mut context, &container);
    assert_eq!(scripts.len(), 61);

    let corpus = corpus_of(&scripts);
    let measurable: Vec<MeasuredCallRow> = family_rows(&corpus)
        .into_iter()
        .filter(|row| MeasuredCall::from_row(row).is_ok())
        .collect();
    let table = ObservedBindingTable::measure(&measurable).expect("the measurable rows");

    // Every site of every program, located by the production source map.
    let mut rows = Vec::new();
    for (spelling, bytes) in &scripts {
        let scan = scan_ui_program(spelling, bytes, UiScriptLimits::default()).expect("scans");
        let mapped = map_sites(&scan, bytes)
            .unwrap_or_else(|error| panic!("{spelling}: the source map must hold: {error}"));
        assert_eq!(
            mapped.len(),
            scan.sites.len(),
            "{spelling}: no site dropped"
        );
        let mut last = 0u64;
        for site in mapped {
            let o = &site.origin;
            assert_eq!(&o.member, spelling);
            assert!(o.line >= 1 && o.column >= 1, "{spelling}: 1-based position");
            assert!(o.offset + o.len <= bytes.len() as u64, "{spelling}: inside");
            assert!(o.offset >= last, "{spelling}: sites are in source order");
            last = o.offset;
            // The statement at the origin is the call the scan measured: it
            // starts with the form's head.
            let head: &[u8] = match site.site.form {
                DispatchForm::Callback => b"callback",
                DispatchForm::Mail => b"mail",
            };
            assert!(
                bytes[o.offset as usize..].starts_with(head),
                "{spelling}:{}:{}: the origin is the site's first byte",
                o.line,
                o.column
            );
            rows.push(SiteRow {
                form: match site.site.form {
                    DispatchForm::Callback => MeasuredForm::Callback,
                    DispatchForm::Mail => MeasuredForm::Mail,
                },
                native_id: site.site.native_id,
                arg_shape_codes: site.site.args.iter().map(|s| s.code()).collect(),
                origin: SiteOrigin {
                    member: o.member.clone(),
                    offset: o.offset,
                    len: o.len,
                    line: o.line,
                    column: o.column,
                },
            });
        }
    }
    assert_eq!(
        rows.len() as u32,
        corpus.sites,
        "every measured site is audited"
    );

    let audit = audit_sites(&table, &rows).expect("the audit is within its bounds");
    assert_eq!(audit.sites.len(), rows.len());
    assert_eq!(audit.bound(), 0, "no family has a measured meaning");
    assert_eq!(
        audit.bound() + audit.refused() + audit.malformed(),
        audit.sites.len(),
        "every site has exactly one verdict"
    );
    assert_eq!(
        audit.with_code("no_native_id").count() as u32,
        corpus.sites_without_native_id,
        "a dispatch expression that names no id is counted, never folded into one"
    );
    assert!(!audit.campaign_ready(), "the audit refuses campaign-ready");
    // Pinned measurement: 704 sites sit in a family the table holds and
    // refuses for want of a measured meaning; 114 spell no integer id; 994 sit
    // in a family whose own sites disagree (arity or argument shape), which the
    // engine refuses by name instead of choosing one reading.
    assert_eq!(audit.with_code("refused").count(), 704);
    assert_eq!(audit.with_code("no_native_id").count(), 114);
    assert_eq!(audit.with_code("no_family").count(), 994);
    assert_eq!(audit.with_code("shape_mismatch").count(), 0);
    assert_eq!(audit.with_code("arity_mismatch").count(), 0);
}
