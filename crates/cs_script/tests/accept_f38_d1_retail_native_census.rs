//! F38-D.1 retail acceptance: the re-keyed native census of the installed UI
//! script programs.
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`, stage
//! `### F38-D`; shared contract `docs/contracts/SCRIPT-MISSION.md`; owner
//! evidence: Rally #159 owner note (2026-10-05), decrypted executable sha256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`. Claim
//! class: static code evidence, never `verified_original`.
//!
//! Reads `$CS_GAME_DIR` **read-only** through production readers
//! (`cs_formats::rof::read_tree` / `read_member` over
//! `GOSDATA/ASSETS/crimson.rof`), measures the corpus with the production
//! scanner, crosses it into `cs_script` field for field and audits every site.
//! Without `CS_GAME_DIR` this test **fails loudly**; it is
//! `#[ignore = "requires CS_GAME_DIR"]` so CI skips it, and the implementing
//! and reviewing agents run it with `--include-ignored`.
//!
//! The pinned numbers are the owner census of #159 (709 callback sites over
//! 385 distinct (native, id) pairs) as re-measured by production code, plus
//! the F38-D.1 verdict census. Nothing from the installation is committed:
//! only counts, ids, spellings and verdict codes are asserted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cs_formats::ParseContext;
use cs_formats::rof::{RofLimits, read_member, read_tree};
use cs_formats::script_raw::ui_host_calls::{
    CorpusMember, HostKind, UiNative, UiScriptLimits, measure_host_call_corpus, scan_ui_program,
};
use cs_script::bindings::located::{SiteOrigin, SiteRow, audit_sites};
use cs_script::bindings::observed::{
    MeasuredCallRow, MeasuredHost, MeasuredShape, ObservedBindingTable,
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
        out.push((spelling, read.data));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn host_of(call: &cs_formats::script_raw::ui_host_calls::ObservedHostCall) -> MeasuredHost {
    MeasuredHost::from_code(call.host.code())
        .unwrap_or_else(|| panic!("code {} is a measured host", call.host.code()))
}

/// The measured census of the installation, re-measured here from the
/// production scanner: (sites, distinct integer ids, computed-dispatch sites)
/// per native.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f38_d1_retail_the_native_census_matches_the_owner_measurement() {
    let root = game_dir();
    let container = std::fs::read(root.join(CRIMSON_ROF)).expect("read crimson.rof");
    let mut context = ParseContext::new(CRIMSON_ROF, 256 * 1024 * 1024, 8);
    let scripts = ui_scripts(&mut context, &container);
    assert_eq!(scripts.len(), 61, "the installation ships 61 UI scripts");
    let corpus = measure_host_call_corpus(
        scripts
            .iter()
            .map(|(spelling, bytes)| CorpusMember { spelling, bytes }),
        UiScriptLimits::default(),
    )
    .expect("every shipped UI script scans");
    assert_eq!(corpus.sites, 1812);

    // The owner census (#159), per native: sites, distinct integer ids, and
    // the sites whose dispatch expression is not an integer literal.
    let census: BTreeMap<UiNative, (u32, usize, u32)> = [
        (UiNative::GosCallback, (111, 57, 1)),
        (UiNative::UiData, (369, 169, 12)),
        (UiNative::GosLocalize, (174, 143, 10)),
        (UiNative::UiControl, (44, 16, 0)),
        (UiNative::GosPageInit, (11, 0, 11)),
    ]
    .into_iter()
    .collect();
    for (native, expected) in &census {
        let families: Vec<_> = corpus.hosts(HostKind::Native(*native)).collect();
        let sites: u32 = families.iter().map(|c| c.sites).sum();
        let ids = families.iter().filter(|c| c.native_id.is_some()).count();
        let computed: u32 = families
            .iter()
            .filter(|c| c.native_id.is_none())
            .map(|c| c.sites)
            .sum();
        assert_eq!(
            (sites, ids, computed),
            *expected,
            "{}: (sites, distinct integer ids, computed-dispatch sites)",
            native.label()
        );
    }

    let callback_sites: u32 = corpus
        .calls
        .iter()
        .filter(|c| c.host.is_native())
        .map(|c| c.sites)
        .sum();
    assert_eq!(
        callback_sites, 709,
        "callback sites attributable to the five measured natives, per the owner census"
    );
    let native_id_pairs = corpus
        .calls
        .iter()
        .filter(|c| c.host.is_native() && c.native_id.is_some())
        .count();
    assert_eq!(
        native_id_pairs, 385,
        "distinct (native, id) pairs, per the owner census (id 6 under two natives)"
    );

    // The one census surprise, pinned: a single callback site spells a `$$X$$`
    // target outside the five-native table. It is attributed to nothing and
    // keeps its own family rather than being folded into a native.
    let unmeasured_sites: u32 = corpus.sites_of_host(HostKind::Unmeasured);
    assert_eq!(unmeasured_sites, 1, "one unmeasured dispatch target site");
    assert_eq!(
        corpus.call(HostKind::Unmeasured, None).map(|c| c.sites),
        Some(1),
        "and it has no integer dispatch value"
    );

    // Mail: 90 message ids plus the computed-dispatch family.
    assert_eq!(corpus.sites_of_host(HostKind::Mail), 1102);
    assert_eq!(
        corpus
            .calls
            .iter()
            .filter(|c| c.host == HostKind::Mail)
            .count(),
        91
    );
}

/// The re-keyed audit of the installation: what binds, what refuses, and that
/// the gate stays closed while any reachable family is unmeasured — with each
/// such family named by native and id.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f38_d1_retail_the_audit_binds_the_measured_hosts_and_names_the_rest() {
    let root = game_dir();
    let container = std::fs::read(root.join(CRIMSON_ROF)).expect("read crimson.rof");
    let mut context = ParseContext::new(CRIMSON_ROF, 256 * 1024 * 1024, 8);
    let scripts = ui_scripts(&mut context, &container);
    let corpus = measure_host_call_corpus(
        scripts
            .iter()
            .map(|(spelling, bytes)| CorpusMember { spelling, bytes }),
        UiScriptLimits::default(),
    )
    .expect("every shipped UI script scans");

    let family_rows: Vec<MeasuredCallRow> = corpus
        .calls
        .iter()
        .map(|call| MeasuredCallRow {
            host: host_of(call),
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
        .collect();
    let table = ObservedBindingTable::measure(&family_rows).expect("every row is valid");
    let coverage = table.coverage();
    assert_eq!(
        coverage.families, 481,
        "389 native + 91 mail + 1 unmeasured"
    );
    assert_eq!(
        coverage.bound_families, 236,
        "144 goslocalize + 1 gospageinit + 91 mail families bind"
    );
    assert_eq!(
        coverage.unimplemented_families, 245,
        "58 goscallback + 170 uidata + 16 uicontrol + 1 unmeasured-target family"
    );
    assert!(!coverage.complete() && !coverage.campaign_ready());
    assert!(
        table.registry().is_empty(),
        "no UI host call enters the mission registry"
    );

    let mut site_rows = Vec::new();
    for (spelling, bytes) in &scripts {
        let scan = scan_ui_program(spelling, bytes, UiScriptLimits::default()).expect("scans");
        for site in &scan.sites {
            site_rows.push(SiteRow {
                host: MeasuredHost::from_code(site.host().code())
                    .unwrap_or_else(|| panic!("code {} is a measured host", site.host().code())),
                native_id: site.native_id,
                arg_shape_codes: site.args.iter().map(|s| s.code()).collect(),
                origin: SiteOrigin {
                    member: spelling.clone(),
                    offset: site.span.offset,
                    len: site.span.len,
                    line: 1,
                    column: 1,
                },
            });
        }
    }
    assert_eq!(site_rows.len() as u32, corpus.sites);

    let audit = audit_sites(&table, &site_rows, corpus.other_call_sites()).expect("within bounds");
    assert_eq!(audit.unjudged_heads, 3188);
    assert_eq!(audit.sites.len(), 1812);
    assert_eq!(
        audit.bound(),
        1287,
        "174 goslocalize + 11 gospageinit + 1102 mail sites bind to their measured meanings"
    );
    assert_eq!(
        audit.refused(),
        525,
        "524 goscallback/uidata/uicontrol sites + 1 unmeasured-target site refuse"
    );
    assert_eq!(
        audit.malformed(),
        0,
        "no site disagrees with its own corpus"
    );
    assert!(!audit.campaign_ready(), "the gate stays closed");

    // Every bound site carries a measured host's name; every unmeasured family
    // carries an unmeasured native's name and its dispatch value.
    let unmeasured = audit.unmeasured_families();
    assert_eq!(unmeasured.len(), 245);
    for spelling in &unmeasured {
        assert!(
            spelling.starts_with("goscallback#")
                || spelling.starts_with("uidata#")
                || spelling.starts_with("uicontrol#")
                || spelling.starts_with("unmeasured#"),
            "an unmeasured family names an unmeasured native: {spelling}"
        );
    }
    let bound_spellings: std::collections::BTreeSet<String> = audit
        .sites
        .iter()
        .filter_map(|site| match &site.verdict {
            cs_script::bindings::located::SiteVerdict::Bound { spelling } => Some(spelling.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(bound_spellings.len(), 236, "one spelling per bound family");
    for spelling in &bound_spellings {
        assert!(
            spelling.starts_with("goslocalize#")
                || spelling.starts_with("gospageinit#")
                || spelling.starts_with("mail#"),
            "only a measured host binds: {spelling}"
        );
    }

    // A refused verdict always names its native and handler address.
    for site in audit.with_code("refused") {
        let text = site.to_string();
        let named = ["0x407670", "0x4093a0", "0x404960"]
            .into_iter()
            .any(|address| text.contains(address));
        assert!(
            named || text.contains("`unmeasured#"),
            "every refusal names its handler (or its unmeasured host): {text}"
        );
    }
}
